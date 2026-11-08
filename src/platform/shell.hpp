#pragma once

#include <optional>
#include <string>
#include <vector>

// Opening things outside the app: the "Open in File Manager", "Open External Terminal", "Open Repo in
// External Editor", links, and the system's file dialogs.
namespace imgit::platform::shell
{
    void open_in_file_manager(const std::string& path);
    void open_external_terminal(const std::string& path);
    // Tries `code`, then the system default for folders.
    void open_in_editor(const std::string& path);
    void open_url(const std::string& url);
    // Starts `args` (args[0] looked up on PATH; on Windows also a .cmd / .bat script, as `code` is) in `workdir`
    // without waiting; false when it could not be started. `hide_console`: a script's console window stays hidden.
    bool launch(const std::vector<std::string>& args, const std::string& workdir = {}, bool hide_console = false);
    // Opens a file with its default program / shows it selected in the file manager.
    void open_path(const std::string& path);
    void show_in_folder(const std::string& path);
    // Native file dialogs (nativefiledialog-extended) filtered to `extension` (any file when empty); nullopt when
    // cancelled.
    [[nodiscard]] std::optional<std::string> save_file_dialog(const std::string& default_name,
                                                              const std::string& extension);
    [[nodiscard]] std::optional<std::string> open_file_dialog(const std::string& extension);
    // A folder picker that starts in `start` when it exists.
    [[nodiscard]] std::optional<std::string> pick_folder_dialog(const std::string& start = {});
} // namespace imgit::platform::shell
