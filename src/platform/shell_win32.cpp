#include "platform/shell.hpp"
#include "platform/win32_util.hpp"
// shellapi.h needs windows.h first; the blank line keeps clang-format from reordering them.

#include <shellapi.h>

namespace imgit::platform::shell
{
    namespace
    {
        // Backslashes, without a trailing one: the shell rejects "C:/repo/".
        std::wstring native_path(const std::string& path)
        {
            std::wstring native = win32::widen(path);
            for (wchar_t& c : native)
            {
                if (c == L'/')
                {
                    c = L'\\';
                }
            }
            while (native.size() > 3 && native.back() == L'\\')
            {
                native.pop_back();
            }
            return native;
        }
    } // namespace

    void open_in_file_manager(const std::string& path)
    {
        ShellExecuteW(nullptr, L"explore", win32::widen(path).c_str(), nullptr, nullptr, SW_SHOWNORMAL);
    }

    void open_external_terminal(const std::string& path)
    {
        ShellExecuteW(nullptr, L"open", L"powershell.exe", nullptr, win32::widen(path).c_str(), SW_SHOWNORMAL);
    }

    void open_in_editor(const std::string& path)
    {
        const auto result = reinterpret_cast<INT_PTR>(ShellExecuteW(
                nullptr, L"open", L"code", (L"\"" + win32::widen(path) + L"\"").c_str(), nullptr, SW_HIDE));
        if (result <= 32)
        {
            ShellExecuteW(nullptr, L"open", win32::widen(path).c_str(), nullptr, nullptr, SW_SHOWNORMAL);
        }
    }

    void open_url(const std::string& url)
    {
        ShellExecuteW(nullptr, L"open", win32::widen(url).c_str(), nullptr, nullptr, SW_SHOWNORMAL);
    }

    void open_path(const std::string& path)
    {
        ShellExecuteW(nullptr, L"open", win32::widen(path).c_str(), nullptr, nullptr, SW_SHOWNORMAL);
    }

    void show_in_folder(const std::string& path)
    {
        ShellExecuteW(nullptr, L"open", L"explorer.exe", (L"/select,\"" + native_path(path) + L"\"").c_str(), nullptr,
                      SW_SHOWNORMAL);
    }
} // namespace imgit::platform::shell
