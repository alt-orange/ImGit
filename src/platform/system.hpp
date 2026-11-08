#pragma once

#include <ctime>
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

// Facts about the user's system that differ per platform.
namespace imgit::platform
{
    // The user's home folder (USERPROFILE on Windows, HOME elsewhere).
    [[nodiscard]] std::filesystem::path home_directory();

    // Where ImGit keeps its `.imgit` folder: %APPDATA% on Windows, the home folder elsewhere.
    [[nodiscard]] std::filesystem::path app_data_directory();

    // The per-user configuration folder: %APPDATA% on Windows, $XDG_CONFIG_HOME (or ~/.config) elsewhere.
    [[nodiscard]] std::filesystem::path config_directory();

    // `t` as local time.
    [[nodiscard]] std::tm local_time(std::time_t t);

    // The user's locale for dates, as "en-us" / "de-de" (empty when unknown).
    [[nodiscard]] std::string system_locale();

    // Seconds a hovered submenu waits before it opens (the system's menu show delay).
    [[nodiscard]] float menu_show_delay();

    enum class SystemFont
    {
        UI, // the native menus' face (Segoe UI)
        MONOSPACE, // the diff and the terminal (Consolas)
        ARIAL,
    };
    // The font file for `font`, empty when the system has none.
    [[nodiscard]] std::filesystem::path system_font_file(SystemFont font);

    // The monospace faces installed on this system that Preferences → Editor / In-App Terminal offer, by name
    // ("Consolas", "Menlo", "DejaVu Sans Mono"...). Looked up once.
    [[nodiscard]] std::vector<std::string> monospace_font_names();
    // The file of one of those faces, empty when it is unknown or not installed.
    [[nodiscard]] std::filesystem::path monospace_font_file(std::string_view name);

    struct Shell
    {
        std::vector<std::string> args;
        std::string label; // the terminal tab's title
    };
    // The interactive shell the terminal panel runs.
    [[nodiscard]] Shell default_shell();
    // The shells the terminal panel can run, by name ("PowerShell", "Git Bash", "zsh"...). Looked up once.
    [[nodiscard]] std::vector<std::string> terminal_shell_names();
    // The shell for one of those names; default_shell() for an empty or unknown one.
    [[nodiscard]] Shell terminal_shell(std::string_view name);

    // The proxy the system routes HTTPS through ("host:port"), empty for a direct connection. Environment proxies
    // (HTTPS_PROXY, ...) are honoured by the HTTP client itself.
    [[nodiscard]] std::string system_proxy();
} // namespace imgit::platform
