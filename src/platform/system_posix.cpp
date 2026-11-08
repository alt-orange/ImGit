#include "platform/process.hpp"
#include "platform/system.hpp"
#include <cstdlib>
#include <iterator>
#include <pwd.h>
#include <span>
#include <unistd.h>
#include <utility>

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

    std::string system_locale()
    {
        // LC_ALL, LC_TIME, then LANG: "de_DE.UTF-8" -> "de_DE".
        for (const char* name : {"LC_ALL", "LC_TIME", "LANG"})
        {
            if (const char* value = std::getenv(name); value != nullptr && *value != '\0')
            {
                std::string locale = value;
                locale = locale.substr(0, locale.find_first_of(".@"));
                return locale == "C" || locale == "POSIX" ? std::string() : locale;
            }
        }
        return {};
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

    namespace
    {
        struct MonospaceFont
        {
            std::string name;
            std::filesystem::path file;
        };

        const std::vector<MonospaceFont>& installed_monospace_fonts()
        {
            static const std::vector<MonospaceFont> fonts = []
            {
                std::vector<MonospaceFont> found;
                std::error_code error;
                const std::filesystem::path home = home_directory();
#ifdef __APPLE__
                // The faces macOS ships, then popular ones a user installs (for everyone, or for themselves).
                static constexpr std::pair<const char*, const char*> KNOWN[] = {
                        {"Menlo", "Menlo.ttc"},
                        {"Monaco", "Monaco.ttf"},
                        {"SF Mono", "SFNSMono.ttf"},
                        {"Courier New", "Courier New.ttf"},
                        {"Andale Mono", "Andale Mono.ttf"},
                        {"PT Mono", "PTMono.ttc"},
                        {"Fira Code", "FiraCode-Regular.ttf"},
                        {"Hack", "Hack-Regular.ttf"},
                        {"JetBrains Mono", "JetBrainsMono-Regular.ttf"},
                        {"Source Code Pro", "SourceCodePro-Regular.ttf"},
                };
                const std::filesystem::path folders[] = {"/System/Library/Fonts", "/System/Library/Fonts/Supplemental",
                                                         "/Library/Fonts", home / "Library/Fonts"};
                for (const auto& [name, file] : KNOWN)
                {
                    for (const std::filesystem::path& folder : folders)
                    {
                        if (std::filesystem::path path = folder / file; std::filesystem::exists(path, error))
                        {
                            found.push_back({name, std::move(path)});
                            break;
                        }
                    }
                }
#else
                // Distributions file fonts in different subfolders (truetype/dejavu, TTF, dejavu-sans-mono-fonts...),
                // so the font folders are searched for the files' names.
                static constexpr std::pair<const char*, const char*> KNOWN[] = {
                        {"DejaVu Sans Mono", "DejaVuSansMono.ttf"},
                        {"Liberation Mono", "LiberationMono-Regular.ttf"},
                        {"Noto Sans Mono", "NotoSansMono-Regular.ttf"},
                        {"Ubuntu Mono", "UbuntuMono-R.ttf"},
                        {"Cascadia Code", "CascadiaCode.ttf"},
                        {"Cascadia Mono", "CascadiaMono.ttf"},
                        {"Fira Code", "FiraCode-Regular.ttf"},
                        {"Hack", "Hack-Regular.ttf"},
                        {"JetBrains Mono", "JetBrainsMono-Regular.ttf"},
                        {"Source Code Pro", "SourceCodePro-Regular.ttf"},
                };
                std::vector<std::filesystem::path> files(std::size(KNOWN));
                const std::filesystem::path folders[] = {"/usr/share/fonts", "/usr/local/share/fonts",
                                                         home / ".local/share/fonts", home / ".fonts"};
                for (const std::filesystem::path& folder : folders)
                {
                    for (auto it = std::filesystem::recursive_directory_iterator(
                                 folder, std::filesystem::directory_options::skip_permission_denied, error);
                         !error && it != std::filesystem::recursive_directory_iterator(); it.increment(error))
                    {
                        const std::string file = it->path().filename().string();
                        for (std::size_t i = 0; i < std::size(KNOWN); ++i)
                        {
                            if (files[i].empty() && file == KNOWN[i].second)
                            {
                                files[i] = it->path();
                            }
                        }
                    }
                    error.clear();
                }
                for (std::size_t i = 0; i < std::size(KNOWN); ++i)
                {
                    if (!files[i].empty())
                    {
                        found.push_back({KNOWN[i].first, files[i]});
                    }
                }
#endif
                return found;
            }();
            return fonts;
        }

        // A shell by its file name in the usual folders; empty when it is not installed.
        std::string find_shell(const char* name)
        {
            for (const char* folder : {"/bin/", "/usr/bin/", "/usr/local/bin/", "/opt/homebrew/bin/"})
            {
                std::error_code error;
                if (const std::string path = std::string(folder) + name; std::filesystem::exists(path, error))
                {
                    return path;
                }
            }
            return {};
        }

        constexpr const char* SYSTEM_DEFAULT = "System Default";
        constexpr const char* SHELLS[] = {"bash", "zsh", "fish", "sh"};
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
        std::string shell = "/bin/sh";
        if (const char* user_shell = std::getenv("SHELL"); user_shell != nullptr && *user_shell != '\0')
        {
            shell = user_shell;
        }
        const std::string label = std::filesystem::path(shell).filename().string();
        // A login shell, as terminal emulators start it, so the profile scripts run.
        return {{shell, "-l"}, label};
    }

    std::vector<std::string> terminal_shell_names()
    {
        static const std::vector<std::string> names = []
        {
            std::vector<std::string> found = {SYSTEM_DEFAULT};
            for (const char* shell : SHELLS)
            {
                if (!find_shell(shell).empty())
                {
                    found.emplace_back(shell);
                }
            }
            return found;
        }();
        return names;
    }

    Shell terminal_shell(std::string_view name)
    {
        for (const char* shell : SHELLS)
        {
            if (name == shell)
            {
                if (std::string path = find_shell(shell); !path.empty())
                {
                    return {{std::move(path), "-l"}, shell};
                }
            }
        }
        return default_shell();
    }

    std::string system_proxy()
    {
        return {};
    }
} // namespace imgit::platform
