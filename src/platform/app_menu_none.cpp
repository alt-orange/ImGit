#include "platform/app_menu.hpp"

// Windows and Linux: the app draws the Windows 11 style menu bar itself.
namespace imgit::platform
{
    bool has_native_menu_bar()
    {
        return false;
    }

    void set_native_menu_bar(std::vector<std::string>, std::function<AppMenu(int)>)
    {
    }

    std::vector<std::function<void()>> take_native_menu_clicks()
    {
        return {};
    }
} // namespace imgit::platform
