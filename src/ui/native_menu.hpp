#pragma once

#include <functional>
#include <imgui.h>
#include <optional>
#include <string>
#include <vector>

// Context menus and the menu bar in the style of Electron's (Chromium views) menus with the Windows 11 dark
// look, drawn inside the ImGui window with the same geometry, colours,
// shadow and behaviour (hover, delayed submenus, keyboard navigation, clicks outside closing the menu and then
// reaching what is under the mouse).
namespace imgit::ui
{
    struct MenuItem
    {
        enum class Type
        {
            NORMAL,
            CHECKBOX,
            SEPARATOR,
        };
        Type type = Type::NORMAL;
        std::string label; // '&' marks the mnemonic in menu bar menus
        std::string accelerator; // right-aligned shortcut text
        bool enabled = true;
        bool checked = false;
        std::function<void()> click;
        std::vector<MenuItem> submenu;

        [[nodiscard]] static MenuItem separator()
        {
            MenuItem item;
            item.type = Type::SEPARATOR;
            return item;
        }
    };
    using MenuItems = std::vector<MenuItem>;

    [[nodiscard]] MenuItem menu_item(std::string label, std::function<void()> click, bool enabled = true);
    [[nodiscard]] MenuItem menu_checkbox(std::string label, bool checked, std::function<void()> click,
                                         bool enabled = true);
    [[nodiscard]] MenuItem menu_submenu(std::string label, MenuItems submenu);

    // The non-empty groups joined by separators.
    [[nodiscard]] MenuItems separated_menu(std::vector<MenuItems> groups);

    // Shows `items` as a context menu with its top-left corner at `pos` (the mouse when omitted), replacing any
    // open menu. Clicks run after the menu has closed.
    void popup_menu(MenuItems items, std::optional<ImVec2> pos = std::nullopt);
    // A menu bar menu, dropped down from the bar button [button_min, button_max]. `keyboard` opens it with the
    // first item selected and mnemonics underlined (Alt / F10 navigation).
    void popup_bar_menu(MenuItems items, int bar_index, ImVec2 button_min, ImVec2 button_max, bool keyboard);
    void close_menus();
    // The menu bar's entries: `build(i)` makes entry i's menu, `rect(i, min, max)` gives its button. Lets an open
    // bar menu switch to another entry on hover and with the Left / Right keys.
    void set_menu_bar(int count, std::function<MenuItems(int)> build, std::function<bool(int, ImVec2*, ImVec2*)> rect);

    [[nodiscard]] bool menu_open() noexcept;
    // The menu bar entry whose menu is showing, -1 when none (or a context menu is showing).
    [[nodiscard]] int open_bar_menu() noexcept;
    // Whether the open bar menu was opened from the keyboard (the bar then underlines mnemonics).
    [[nodiscard]] bool menu_keyboard_mode() noexcept;

    // Draws the open menus and handles their input. Call once per frame, after everything else.
    void draw_menus();
    // Labels without the '&' mnemonic markers ("&&" stays a literal '&').
    [[nodiscard]] std::string strip_mnemonic(const std::string& label);
} // namespace imgit::ui
