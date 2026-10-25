#pragma once

#include <ctime>
#include <filesystem>
#include <string>
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

    struct Shell
    {
        std::vector<std::string> args;
        std::string label; // the terminal tab's title
    };
    // The interactive shell the terminal panel runs.
    [[nodiscard]] Shell default_shell();

    // The proxy the system routes HTTPS through ("host:port"), empty for a direct connection. Environment proxies
    // (HTTPS_PROXY, ...) are honoured by the HTTP client itself.
    [[nodiscard]] std::string system_proxy();
} // namespace imgit::platform
