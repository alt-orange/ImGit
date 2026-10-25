#pragma once

#include "app/profiles.hpp"
#include <imgui.h>
#include <string>

namespace imgit::app
{
    class Application;
}

// What shows a profile outside the Profiles page: its icon and the react-select menu that switches profiles (the tabs
// bar's profile menu and the Preferences sidebar).
namespace imgit::ui
{
    // A profile's picture, `size` device px square at `min`: its author's Gravatar with 2px corners, or the author's
    // initials when there is none (or while it loads).
    void draw_profile_icon(ImDrawList* draw, ImVec2 min, float size, const app::Profile& profile);

    struct ProfileMenuStyle
    {
        const ImVec4& background; // .react-select__menu and its options (a palette colour)
        ImVec4 border; // the menu list's and each option's 1px border
        ImVec4 text;
    };
    // The tabs bar menu's picker (toolbar colours) and the Preferences sidebar's (panel-bg0).
    extern const ProfileMenuStyle POPOVER_PROFILE_MENU;
    extern const ProfileMenuStyle SIDEBAR_PROFILE_MENU;

    // The profile picker's react-select menu for popup `id` (opened by the caller's control at [control_min,
    // control_max]): 8px below the control, a 45px option per profile with its 28px icon. Returns the guid picked,
    // empty when none was.
    [[nodiscard]] std::string profile_select_menu(app::Application& app, const char* id, ImVec2 control_min,
                                                  ImVec2 control_max, const ProfileMenuStyle& style);
    // react-select's focused control: a 1px #2684FF border ring around [min, max].
    void draw_select_focus(ImDrawList* draw, ImVec2 min, ImVec2 max, float rounding);
} // namespace imgit::ui
