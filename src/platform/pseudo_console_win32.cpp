#include "platform/pseudo_console.hpp"
#include "platform/win32_util.hpp"
#include <algorithm>
#include <atomic>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

namespace imgit::platform
{
    namespace
    {
        // ConPTY (Windows 10 1809+), looked up at run time so older SDK headers and systems still work.
        using PseudoConsoleHandle = void*;
        using CreateFn = HRESULT(WINAPI*)(COORD, HANDLE, HANDLE, DWORD, PseudoConsoleHandle*);
        using ResizeFn = HRESULT(WINAPI*)(PseudoConsoleHandle, COORD);
        using CloseFn = void(WINAPI*)(PseudoConsoleHandle);
        constexpr DWORD_PTR ATTRIBUTE_PSEUDOCONSOLE = 0x00020016; // PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE

        struct Api
        {
            CreateFn create = nullptr;
            ResizeFn resize = nullptr;
            CloseFn close = nullptr;
        };

        const Api& api()
        {
            static const Api loaded = []
            {
                Api result;
                if (HMODULE kernel = GetModuleHandleW(L"kernel32.dll"); kernel != nullptr)
                {
                    result.create = reinterpret_cast<CreateFn>(GetProcAddress(kernel, "CreatePseudoConsole"));
                    result.resize = reinterpret_cast<ResizeFn>(GetProcAddress(kernel, "ResizePseudoConsole"));
                    result.close = reinterpret_cast<CloseFn>(GetProcAddress(kernel, "ClosePseudoConsole"));
                }
                return result;
            }();
            return loaded;
        }

        COORD size_of(int columns, int rows)
        {
            return COORD{static_cast<SHORT>(std::max(1, columns)), static_cast<SHORT>(std::max(1, rows))};
        }
    } // namespace

    struct PseudoConsole::Impl
    {
        PseudoConsoleHandle console = nullptr;
        HANDLE input = nullptr; // our end of the console's input pipe
        HANDLE output = nullptr; // our end of the console's output pipe
        PROCESS_INFORMATION process{};
        std::thread reader;
        std::mutex mutex;
        std::string pending;
        std::atomic<bool> drained{false};

        ~Impl()
        {
            // Closing the console ends its clients and its output pipe, which ends the reader.
            if (console != nullptr)
            {
                api().close(console);
            }
            if (input != nullptr)
            {
                CloseHandle(input);
            }
            if (reader.joinable())
            {
                reader.join();
            }
            if (output != nullptr)
            {
                CloseHandle(output);
            }
            if (process.hProcess != nullptr)
            {
                CloseHandle(process.hProcess);
                CloseHandle(process.hThread);
            }
        }
    };

    PseudoConsole::PseudoConsole() = default;
    PseudoConsole::~PseudoConsole() = default;

    bool PseudoConsole::start(const std::vector<std::string>& args, const std::string& workdir, int columns, int rows)
    {
        if (api().create == nullptr || m_impl)
        {
            return false;
        }
        auto impl = std::make_unique<Impl>();
        HANDLE console_input = nullptr;
        HANDLE console_output = nullptr;
        if (!CreatePipe(&console_input, &impl->input, nullptr, 0))
        {
            return false;
        }
        if (!CreatePipe(&impl->output, &console_output, nullptr, 0))
        {
            CloseHandle(console_input);
            return false;
        }
        const HRESULT created = api().create(size_of(columns, rows), console_input, console_output, 0, &impl->console);
        if (FAILED(created))
        {
            impl->console = nullptr;
            CloseHandle(console_input);
            CloseHandle(console_output);
            return false;
        }

        STARTUPINFOEXW startup{};
        startup.StartupInfo.cb = sizeof(startup);
        // No std handles of ours: the child talks to the pseudo console even when ImGit's own are redirected.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        SIZE_T list_size = 0;
        InitializeProcThreadAttributeList(nullptr, 1, 0, &list_size);
        std::vector<unsigned char> list(list_size);
        startup.lpAttributeList = reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(list.data());
        const bool initialised = InitializeProcThreadAttributeList(startup.lpAttributeList, 1, 0, &list_size) != FALSE;
        bool launched = initialised
                        && UpdateProcThreadAttribute(startup.lpAttributeList, 0, ATTRIBUTE_PSEUDOCONSOLE, impl->console,
                                                     sizeof(impl->console), nullptr, nullptr);
        if (launched)
        {
            std::wstring command = win32::command_line(args);
            const std::wstring directory = win32::widen(workdir);
            launched = CreateProcessW(nullptr, command.data(), nullptr, nullptr, FALSE,
                                      EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT, nullptr,
                                      directory.empty() ? nullptr : directory.c_str(), &startup.StartupInfo,
                                      &impl->process)
                       != FALSE;
        }
        if (initialised)
        {
            DeleteProcThreadAttributeList(startup.lpAttributeList);
        }
        // The console holds its own references to the pipe ends it was given.
        CloseHandle(console_input);
        CloseHandle(console_output);
        if (!launched)
        {
            impl->process = {};
            return false;
        }

        Impl* raw = impl.get();
        impl->reader = std::thread(
                [raw]
                {
                    char buffer[8192];
                    DWORD count = 0;
                    while (ReadFile(raw->output, buffer, sizeof(buffer), &count, nullptr) && count > 0)
                    {
                        const std::lock_guard lock(raw->mutex);
                        raw->pending.append(buffer, count);
                    }
                    raw->drained = true;
                });
        m_impl = std::move(impl);
        return true;
    }

    void PseudoConsole::write(std::string_view bytes)
    {
        if (!m_impl || bytes.empty())
        {
            return;
        }
        DWORD written = 0;
        WriteFile(m_impl->input, bytes.data(), static_cast<DWORD>(bytes.size()), &written, nullptr);
    }

    void PseudoConsole::resize(int columns, int rows)
    {
        if (m_impl && api().resize != nullptr)
        {
            api().resize(m_impl->console, size_of(columns, rows));
        }
    }

    std::string PseudoConsole::read()
    {
        if (!m_impl)
        {
            return {};
        }
        const std::lock_guard lock(m_impl->mutex);
        return std::exchange(m_impl->pending, {});
    }

    bool PseudoConsole::exited() const
    {
        if (!m_impl)
        {
            return true;
        }
        if (WaitForSingleObject(m_impl->process.hProcess, 0) != WAIT_OBJECT_0)
        {
            return false;
        }
        const std::lock_guard lock(m_impl->mutex);
        return m_impl->pending.empty();
    }
} // namespace imgit::platform
