#include "platform/system.hpp"
#include <cstdlib>

// The browser: a virtual file system whose home folder (/home/web_user) res/web/pre.js keeps in IndexedDB.
namespace imgit::platform
{
    std::filesystem::path home_directory()
    {
        if (const char* home = std::getenv("HOME"); home != nullptr && *home != '\0')
        {
            return home;
        }
        return "/home/web_user";
    }

    std::filesystem::path app_data_directory()
    {
        return home_directory();
    }

    std::filesystem::path config_directory()
    {
        return home_directory() / ".config";
    }

    std::tm local_time(std::time_t t)
    {
        // The browser's time zone.
        std::tm local{};
        localtime_r(&t, &local);
        return local;
    }

    float menu_show_delay()
    {
        return 0.4f; // Windows' default
    }

    std::filesystem::path system_font_file(SystemFont font)
    {
        // Only the monospace face is linked in (--embed-file in CMakeLists.txt); the others fall back to Open Sans.
        if (font == SystemFont::MONOSPACE)
        {
            return "/fonts/DejaVuSansMono.ttf";
        }
        return {};
    }

    Shell default_shell()
    {
        return {{}, "Terminal"};
    }

    std::string system_proxy()
    {
        return {}; // the browser's own
    }
} // namespace imgit::platform
