#pragma once

#include <functional>
#include <optional>
#include <string>

// Opening things outside the app: the "Open in File Manager", "Open External Terminal", "Open Repo in
// External Editor", links, and the system's file dialogs.
namespace imgit::platform::shell
{
    void open_in_file_manager(const std::string& path);
    void open_external_terminal(const std::string& path);
    // Tries `code`, then the system default for folders.
    void open_in_editor(const std::string& path);
    void open_url(const std::string& url);
    // Opens a file with its default program / shows it selected in the file manager.
    void open_path(const std::string& path);
    void show_in_folder(const std::string& path);
    // Native file dialogs (nativefiledialog-extended) filtered to `extension` (any file when empty).
    //
    // Save: the path to write, nullopt when cancelled. In the browser it is a path in the page's storage at once,
    // and the file is handed to the browser as a download once the app has written it.
    [[nodiscard]] std::optional<std::string> save_file_dialog(const std::string& default_name,
                                                              const std::string& extension);

    // Open: `on_picked` gets the chosen path, and is not called when the dialog is cancelled. The desktop's dialogs
    // are modal, so it runs before this returns; in the browser it runs on a later frame, with the file copied
    // from the user's disk into the page's storage.
    void open_file_dialog(const std::string& extension, std::function<void(const std::string&)> on_picked);

    enum class FolderPurpose
    {
        OPEN, // an existing folder to open (in the browser: copied in from the user's disk)
        LOCATION, // where to put something new (in the browser: a path in the page's storage)
    };
    // A folder picker that starts in `start` when it exists; `on_picked` as for open_file_dialog.
    void pick_folder_dialog(const std::string& start, FolderPurpose purpose,
                            std::function<void(const std::string&)> on_picked);
} // namespace imgit::platform::shell
