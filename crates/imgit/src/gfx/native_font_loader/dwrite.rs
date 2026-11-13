//! DirectWrite ClearType as an `ImFontLoader`.

use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::OnceLock;

use dear_imgui_rs::{FontLoader, sys};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FACE_TYPE_UNKNOWN, DWRITE_FONT_FILE_TYPE_UNKNOWN,
    DWRITE_FONT_METRICS, DWRITE_FONT_SIMULATIONS_NONE, DWRITE_GLYPH_METRICS, DWRITE_GLYPH_OFFSET,
    DWRITE_GLYPH_RUN, DWRITE_GRID_FIT_MODE_DEFAULT, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_RENDERING_MODE, DWRITE_RENDERING_MODE_ALIASED, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
    DWRITE_RENDERING_MODE_OUTLINE, DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE,
    DWRITE_TEXTURE_CLEARTYPE_3x1, DWriteCreateFactory, IDWriteFactory2, IDWriteFactory5,
    IDWriteFontFace, IDWriteInMemoryFontFileLoader, IDWriteRenderingParams,
};
use windows::core::{BOOL, IUnknown};

use super::CoverageCurve;

/// The process-wide DirectWrite objects. They are thread-safe COM objects; ImGui only calls the loader on the
/// UI thread anyway.
struct DWrite {
    factory: IDWriteFactory5,
    memory_loader: IDWriteInMemoryFontFileLoader,
    params: Option<IDWriteRenderingParams>,
    curve: [u8; 256],
}

static DWRITE: OnceLock<Option<DWrite>> = OnceLock::new();

impl DWrite {
    fn create(curve: CoverageCurve) -> Option<Self> {
        // SAFETY: plain COM factory calls; every returned interface is owned by the wrapper it lands in.
        unsafe {
            let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let memory_loader = factory.CreateInMemoryFontFileLoader().ok()?;
            factory.RegisterFontFileLoader(&memory_loader).ok()?;
            let params = factory.CreateRenderingParams().ok();
            Some(Self {
                factory,
                memory_loader,
                params,
                curve: curve.lut(),
            })
        }
    }
}

/// Per font source: its DirectWrite face.
struct SourceData {
    face: IDWriteFontFace,
    metrics: DWRITE_FONT_METRICS,
}

/// Per (source, baked size): the em size DirectWrite renders at. Lives in the buffer ImGui allocates for the
/// loader (`FontBakedSrcLoaderDataSize` bytes, unaligned as far as the contract goes).
#[derive(Clone, Copy)]
struct BakedData {
    em: f32,
    mode: DWRITE_RENDERING_MODE,
}

struct LoaderTable(sys::ImFontLoader);

// SAFETY: the table is immutable: function pointers and a pointer to a `'static` C string.
unsafe impl Sync for LoaderTable {}

static LOADER: LoaderTable = LoaderTable(sys::ImFontLoader {
    Name: c"DirectWrite ClearType".as_ptr(),
    // The DirectWrite objects are process-wide and created by `font_loader()`; nothing to do per atlas.
    LoaderInit: None,
    LoaderShutdown: None,
    FontSrcInit: Some(source_init),
    FontSrcDestroy: Some(source_destroy),
    FontSrcGetGlyphIndexFromCodepoint: Some(source_glyph_index),
    FontSrcContainsGlyph: Some(source_contains_glyph),
    FontBakedInit: Some(baked_init),
    FontBakedDestroy: None,
    FontBakedLoadGlyph: Some(baked_load_glyph),
    FontBakedSrcLoaderDataSize: size_of::<BakedData>(),
});

pub(super) fn font_loader(curve: CoverageCurve) -> Option<&'static FontLoader> {
    DWRITE.get_or_init(|| DWrite::create(curve)).as_ref()?;
    // SAFETY: `LOADER` is a fully initialised, immutable `'static` table whose callbacks follow ImGui's
    // loader contract (below) and never unwind: a panic in an `extern "C"` function aborts.
    Some(unsafe { FontLoader::from_raw(&LOADER.0) })
}

fn dwrite() -> Option<&'static DWrite> {
    DWRITE.get()?.as_ref()
}

/// # Safety
/// `src` is a live font source; its loader data is null or was set by [`source_init`].
unsafe fn source_data<'a>(src: *const sys::ImFontConfig) -> Option<&'a SourceData> {
    // SAFETY: `FontLoaderData` is null or the `Box<SourceData>` leaked by `source_init`, alive until
    // `source_destroy`.
    unsafe { (*src).FontLoaderData.cast::<SourceData>().as_ref() }
}

unsafe extern "C" fn source_init(
    _atlas: *mut sys::ImFontAtlas,
    src: *mut sys::ImFontConfig,
) -> bool {
    let Some(dwrite) = dwrite() else {
        return false;
    };
    // SAFETY: ImGui passes a live source whose `FontData` holds `FontDataSize` bytes.
    let src = unsafe { &mut *src };
    let Ok(size) = u32::try_from(src.FontDataSize) else {
        return false;
    };
    // SAFETY: COM calls on live interfaces. With no owner object the in-memory loader copies the font data.
    let face = unsafe {
        let Ok(file) = dwrite.memory_loader.CreateInMemoryFontFileReference(
            &dwrite.factory,
            src.FontData.cast_const(),
            size,
            None::<&IUnknown>,
        ) else {
            return false;
        };
        // The face type comes from the file: a CFF .otf or a .ttc collection is not
        // DWRITE_FONT_FACE_TYPE_TRUETYPE.
        let mut supported = BOOL::default();
        let mut file_type = DWRITE_FONT_FILE_TYPE_UNKNOWN;
        let mut face_type = DWRITE_FONT_FACE_TYPE_UNKNOWN;
        let mut face_count = 0u32;
        if file
            .Analyze(
                &mut supported,
                &mut file_type,
                Some(&mut face_type),
                &mut face_count,
            )
            .is_err()
            || !supported.as_bool()
        {
            return false;
        }
        match dwrite.factory.CreateFontFace(
            face_type,
            &[Some(file)],
            src.FontNo,
            DWRITE_FONT_SIMULATIONS_NONE,
        ) {
            Ok(face) => face,
            Err(_) => return false,
        }
    };
    let mut metrics = DWRITE_FONT_METRICS::default();
    // SAFETY: writes the metrics of a live face into a local.
    unsafe { face.GetMetrics(&mut metrics) };
    src.FontLoaderData = Box::into_raw(Box::new(SourceData { face, metrics })).cast();
    true
}

unsafe extern "C" fn source_destroy(_atlas: *mut sys::ImFontAtlas, src: *mut sys::ImFontConfig) {
    // SAFETY: ImGui passes a live source; its loader data is null or the box `source_init` leaked, which is
    // reclaimed exactly once here.
    unsafe {
        let data = (*src).FontLoaderData.cast::<SourceData>();
        if !data.is_null() {
            drop(Box::from_raw(data));
        }
        (*src).FontLoaderData = ptr::null_mut();
    }
}

unsafe extern "C" fn source_glyph_index(
    _atlas: *mut sys::ImFontAtlas,
    src: *mut sys::ImFontConfig,
    codepoint: sys::ImWchar,
) -> c_int {
    // SAFETY: called for a source this loader initialised.
    let Some(data) = (unsafe { source_data(src) }) else {
        return 0;
    };
    let codepoint: u32 = codepoint;
    let mut index = 0u16;
    // SAFETY: one code point in, one glyph index out.
    match unsafe { data.face.GetGlyphIndices(&codepoint, 1, &mut index) } {
        Ok(()) => c_int::from(index),
        Err(_) => 0,
    }
}

unsafe extern "C" fn source_contains_glyph(
    _atlas: *mut sys::ImFontAtlas,
    src: *mut sys::ImFontConfig,
    glyph_index: c_int,
) -> bool {
    // SAFETY: called for a source this loader initialised.
    let Some(data) = (unsafe { source_data(src) }) else {
        return false;
    };
    // SAFETY: no preconditions on a live face.
    let count = unsafe { data.face.GetGlyphCount() };
    glyph_index > 0 && glyph_index < c_int::from(count)
}

unsafe extern "C" fn baked_init(
    _atlas: *mut sys::ImFontAtlas,
    src: *mut sys::ImFontConfig,
    baked: *mut sys::ImFontBaked,
    loader_data: *mut c_void,
) -> bool {
    // SAFETY: ImGui passes a live source (initialised by this loader), the baked font being set up, whose
    // owner font has at least this source, and a buffer of `FontBakedSrcLoaderDataSize` bytes.
    unsafe {
        let Some(data) = source_data(src) else {
            return false;
        };
        let src = &*src;
        let baked = &mut *baked;
        let mut size = baked.Size;
        let reference_size = (**(*baked.OwnerFont).Sources.Data).SizePixels;
        if src.MergeMode && src.SizePixels != 0.0 {
            size *= src.SizePixels / reference_size;
        }
        size *= src.ExtraSizeScale;
        let density = src.RasterizerDensity * baked.RasterizerDensity;
        // ImGui sizes a font by ascent + descent (FreeType's REAL_DIM request); DirectWrite takes the em.
        let units_per_em = f32::from(data.metrics.designUnitsPerEm);
        let units = f32::from(data.metrics.ascent) + f32::from(data.metrics.descent);
        let em = size * density * units_per_em / units;
        let mut mode = DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC;
        if let Some(params) = dwrite().and_then(|dwrite| dwrite.params.as_ref())
            && let Ok(recommended) = data.face.GetRecommendedRenderingMode(
                em,
                1.0,
                DWRITE_MEASURING_MODE_NATURAL,
                params,
            )
            && recommended != DWRITE_RENDERING_MODE_OUTLINE
            && recommended != DWRITE_RENDERING_MODE_ALIASED
        {
            mode = recommended;
        }
        loader_data
            .cast::<BakedData>()
            .write_unaligned(BakedData { em, mode });
        if !src.MergeMode {
            let to_px = em / units_per_em;
            let scale = 1.0 / (density * src.ExtraSizeScale);
            baked.Ascent = (f32::from(data.metrics.ascent) * to_px).ceil() * scale;
            baked.Descent = -(f32::from(data.metrics.descent) * to_px).floor() * scale;
        }
    }
    true
}

unsafe extern "C" fn baked_load_glyph(
    atlas: *mut sys::ImFontAtlas,
    src: *mut sys::ImFontConfig,
    baked: *mut sys::ImFontBaked,
    loader_data: *mut c_void,
    glyph_index: c_int,
    out_glyph: *mut sys::ImFontGlyph,
    out_advance_x: *mut f32,
) -> bool {
    let (Some(dwrite), Ok(index)) = (dwrite(), u16::try_from(glyph_index)) else {
        return false;
    };
    // SAFETY: ImGui passes the live atlas, source and baked font this loader set up, the loader data
    // `baked_init` wrote, and exactly one of `out_glyph` / `out_advance_x` (the other is null). The COM calls
    // read locals that outlive them; the pixel buffers have exactly the sizes passed along.
    unsafe {
        let Some(data) = source_data(src) else {
            return false;
        };
        let baked_data = loader_data.cast::<BakedData>().read_unaligned();
        let src_ref = &*src;
        let baked_ref = &*baked;
        let density = src_ref.RasterizerDensity * baked_ref.RasterizerDensity;
        let mut glyph_metrics = DWRITE_GLYPH_METRICS::default();
        if data
            .face
            .GetDesignGlyphMetrics(&index, 1, &mut glyph_metrics, false)
            .is_err()
        {
            return false;
        }
        let to_px = baked_data.em / f32::from(data.metrics.designUnitsPerEm);
        let advance_x = glyph_metrics.advanceWidth as f32 * to_px / density;
        // Metrics-only mode.
        if let Some(out_advance_x) = out_advance_x.as_mut() {
            *out_advance_x = advance_x;
            return true;
        }
        let out_glyph = &mut *out_glyph;
        out_glyph.AdvanceX = advance_x;

        let advance = 0.0f32;
        let offset = DWRITE_GLYPH_OFFSET::default();
        let run = DWRITE_GLYPH_RUN {
            // Borrowed for the call: a bitwise copy of the interface pointer that is never released.
            fontFace: std::mem::transmute_copy(&data.face),
            fontEmSize: baked_data.em,
            glyphCount: 1,
            glyphIndices: &index,
            glyphAdvances: &advance,
            glyphOffsets: &offset,
            isSideways: BOOL::default(),
            bidiLevel: 0,
        };
        // IDWriteFactory2's overload (later interfaces hide it behind DWRITE_RENDERING_MODE1 versions).
        let factory2: &IDWriteFactory2 = &dwrite.factory;
        let Ok(analysis) = factory2.CreateGlyphRunAnalysis(
            &run,
            None,
            baked_data.mode,
            DWRITE_MEASURING_MODE_NATURAL,
            DWRITE_GRID_FIT_MODE_DEFAULT,
            DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE,
            0.0,
            0.0,
        ) else {
            return false;
        };
        let bounds: RECT = analysis
            .GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1)
            .unwrap_or_default();
        let width = usize::try_from(bounds.right - bounds.left).unwrap_or(0);
        let height = usize::try_from(bounds.bottom - bounds.top).unwrap_or(0);
        if width == 0 || height == 0 {
            return true; // a space: advance only
        }
        let mut coverage = vec![0u8; width * height * 3];
        if analysis
            .CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, &mut coverage)
            .is_err()
        {
            return false;
        }
        // One empty column on each side: the renderer shifts glyphs by up to half a pixel for sub-pixel
        // positioning, and the quad must cover the shifted coverage.
        let padded_width = width + 2;
        let mut rgba = vec![0u8; padded_width * height * 4];
        for (source_row, target_row) in coverage
            .chunks_exact(width * 3)
            .zip(rgba.chunks_exact_mut(padded_width * 4))
        {
            let source_texels = source_row.as_chunks::<3>().0;
            let target_texels = target_row[4..].as_chunks_mut::<4>().0;
            for (source, target) in source_texels.iter().zip(target_texels) {
                let [red, green, blue] = source.map(|coverage| dwrite.curve[usize::from(coverage)]);
                let alpha = if (red | green | blue) != 0 { 255 } else { 0 };
                *target = [red, green, blue, alpha];
            }
        }
        let pack_id = sys::igImFontAtlasPackAddRect(
            atlas,
            padded_width as c_int,
            height as c_int,
            ptr::null_mut(),
        );
        if pack_id == crate::gfx::FONT_ATLAS_RECT_ID_INVALID {
            return false;
        }
        let rect = sys::igImFontAtlasPackGetRect(atlas, pack_id);
        let reference_size = (**(*baked_ref.OwnerFont).Sources.Data).SizePixels;
        let offsets_scale = if reference_size != 0.0 {
            baked_ref.Size / reference_size
        } else {
            1.0
        };
        let font_offset_x = (src_ref.GlyphOffset.x * offsets_scale + 0.5).floor();
        let font_offset_y =
            (src_ref.GlyphOffset.y * offsets_scale + 0.5).floor() + baked_ref.Ascent;
        out_glyph.X0 = (bounds.left - 1) as f32 / density + font_offset_x;
        out_glyph.Y0 = bounds.top as f32 / density + font_offset_y;
        out_glyph.X1 = (bounds.right + 1) as f32 / density + font_offset_x;
        out_glyph.Y1 = bounds.bottom as f32 / density + font_offset_y;
        out_glyph.set_Visible(1);
        out_glyph.set_Colored(0);
        out_glyph.PackId = pack_id;
        sys::igImFontAtlasBakedSetFontGlyphBitmap(
            atlas,
            baked,
            src,
            out_glyph,
            rect,
            rgba.as_ptr(),
            sys::ImTextureFormat_RGBA32,
            (padded_width * 4) as c_int,
        );
    }
    true
}
