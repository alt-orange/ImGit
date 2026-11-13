//! stb_truetype with a coverage gamma: the text rasteriser where there is no native one.
//!
//! Chromium blends ClearType text with gamma correction, so light text on the dark panels carries more ink
//! than ImGui's linear grey-scale coverage. The built-in loader is wrapped to pass each new glyph's coverage
//! through the same kind of gamma curve as it lands in the atlas. (The C++ app wraps FreeType without hinting
//! the same way; stb_truetype is the unhinted rasteriser this build has.)

use std::ffi::{c_int, c_void};
use std::sync::OnceLock;

use dear_imgui_rs::{FontLoader, sys};

struct GammaLoader {
    /// The stb_truetype table with `FontBakedLoadGlyph` replaced.
    table: sys::ImFontLoader,
    base_load_glyph: LoadGlyph,
    lut: [u8; 256],
}

type LoadGlyph = unsafe extern "C" fn(
    *mut sys::ImFontAtlas,
    *mut sys::ImFontConfig,
    *mut sys::ImFontBaked,
    *mut c_void,
    c_int,
    *mut sys::ImFontGlyph,
    *mut f32,
) -> bool;

// SAFETY: immutable once built: function pointers, a pointer to a `'static` C string and a lookup table.
unsafe impl Sync for GammaLoader {}
// SAFETY: as above; nothing in it is tied to a thread.
unsafe impl Send for GammaLoader {}

static LOADER: OnceLock<GammaLoader> = OnceLock::new();

/// The coverage gamma of the grey-scale fallback for a framebuffer `density` (pixels per logical pixel): 3.0 at
/// 1x reproduces ClearType's stem weight with grey-scale glyphs; at 2x (Retina) the edge pixels are half as wide
/// and the same boost makes both sides of a bold stem solid, which reads as blur, so it falls to 1.5.
#[must_use]
pub fn default_gamma(density: f32) -> f32 {
    1.0 + 2.0 / (density * density).max(1.0)
}

/// The loader to install with `FontAtlas::set_font_loader()` before fonts are added. The first call fixes
/// `gamma` for the life of the process.
#[must_use]
pub fn gamma_font_loader(gamma: f32) -> &'static FontLoader {
    let loader = LOADER.get_or_init(|| {
        // SAFETY: ImGui returns its static stb_truetype table, complete and immutable.
        let base = unsafe { *sys::igImFontAtlasGetFontLoaderForStbTruetype() };
        let mut table = base;
        table.Name = c"stb_truetype + gamma".as_ptr();
        table.FontBakedLoadGlyph = Some(load_glyph_with_gamma);
        GammaLoader {
            table,
            base_load_glyph: base.FontBakedLoadGlyph.expect("stb_truetype loads glyphs"),
            lut: std::array::from_fn(|index| {
                ((index as f32 / 255.0).powf(1.0 / gamma.max(1.0)) * 255.0).round() as u8
            }),
        }
    });
    // SAFETY: a complete `'static` table: stb_truetype's own callbacks plus the wrapper below, which keeps
    // their contract and never unwinds.
    unsafe { FontLoader::from_raw(&loader.table) }
}

unsafe extern "C" fn load_glyph_with_gamma(
    atlas: *mut sys::ImFontAtlas,
    src: *mut sys::ImFontConfig,
    baked: *mut sys::ImFontBaked,
    loader_data: *mut c_void,
    glyph_index: c_int,
    out_glyph: *mut sys::ImFontGlyph,
    out_advance_x: *mut f32,
) -> bool {
    let Some(loader) = LOADER.get() else {
        return false;
    };
    // SAFETY: the arguments are passed through unchanged to the loader they were meant for. Afterwards the
    // glyph's packed rectangle lies inside the atlas texture, whose pixels this thread may write while the
    // atlas is being built.
    unsafe {
        if !(loader.base_load_glyph)(
            atlas,
            src,
            baked,
            loader_data,
            glyph_index,
            out_glyph,
            out_advance_x,
        ) {
            return false;
        }
        let Some(glyph) = out_glyph.as_ref() else {
            return true; // metrics only
        };
        if glyph.Visible() == 0
            || glyph.Colored() != 0
            || glyph.PackId == super::FONT_ATLAS_RECT_ID_INVALID
        {
            return true;
        }
        let rect = *sys::igImFontAtlasPackGetRect(atlas, glyph.PackId);
        let texture = (*atlas).TexData;
        let bytes_per_pixel = if (*texture).Format == sys::ImTextureFormat_RGBA32 {
            4
        } else {
            1
        };
        let alpha = bytes_per_pixel - 1;
        for y in 0..c_int::from(rect.h) {
            let row = sys::ImTextureData_GetPixelsAt(
                texture,
                c_int::from(rect.x),
                c_int::from(rect.y) + y,
            );
            let row = std::slice::from_raw_parts_mut(
                row.cast::<u8>(),
                usize::from(rect.w) * bytes_per_pixel,
            );
            for pixel in row.chunks_exact_mut(bytes_per_pixel) {
                pixel[alpha] = loader.lut[usize::from(pixel[alpha])];
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_follows_the_density() {
        assert_eq!(default_gamma(1.0), 3.0);
        assert_eq!(default_gamma(2.0), 1.5);
        assert_eq!(default_gamma(0.5), 3.0);
    }
}
