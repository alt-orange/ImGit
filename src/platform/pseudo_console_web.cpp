#include "platform/programs.hpp"
#include "platform/pseudo_console.hpp"
#include "platform/system.hpp"
#include <algorithm>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <thread>

// The browser has no shell to attach, so the terminal panel gets a small one here: line editing with history, a
// few built-in commands (cd, ls, cat, ...) and the app's own programs (git), which run on a worker thread so the
// page keeps drawing while a fetch or clone waits on the network.
namespace imgit::platform
{
    namespace
    {
        constexpr const char* HELP =
                "This shell runs inside the page. Commands:\r\n"
                "  git ...        the browser's git (git help lists what it knows)\r\n"
                "  cd [dir]  pwd  ls [dir]  cat <file>  echo ... [> file]  mkdir <dir>  rm <file>\r\n"
                "  clear  help  exit\r\n";

        // The terminal wants CR LF; programs write LF.
        std::string terminal_text(std::string_view text)
        {
            std::string out;
            out.reserve(text.size());
            for (const char c : text)
            {
                if (c == '\n')
                {
                    out += '\r';
                }
                out += c;
            }
            return out;
        }

        // Splits a command line into words: spaces separate, quotes group, a backslash escapes the next character.
        std::vector<std::string> split_words(const std::string& line)
        {
            std::vector<std::string> words;
            std::string word;
            bool in_word = false;
            char quote = 0;
            for (std::size_t i = 0; i < line.size(); ++i)
            {
                const char c = line[i];
                if (quote != 0)
                {
                    if (c == quote)
                    {
                        quote = 0;
                    }
                    else if (c == '\\' && quote == '"' && i + 1 < line.size())
                    {
                        word += line[++i];
                    }
                    else
                    {
                        word += c;
                    }
                }
                else if (c == '\'' || c == '"')
                {
                    quote = c;
                    in_word = true;
                }
                else if (c == '\\' && i + 1 < line.size())
                {
                    word += line[++i];
                    in_word = true;
                }
                else if (c == ' ' || c == '\t')
                {
                    if (in_word)
                    {
                        words.push_back(word);
                        word.clear();
                        in_word = false;
                    }
                }
                else
                {
                    word += c;
                    in_word = true;
                }
            }
            if (in_word)
            {
                words.push_back(word);
            }
            return words;
        }
    } // namespace

    struct PseudoConsole::Impl
    {
        // Shared with the worker thread of a running command, which may outlive the console.
        struct State
        {
            std::mutex mutex;
            std::string output;
            bool running = false;
        };
        std::shared_ptr<State> state = std::make_shared<State>();
        std::filesystem::path cwd;
        std::string line;
        std::vector<std::string> history;
        std::size_t history_at = 0;
        int escape = 0; // 1 after ESC, 2 after ESC [
        bool exited = false;
        std::string typed; // keys not yet handled: typed ahead while a command runs (there is no job control)

        bool running() const
        {
            const std::lock_guard lock(state->mutex);
            return state->running;
        }

        // Handles the typed keys up to the next command that starts running.
        void take_typed()
        {
            std::size_t used = 0;
            while (used < typed.size() && !running())
            {
                key(typed[used++]);
            }
            typed.erase(0, used);
        }

        void emit(const std::string& text) const
        {
            const std::lock_guard lock(state->mutex);
            state->output += text;
        }

        std::string prompt() const
        {
            std::string shown = cwd.string();
            const std::string home = home_directory().string();
            if (shown.starts_with(home))
            {
                shown = "~" + shown.substr(home.size());
            }
            return "\x1b[32m" + shown + "\x1b[0m$ ";
        }

        std::filesystem::path resolve(const std::string& path) const
        {
            if (path == "~" || path.empty())
            {
                return home_directory();
            }
            if (path.starts_with("~/"))
            {
                return (home_directory() / path.substr(2)).lexically_normal();
            }
            const std::filesystem::path given(path);
            return (given.is_absolute() ? given : cwd / given).lexically_normal();
        }

        // Runs a built-in; false when `words` is not one.
        bool builtin(const std::vector<std::string>& words)
        {
            const std::string& name = words.front();
            const std::string argument = words.size() > 1 ? words[1] : std::string();
            std::error_code error;
            if (name == "help")
            {
                emit(HELP);
            }
            else if (name == "pwd")
            {
                emit(cwd.string() + "\r\n");
            }
            else if (name == "cd")
            {
                const std::filesystem::path target = resolve(argument);
                if (std::filesystem::is_directory(target, error))
                {
                    cwd = target;
                }
                else
                {
                    emit("cd: no such directory: " + argument + "\r\n");
                }
            }
            else if (name == "ls")
            {
                std::vector<std::string> names;
                for (const auto& entry :
                     std::filesystem::directory_iterator(resolve(argument.empty() ? "." : argument), error))
                {
                    const std::string file = entry.path().filename().string();
                    names.push_back(entry.is_directory(error) ? "\x1b[34m" + file + "/\x1b[0m" : file);
                }
                std::sort(names.begin(), names.end());
                std::string listed;
                for (const std::string& file : names)
                {
                    listed += file + "\r\n";
                }
                emit(error ? "ls: cannot open " + argument + "\r\n" : listed);
            }
            else if (name == "cat")
            {
                std::ifstream in(resolve(argument), std::ios::binary);
                if (!in)
                {
                    emit("cat: " + argument + ": no such file\r\n");
                }
                else
                {
                    emit(terminal_text(std::string(std::istreambuf_iterator<char>(in), {})));
                }
            }
            else if (name == "echo")
            {
                // `echo text > file` / `>> file` writes it (the one redirection here: there is no editor either).
                std::string text;
                std::size_t i = 1;
                for (; i < words.size() && words[i] != ">" && words[i] != ">>"; ++i)
                {
                    text += (i > 1 ? " " : "") + words[i];
                }
                if (i + 1 < words.size())
                {
                    std::ofstream file(resolve(words[i + 1]),
                                       words[i] == ">>" ? std::ios::binary | std::ios::app : std::ios::binary);
                    file << text << '\n';
                    if (!file)
                    {
                        emit("echo: cannot write " + words[i + 1] + "\r\n");
                    }
                }
                else
                {
                    emit(text + "\r\n");
                }
            }
            else if (name == "mkdir")
            {
                if (!std::filesystem::create_directories(resolve(argument), error) && error)
                {
                    emit("mkdir: " + error.message() + "\r\n");
                }
            }
            else if (name == "rm")
            {
                if (!std::filesystem::remove(resolve(argument), error))
                {
                    emit("rm: cannot remove " + argument + "\r\n");
                }
            }
            else if (name == "clear")
            {
                emit("\x1b[2J\x1b[H");
            }
            else if (name == "exit")
            {
                exited = true;
            }
            else
            {
                return false;
            }
            return true;
        }

        void execute(const std::string& command_line)
        {
            const std::vector<std::string> words = split_words(command_line);
            if (words.empty())
            {
                emit(prompt());
                return;
            }
            if (history.empty() || history.back() != command_line)
            {
                history.push_back(command_line);
            }
            history_at = history.size();
            if (builtin(words))
            {
                if (!exited)
                {
                    emit(prompt());
                }
                return;
            }
            // A program: on a worker thread, its output streamed into the terminal; the prompt returns after it.
            {
                const std::lock_guard lock(state->mutex);
                state->running = true;
            }
            std::thread(
                    [state = state, workdir = cwd.string(), words, prompt = prompt()]
                    {
                        const ProcessOutput result =
                                run_process(workdir, words, {}, {},
                                            [&](std::string_view chunk)
                                            {
                                                const std::lock_guard lock(state->mutex);
                                                state->output += terminal_text(chunk);
                                            });
                        const std::lock_guard lock(state->mutex);
                        if (!result.started)
                        {
                            state->output += words.front() + ": command not found (try 'help')\r\n";
                        }
                        state->output += prompt;
                        state->running = false;
                    })
                    .detach();
        }

        void show_line(const std::string& text)
        {
            // Erase what was typed and type `text` instead.
            emit("\r\x1b[K" + prompt() + text);
            line = text;
        }

        void key(char c)
        {
            if (escape == 1)
            {
                escape = c == '[' || c == 'O' ? 2 : 0;
                return;
            }
            if (escape == 2)
            {
                escape = 0;
                if (c == 'A' && history_at > 0)
                {
                    show_line(history[--history_at]);
                }
                else if (c == 'B' && history_at < history.size())
                {
                    ++history_at;
                    show_line(history_at < history.size() ? history[history_at] : std::string());
                }
                return;
            }
            switch (c)
            {
            case '\x1b':
                escape = 1;
                break;
            case '\r':
            case '\n':
            {
                emit("\r\n");
                const std::string command = line;
                line.clear();
                execute(command);
                break;
            }
            case '\x7f':
            case '\b':
                if (!line.empty())
                {
                    // One character: a UTF-8 sequence's continuation bytes go with it.
                    while (!line.empty() && (static_cast<unsigned char>(line.back()) & 0xC0) == 0x80)
                    {
                        line.pop_back();
                    }
                    if (!line.empty())
                    {
                        line.pop_back();
                    }
                    emit("\b \b");
                }
                break;
            case '\x03': // Ctrl+C
                line.clear();
                emit("^C\r\n" + prompt());
                break;
            case '\x0c': // Ctrl+L
                emit("\x1b[2J\x1b[H" + prompt() + line);
                break;
            case '\x15': // Ctrl+U
                show_line({});
                break;
            default:
                if (static_cast<unsigned char>(c) >= 0x20)
                {
                    line += c;
                    emit(std::string(1, c));
                }
                break;
            }
        }
    };

    PseudoConsole::PseudoConsole(): m_impl(std::make_unique<Impl>())
    {
    }

    PseudoConsole::~PseudoConsole() = default;

    bool PseudoConsole::start(const std::vector<std::string>&, const std::string& workdir, int, int)
    {
        std::error_code error;
        m_impl->cwd = std::filesystem::is_directory(workdir, error)
                              ? std::filesystem::path(workdir).lexically_normal()
                              : home_directory();
        if (!m_impl->cwd.has_filename() && m_impl->cwd.has_parent_path() && m_impl->cwd != m_impl->cwd.root_path())
        {
            m_impl->cwd = m_impl->cwd.parent_path(); // no trailing separator in the prompt
        }
        m_impl->emit("ImGit's browser shell: git runs inside the page. Type 'help' for the commands.\r\n"
                     + m_impl->prompt());
        return true;
    }

    void PseudoConsole::write(std::string_view bytes)
    {
        m_impl->typed.append(bytes);
        m_impl->take_typed();
    }

    void PseudoConsole::resize(int, int)
    {
    }

    std::string PseudoConsole::read()
    {
        m_impl->take_typed(); // what was typed ahead, once the command it waited for is done
        const std::lock_guard lock(m_impl->state->mutex);
        return std::exchange(m_impl->state->output, {});
    }

    bool PseudoConsole::exited() const
    {
        return m_impl->exited;
    }
} // namespace imgit::platform
