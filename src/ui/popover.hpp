#pragma once

#include <imgui.h>
#include <string>

// The HTML dropdowns (bootstrap .dropdown-menu): a toolbar-bg2 box with a
// `0 6px 12px rgba(0,0,0,.176)` shadow, an optional focused search box, and 27px rows of 12px text that tint on
// hover (hover-row) or mark the current entry in green.
namespace imgit::ui
{
    // CSS box-shadow (outer): a Gaussian of `blur` px around [min, max] moved by `offset`.
    void draw_box_shadow(ImDrawList* draw, ImVec2 min, ImVec2 max, ImVec2 offset, float blur, float spread,
                         ImVec4 color, float rounding = 0.f);

    void open_popover(const char* id);
    // Begins popover `id` at `pos` (screen px) `width` wide when it is open; the content height is measured on
    // the previous frame. Closes on a click outside or Escape. Call end_popover() only when this returns true.
    bool begin_popover(const char* id, ImVec2 pos, float width);
    void end_popover();
    void close_popover();

    // The search box: a 34px field with the magnifier, focused when the popover opens. Returns true when the
    // text changed.
    bool popover_search(std::string& text, const char* placeholder = "Search");
    // li.dropdown-header: 12px secondary text after a 12px icon ("far:clock").
    void popover_header(const char* text, const char* icon = nullptr);
    // li > a: returns true when clicked. `current` marks the checked-out branch / open repository.
    bool popover_row(const char* text, const char* icon = nullptr, bool current = false);
    // A row whose label is a btn-link ("View all repositories").
    bool popover_link_row(const char* text);
    // A wrapped 12px secondary paragraph with 10px padding (the pull dropdown's explanation).
    void popover_note(const char* text);
    // The pull dropdown's rows: 14px circle / dot-circle, 12px text, 5px 10px padding; `selected` in green.
    bool popover_radio_row(const char* text, bool selected);
    // Empty space in the layout (the .dropdown-menu's 5px top / bottom padding).
    void popover_padding(float height);
    // li.lfs-option > a: 12px text with 6px 12px padding (29.14px rows), tinted on hover.
    bool popover_menu_item(const char* text);
    // Rows the keyboard selects with Up / Down; Enter clicks the selected one.
    void popover_reset_keyboard();
} // namespace imgit::ui
