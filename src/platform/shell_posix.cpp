#include "platform/process.hpp"
#include "platform/shell.hpp"
#include <cstdlib>
#include <filesystem>

namespace imgit::platform::shell
{
    namespace
    {
        // The desktop's "open this" command.
        void open_with_desktop(const std::string& target)
        {
#ifdef __APPLE__
            spawn_detached({"open", target});
#else
            spawn_detached({"xdg-open", target});
#endif
        }
    } // namespace

    void open_in_file_manager(const std::string& path)
    {
        open_with_desktop(path);
    }

    void open_external_terminal(const std::string& path)
    {
#ifdef __APPLE__
        spawn_detached({"open", "-a", "Terminal", path});
#else
        // $TERMINAL first, then the Debian alternative and the two big desktops' terminals.
        if (const char* terminal = std::getenv("TERMINAL");
            terminal != nullptr && *terminal != '\0' && spawn_detached({terminal}, path))
        {
            return;
        }
        for (const char* terminal : {"x-terminal-emulator", "gnome-terminal", "konsole", "xterm"})
        {
            if (spawn_detached({terminal}, path))
            {
                return;
            }
        }
#endif
    }

    void open_in_editor(const std::string& path)
    {
        if (!spawn_detached({"code", path}))
        {
            open_with_desktop(path);
        }
    }

    void open_url(const std::string& url)
    {
        open_with_desktop(url);
    }

    void open_path(const std::string& path)
    {
        open_with_desktop(path);
    }

    void show_in_folder(const std::string& path)
    {
#ifdef __APPLE__
        spawn_detached({"open", "-R", path});
#else
        // File managers differ in how they select an item; open the containing folder.
        open_with_desktop(std::filesystem::path(path).parent_path().string());
#endif
    }
} // namespace imgit::platform::shell
