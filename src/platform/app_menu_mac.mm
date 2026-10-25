// The macOS menu bar: ImGit's menus as NSMenus between the application menu and the Window menu GLFW creates.
#import <Cocoa/Cocoa.h>
#include "platform/app_menu.hpp"
#include <cctype>
#include <string>
#include <utility>
#include <vector>

namespace
{
    constexpr NSInteger ACTIONS_PER_ENTRY = 4096; // NSMenuItem.tag = entry * ACTIONS_PER_ENTRY + action

    std::function<imgit::platform::AppMenu(int)> g_build;
    // The click of every item in each top-level menu as last built, by the item's tag.
    std::vector<std::vector<std::function<void()>>> g_actions;
    std::vector<std::function<void()>> g_clicks; // picked in the menus, waiting for the UI thread's next frame
    NSMutableArray<NSMenuItem*>* g_entries = nil; // our top-level items in the main menu
    id g_delegate = nil;

    // "Cmd+Shift+T", "Option+Cmd+O", "Cmd+Page Down", "F5" as a key equivalent; false when it has no key.
    bool key_equivalent(const std::string& accelerator, NSString** key, NSEventModifierFlags* modifiers)
    {
        std::vector<std::string> tokens;
        std::string current;
        for (const char c : accelerator)
        {
            if (c == '+' && !current.empty())
            {
                tokens.push_back(current);
                current.clear();
            }
            else if (c != ' ' || !current.empty())
            {
                current += c;
            }
        }
        if (!current.empty())
        {
            tokens.push_back(current);
        }
        if (tokens.empty())
        {
            return false;
        }
        *modifiers = 0;
        for (std::size_t i = 0; i + 1 < tokens.size(); ++i)
        {
            const std::string& token = tokens[i];
            if (token == "Cmd" || token == "Ctrl")
            {
                *modifiers |= NSEventModifierFlagCommand;
            }
            else if (token == "Shift")
            {
                *modifiers |= NSEventModifierFlagShift;
            }
            else if (token == "Option" || token == "Alt")
            {
                *modifiers |= NSEventModifierFlagOption;
            }
            else if (token == "Control")
            {
                *modifiers |= NSEventModifierFlagControl;
            }
        }
        std::string name = tokens.back();
        while (!name.empty() && name.back() == ' ')
        {
            name.pop_back();
        }
        unichar character = 0;
        if (name.size() == 1)
        {
            character = static_cast<unichar>(std::tolower(static_cast<unsigned char>(name[0])));
        }
        else if (name == "Tab")
        {
            character = '\t';
        }
        else if (name == "Enter" || name == "Return")
        {
            character = '\r';
        }
        else if (name == "Esc" || name == "Escape")
        {
            character = 0x1B;
        }
        else if (name == "Space")
        {
            character = ' ';
        }
        else if (name == "Comma")
        {
            character = ',';
        }
        else if (name == "Backspace")
        {
            character = 0x08;
        }
        else if (name == "Delete" || name == "Del")
        {
            character = NSDeleteFunctionKey;
        }
        else if (name == "Page Up" || name == "PageUp")
        {
            character = NSPageUpFunctionKey;
        }
        else if (name == "Page Down" || name == "PageDown")
        {
            character = NSPageDownFunctionKey;
        }
        else if (name == "Home")
        {
            character = NSHomeFunctionKey;
        }
        else if (name == "End")
        {
            character = NSEndFunctionKey;
        }
        else if (name == "Up")
        {
            character = NSUpArrowFunctionKey;
        }
        else if (name == "Down")
        {
            character = NSDownArrowFunctionKey;
        }
        else if (name == "Left")
        {
            character = NSLeftArrowFunctionKey;
        }
        else if (name == "Right")
        {
            character = NSRightArrowFunctionKey;
        }
        else if (name.size() >= 2 && name.size() <= 3 && name[0] == 'F')
        {
            const int number = std::atoi(name.c_str() + 1);
            if (number < 1 || number > 12)
            {
                return false;
            }
            character = static_cast<unichar>(NSF1FunctionKey + number - 1);
        }
        else
        {
            return false;
        }
        *key = [NSString stringWithCharacters:&character length:1];
        return true;
    }

    void fill(NSMenu* menu, const imgit::platform::AppMenu& items, NSInteger entry,
              std::vector<std::function<void()>>& actions);
} // namespace

@interface ImGitMenuDelegate : NSObject <NSMenuDelegate>
@end

@implementation ImGitMenuDelegate

- (void)menuNeedsUpdate:(NSMenu*)menu
{
    // A top-level menu is rebuilt (with its submenus) each time it opens; a submenu opening changes nothing.
    if (!g_build)
    {
        return;
    }
    for (NSUInteger i = 0; i < g_entries.count; ++i)
    {
        if (g_entries[i].submenu == menu)
        {
            const imgit::platform::AppMenu items = g_build(static_cast<int>(i));
            g_actions[i].clear();
            fill(menu, items, static_cast<NSInteger>(i), g_actions[i]);
            return;
        }
    }
}

- (BOOL)menuHasKeyEquivalent:(NSMenu*)menu
                    forEvent:(NSEvent*)event
                      target:(id*)target
                      action:(SEL*)action
{
    // The key equivalents are shown, not acted on: the keystrokes reach the window, where ImGit's own shortcut
    // handling (which knows about focused fields and open dialogs) runs the same commands.
    return NO;
}

- (void)activate:(NSMenuItem*)sender
{
    const NSInteger entry = sender.tag / ACTIONS_PER_ENTRY;
    const NSInteger index = sender.tag % ACTIONS_PER_ENTRY;
    if (entry >= 0 && static_cast<std::size_t>(entry) < g_actions.size() && index >= 0
        && static_cast<std::size_t>(index) < g_actions[entry].size() && g_actions[entry][index])
    {
        g_clicks.push_back(g_actions[entry][index]);
    }
}

@end

namespace
{
    void fill(NSMenu* menu, const imgit::platform::AppMenu& items, NSInteger entry,
              std::vector<std::function<void()>>& actions)
    {
        [menu removeAllItems];
        menu.autoenablesItems = NO;
        menu.delegate = g_delegate;
        for (const imgit::platform::AppMenuItem& item : items)
        {
            if (item.type == imgit::platform::AppMenuItem::Type::SEPARATOR)
            {
                [menu addItem:[NSMenuItem separatorItem]];
                continue;
            }
            NSString* title = [NSString stringWithUTF8String:item.label.c_str()];
            NSMenuItem* menu_item = [[[NSMenuItem alloc] initWithTitle:title
                                                                action:@selector(activate:)
                                                         keyEquivalent:@""] autorelease];
            NSString* key = nil;
            NSEventModifierFlags modifiers = 0;
            if (key_equivalent(item.accelerator, &key, &modifiers))
            {
                menu_item.keyEquivalent = key;
                menu_item.keyEquivalentModifierMask = modifiers;
            }
            menu_item.target = g_delegate;
            menu_item.enabled = item.enabled ? YES : NO;
            menu_item.state = item.checked ? NSControlStateValueOn : NSControlStateValueOff;
            menu_item.tag = entry * ACTIONS_PER_ENTRY + static_cast<NSInteger>(actions.size());
            actions.push_back(item.click);
            if (!item.submenu.empty())
            {
                NSMenu* submenu = [[[NSMenu alloc] initWithTitle:title] autorelease];
                fill(submenu, item.submenu, entry, actions);
                menu_item.submenu = submenu;
            }
            [menu addItem:menu_item];
        }
    }
} // namespace

namespace imgit::platform
{
    bool has_native_menu_bar()
    {
        return true;
    }

    void set_native_menu_bar(std::vector<std::string> titles, std::function<AppMenu(int)> build)
    {
        @autoreleasepool
        {
            NSMenu* main = [NSApp mainMenu];
            if (main == nil)
            {
                return; // GLFW has not made the menu bar (no NSApp yet)
            }
            if (g_delegate == nil)
            {
                g_delegate = [[ImGitMenuDelegate alloc] init];
                g_entries = [[NSMutableArray alloc] init];
            }
            for (NSMenuItem* entry in g_entries)
            {
                if (entry.menu == main)
                {
                    [main removeItem:entry];
                }
            }
            [g_entries removeAllObjects];
            g_actions.clear();
            g_clicks.clear();
            g_build = std::move(build);
            // After the application menu (index 0), before GLFW's Window menu.
            const NSInteger at = main.numberOfItems > 0 ? 1 : 0;
            for (std::size_t i = 0; i < titles.size(); ++i)
            {
                NSString* title = [NSString stringWithUTF8String:titles[i].c_str()];
                NSMenuItem* entry = [[[NSMenuItem alloc] initWithTitle:title action:nil keyEquivalent:@""] autorelease];
                NSMenu* menu = [[[NSMenu alloc] initWithTitle:title] autorelease];
                menu.autoenablesItems = NO;
                menu.delegate = g_delegate;
                entry.submenu = menu;
                [main insertItem:entry atIndex:at + static_cast<NSInteger>(i)];
                [g_entries addObject:entry];
            }
            g_actions.resize(titles.size());
        }
    }

    std::vector<std::function<void()>> take_native_menu_clicks()
    {
        return std::exchange(g_clicks, {});
    }
} // namespace imgit::platform
