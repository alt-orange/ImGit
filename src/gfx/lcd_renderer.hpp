#pragma once

#include <imgui.h>

// Renders ImGui draw data like imgui_impl_opengl3, but blends each colour channel with its own coverage so the
// ClearType glyphs of gfx/native_font_loader keep their sub-pixel sharpness (Chromium's LCD text). Dual-source blending
// computes  dst = colour * coverage + dst * (1 - coverage)  per channel, which equals ordinary alpha blending for
// everything that is not sub-pixel text. Textures are still created / updated by the OpenGL3 backend.
namespace imgit::gfx
{
    using GlProcLoader = void* (*)(const char* name);

    // Loads the GL 3.3 entry points it needs and builds its shader. Returns false when that is not possible
    // (the caller then keeps using ImGui_ImplOpenGL3_RenderDrawData).
    [[nodiscard]] bool lcd_renderer_init(GlProcLoader loader);
    // `lcd_texture` is the font atlas: drawn with per-channel coverage when it is a sub-pixel atlas.
    void lcd_renderer_render(ImDrawData* draw_data, ImTextureID lcd_texture);
    void lcd_renderer_shutdown();

    // Whether the font atlas holds the native loader's 3x1 sub-pixel coverage in RGB. A grey-scale atlas (FreeType:
    // one coverage per texel in A) is drawn like any other texture; sampling it per sub-pixel would smear each
    // glyph a third of a pixel to each side with colour fringes. False until set.
    void set_subpixel_atlas(bool subpixel);

    // Chromium draws text in composited scrolling layers with grey-scale anti-aliasing. Adding
    // this to a draw list switches the rest of that list's text to grey-scale (the channel average of the ClearType
    // coverage); `on` = false switches back. Harmless without the sub-pixel renderer.
    void set_grayscale_text(ImDrawList* list, bool on);
    // Extra coverage exponent for grey-scale text (Chromium boosts its A8 text more than its LCD text).
    void set_grayscale_gamma(float gamma);
} // namespace imgit::gfx
