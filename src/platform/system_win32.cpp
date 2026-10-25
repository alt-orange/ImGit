#include "platform/system.hpp"
#include "platform/win32_util.hpp"
#include <cstdlib>
// winhttp.h needs windows.h first; the blank line keeps clang-format from reordering them.

#include <winhttp.h>

namespace imgit::platform
{
    std::filesystem::path home_directory()
    {
        for (const char* name : {"USERPROFILE", "HOME"})
        {
            if (const char* value = std::getenv(name); value != nullptr && *value != '\0')
            {
                return value;
            }
        }
        return std::filesystem::current_path();
    }

    std::filesystem::path app_data_directory()
    {
        // The variable rather than the known folder, so a test run can point it elsewhere.
        if (const char* appdata = std::getenv("APPDATA"); appdata != nullptr && *appdata != '\0')
        {
            return appdata;
        }
        return home_directory();
    }

    std::filesystem::path config_directory()
    {
        return app_data_directory();
    }

    std::tm local_time(std::time_t t)
    {
        std::tm local{};
        localtime_s(&local, &t);
        return local;
    }

    float menu_show_delay()
    {
        static const float delay = []
        {
            DWORD ms = 400;
            SystemParametersInfoW(SPI_GETMENUSHOWDELAY, 0, &ms, 0);
            return static_cast<float>(ms) / 1000.f;
        }();
        return delay;
    }

    std::filesystem::path system_font_file(SystemFont font)
    {
        wchar_t windows[MAX_PATH] = {};
        if (GetWindowsDirectoryW(windows, MAX_PATH) == 0)
        {
            return {};
        }
        const wchar_t* file = font == SystemFont::UI          ? L"segoeui.ttf"
                              : font == SystemFont::MONOSPACE ? L"consola.ttf"
                                                              : L"arial.ttf";
        std::filesystem::path path = std::filesystem::path(windows) / L"Fonts" / file;
        std::error_code error;
        return std::filesystem::exists(path, error) ? path : std::filesystem::path();
    }

    Shell default_shell()
    {
        // The default terminal on Windows.
        return {{"powershell.exe", "-NoLogo", "-ExecutionPolicy", "RemoteSigned"}, "powershell.exe"};
    }

    std::string system_proxy()
    {
        // The Internet Options proxy, which URLDownloadToFile (and Chromium) use. A PAC script is not evaluated.
        WINHTTP_CURRENT_USER_IE_PROXY_CONFIG config{};
        if (!WinHttpGetIEProxyConfigForCurrentUser(&config))
        {
            return {};
        }
        std::string proxy = config.lpszProxy != nullptr ? win32::narrow(config.lpszProxy) : std::string();
        for (wchar_t* text : {config.lpszProxy, config.lpszProxyBypass, config.lpszAutoConfigUrl})
        {
            if (text != nullptr)
            {
                GlobalFree(text);
            }
        }
        // "host:port", or per scheme: "http=host:port;https=host:port".
        if (proxy.find('=') == std::string::npos)
        {
            return proxy;
        }
        std::string fallback;
        for (std::string_view rest = proxy; !rest.empty();)
        {
            const std::size_t end = rest.find_first_of("; ");
            const std::string_view entry = rest.substr(0, end);
            rest = end == std::string_view::npos ? std::string_view() : rest.substr(end + 1);
            if (entry.starts_with("https="))
            {
                return std::string(entry.substr(6));
            }
            if (entry.starts_with("http=") && fallback.empty())
            {
                fallback = entry.substr(5);
            }
        }
        return fallback;
    }
} // namespace imgit::platform
