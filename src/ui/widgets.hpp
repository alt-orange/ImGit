#pragma once

#include "git/types.hpp"
#include <cstdint>
#include <imgui.h>
#include <string>

namespace imgit::ui
{
    struct Fonts
    {
        ImFont* regular = nullptr;
        ImFont* semibold = nullptr;
        ImFont* bold = nullptr;
        ImFont* italic = nullptr;
        ImFont* mono = nullptr;
        ImFont* system = nullptr; // Segoe UI: the native Windows menu bar
        ImFont* arial = nullptr; // Bootstrap tooltips ("Helvetica Neue", Helvetica, Arial)
    };

    [[nodiscard]] Fonts& fonts() noexcept;

    // Sizes in this code are CSS font sizes (the em box), as in the stylesheets. ImGui sizes a font by
    // its ascent + descent instead, so a CSS size is multiplied by this per-font factor (1.36 for Open Sans).
    [[nodiscard]] float font_em_scale(const ImFont* font) noexcept;
    // Device pixel ImGui size for a CSS font size.
    [[nodiscard]] float font_px(const ImFont* font, float css_size) noexcept;
    // ImGui::PushFont with a CSS font size.
    void push_font(ImFont* font, float css_size);

    // DPI: logical pixels (the CSS values) to device pixels. The scale includes the user zoom.
    void set_dpi_scale(float scale) noexcept;
    [[nodiscard]] float dpi_scale() noexcept;
    [[nodiscard]] float px(float logical) noexcept;

    // OS cursors ImGui has no shape for (main.cpp shows them): during a drag the system drag-and-drop cursors,
    // MOVE over a drop target and NO_DROP elsewhere; over a resize handle Chromium's col-resize / row-resize. Set
    // it every frame it is wanted.
    enum class DragCursor
    {
        NONE,
        MOVE,
        NO_DROP,
        COL_RESIZE,
        ROW_RESIZE,
    };
    void set_drag_cursor(DragCursor cursor) noexcept;
    // The cursor asked for this frame; resets it for the next one.
    [[nodiscard]] DragCursor take_drag_cursor() noexcept;

    // The CSS transitions (`ease` over `seconds`) between 0 and 1: a change of `on` restarts the
    // transition from the value shown. Keyed by id; call every frame it is needed.
    [[nodiscard]] float smooth(const char* id, bool on, float seconds = 0.25f);
    [[nodiscard]] float smooth_value(const char* id, float target, float seconds = 0.25f);
    [[nodiscard]] float ease_in_out(float t) noexcept;
    // Whether a transition was still under way in the last frame (the main loop then keeps drawing at the display
    // rate instead of waiting for input).
    [[nodiscard]] bool animating() noexcept;

    // Text drawn with `font` at logical `size`, clipped with an ellipsis to `max_width` device px.
    // Returns the width used.
    float draw_text_ellipsis(ImDrawList* draw, ImFont* font, float size, ImVec2 pos, float max_width, ImU32 color,
                             const char* text, const char* text_end = nullptr);
    [[nodiscard]] ImVec2 text_size(ImFont* font, float size, const char* text, const char* text_end = nullptr);
    void draw_text(ImDrawList* draw, ImFont* font, float size, ImVec2 pos, ImU32 color, const char* text,
                   const char* text_end = nullptr);
    void draw_dashed_line(ImDrawList* draw, ImVec2 a, ImVec2 b, ImU32 color, float thickness, float dash = 3.f,
                          float gap = 3.f);

    // The avatar: the author's Gravatar, or a coloured disc with their initials (without one, or until it loads).
    // `rounding` < 0 draws a circle.
    void draw_avatar(ImDrawList* draw, ImVec2 center, float diameter, const git::Author& author, float rounding = -1.f);
    [[nodiscard]] std::string initials(const git::Author& author);
    [[nodiscard]] ImVec4 avatar_color(const std::string& email);

    // A rounded label; `height` is logical. Returns true when clicked. Hover brightens `bg` to `bg_hover`.
    bool pill(const char* id, const char* text, ImVec4 bg, ImVec4 bg_hover, ImVec4 fg, bool bold, float height,
              float max_width = 0.f, ImVec2* out_size = nullptr);
    [[nodiscard]] float pill_width(const char* text, bool bold, float height, float extra = 0.f);

    // Buttons in the button families (border + translucent fill).
    bool primary_button(const char* label, ImVec2 size = {0.f, 0.f}, bool enabled = true);
    bool default_button(const char* label, ImVec2 size = {0.f, 0.f}, bool enabled = true);
    bool danger_button(const char* label, ImVec2 size = {0.f, 0.f}, bool enabled = true);
    bool success_button(const char* label, ImVec2 size = {0.f, 0.f}, bool enabled = true);
    // Transparent button that only highlights on hover.
    bool text_button(const char* label, ImVec2 size = {0.f, 0.f}, bool enabled = true);
    bool link_text(const char* label); // ImGit link colour, underline on hover
    // Two or three options in one rounded frame, like "List | Agents" and "Path | Tree".
    bool segmented(const char* id, const char* const* labels, int count, int* selected, float height = 24.f);

    bool input_text(const char* id, std::string& value, const char* hint = nullptr, ImGuiInputTextFlags flags = 0,
                    float width = -1.f);
    bool input_multiline(const char* id, std::string& value, const char* hint, ImVec2 size);
    bool checkbox(const char* label, bool* value);

    // Uppercase small section header in the secondary colour.
    void section_label(const char* text);
    void secondary_text(const char* text);

    // A resize handle: an invisible hit box over [min, max) that takes no
    // layout space, showing Chromium's col-resize (`columns`) or row-resize cursor while hovered or held.
    struct ResizeHandle
    {
        bool held = false;
        bool activated = false; // pressed this frame
    };
    ResizeHandle resize_handle(const char* id, ImVec2 min, ImVec2 max, bool columns = true);
    // A handle that resizes `size` like re-resizable: the size when it was pressed plus the mouse's travel since
    // (times `direction`, -1 for a handle on the region's left or top edge), clamped to [min_size, max_size].
    // Returns true on the frame it is released after changing the size.
    bool resize_drag(const char* id, ImVec2 min, ImVec2 max, bool columns, float direction, float* size, float min_size,
                     float max_size);
    // resize_drag for a handle over other windows (the panels' child windows): it gets a transparent window of
    // its own, above them.
    bool overlay_resize_drag(const char* id, ImVec2 min, ImVec2 max, bool columns, float direction, float* size,
                             float min_size, float max_size);

    // A horizontal scrollbar over [min, max) for `view` of `content` (any unit, `scroll` in the same): dragging
    // the thumb follows the mouse, a press on the track pages towards it. It takes no layout space; submit it
    // before what it covers so it wins the hover, and draw it with draw_scrollbar_x over them.
    struct ScrollbarX
    {
        float thumb_x = 0.f;
        float thumb_w = 0.f;
        bool hovered = false;
        bool held = false;
    };
    ScrollbarX scrollbar_x(const char* id, ImVec2 min, ImVec2 max, float* scroll, float content, float view);
    // The .scrollbar-outer.horizontal: app__bg0 under a thumb rounded by 5/6 of the thickness.
    void draw_scrollbar_x(ImDrawList* draw, ImVec2 min, ImVec2 max, const ScrollbarX& bar);

    [[nodiscard]] ImVec4 status_color(git::FileStatus status);
    void draw_status_badge(ImDrawList* draw, ImVec2 pos, float size, git::FileStatus status);
    // The file status glyphs: pencil (modified), plus (added), minus (deleted), arrow (renamed).
    void draw_status_icon(ImDrawList* draw, ImVec2 center, float size, git::FileStatus status);

    [[nodiscard]] std::string format_date_time(std::int64_t unix_seconds);
    [[nodiscard]] std::string format_date_long(std::int64_t unix_seconds);
    [[nodiscard]] std::string format_relative(std::int64_t unix_seconds);
    [[nodiscard]] std::string file_name(const std::string& path);
    [[nodiscard]] std::string directory_name(const std::string& path);
    [[nodiscard]] std::string to_upper(std::string text);
    [[nodiscard]] bool contains_ci(const std::string& haystack, const std::string& needle);
    // Subsequence fuzzy match score; < 0 when `needle` does not match.
    [[nodiscard]] int fuzzy_score(const std::string& haystack, const std::string& needle);

    // The (bootstrap) tooltip under the last item: Arial 12px white on black, rad4, a 10x5 arrow, 90%
    // opacity, fading in after a short hover.
    void tooltip(const char* text);
    // The same for an explicit anchor rectangle, `above` placing it over the anchor.
    void styled_tooltip(ImVec2 anchor_min, ImVec2 anchor_max, const char* text, bool above = false);
    // placement="left" (delayShow 250): vertically centred, its arrow 3px left of the anchor.
    void styled_tooltip_left(ImVec2 anchor_min, ImVec2 anchor_max, const char* text);
    // Tooltip for an area drawn without an item (mouse inside [min, max) of the hovered window).
    void tooltip_on_rect(ImVec2 min, ImVec2 max, const char* text, bool above = false);

    // Small vector icons for the toolbar and panels.
    enum class Icon
    {
        PULL,
        PUSH,
        BRANCH,
        STASH,
        POP,
        TAG,
        REMOTE,
        FOLDER,
        LAPTOP,
        CLOSE,
        PLUS,
        MINUS,
        BACK,
        REFRESH,
        CHEVRON_DOWN,
        CHEVRON_RIGHT,
        CHEVRON_LEFT,
        CHEVRON_UP,
        CHECK,
        WARNING,
        COPY,
        UNDO,
        REDO,
        TERMINAL,
        SEARCH,
        GEAR,
        BELL,
        ROCKET,
        TRASH,
        EYE,
        EYE_OFF,
        CLOUD,
        PENCIL,
        SORT,
        EXPAND,
        COLLAPSE,
        LIST,
        COMMIT,
        HEADPHONES,
        PLUG,
        USER,
        ARROW_RIGHT,
        WORKTREE,
        SUBMODULE,
        PATCH,
        ISSUE,
        TEAM,
        LFS,
        SPINNER,
        EXTERNAL,
        FILE,
        MAGIC,
        ROBOT,
        KEYBOARD,
        GLOBE,
        SEARCH_PLUS,
        CHECKED_OUT,
        CARET_DOWN,
        SPARKLES,
        BARS,
        FOLDER_TREE,
        GIFT,
    };
    // Draws the Font Awesome glyph for `icon`; `size` is its em height in device pixels.
    void draw_icon(ImDrawList* draw, Icon icon, ImVec2 center, float size, ImU32 color);
    // Width of `icon` drawn at em height `size` (Font Awesome glyphs are not square).
    [[nodiscard]] float icon_width(Icon icon, float size);
    // Line-drawn icons, for the ones not drawn with Font Awesome (LFS, ...).
    void draw_icon_vector(ImDrawList* draw, Icon icon, ImVec2 center, float size, ImU32 color);
    bool icon_button(const char* id, Icon icon, float size, ImVec4 color, const char* tip = nullptr,
                     bool active = false);
} // namespace imgit::ui
