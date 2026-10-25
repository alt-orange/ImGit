#pragma once

#include <string>
#include <utility>
#include <vector>

// Child processes: run-and-capture (the git executable) and detached launches (relaunch).
namespace imgit::platform
{
    struct ProcessOutput
    {
        bool started = false;
        int exit_code = -1;
        std::string text; // stdout + stderr, in order
    };

    // Variables added to (or replacing) the inherited environment.
    using Environment = std::vector<std::pair<std::string, std::string>>;

    // Runs `args` (args[0] looked up on PATH) in `workdir` without a console window and waits for it to finish.
    // `input` is written to its stdin, which is then closed.
    [[nodiscard]] ProcessOutput run_process(const std::string& workdir, const std::vector<std::string>& args,
                                            const std::string& input = {}, const Environment& environment = {});

    // Starts `args` without waiting for it; false when it could not be started.
    bool spawn_detached(const std::vector<std::string>& args, const std::string& workdir = {});

    // The running executable's path (UTF-8).
    [[nodiscard]] std::string executable_path();
} // namespace imgit::platform
