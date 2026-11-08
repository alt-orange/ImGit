#include "platform/process.hpp"
#include "platform/system.hpp"
#include "platform/win32_util.hpp"
#include <cstdlib>
#include <utility>
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

    std::string system_locale()
    {
        wchar_t name[LOCALE_NAME_MAX_LENGTH] = {};
        if (GetUserDefaultLocaleName(name, LOCALE_NAME_MAX_LENGTH) == 0)
        {
            return {};
        }
        return win32::narrow(name);
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

    namespace
    {
        struct MonospaceFont
        {
            std::string name;
            std::filesystem::path file;
        };

        // The faces Windows ships, then popular ones a user installs (for everyone, or for themselves only).
        const std::vector<MonospaceFont>& installed_monospace_fonts()
        {
            static const std::vector<MonospaceFont> fonts = []
            {
                static constexpr std::pair<const char*, const wchar_t*> KNOWN[] = {
                        {"Cascadia Code", L"CascadiaCode.ttf"},
                        {"Cascadia Mono", L"CascadiaMono.ttf"},
                        {"Consolas", L"consola.ttf"},
                        {"Courier New", L"cour.ttf"},
                        {"Lucida Console", L"lucon.ttf"},
                        {"Fira Code", L"FiraCode-Regular.ttf"},
                        {"Hack", L"Hack-Regular.ttf"},
                        {"JetBrains Mono", L"JetBrainsMono-Regular.ttf"},
                        {"Source Code Pro", L"SourceCodePro-Regular.ttf"},
                };
                std::vector<std::filesystem::path> folders;
                if (wchar_t windows[MAX_PATH] = {}; GetWindowsDirectoryW(windows, MAX_PATH) != 0)
                {
                    folders.push_back(std::filesystem::path(windows) / L"Fonts");
                }
                if (const wchar_t* local = _wgetenv(L"LOCALAPPDATA"); local != nullptr && *local != L'\0')
                {
                    folders.push_back(std::filesystem::path(local) / L"Microsoft" / L"Windows" / L"Fonts");
                }
                std::vector<MonospaceFont> found;
                for (const auto& [name, file] : KNOWN)
                {
                    for (const std::filesystem::path& folder : folders)
                    {
                        std::error_code error;
                        if (std::filesystem::path path = folder / file; std::filesystem::exists(path, error))
                        {
                            found.push_back({name, std::move(path)});
                            break;
                        }
                    }
                }
                return found;
            }();
            return fonts;
        }

        // Git for Windows' bash: next to the git on PATH (<git>/mingw64/libexec/git-core -> <git>/bin/bash.exe),
        // else where its installer puts it. Empty when there is none.
        const std::filesystem::path& git_bash()
        {
            static const std::filesystem::path bash = []
            {
                std::vector<std::filesystem::path> candidates;
                if (const ProcessOutput exec_path = run_process({}, {"git", "--exec-path"}); exec_path.exit_code == 0)
                {
                    std::string text = exec_path.text;
                    while (!text.empty() && (text.back() == '\n' || text.back() == '\r' || text.back() == ' '))
                    {
                        text.pop_back();
                    }
                    const std::filesystem::path root =
                            std::filesystem::path(win32::widen(text)).parent_path().parent_path().parent_path();
                    candidates.push_back(root / L"bin" / L"bash.exe");
                }
                for (const wchar_t* variable : {L"ProgramFiles", L"ProgramW6432", L"LOCALAPPDATA"})
                {
                    if (const wchar_t* folder = _wgetenv(variable); folder != nullptr && *folder != L'\0')
                    {
                        const std::filesystem::path base = std::wstring_view(variable) == L"LOCALAPPDATA"
                                                                   ? std::filesystem::path(folder) / L"Programs"
                                                                   : std::filesystem::path(folder);
                        candidates.push_back(base / L"Git" / L"bin" / L"bash.exe");
                    }
                }
                for (const std::filesystem::path& candidate : candidates)
                {
                    std::error_code error;
                    if (std::filesystem::is_regular_file(candidate, error))
                    {
                        return candidate;
                    }
                }
                return std::filesystem::path();
            }();
            return bash;
        }
    } // namespace

    std::vector<std::string> monospace_font_names()
    {
        std::vector<std::string> names;
        for (const MonospaceFont& font : installed_monospace_fonts())
        {
            names.push_back(font.name);
        }
        return names;
    }

    std::filesystem::path monospace_font_file(std::string_view name)
    {
        for (const MonospaceFont& font : installed_monospace_fonts())
        {
            if (font.name == name)
            {
                return font.file;
            }
        }
        return {};
    }

    Shell default_shell()
    {
        // The default terminal on Windows.
        return {{"powershell.exe", "-NoLogo", "-ExecutionPolicy", "RemoteSigned"}, "powershell.exe"};
    }

    std::vector<std::string> terminal_shell_names()
    {
        std::vector<std::string> names = {"PowerShell", "Command Prompt"};
        if (!git_bash().empty())
        {
            names.emplace_back("Git Bash");
        }
        return names;
    }

    Shell terminal_shell(std::string_view name)
    {
        if (name == "Command Prompt")
        {
            return {{"cmd.exe"}, "cmd.exe"};
        }
        if (name == "Git Bash" && !git_bash().empty())
        {
            // A login shell, as Git Bash's own shortcut starts it, so ~/.bash_profile runs.
            return {{win32::narrow(git_bash().c_str()), "--login", "-i"}, "bash"};
        }
        return default_shell();
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
