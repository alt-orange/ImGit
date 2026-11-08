#pragma once

#include <expected>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

// Custom theme files: JSON, one theme per file, in the profile's "themes" folder.
//
//     {
//         "name": "Paper",
//         "base": "ImGit Light",
//         "background": "#FAF7F0",
//         "accent": "#2F7D6D",
//         "colors": { "text_normal": "#1F2328E0", "selected_row": "rgba(47, 125, 109, 0.25)" }
//     }
//
// "base" is the built-in theme it starts from (ImGit Dark when left out). "background", "accent", "link",
// "text_accent", "avatar" and "prompt" replace that theme's own: the surfaces and text are derived from the
// background again (light ones get dark text). "colors" then sets palette colours by name (theme.hpp's Palette
// fields, "graph_0" .. "graph_9" for the lanes). Colours are "#RGB", "#RRGGBB", "#RRGGBBAA", "rgb(r, g, b)" or
// "rgba(r, g, b, a)".
namespace imgit::theme
{
    struct Rgba
    {
        float r = 0.f;
        float g = 0.f;
        float b = 0.f;
        float a = 1.f;
    };

    struct ThemeFile
    {
        std::string name;
        std::string base; // "" for the default theme
        std::optional<Rgba> background;
        std::optional<Rgba> accent;
        std::optional<Rgba> link;
        std::optional<Rgba> text_accent;
        std::optional<Rgba> avatar;
        std::optional<Rgba> prompt;
        std::vector<std::pair<std::string, Rgba>> colors; // in file order
    };

    [[nodiscard]] std::optional<Rgba> parse_color(std::string_view text);
    // The theme in `json`, or why it is not one.
    [[nodiscard]] std::expected<ThemeFile, std::string> parse_theme_file(std::string_view json);
} // namespace imgit::theme
