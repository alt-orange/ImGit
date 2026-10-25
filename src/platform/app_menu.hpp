#pragma once

#include <functional>
#include <string>
#include <vector>

// The application menu (File / Edit / View / Help) where the platform shows it itself: the macOS menu bar.
// Elsewhere the app draws its own bar (Application::draw_menu_bar) and these do nothing.
namespace imgit::platform
{
    struct AppMenuItem
    {
        enum class Type
        {
            NORMAL,
            CHECKBOX,
            SEPARATOR,
        };
        Type type = Type::NORMAL;
        std::string label; // without mnemonic markers
        std::string accelerator; // the shortcut as shown ("Cmd+Shift+T"), displayed next to the item
        bool enabled = true;
        bool checked = false;
        std::function<void()> click;
        std::vector<AppMenuItem> submenu;
    };
    using AppMenu = std::vector<AppMenuItem>;

    // True where the platform's own menu bar carries the application menu.
    [[nodiscard]] bool has_native_menu_bar();

    // Installs one top-level menu per title; `build(i)` is called each time menu i is about to open, so its
    // items reflect the current state. Empty `titles` removes the menus. The shortcuts shown are not acted on by
    // the menus: the keystrokes still reach the window and the app's own shortcut handling.
    void set_native_menu_bar(std::vector<std::string> titles, std::function<AppMenu(int)> build);

    // The clicks made in the native menus since the last call, for the UI thread to run during its frame.
    [[nodiscard]] std::vector<std::function<void()>> take_native_menu_clicks();
} // namespace imgit::platform
