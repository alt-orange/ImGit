#include "platform/shell.hpp"
#include <cstdlib>
#include <emscripten/emscripten.h>

// JavaScript helpers, at global scope as EM_JS requires.
EM_JS_DEPS(imgit_shell, "$UTF8ToString,$stringToNewUTF8");
EM_JS(void, imgit_open_in_new_tab, (const char* url), { window.open(UTF8ToString(url), '_blank', 'noopener'); });

// The browser's prompt() as the folder picker: the page cannot browse its own virtual file system. A string from
// malloc, null when cancelled.
EM_JS(char*, imgit_prompt_for_path, (const char* message, const char* start), {
    const path = window.prompt(UTF8ToString(message), UTF8ToString(start));
    return path === null ? 0 : stringToNewUTF8(path);
});

namespace imgit::platform::shell
{
    // The page has no file manager, terminal or editor to hand anything to.
    void open_in_file_manager(const std::string&)
    {
    }

    void open_external_terminal(const std::string&)
    {
    }

    void open_in_editor(const std::string&)
    {
    }

    void open_url(const std::string& url)
    {
        imgit_open_in_new_tab(url.c_str());
    }

    void open_path(const std::string&)
    {
    }

    void show_in_folder(const std::string&)
    {
    }

    // Files picked or saved here would live in the page's own storage, out of the user's reach: no dialogs.
    std::optional<std::string> save_file_dialog(const std::string&, const std::string&)
    {
        return std::nullopt;
    }

    std::optional<std::string> open_file_dialog(const std::string&)
    {
        return std::nullopt;
    }

    std::optional<std::string> pick_folder_dialog(const std::string& start)
    {
        char* picked =
                imgit_prompt_for_path("Path of a folder in the browser's storage (the home folder is /home/web_user):",
                                      start.empty() ? "/home/web_user/" : start.c_str());
        if (picked == nullptr)
        {
            return std::nullopt;
        }
        std::string path = picked;
        std::free(picked);
        if (path.empty())
        {
            return std::nullopt;
        }
        return path;
    }
} // namespace imgit::platform::shell
