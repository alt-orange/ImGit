//! The app's fonts: Open Sans from the embedded assets, the system's monospace / UI / Arial faces, and the
//! rasteriser they are drawn with.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use dear_imgui_rs::{Context, FontConfig, FontId, FontSource, FontStackToken, Ui, sys};

use crate::assets;
use crate::gfx::Renderer;
use crate::gfx::gamma_font_loader::{default_gamma, gamma_font_loader};
use crate::gfx::native_font_loader::{CoverageCurve, native_font_loader};
use crate::theme::{FONT_SIZE_NORMAL, FONT_SIZE_SMALL};

/// Open Sans' line height over its em (`usWinAscent + usWinDescent` over `unitsPerEm`), the fallback for a font
/// whose tables cannot be read.
const OPEN_SANS_EM_SCALE: f32 = 2789.0 / 2048.0;

/// A loaded font and what it takes to size it like CSS does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Font {
    /// The handle ImGui knows the font by.
    pub id: FontId,
    /// The font's line height over its em. ImGui sizes a font by its line height (ascent + descent), CSS by its
    /// em; a CSS `font-size` is multiplied by this to get the ImGui size that draws the same glyphs.
    pub em_scale: f32,
    /// The `ImFont` behind [`Self::id`], which the safe API keeps to itself: what text measuring and the draw
    /// list's text calls take. The atlas owns it for as long as the ImGui context lives.
    raw: *mut sys::ImFont,
}

impl Font {
    /// The ImGui size (before the DPI scale ImGui applies itself) for a CSS font size in logical pixels: what
    /// [`Ui::push_font_with_size`] takes.
    #[must_use]
    pub fn size(self, css_size: f32) -> f32 {
        css_size * self.em_scale
    }

    /// The `ImFont` for `sys` calls (`ImFont::CalcTextSizeA`, `ImDrawList::AddText`). Valid while the ImGui
    /// context the font was loaded into lives.
    #[must_use]
    pub fn raw(self) -> *mut sys::ImFont {
        self.raw
    }

    /// Makes this the current font at a CSS size until the token is dropped (`ui::push_font` in C++).
    pub fn push(self, ui: &Ui, css_size: f32) -> FontStackToken<'_> {
        ui.push_font_with_size(Some(self.id), self.size(css_size))
    }
}

/// The fonts of the UI. Where a face is missing, the next best one stands in, so every handle is usable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fonts {
    /// Open Sans, the UI's body text and ImGui's default font.
    pub regular: Font,
    pub semibold: Font,
    pub bold: Font,
    pub italic: Font,
    /// The system's monospace face (Consolas): diffs, hashes.
    pub mono: Font,
    /// The native menus' face (Segoe UI).
    pub system: Font,
    /// Arial, the tooltips' face.
    pub arial: Font,
    /// The Editor and In-App Terminal faces, each an ImFont of its own (even on the system face) so the
    /// editor's tab width can be set on it alone.
    pub editor: Font,
    pub terminal: Font,
    /// The monitor scale the layout is multiplied by (`style.FontScaleDpi`).
    pub dpi_scale: f32,
    /// Whether the glyphs are sub-pixel (ClearType) coverage drawn by the LCD renderer.
    pub subpixel: bool,
}

impl Fonts {
    /// The pixel height ImGui draws `font` at for a CSS size, DPI scale included: what the draw list's
    /// `add_text_with_font` takes (`ui::font_px` in C++).
    #[must_use]
    pub fn px(&self, font: Font, css_size: f32) -> f32 {
        font.size(css_size) * self.dpi_scale
    }
}

/// The font files the user chose in the preferences; `None` uses the system's monospace face.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontRequest {
    pub editor: Option<PathBuf>,
    pub terminal: Option<PathBuf>,
}

/// How glyph coverage is shaped. The defaults are fitted to Chromium's ClearType: ink and colour fringes within
/// about 3 %.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSettings {
    /// Whether to use the platform's sub-pixel rasteriser where there is one. `IMGIT_NATIVE_TEXT=0` turns it
    /// off, leaving the grey-scale fallback every other platform gets.
    pub native: bool,
    /// Coverage curve of the native (sub-pixel) rasteriser. `IMGIT_TEXT_GAMMA`, `IMGIT_TEXT_CONTRAST`.
    pub curve: CoverageCurve,
    /// Extra exponent for grey-scale text within a sub-pixel atlas; `None` keeps the renderer's default.
    /// `IMGIT_GRAY_GAMMA`.
    pub grayscale_gamma: Option<f32>,
    /// Coverage gamma of the grey-scale fallback rasteriser; `None` derives it from the framebuffer density.
    /// `IMGIT_FREETYPE_GAMMA` (the name the C++ app gave it).
    pub fallback_gamma: Option<f32>,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            native: true,
            curve: CoverageCurve {
                gamma: 1.9,
                contrast: 0.0,
            },
            grayscale_gamma: None,
            fallback_gamma: None,
        }
    }
}

impl TextSettings {
    /// The defaults with the `IMGIT_*` environment overrides applied.
    #[must_use]
    pub fn from_env() -> Self {
        let number = |name: &str| {
            env::var(name)
                .ok()
                .and_then(|value| value.trim().parse::<f32>().ok())
        };
        let mut settings = Self {
            native: env::var_os("IMGIT_NATIVE_TEXT").is_none_or(|value| value != "0"),
            ..Self::default()
        };
        if let Some(gamma) = number("IMGIT_TEXT_GAMMA").filter(|gamma| *gamma > 0.0) {
            settings.curve.gamma = gamma;
        }
        if let Some(contrast) = number("IMGIT_TEXT_CONTRAST") {
            settings.curve.contrast = contrast;
        }
        settings.grayscale_gamma = number("IMGIT_GRAY_GAMMA").filter(|gamma| *gamma > 0.0);
        settings.fallback_gamma = number("IMGIT_FREETYPE_GAMMA").map(|gamma| gamma.max(1.0));
        settings
    }
}

/// The roles ImGit fills with a system font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystemFont {
    /// The native menus' face.
    Ui,
    /// The diff and the terminal.
    Monospace,
    /// The tooltips.
    Arial,
}

/// The font file for `font`; `None` when the system has none.
///
/// TODO: switch to `imgit_platform::system::system_font_file` (which also knows macOS and fontconfig) once the
/// shell depends on that crate.
fn system_font_file(font: SystemFont) -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let file = match font {
        SystemFont::Ui => "segoeui.ttf",
        SystemFont::Monospace => "consola.ttf",
        SystemFont::Arial => "arial.ttf",
    };
    let windows = env::var_os("WINDIR").or_else(|| env::var_os("SystemRoot"))?;
    let path = Path::new(&windows).join("Fonts").join(file);
    path.is_file().then_some(path)
}

/// `(usWinAscent + usWinDescent) / unitsPerEm` of the face `font_no` in a TrueType / OpenType file: the line
/// height Windows (and Chromium on Windows) report for it. Open Sans 2789, Segoe UI 2724, Consolas 2398 (its
/// hhea is smaller), Arial 2288, all over 2048. `None` when the tables are missing.
fn em_scale_from_file(data: &[u8], font_no: usize) -> Option<f32> {
    let u16_at = |at: usize| {
        Some(u32::from(u16::from_be_bytes(
            data.get(at..at + 2)?.try_into().ok()?,
        )))
    };
    let u32_at = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    // 'ttcf': a collection, offsets to each font's table directory.
    let base = if u32_at(0)? == 0x7474_6366 {
        u32_at(12 + 4 * font_no)? as usize
    } else {
        0
    };
    let mut head = None;
    let mut os2 = None;
    for index in 0..u16_at(base + 4)? as usize {
        let record = base + 12 + 16 * index;
        match &u32_at(record)?.to_be_bytes() {
            b"head" => head = Some(u32_at(record + 8)? as usize),
            b"OS/2" => os2 = Some(u32_at(record + 8)? as usize),
            _ => {}
        }
    }
    let units_per_em = u16_at(head? + 18)?;
    let os2 = os2?;
    let line_height = u16_at(os2 + 74)? + u16_at(os2 + 76)?;
    (units_per_em != 0 && line_height != 0).then(|| line_height as f32 / units_per_em as f32)
}

/// Whether the first face of the TrueType / OpenType file `data` is complete enough to be loaded: every table
/// of its directory lies inside the file, and the tables a rasteriser needs are there (`head` with its magic
/// number, `maxp`, `cmap`, `hhea`, `hmtx`, and TrueType outlines or CFF ones). What a loader rejects (and
/// dear-imgui-rs turns into a panic) is a truncated or damaged file; this finds it first. Applied to the files
/// the user chose, not to the ones that ship with the app or the system.
fn is_loadable_font(data: &[u8]) -> bool {
    let u16_at = |at: usize| Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let check = || -> Option<bool> {
        if !is_sfnt(data) {
            return Some(false);
        }
        let base = if u32_at(0)? == 0x7474_6366 {
            u32_at(12)? as usize
        } else {
            0
        };
        let mut tags = Vec::new();
        let mut head = None;
        for index in 0..usize::from(u16_at(base.checked_add(4)?)?) {
            let record = base.checked_add(12 + 16 * index)?;
            let tag = u32_at(record)?.to_be_bytes();
            let offset = u32_at(record + 8)? as usize;
            let length = u32_at(record + 12)? as usize;
            if offset.checked_add(length)? > data.len() {
                return Some(false);
            }
            if &tag == b"head" {
                head = Some((offset, length));
            }
            tags.push(tag);
        }
        let has = |tag: &[u8; 4]| tags.contains(tag);
        let (head, head_length) = head?;
        let outlines = (has(b"glyf") && has(b"loca")) || has(b"CFF ") || has(b"CFF2");
        Some(
            head_length >= 54
                && u32_at(head + 12)? == 0x5F0F_3CF5
                && outlines
                && [b"maxp", b"cmap", b"hhea", b"hmtx"].into_iter().all(has),
        )
    };
    check().unwrap_or(false)
}

/// Whether `data` starts like a font file a loader may be handed: TrueType, OpenType (CFF) or a collection.
fn is_sfnt(data: &[u8]) -> bool {
    matches!(
        data.get(..4),
        Some([0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf")
    )
}

/// Adds a complete font file to the atlas with the reference size `css_size`. `None` when it is not a font.
fn add_font(
    imgui: &Context,
    name: &str,
    data: &[u8],
    css_size: f32,
    grayscale: bool,
) -> Option<Font> {
    if !is_sfnt(data) {
        return None;
    }
    let mut config = FontConfig::new().name(name);
    if grayscale {
        // Whole-pixel coverage, like the unhinted FreeType glyphs the gamma was fitted on; stb's default 2x
        // horizontal oversampling would be blurred further by the gamma boost.
        config = config.oversample_h(1).oversample_v(1);
    }
    // SAFETY: `data` is a complete font file: an asset compiled into the executable or a font installed with
    // the system, read whole. ImGui copies it, so it only has to live for this call.
    let source = unsafe { FontSource::ttf_data_with_size(data, css_size) }.with_config(config);
    let id = imgui.font_atlas().add_font(&[source]);
    Some(Font {
        id,
        em_scale: em_scale_from_file(data, 0).unwrap_or(OPEN_SANS_EM_SCALE),
        raw: last_font(imgui),
    })
}

/// The `ImFont` the atlas added last: the one behind the `FontId` it just returned.
fn last_font(imgui: &Context) -> *mut sys::ImFont {
    let atlas = imgui.font_atlas().raw();
    // SAFETY: the atlas belongs to `imgui`, which is alive; its font list is read once, on the thread that
    // owns the context, right after a font was added to it.
    unsafe {
        let fonts = &(*atlas).Fonts;
        assert!(
            !fonts.Data.is_null() && fonts.Size > 0,
            "the atlas holds the font it just added"
        );
        *fonts.Data.add(fonts.Size as usize - 1)
    }
}

/// Loads every font and sets the base font size and DPI scale, as the C++ build's `load_fonts()` did.
///
/// `dpi_scale` is the layout scale of the window, `density` the framebuffer's pixels per window coordinate
/// (above 1 on macOS only). Must run before the first frame, after the renderer is attached.
pub fn load_fonts(
    imgui: &mut Context,
    renderer: &mut Renderer,
    request: &FontRequest,
    settings: TextSettings,
    dpi_scale: f32,
    density: f32,
) -> Fonts {
    // The text is Chromium's DirectWrite ClearType. Use the same rasteriser where it exists (drawn by the
    // sub-pixel renderer); elsewhere unhinted grey-scale glyphs with a coverage gamma for the same stem weight.
    let native = if settings.native && renderer.supports_lcd_text() {
        native_font_loader(settings.curve)
    } else {
        None
    };
    let subpixel = native.is_some();
    let loader = native.unwrap_or_else(|| {
        gamma_font_loader(
            settings
                .fallback_gamma
                .unwrap_or_else(|| default_gamma(density)),
        )
    });
    let installed = imgui.font_atlas().set_font_loader(loader).is_ok();
    debug_assert!(
        installed,
        "the font loader is chosen before any font is added"
    );
    renderer.set_subpixel_atlas(subpixel && installed);
    if let Some(gamma) = settings.grayscale_gamma {
        renderer.set_grayscale_gamma(gamma);
    }
    let grayscale = !subpixel;

    // A font compiled into the executable (assets/fonts).
    let embedded = |imgui: &Context, name: &str, size: f32| {
        add_font(
            imgui,
            name,
            assets::data(&format!("fonts/{name}.ttf"))?,
            size,
            grayscale,
        )
    };
    // A font file on disk.
    let from_file = |imgui: &Context, path: &Path, size: f32| {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        add_font(imgui, &name, &fs::read(path).ok()?, size, grayscale)
    };
    // A font installed with the system.
    let system = |imgui: &Context, font: SystemFont, size: f32| {
        from_file(imgui, &system_font_file(font)?, size)
    };

    // The first font added is ImGui's default font (the C++ sets `io.FontDefault` to it explicitly).
    let regular = embedded(imgui, "OpenSans-Regular", FONT_SIZE_NORMAL).unwrap_or_else(|| {
        let id = imgui.font_atlas().add_font(&[FontSource::default_font()]);
        Font {
            id,
            em_scale: OPEN_SANS_EM_SCALE,
            raw: last_font(imgui),
        }
    });
    let semibold = embedded(imgui, "OpenSans-Semibold", FONT_SIZE_NORMAL).unwrap_or(regular);
    let bold = embedded(imgui, "OpenSans-Bold", FONT_SIZE_NORMAL).unwrap_or(semibold);
    let italic = embedded(imgui, "OpenSans-Italic", FONT_SIZE_NORMAL).unwrap_or(regular);
    let mono = system(imgui, SystemFont::Monospace, FONT_SIZE_SMALL).unwrap_or(regular);
    let ui_font = system(imgui, SystemFont::Ui, FONT_SIZE_SMALL).unwrap_or(regular);
    let arial = system(imgui, SystemFont::Arial, FONT_SIZE_SMALL).unwrap_or(regular);
    // The user's file when it is a font the loader accepts (a damaged one would make `add_font` panic), else
    // the system's monospace face, in an ImFont of its own.
    let preferred_mono = |imgui: &Context, preference: Option<&PathBuf>| {
        let chosen = preference.and_then(|path| {
            let data = fs::read(path).ok()?;
            if !is_loadable_font(&data) {
                eprintln!(
                    "imgit: {} is not a usable font; using the system monospace face",
                    path.display()
                );
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().into_owned();
            add_font(imgui, &name, &data, FONT_SIZE_SMALL, grayscale)
        });
        chosen
            .or_else(|| system(imgui, SystemFont::Monospace, FONT_SIZE_SMALL))
            .unwrap_or(mono)
    };
    let editor = preferred_mono(imgui, request.editor.as_ref());
    let terminal = preferred_mono(imgui, request.terminal.as_ref());

    let style = imgui.style_mut();
    style.set_font_size_base(regular.size(FONT_SIZE_NORMAL));
    style.set_font_scale_dpi(dpi_scale);

    Fonts {
        regular,
        semibold,
        bold,
        italic,
        mono,
        system: ui_font,
        arial,
        editor,
        terminal,
        dpi_scale,
        subpixel,
    }
}

/// A font given to the draw list's `add_text_with_font` (the bold and semibold labels) bakes its glyphs at its
/// own `CurrentRasterizerDensity`, which ImGui keeps current only for the font `PushFont()` made current; on a
/// Retina framebuffer the others would bake at 1x and be drawn pixel-doubled. Every font follows the
/// framebuffer's scale. Call each frame, once it has begun.
pub fn sync_rasterizer_density(ui: &Ui) {
    let density = ui.io().display_framebuffer_scale()[0];
    if density <= 0.0 {
        return;
    }
    // SAFETY: `ui` proves a frame of the current context is open on this thread, so its IO, atlas and fonts
    // are alive; `CurrentRasterizerDensity` is a plain field ImGui itself rewrites in `PushFont()`. The safe
    // API has no setter for it, hence the raw access.
    unsafe {
        let io = sys::igGetIO_Nil();
        if io.is_null() || (*io).Fonts.is_null() {
            return;
        }
        let fonts = &(*(*io).Fonts).Fonts;
        if fonts.Data.is_null() || fonts.Size <= 0 {
            return;
        }
        for &font in std::slice::from_raw_parts(fonts.Data, fonts.Size as usize) {
            if !font.is_null() {
                (*font).CurrentRasterizerDensity = density;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_line_height_of_the_embedded_faces() {
        for face in ["Regular", "Semibold", "Bold", "Italic"] {
            let data = assets::data(&format!("fonts/OpenSans-{face}.ttf")).expect("embedded");
            assert!(is_sfnt(data));
            assert_eq!(
                em_scale_from_file(data, 0),
                Some(OPEN_SANS_EM_SCALE),
                "{face}"
            );
        }
        assert_eq!(em_scale_from_file(b"not a font", 0), None);
        assert!(!is_sfnt(b"PNG"));
    }

    #[test]
    fn rejects_truncated_and_damaged_font_files() {
        let data = assets::data("fonts/OpenSans-Regular.ttf").expect("embedded");
        assert!(is_loadable_font(data));
        // Cut anywhere past the header, the table directory or a table runs off the end.
        for length in [0, 3, 4, 12, 100, data.len() / 2, data.len() - 1] {
            assert!(!is_loadable_font(&data[..length]), "{length} bytes");
        }
        // Wrong head magic, and a table count that promises more than the file has.
        let mut damaged = data.to_vec();
        let head = (0..usize::from(u16::from_be_bytes([damaged[4], damaged[5]])))
            .map(|index| 12 + 16 * index)
            .find(|&record| &damaged[record..record + 4] == b"head")
            .expect("head table");
        let offset = u32::from_be_bytes(damaged[head + 8..head + 12].try_into().unwrap()) as usize;
        damaged[offset + 12] ^= 0xFF;
        assert!(!is_loadable_font(&damaged));
        let mut many = data.to_vec();
        many[4..6].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(!is_loadable_font(&many));
        assert!(!is_loadable_font(b"not a font"));
    }

    #[cfg(windows)]
    #[test]
    fn accepts_the_system_faces() {
        for font in [SystemFont::Ui, SystemFont::Monospace, SystemFont::Arial] {
            let data = fs::read(system_font_file(font).expect("installed")).expect("readable");
            assert!(is_loadable_font(&data));
        }
    }

    #[cfg(windows)]
    #[test]
    fn reads_the_line_height_of_the_system_faces() {
        // The values the C++ app hard-coded before it read them from the files.
        for (font, line_height) in [
            (SystemFont::Ui, 2724.0),
            (SystemFont::Monospace, 2398.0),
            (SystemFont::Arial, 2288.0),
        ] {
            let path = system_font_file(font).expect("installed with Windows");
            let data = fs::read(&path).expect("readable");
            assert_eq!(
                em_scale_from_file(&data, 0),
                Some(line_height / 2048.0),
                "{}",
                path.display()
            );
        }
    }
}
