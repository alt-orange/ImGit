#pragma once

#include <imgui.h>
#include <span>
#include <string_view>

// The palette of the current theme, resolved to RGBA. Themes share the layout and the semantic colours (red, green,
// the graph lanes...) and differ in their backgrounds and accent.
namespace imgit::theme
{
    constexpr int GRAPH_COLOR_COUNT = 10;

    struct Palette
    {
        ImVec4 white{1.f, 1.f, 1.f, 1.f};
        ImVec4 red;
        ImVec4 orange;
        ImVec4 yellow;
        ImVec4 green;
        ImVec4 teal;
        ImVec4 blue;
        ImVec4 ltblue;
        ImVec4 purple;
        ImVec4 accent; // selection, focus rings, primary buttons, badges

        ImVec4 app_bg0;
        ImVec4 toolbar_bg0;
        ImVec4 toolbar_bg1;
        ImVec4 toolbar_bg2;
        ImVec4 panel_bg0;
        ImVec4 panel_bg1;
        ImVec4 panel_bg2;
        ImVec4 input_bg;
        ImVec4 panel_border;
        ImVec4 section_border;
        ImVec4 subtle_border;
        ImVec4 modal_overlay;

        ImVec4 graph_colors[GRAPH_COLOR_COUNT];
        ImVec4 stats_added;
        ImVec4 stats_deleted;
        ImVec4 stats_files;

        ImVec4 text_selected;
        ImVec4 text_normal;
        ImVec4 text_secondary;
        ImVec4 text_disabled;
        ImVec4 text_accent;
        ImVec4 text_inverse;
        ImVec4 text_dimmed;

        ImVec4 hover_row;
        ImVec4 selected_row;
        ImVec4 link;
        ImVec4 wip_status;
        ImVec4 checked_out;
        ImVec4 added_line;
        ImVec4 deleted_line;
        ImVec4 hunk_header;
        ImVec4 avatar_default; // DEFAULT_AVATAR_BACKGROUND_COLOR
        ImVec4 scroll_thumb;
        ImVec4 toggle_border; // toggle-button-border
        ImVec4 filter_match; // filter-match
        ImVec4 droppable; // droppable / drop-target
        ImVec4 drop_target;
        ImVec4 terminal_bg; // terminal__background
        ImVec4 terminal_fg;
        ImVec4 terminal_prompt; // terminal__repo-name-color
        ImVec4 terminal_red;
    };

    [[nodiscard]] const Palette& palette() noexcept;

    constexpr const char* DEFAULT_THEME = "ImGit Dark";
    // The built-in themes, the default first.
    [[nodiscard]] std::span<const char* const> theme_names() noexcept;
    // Switches the palette to the theme called `name` (the default one for a name it does not know). Call
    // apply_imgui_style() after it.
    void set_theme(std::string_view name);
    [[nodiscard]] const char* current_theme() noexcept;

    // Applies the palette to the current ImGui style (colors, rounding, paddings).
    void apply_imgui_style();

    [[nodiscard]] ImVec4 with_alpha(ImVec4 color, float alpha) noexcept;
    // LESS mix(color, background, weight): `weight` of color over 1-weight of background, opaque.
    [[nodiscard]] ImVec4 mix(ImVec4 color, ImVec4 background, float weight) noexcept;
    [[nodiscard]] ImU32 u32(ImVec4 color) noexcept;
    [[nodiscard]] ImVec4 graph_color(int column) noexcept;

    // Layout constants of the graph.
    constexpr float TABS_BAR_HEIGHT = 34.f;
    constexpr float TOOLBAR_HEIGHT = 48.f;
    constexpr float GRAPH_HEADER_HEIGHT = 22.f;
    constexpr float GRAPH_HEADER_MARGIN_BOTTOM = 2.f; // HEADER_ROW_MARGIN_BOTTOM
    constexpr float GRAPH_LEFT_MARGIN = 10.f; // LeftPanelToGraphMarginGap
    constexpr float GRAPH_REF_ZONE_DEFAULT_WIDTH = 129.f;
    constexpr float GRAPH_ROW_HEIGHT = 28.f;
    constexpr float GRAPH_ROW_INNER_HEIGHT = 22.f;
    constexpr float COMMIT_ZONE_MARGIN = 3.f; // top and bottom
    constexpr float COMMIT_COLUMN_WIDTH = 22.f;
    constexpr float COMMIT_ZONE_GUTTER_WIDTH = 28.f;
    constexpr float COMMIT_ZONE_PADDING = 3.f; // COMMIT_ZONE_PADDING_LEFT / _RIGHT
    constexpr float COMMIT_ZONE_LINE_WIDTH = 2.f;
    constexpr float COMMIT_NODE_DIAMETER = 22.f;
    constexpr float COMMIT_MERGE_NODE_DIAMETER = 12.f;
    constexpr float COMMIT_ZONE_EDGE_ARC_RADIUS = 11.f;
    constexpr float REF_ZONE_TEXT_HEIGHT = 22.f; // ref node height (GRAPH_ROW_INNER_HEIGHT)
    constexpr float AVATAR_DIAMETER = 22.f;
    constexpr float COMMIT_ZONE_AVATAR_DIAMETER = 18.f; // node diameter - 2 * line width
    constexpr float LEFT_PANEL_DEFAULT_WIDTH = 208.f; // including its 1px border
    constexpr float DETAIL_PANEL_DEFAULT_WIDTH = 438.f; // including its 1px border
    constexpr float LEFT_PANEL_ROW_HEIGHT = 24.f;
    constexpr float LEFT_PANEL_HEADER_HEIGHT = 60.f; // --left-panel-header-height
    constexpr float INFO_BAR_HEIGHT = 26.f; // --info-bar-height (status bar)
    constexpr float MENU_BAR_HEIGHT = 26.f; // the native Windows menu bar
    constexpr float TERMINAL_DEFAULT_HEIGHT = 275.f;
    constexpr float FONT_SIZE_ROW = 12.f; // graph rows, left panel

    constexpr float FONT_SIZE_NORMAL = 14.f; // --fs-3
    constexpr float FONT_SIZE_SMALL = 12.f; // --fs-2
    constexpr float FONT_SIZE_TINY = 11.f; // section headers
    constexpr float FONT_SIZE_LARGE = 16.f; // --fs-4
} // namespace imgit::theme
