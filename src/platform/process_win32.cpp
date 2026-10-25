#include "platform/process.hpp"
#include "platform/win32_util.hpp"
#include <array>
#include <cwctype>
#include <vector>

namespace imgit::platform
{
    namespace
    {
        bool same_name(std::wstring_view a, std::wstring_view b)
        {
            if (a.size() != b.size())
            {
                return false;
            }
            for (std::size_t i = 0; i < a.size(); ++i)
            {
                if (std::towupper(a[i]) != std::towupper(b[i]))
                {
                    return false;
                }
            }
            return true;
        }

        // The inherited environment block with `environment` applied (names are case-insensitive on Windows).
        std::wstring environment_block(const Environment& environment)
        {
            std::vector<std::wstring> names;
            for (const auto& [name, value] : environment)
            {
                names.push_back(win32::widen(name));
            }
            std::wstring block;
            if (wchar_t* strings = GetEnvironmentStringsW(); strings != nullptr)
            {
                for (const wchar_t* entry = strings; *entry != L'\0'; entry += std::wcslen(entry) + 1)
                {
                    const std::wstring_view text(entry);
                    // "=C:=C:\..." entries (per-drive directories) start with '='.
                    const std::size_t equals = text.find(L'=', 1);
                    const std::wstring_view name = text.substr(0, equals);
                    bool replaced = false;
                    for (const std::wstring& override_name : names)
                    {
                        replaced |= same_name(name, override_name);
                    }
                    if (!replaced)
                    {
                        block.append(text);
                        block += L'\0';
                    }
                }
                FreeEnvironmentStringsW(strings);
            }
            for (std::size_t i = 0; i < environment.size(); ++i)
            {
                block += names[i] + L"=" + win32::widen(environment[i].second);
                block += L'\0';
            }
            block += L'\0';
            return block;
        }
    } // namespace

    ProcessOutput run_process(const std::string& workdir, const std::vector<std::string>& args,
                              const std::string& input, const Environment& environment, const OutputSink& on_output)
    {
        ProcessOutput result;
        SECURITY_ATTRIBUTES sa{};
        sa.nLength = sizeof(sa);
        sa.bInheritHandle = TRUE;
        HANDLE read_pipe = nullptr;
        HANDLE write_pipe = nullptr;
        if (!CreatePipe(&read_pipe, &write_pipe, &sa, 0))
        {
            return result;
        }
        SetHandleInformation(read_pipe, HANDLE_FLAG_INHERIT, 0);
        HANDLE input_read = nullptr;
        HANDLE input_write = nullptr;
        if (!input.empty())
        {
            if (!CreatePipe(&input_read, &input_write, &sa, 0))
            {
                CloseHandle(read_pipe);
                CloseHandle(write_pipe);
                return result;
            }
            SetHandleInformation(input_write, HANDLE_FLAG_INHERIT, 0);
        }

        STARTUPINFOEXW si{};
        si.StartupInfo.cb = sizeof(si);
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdOutput = write_pipe;
        si.StartupInfo.hStdError = write_pipe;
        si.StartupInfo.hStdInput = input_read;
        PROCESS_INFORMATION pi{};

        // The child inherits only its own pipe ends. With bInheritHandles alone it would get every inheritable
        // handle of this process, among them the pipes of a run_process on another thread (auto-fetch next to a
        // user's fetch), whose read loop would then not see EOF until this child's tree has exited.
        HANDLE inherited[2] = {write_pipe, input_read};
        const SIZE_T inherited_count = input_read != nullptr ? 2 : 1;
        SIZE_T list_size = 0;
        InitializeProcThreadAttributeList(nullptr, 1, 0, &list_size);
        std::vector<unsigned char> list(list_size);
        si.lpAttributeList = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(list.data());
        const bool initialised = InitializeProcThreadAttributeList(si.lpAttributeList, 1, 0, &list_size) != FALSE;
        BOOL ok = FALSE;
        if (initialised)
        {
            ok = UpdateProcThreadAttribute(si.lpAttributeList, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, inherited,
                                           inherited_count * sizeof(HANDLE), nullptr, nullptr);
        }
        if (ok)
        {
            std::wstring command = win32::command_line(args);
            const std::wstring directory = win32::widen(workdir);
            std::wstring block = environment.empty() ? std::wstring() : environment_block(environment);
            ok = CreateProcessW(nullptr, command.data(), nullptr, nullptr, TRUE,
                                CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                                block.empty() ? nullptr : block.data(), directory.empty() ? nullptr : directory.c_str(),
                                &si.StartupInfo, &pi);
        }
        if (initialised)
        {
            DeleteProcThreadAttributeList(si.lpAttributeList);
        }
        CloseHandle(write_pipe);
        if (input_read != nullptr)
        {
            CloseHandle(input_read);
        }
        if (!ok)
        {
            CloseHandle(read_pipe);
            if (input_write != nullptr)
            {
                CloseHandle(input_write);
            }
            return result;
        }
        result.started = true;
        // The commands fed this way (rev-list --stdin) read all of their input before writing anything.
        if (input_write != nullptr)
        {
            DWORD written = 0;
            WriteFile(input_write, input.data(), static_cast<DWORD>(input.size()), &written, nullptr);
            CloseHandle(input_write);
        }

        std::array<char, 4096> buffer{};
        DWORD read = 0;
        while (ReadFile(read_pipe, buffer.data(), static_cast<DWORD>(buffer.size()), &read, nullptr) && read > 0)
        {
            result.text.append(buffer.data(), read);
            if (on_output)
            {
                on_output(std::string_view(buffer.data(), read));
            }
        }
        CloseHandle(read_pipe);
        WaitForSingleObject(pi.hProcess, INFINITE);
        DWORD exit_code = 0;
        GetExitCodeProcess(pi.hProcess, &exit_code);
        result.exit_code = static_cast<int>(exit_code);
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);
        return result;
    }

    bool spawn_detached(const std::vector<std::string>& args, const std::string& workdir)
    {
        if (args.empty())
        {
            return false;
        }
        std::wstring command = win32::command_line(args);
        const std::wstring directory = win32::widen(workdir);
        STARTUPINFOW startup{};
        startup.cb = sizeof(startup);
        PROCESS_INFORMATION process{};
        if (!CreateProcessW(nullptr, command.data(), nullptr, nullptr, FALSE, CREATE_NEW_CONSOLE, nullptr,
                            directory.empty() ? nullptr : directory.c_str(), &startup, &process))
        {
            return false;
        }
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
        return true;
    }

    std::string executable_path()
    {
        std::wstring path(MAX_PATH, L'\0');
        while (true)
        {
            const DWORD length = GetModuleFileNameW(nullptr, path.data(), static_cast<DWORD>(path.size()));
            if (length < path.size())
            {
                path.resize(length);
                return win32::narrow(path.c_str());
            }
            path.resize(path.size() * 2);
        }
    }
} // namespace imgit::platform
