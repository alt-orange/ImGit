#pragma once

#include <memory>
#include <string>
#include <string_view>
#include <vector>

// A shell attached to a pseudo terminal (ConPTY on Windows, forkpty elsewhere)
// sessions: keystrokes go in as VT input, the screen comes back as VT output, read on a background thread.
namespace imgit::platform
{
    class PseudoConsole
    {
    public:
        PseudoConsole();
        ~PseudoConsole();
        PseudoConsole(const PseudoConsole&) = delete;
        PseudoConsole& operator=(const PseudoConsole&) = delete;

        // Starts `args` (UTF-8, args[0] looked up on PATH) in `workdir` on a `columns` x `rows` console; false when
        // it cannot.
        [[nodiscard]] bool start(const std::vector<std::string>& args, const std::string& workdir, int columns,
                                 int rows);
        void write(std::string_view bytes);
        void resize(int columns, int rows);
        // The output that arrived since the last call.
        [[nodiscard]] std::string read();
        // True once the shell process has ended (and all of its output has been read).
        [[nodiscard]] bool exited() const;

    private:
        struct Impl;
        std::unique_ptr<Impl> m_impl;
    };
} // namespace imgit::platform
