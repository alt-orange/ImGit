#include "platform/process.hpp"
#include "platform/system.hpp"
#include <cstdlib>
#include <pwd.h>
#include <span>
#include <unistd.h>

namespace imgit::platform
{
    std::filesystem::path home_directory()
    {
        if (const char* home = std::getenv("HOME"); home != nullptr && *home != '\0')
        {
            return home;
        }
        if (const passwd* user = getpwuid(getuid()); user != nullptr && user->pw_dir != nullptr)
        {
            return user->pw_dir;
        }
        return std::filesystem::current_path();
    }

    std::filesystem::path app_data_directory()
    {
        return home_directory();
    }

    std::filesystem::path config_directory()
    {
        if (const char* xdg = std::getenv("XDG_CONFIG_HOME"); xdg != nullptr && *xdg != '\0')
        {
            return xdg;
        }
        return home_directory() / ".config";
    }

    std::tm local_time(std::time_t t)
    {
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
        std::error_code error;
#ifdef __APPLE__
        // The system faces in the roles Windows fills with Segoe UI, Consolas and Arial, oldest macOS release
        // last: San Francisco (the variable SFNS.ttf, since Big Sur), Menlo and Arial (Supplemental since Catalina).
        static constexpr const char* UI_FACES[] = {"/System/Library/Fonts/SFNS.ttf",
                                                   "/System/Library/Fonts/HelveticaNeue.ttc",
                                                   "/System/Library/Fonts/Helvetica.ttc"};
        static constexpr const char* MONOSPACE_FACES[] = {"/System/Library/Fonts/Menlo.ttc",
                                                          "/System/Library/Fonts/SFNSMono.ttf",
                                                          "/System/Library/Fonts/Monaco.ttf"};
        static constexpr const char* ARIAL_FACES[] = {"/System/Library/Fonts/Supplemental/Arial.ttf",
                                                      "/Library/Fonts/Arial.ttf",
                                                      "/System/Library/Fonts/Helvetica.ttc"};
        const auto candidates = font == SystemFont::MONOSPACE ? std::span<const char* const>(MONOSPACE_FACES)
                                : font == SystemFont::ARIAL   ? std::span<const char* const>(ARIAL_FACES)
                                                              : std::span<const char* const>(UI_FACES);
        for (const char* candidate : candidates)
        {
            if (std::filesystem::exists(candidate, error))
            {
                return candidate;
            }
        }
        return {};
#else
        // fontconfig's choice for the role (Arial usually maps to Liberation Sans or DejaVu Sans).
        const char* pattern = font == SystemFont::MONOSPACE ? "monospace"
                              : font == SystemFont::ARIAL   ? "Arial"
                                                            : "sans-serif";
        const ProcessOutput match = run_process({}, {"fc-match", "--format=%{file}", pattern});
        // TrueType / OpenType outlines only: ImGui's loaders take no Type 1 or bitmap fonts.
        const std::filesystem::path path =
                match.exit_code == 0 ? std::filesystem::path(match.text) : std::filesystem::path();
        const std::string extension = path.extension().string();
        if ((extension == ".ttf" || extension == ".otf" || extension == ".ttc") && std::filesystem::exists(path, error))
        {
            return path;
        }
        return {};
#endif
    }

    Shell default_shell()
    {
        std::string shell = "/bin/sh";
        if (const char* user_shell = std::getenv("SHELL"); user_shell != nullptr && *user_shell != '\0')
        {
            shell = user_shell;
        }
        const std::string label = std::filesystem::path(shell).filename().string();
        // A login shell, as terminal emulators start it, so the profile scripts run.
        return {{shell, "-l"}, label};
    }

    std::string system_proxy()
    {
        return {};
    }
} // namespace imgit::platform
