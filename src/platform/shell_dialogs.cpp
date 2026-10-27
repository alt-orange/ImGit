#include "platform/shell.hpp"
#include "platform/window.hpp"
#include <clocale>
#include <filesystem>
#include <nfd.h>

namespace imgit::platform::shell
{
    namespace
    {
        nfdwindowhandle_t parent()
        {
            const NativeWindow native = native_window(main_window());
            switch (native.kind)
            {
            case NativeWindow::Kind::WINDOWS_HWND:
                return {NFD_WINDOW_HANDLE_TYPE_WINDOWS, native.handle};
            case NativeWindow::Kind::COCOA_NSWINDOW:
                return {NFD_WINDOW_HANDLE_TYPE_COCOA, native.handle};
            case NativeWindow::Kind::X11_WINDOW:
                return {NFD_WINDOW_HANDLE_TYPE_X11, native.handle};
            default:
                return {NFD_WINDOW_HANDLE_TYPE_UNSET, nullptr};
            }
        }

        // Runs `show` between NFD_Init / NFD_Quit (COM on Windows, GTK on Linux) and takes the path it returns.
        template<class Show>
        std::optional<std::string> run_dialog(Show&& show)
        {
            if (NFD_Init() != NFD_OKAY)
            {
                return std::nullopt;
            }
            // The GTK backend's NFD_Init calls setlocale(LC_ALL, ""), after which std::atof / std::strtof /
            // std::to_string elsewhere in the app would read and print decimals with the user's separator.
            std::setlocale(LC_NUMERIC, "C");
            std::optional<std::string> result;
            nfdu8char_t* path = nullptr;
            if (show(&path) == NFD_OKAY && path != nullptr)
            {
                result = path;
                NFD_FreePathU8(path);
            }
            NFD_Quit();
            return result;
        }
    } // namespace

    std::optional<std::string> save_file_dialog(const std::string& default_name, const std::string& extension)
    {
        const std::string name = extension + " files";
        const nfdu8filteritem_t filter{name.c_str(), extension.c_str()};
        return run_dialog(
                [&](nfdu8char_t** path)
                {
                    nfdsavedialogu8args_t args{};
                    args.filterList = extension.empty() ? nullptr : &filter;
                    args.filterCount = extension.empty() ? 0 : 1;
                    args.defaultName = default_name.empty() ? nullptr : default_name.c_str();
                    args.parentWindow = parent();
                    return NFD_SaveDialogU8_With(path, &args);
                });
    }

    void open_file_dialog(const std::string& extension, std::function<void(const std::string&)> on_picked)
    {
        const std::string name = extension + " files";
        const nfdu8filteritem_t filter{name.c_str(), extension.c_str()};
        const std::optional<std::string> picked = run_dialog(
                [&](nfdu8char_t** path)
                {
                    nfdopendialogu8args_t args{};
                    args.filterList = extension.empty() ? nullptr : &filter;
                    args.filterCount = extension.empty() ? 0 : 1;
                    args.parentWindow = parent();
                    return NFD_OpenDialogU8_With(path, &args);
                });
        if (picked)
        {
            on_picked(*picked);
        }
    }

    void pick_folder_dialog(const std::string& start, FolderPurpose, std::function<void(const std::string&)> on_picked)
    {
        std::error_code error;
        // The shell rejects "C:/repo/": native separators, no trailing one.
        std::string folder;
        std::filesystem::path path = std::filesystem::path(std::u8string(start.begin(), start.end())).make_preferred();
        if (!start.empty() && std::filesystem::is_directory(path, error))
        {
            if (!path.has_filename() && path.has_parent_path() && path != path.root_path())
            {
                path = path.parent_path();
            }
            const std::u8string text = path.u8string();
            folder.assign(text.begin(), text.end());
        }
        const std::optional<std::string> chosen = run_dialog(
                [&](nfdu8char_t** picked)
                {
                    nfdpickfolderu8args_t args{};
                    args.defaultPath = folder.empty() ? nullptr : folder.c_str();
                    args.parentWindow = parent();
                    return NFD_PickFolderU8_With(picked, &args);
                });
        if (chosen)
        {
            on_picked(*chosen);
        }
    }
} // namespace imgit::platform::shell
