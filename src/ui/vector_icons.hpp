#pragma once

#include <imgui.h>
#include <string_view>

// The icons: Font Awesome Free SVG paths (assets/icons/fa_icons.tsv), rasterised on demand into small
// alpha textures (one per icon and pixel size) and drawn tinted, like an inline <svg> with `fill: currentColor`.
namespace imgit::ui
{
    // How the host creates and frees RGBA8 textures (OpenGL in main.cpp).
    struct TextureBackend
    {
        ImTextureID (*create)(int width, int height, const unsigned char* rgba) = nullptr;
        void (*destroy)(ImTextureID texture) = nullptr;
    };

    void set_texture_backend(TextureBackend backend) noexcept;
    [[nodiscard]] ImTextureID create_texture(int width, int height, const unsigned char* rgba);
    void destroy_texture(ImTextureID texture);

    // True when `name` ("far:undo-alt", "fas:flask", ...) is a known icon.
    [[nodiscard]] bool has_fa_icon(std::string_view name) noexcept;
    // Width of the icon drawn at `size` (its em height, the CSS font-size) in device pixels.
    [[nodiscard]] float fa_icon_width(std::string_view name, float size) noexcept;
    // Draws the icon with its em box centred on `center`; `size` is the em height in device pixels. A rotation
    // (degrees, clockwise) turns the glyph about its centre like a CSS `transform: rotate()`.
    void draw_fa_icon(ImDrawList* draw, std::string_view name, ImVec2 center, float size, ImU32 color,
                      float rotation_degrees = 0.f);
    // Mirrored left to right (CSS fa-flip-horizontal).
    void draw_fa_icon_mirrored(ImDrawList* draw, std::string_view name, ImVec2 center, float size, ImU32 color);
    // Same, with the em box's top-left corner at `pos`.
    void draw_fa_icon_at(ImDrawList* draw, std::string_view name, ImVec2 pos, float size, ImU32 color);
    // A duotone ("fad:...") with each path in its own colour (--fa-primary-color / --fa-secondary-color, both at
    // full opacity).
    void draw_fa_duotone(ImDrawList* draw, std::string_view name, ImVec2 center, float size, ImU32 primary,
                         ImU32 secondary);

    // The path data (the first <path>'s d) of assets/svg/<name>.svg; empty when there is none.
    [[nodiscard]] std::string_view svg_path(std::string_view name);
    // Rasterises an arbitrary SVG path (`view_w` x `view_h` viewBox) into a cached texture and draws it into
    // the rectangle [p0, p1]. `key` identifies the path in the cache; a leading '!' selects fill-rule evenodd.
    void draw_svg_path(ImDrawList* draw, std::string_view key, std::string_view path, float view_w, float view_h,
                       ImVec2 p0, ImVec2 p1, ImU32 color);
} // namespace imgit::ui
