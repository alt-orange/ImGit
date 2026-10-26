#include "platform/pseudo_console.hpp"
#include <algorithm>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <csignal>
#include <cstdlib>
#include <fcntl.h>
#include <filesystem>
#include <mutex>
#include <poll.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>
#include <utility>
#ifdef __APPLE__
#include <util.h>
#else
#include <pty.h>
#endif

extern char** environ;

namespace imgit::platform
{
    namespace
    {
        winsize size_of(int columns, int rows)
        {
            winsize size{};
            size.ws_col = static_cast<unsigned short>(std::max(1, columns));
            size.ws_row = static_cast<unsigned short>(std::max(1, rows));
            return size;
        }

        // `program` resolved against PATH the way execvp would (left as is when it holds a slash).
        std::string find_program(const std::string& program)
        {
            const char* path = std::getenv("PATH");
            if (program.find('/') != std::string::npos || path == nullptr)
            {
                return program;
            }
            for (std::string_view rest = path; !rest.empty();)
            {
                const std::size_t colon = rest.find(':');
                const std::string_view dir = rest.substr(0, colon);
                rest = colon == std::string_view::npos ? std::string_view() : rest.substr(colon + 1);
                const std::filesystem::path candidate = std::filesystem::path(dir.empty() ? "." : dir) / program;
                if (access(candidate.c_str(), X_OK) == 0)
                {
                    return candidate.string();
                }
            }
            return program;
        }
    } // namespace

    struct PseudoConsole::Impl
    {
        int master = -1; // our side of the pseudo terminal
        pid_t pid = 0;
        mutable bool reaped = false;
        mutable std::chrono::steady_clock::time_point reaped_at{};
        std::thread reader;
        std::mutex mutex;
        std::string pending;
        std::atomic<bool> stop{false};
        std::atomic<bool> drained{false};

        ~Impl()
        {
            // SIGHUP to the shell's process group, as a terminal emulator closing its window does.
            if (pid > 0 && !reaped)
            {
                kill(-pid, SIGHUP);
            }
            stop = true;
            if (reader.joinable())
            {
                reader.join();
            }
            if (master >= 0)
            {
                close(master);
            }
            if (pid > 0 && !reaped)
            {
                // This runs on the UI thread: a shell that ignores SIGHUP gets about half a second, then SIGKILL.
                int status = 0;
                bool ended = false;
                for (int i = 0; i < 50 && !ended; ++i)
                {
                    const pid_t waited = waitpid(pid, &status, WNOHANG);
                    ended = waited == pid || (waited < 0 && errno != EINTR);
                    if (!ended)
                    {
                        std::this_thread::sleep_for(std::chrono::milliseconds(10));
                    }
                }
                if (!ended)
                {
                    kill(-pid, SIGKILL);
                    waitpid(pid, &status, 0);
                }
            }
        }
    };

    PseudoConsole::PseudoConsole() = default;
    PseudoConsole::~PseudoConsole() = default;

    bool PseudoConsole::start(const std::vector<std::string>& args, const std::string& workdir, int columns, int rows)
    {
        if (args.empty() || m_impl)
        {
            return false;
        }
        // Everything the child needs is prepared before the fork: only async-signal-safe calls run after it.
        const std::string program = find_program(args[0]);
        std::vector<std::string> strings = args;
        std::vector<char*> argv;
        for (std::string& arg : strings)
        {
            argv.push_back(arg.data());
        }
        argv.push_back(nullptr);
        std::vector<std::string> variables;
        for (char** entry = environ; *entry != nullptr; ++entry)
        {
            if (!std::string_view(*entry).starts_with("TERM="))
            {
                variables.emplace_back(*entry);
            }
        }
        variables.emplace_back("TERM=xterm-256color");
        std::vector<char*> envp;
        for (std::string& variable : variables)
        {
            envp.push_back(variable.data());
        }
        envp.push_back(nullptr);
        winsize size = size_of(columns, rows);

        auto impl = std::make_unique<Impl>();
        const pid_t pid = forkpty(&impl->master, nullptr, nullptr, &size);
        if (pid < 0)
        {
            return false;
        }
        if (pid == 0)
        {
            if (!workdir.empty() && chdir(workdir.c_str()) != 0)
            {
                _exit(127);
            }
            execve(program.c_str(), argv.data(), envp.data());
            _exit(127);
        }
        impl->pid = pid;
        // forkpty leaves our side inheritable: every git ImGit spawns afterwards would hold the terminal open.
        fcntl(impl->master, F_SETFD, FD_CLOEXEC);

        Impl* raw = impl.get();
        impl->reader = std::thread(
                [raw]
                {
                    char buffer[8192];
                    while (!raw->stop.load())
                    {
                        pollfd fd{raw->master, POLLIN, 0};
                        const int ready = poll(&fd, 1, 100);
                        if (ready < 0 && errno == EINTR)
                        {
                            continue;
                        }
                        if (ready <= 0)
                        {
                            continue;
                        }
                        const ssize_t count = ::read(raw->master, buffer, sizeof(buffer));
                        if (count < 0 && errno == EINTR)
                        {
                            continue;
                        }
                        // EIO once the last process holding the terminal has ended (Linux), 0 on macOS.
                        if (count <= 0)
                        {
                            break;
                        }
                        const std::lock_guard lock(raw->mutex);
                        raw->pending.append(buffer, static_cast<std::size_t>(count));
                    }
                    raw->drained = true;
                });
        m_impl = std::move(impl);
        return true;
    }

    void PseudoConsole::write(std::string_view bytes)
    {
        if (!m_impl)
        {
            return;
        }
        while (!bytes.empty())
        {
            const ssize_t count = ::write(m_impl->master, bytes.data(), bytes.size());
            if (count < 0 && errno == EINTR)
            {
                continue;
            }
            if (count <= 0)
            {
                return;
            }
            bytes.remove_prefix(static_cast<std::size_t>(count));
        }
    }

    void PseudoConsole::resize(int columns, int rows)
    {
        if (m_impl)
        {
            const winsize size = size_of(columns, rows);
            ioctl(m_impl->master, TIOCSWINSZ, &size);
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
        if (!m_impl->reaped)
        {
            int status = 0;
            if (waitpid(m_impl->pid, &status, WNOHANG) != m_impl->pid)
            {
                return false;
            }
            m_impl->reaped = true;
            m_impl->reaped_at = std::chrono::steady_clock::now();
        }
        // The reader only ends (EIO) once the last process holding the terminal is gone, which a background job
        // the shell left behind (`sleep 1000 &` then `exit`) may never be: shortly after the shell itself has been
        // reaped, its output counts as complete.
        const bool settled = m_impl->drained.load()
                             || std::chrono::steady_clock::now() - m_impl->reaped_at > std::chrono::milliseconds(300);
        if (!settled)
        {
            return false;
        }
        const std::lock_guard lock(m_impl->mutex);
        return m_impl->pending.empty();
    }
} // namespace imgit::platform
