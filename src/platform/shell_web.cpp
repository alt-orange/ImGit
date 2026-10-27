#include "platform/shell.hpp"
#include <chrono>
#include <cstdlib>
#include <emscripten/emscripten.h>
#include <filesystem>
#include <map>
#include <mutex>
#include <vector>

// JavaScript helpers, at global scope as EM_JS requires.
EM_JS_DEPS(imgit_shell, "$UTF8ToString,$stringToNewUTF8,$FS");
EM_JS(void, imgit_open_in_new_tab, (const char* url), { window.open(UTF8ToString(url), '_blank', 'noopener'); });

// The browser's prompt() for a path in the page's storage. A string from malloc, null when cancelled.
EM_JS(char*, imgit_prompt_for_path, (const char* message, const char* start), {
    const path = window.prompt(UTF8ToString(message), UTF8ToString(start));
    return path === null ? 0 : stringToNewUTF8(path);
});

// A file input: the picked file (or, with `folder`, the folder with everything in it, .git included) is copied into
// the page's storage, and imgit_dialog_done(request, path) reports where (path 0: cancelled). The page's click
// that led here still counts as the user's gesture the browser wants for a file input.
EM_JS(void, imgit_pick_files, (int request, const char* accept, int folder), {
    var input = document.createElement('input');
    input.type = 'file';
    input.style.display = 'none';
    if (folder) {
        input.webkitdirectory = true;
        input.multiple = true;
    }
    var types = UTF8ToString(accept);
    if (types) {
        input.accept = types;
    }
    document.body.appendChild(input);
    var done = false;
    function finish(path) {
        if (done) {
            return;
        }
        done = true;
        input.remove();
        _imgit_dialog_done(request, path === null ? 0 : stringToNewUTF8(path));
    }
    function unused(path) {
        var candidate = path;
        for (var n = 2; FS.analyzePath(candidate).exists; ++n) {
            candidate = path + ' (' + n + ')';
        }
        return candidate;
    }
    input.addEventListener('cancel', function () { finish(null); });
    input.addEventListener('change', function () {
        var files = Array.prototype.slice.call(input.files || []);
        if (files.length === 0) {
            finish(null);
            return;
        }
        Promise.all(files.map(function (file) {
            return file.arrayBuffer().then(function (data) { return { file: file, data: new Uint8Array(data) }; });
        })).then(function (loaded) {
            var target;
            if (folder) {
                // Into the home folder (kept in IndexedDB), under the folder's own name.
                var top = (loaded[0].file.webkitRelativePath || loaded[0].file.name).split('/')[0];
                target = unused('/home/web_user/' + top);
                loaded.forEach(function (entry) {
                    var relative = (entry.file.webkitRelativePath || entry.file.name).split('/').slice(1).join('/');
                    var path = target + '/' + relative;
                    FS.mkdirTree(path.substring(0, path.lastIndexOf('/')));
                    FS.writeFile(path, entry.data);
                });
                // The browser leaves empty folders out; a repository needs these even when they are empty.
                if (FS.analyzePath(target + '/.git/HEAD').exists) {
                    ['objects/pack', 'objects/info', 'refs/heads', 'refs/tags'].forEach(function (dir) {
                        FS.mkdirTree(target + '/.git/' + dir);
                    });
                }
            } else {
                FS.mkdirTree('/tmp/imgit-uploads');
                target = unused('/tmp/imgit-uploads/' + loaded[0].file.name);
                FS.writeFile(target, loaded[0].data);
            }
            finish(target);
        }).catch(function (error) {
            console.error('ImGit: the picked files could not be read', error);
            finish(null);
        });
    });
    input.click();
});

// Hands the file at `path` to the browser as a download named `name`.
EM_JS(void, imgit_download_file, (const char* path, const char* name), {
    var data = new Uint8Array(FS.readFile(UTF8ToString(path)));
    var link = document.createElement('a');
    link.href = URL.createObjectURL(new Blob([data], { type: 'application/octet-stream' }));
    link.download = UTF8ToString(name);
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(function () { URL.revokeObjectURL(link.href); }, 60000);
});

namespace imgit::platform::shell
{
    namespace
    {
        using Callback = std::function<void(const std::string&)>;

        struct Dialogs
        {
            std::mutex mutex;
            int next_request = 1;
            std::map<int, Callback> waiting;
            std::vector<std::pair<Callback, std::string>> answered;
        };

        Dialogs& dialogs()
        {
            static Dialogs instance;
            return instance;
        }

        struct Download
        {
            std::filesystem::path path;
            std::string name;
            std::chrono::steady_clock::time_point deadline;
        };
        std::vector<Download> g_downloads;

        void pick_files(const std::string& accept, bool folder, Callback on_picked)
        {
            Dialogs& state = dialogs();
            int request = 0;
            {
                const std::lock_guard lock(state.mutex);
                request = state.next_request++;
                state.waiting[request] = std::move(on_picked);
            }
            imgit_pick_files(request, accept.c_str(), folder ? 1 : 0);
        }
    } // namespace

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

    std::optional<std::string> save_file_dialog(const std::string& default_name, const std::string& extension)
    {
        std::string name = default_name.empty() ? "download" : default_name;
        if (!extension.empty() && !name.ends_with("." + extension))
        {
            name += "." + extension;
        }
        const std::filesystem::path folder = "/tmp/imgit-downloads";
        std::error_code error;
        std::filesystem::create_directories(folder, error);
        const std::filesystem::path path = folder / name;
        std::filesystem::remove(path, error); // its appearance is the sign the app wrote it
        g_downloads.push_back({path, name, std::chrono::steady_clock::now() + std::chrono::seconds(30)});
        return path.string();
    }

    void open_file_dialog(const std::string& extension, Callback on_picked)
    {
        pick_files(extension.empty() || extension == "exe" ? std::string() : "." + extension, false,
                   std::move(on_picked));
    }

    void pick_folder_dialog(const std::string& start, FolderPurpose purpose, Callback on_picked)
    {
        if (purpose == FolderPurpose::OPEN)
        {
            pick_files({}, true, std::move(on_picked));
            return;
        }
        char* picked =
                imgit_prompt_for_path("Path of a folder in the browser's storage (the home folder is /home/web_user):",
                                      start.empty() ? "/home/web_user/" : start.c_str());
        if (picked == nullptr)
        {
            return;
        }
        const std::string path = picked;
        std::free(picked);
        if (!path.empty())
        {
            on_picked(path);
        }
    }
} // namespace imgit::platform::shell

// Called by imgit_pick_files when the user is done; the answer runs during the next frame.
extern "C" EMSCRIPTEN_KEEPALIVE void imgit_dialog_done(int request, char* path)
{
    using namespace imgit::platform::shell;
    Dialogs& state = dialogs();
    const std::lock_guard lock(state.mutex);
    const auto found = state.waiting.find(request);
    if (found != state.waiting.end())
    {
        if (path != nullptr)
        {
            state.answered.emplace_back(std::move(found->second), path);
        }
        state.waiting.erase(found);
    }
    std::free(path);
}

namespace imgit::platform::detail
{
    // main_loop_web.cpp, before each frame: the dialogs' answers.
    void run_dialog_answers()
    {
        std::vector<std::pair<shell::Callback, std::string>> answered;
        {
            shell::Dialogs& state = shell::dialogs();
            const std::lock_guard lock(state.mutex);
            answered.swap(state.answered);
        }
        for (const auto& [callback, path] : answered)
        {
            callback(path);
        }
    }

    // main_loop_web.cpp, after each frame: the files saved for download that the app has written by now.
    void deliver_downloads()
    {
        const auto now = std::chrono::steady_clock::now();
        std::erase_if(shell::g_downloads,
                      [&](const shell::Download& download)
                      {
                          std::error_code error;
                          if (std::filesystem::exists(download.path, error))
                          {
                              imgit_download_file(download.path.c_str(), download.name.c_str());
                              std::filesystem::remove(download.path, error);
                              return true;
                          }
                          return now > download.deadline;
                      });
    }
} // namespace imgit::platform::detail
