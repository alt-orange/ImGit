//! The platform's own glyph rasteriser as an `ImFontLoader`, the way Chromium draws the text: DirectWrite
//! ClearType (3x1 sub-pixel) coverage on Windows. Each atlas texel holds the red / green / blue coverage in RGB
//! and 255 in A (0 where the glyph is empty), so the atlas must be drawn by the sub-pixel renderer
//! ([`super::lcd_renderer`]). `None` where there is none yet; stb_truetype with a coverage gamma
//! ([`super::gamma_font_loader`]) is used there.

use dear_imgui_rs::FontLoader;

#[cfg(all(windows, feature = "lcd-text"))]
mod dwrite;

/// Coverage curve applied to each channel as glyphs are rasterised: `c' = c ^ (1 / gamma)`, then the contrast
/// boost of DirectWrite's "enhanced contrast" (`c' += contrast * c' * (1 - c')`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverageCurve {
    pub gamma: f32,
    pub contrast: f32,
}

impl CoverageCurve {
    /// Coverage as rasterised.
    pub const IDENTITY: Self = Self {
        gamma: 1.0,
        contrast: 0.0,
    };

    /// The curve as a lookup table over 8-bit coverage.
    #[must_use]
    pub fn lut(self) -> [u8; 256] {
        std::array::from_fn(|index| {
            let coverage = (index as f32 / 255.0).powf(1.0 / self.gamma);
            let boosted = (coverage + self.contrast * coverage * (1.0 - coverage)).clamp(0.0, 1.0);
            (boosted * 255.0).round() as u8
        })
    }
}

/// The loader to install with `FontAtlas::set_font_loader()` before fonts are added, or `None` when the
/// platform rasteriser is not available. The first call fixes `curve` for the life of the process.
#[must_use]
pub fn native_font_loader(curve: CoverageCurve) -> Option<&'static FontLoader> {
    #[cfg(all(windows, feature = "lcd-text"))]
    {
        dwrite::font_loader(curve)
    }
    #[cfg(not(all(windows, feature = "lcd-text")))]
    {
        let _ = curve;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_curve_keeps_the_ends_and_lifts_the_middle() {
        let identity = CoverageCurve::IDENTITY.lut();
        assert!(
            identity
                .iter()
                .enumerate()
                .all(|(index, &value)| usize::from(value) == index)
        );
        let boosted = CoverageCurve {
            gamma: 1.9,
            contrast: 0.0,
        }
        .lut();
        assert_eq!((boosted[0], boosted[255]), (0, 255));
        assert!(boosted[64] > 64 && boosted[128] > 128);
        assert!(boosted.is_sorted());
    }
}
