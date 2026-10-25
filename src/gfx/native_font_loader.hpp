#pragma once

#include <imgui.h>

// The platform's own glyph rasteriser as an ImFontLoader, the way Chromium draws the text: DirectWrite
// ClearType (3x1 sub-pixel) coverage on Windows. Each atlas texel holds the red / green / blue coverage in RGB and
// 255 in A (0 where the glyph is empty), so the atlas must be drawn by the sub-pixel renderer (gfx/lcd_renderer).
// Null where there is none yet; FreeType is used there.
namespace imgit::gfx
{
    // The ImFontLoader to install with ImFontAtlas::SetFontLoader() before fonts are added, or nullptr when the
    // platform rasteriser is not available.
    [[nodiscard]] const ImFontLoader* native_font_loader();

    // Coverage curve applied to each channel as glyphs are rasterised: c' = c ^ (1 / gamma), then the contrast
    // boost of DirectWrite's "enhanced contrast" (c' += contrast * c' * (1 - c')).
    void set_native_coverage_curve(float gamma, float contrast);
} // namespace imgit::gfx
