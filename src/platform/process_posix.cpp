#include "platform/process.hpp"
#include <array>
#include <cerrno>
#include <climits>
#include <csignal>
#include <cstdint>
#include <cstring>
#include <fcntl.h>
#include <poll.h>
#include <spawn.h>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif

extern char** environ;

namespace imgit::platform
{
    namespace
    {
        // `environ` with `environment` applied, as the "NAME=value" strings posix_spawn takes.
        std::vector<std::string> environment_strings(const Environment& environment)
        {
            std::vector<std::string> result;
            for (char** entry = environ; *entry != nullptr; ++entry)
            {
                const std::string_view text(*entry);
                const std::string_view name = text.substr(0, text.find('='));
                bool replaced = false;
                for (const auto& [override_name, value] : environment)
                {
                    replaced |= name == override_name;
                }
                if (!replaced)
                {
                    result.emplace_back(text);
                }
            }
            for (const auto& [name, value] : environment)
            {
                result.push_back(name + "=" + value);
            }
            return result;
        }

        std::vector<char*> pointers(std::vector<std::string>& strings)
        {
            std::vector<char*> result;
            for (std::string& text : strings)
            {
                result.push_back(text.data());
            }
            result.push_back(nullptr);
            return result;
        }

        int wait_for(pid_t pid)
        {
            int status = 0;
            while (waitpid(pid, &status, 0) < 0)
            {
                if (errno != EINTR)
                {
                    return -1;
                }
            }
            return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
        }

        // A pipe with both ends close-on-exec from the start: a child spawned meanwhile from another thread, or by
        // the child itself (a daemonized `git gc --auto`), must not keep an end open, or the read loop never sees
        // EOF. The child's own copies come from dup2 onto 0 / 1 / 2, which clears the flag.
        bool open_pipe(int (&fd)[2])
        {
#ifdef __linux__
            return pipe2(fd, O_CLOEXEC) == 0;
#else
            if (pipe(fd) != 0)
            {
                return false;
            }
            fcntl(fd[0], F_SETFD, FD_CLOEXEC);
            fcntl(fd[1], F_SETFD, FD_CLOEXEC);
            return true;
#endif
        }

        // posix_spawnp in `workdir` with the given stdin / stdout / stderr descriptors (-1 leaves one inherited);
        // 0 when it could not start.
        pid_t spawn(const std::string& workdir, std::vector<std::string> args, const Environment& environment, int in,
                    int out, const std::vector<int>& close_in_child)
        {
            posix_spawn_file_actions_t actions;
            posix_spawn_file_actions_init(&actions);
            if (in >= 0)
            {
                posix_spawn_file_actions_adddup2(&actions, in, STDIN_FILENO);
            }
            if (out >= 0)
            {
                posix_spawn_file_actions_adddup2(&actions, out, STDOUT_FILENO);
                posix_spawn_file_actions_adddup2(&actions, out, STDERR_FILENO);
            }
            for (const int fd : close_in_child)
            {
                posix_spawn_file_actions_addclose(&actions, fd);
            }
            if (!workdir.empty())
            {
                posix_spawn_file_actions_addchdir_np(&actions, workdir.c_str());
            }
            std::vector<std::string> variables = environment_strings(environment);
            std::vector<char*> argv = pointers(args);
            std::vector<char*> envp = pointers(variables);
            pid_t pid = 0;
            const int error = posix_spawnp(&pid, argv[0], &actions, nullptr, argv.data(), envp.data());
            posix_spawn_file_actions_destroy(&actions);
            return error == 0 ? pid : 0;
        }
    } // namespace

    ProcessOutput run_process(const std::string& workdir, const std::vector<std::string>& args,
                              const std::string& input, const Environment& environment, const OutputSink& on_output)
    {
        // A GUI app has no use for SIGPIPE's default action (ending the process): a child that exits before
        // reading all of its input makes write() fail with EPIPE instead.
        [[maybe_unused]] static const bool sigpipe_ignored = []
        {
            std::signal(SIGPIPE, SIG_IGN);
            return true;
        }();
        ProcessOutput result;
        if (args.empty())
        {
            return result;
        }
        int output[2] = {-1, -1};
        int feed[2] = {-1, -1};
        if (!open_pipe(output))
        {
            return result;
        }
        if (!open_pipe(feed))
        {
            close(output[0]);
            close(output[1]);
            return result;
        }
        // Our end of the stdin pipe is non-blocking: a blocking write of more than the pipe holds returns only once
        // the child has read all of it, even after poll() said there was room, so the loop below could not read.
        fcntl(feed[1], F_SETFL, O_NONBLOCK);
        const pid_t pid = spawn(workdir, args, environment, feed[0], output[1], {output[0], feed[1]});
        close(output[1]);
        close(feed[0]);
        if (pid == 0)
        {
            close(output[0]);
            close(feed[1]);
            return result;
        }
        result.started = true;
        // The input is fed while the output is drained: writing it all first deadlocks on a child that fills the
        // output pipe before it has read its input, and a child that exits early leaves the rest unread (EPIPE).
        std::size_t written = 0;
        std::array<char, 4096> buffer{};
        while (output[0] >= 0 || feed[1] >= 0)
        {
            if (feed[1] >= 0 && written == input.size())
            {
                close(feed[1]);
                feed[1] = -1;
            }
            pollfd fds[2] = {{output[0], POLLIN, 0}, {feed[1], POLLOUT, 0}}; // a -1 descriptor is ignored
            if (poll(fds, 2, -1) < 0)
            {
                if (errno == EINTR)
                {
                    continue;
                }
                break;
            }
            if (fds[1].revents != 0)
            {
                const ssize_t count = write(feed[1], input.data() + written, input.size() - written);
                if (count > 0)
                {
                    written += static_cast<std::size_t>(count);
                }
                else if (count < 0 && errno != EINTR && errno != EAGAIN)
                {
                    close(feed[1]);
                    feed[1] = -1;
                }
            }
            if (fds[0].revents != 0)
            {
                const ssize_t count = read(output[0], buffer.data(), buffer.size());
                if (count > 0)
                {
                    result.text.append(buffer.data(), static_cast<std::size_t>(count));
                    if (on_output)
                    {
                        on_output(std::string_view(buffer.data(), static_cast<std::size_t>(count)));
                    }
                }
                else if (count == 0 || (errno != EINTR && errno != EAGAIN))
                {
                    close(output[0]);
                    output[0] = -1;
                }
            }
        }
        if (feed[1] >= 0)
        {
            close(feed[1]);
        }
        if (output[0] >= 0)
        {
            close(output[0]);
        }
        result.exit_code = wait_for(pid);
        return result;
    }

    bool spawn_detached(const std::vector<std::string>& args, const std::string& workdir)
    {
        if (args.empty())
        {
            return false;
        }
        const pid_t pid = spawn(workdir, args, {}, -1, -1, {});
        if (pid == 0)
        {
            return false;
        }
        // Reap it when it ends so it does not stay a zombie.
        std::thread(
                [pid]
                {
                    wait_for(pid);
                })
                .detach();
        return true;
    }

    std::string executable_path()
    {
#ifdef __APPLE__
        std::uint32_t size = 0;
        _NSGetExecutablePath(nullptr, &size);
        std::string path(size, '\0');
        if (_NSGetExecutablePath(path.data(), &size) != 0)
        {
            return {};
        }
        path.resize(std::strlen(path.c_str()));
        return path;
#else
        std::array<char, PATH_MAX> path{};
        const ssize_t length = readlink("/proc/self/exe", path.data(), path.size() - 1);
        return length > 0 ? std::string(path.data(), static_cast<std::size_t>(length)) : std::string();
#endif
    }
} // namespace imgit::platform
