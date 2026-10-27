#pragma once

#include "platform/process.hpp"
#include <span>

// Programs that run inside the app, for the browser build, where no other program can be started: run_process()
// runs args[0] from here (on the calling thread) instead. The app registers its own `git` (git/web_git.cpp).
// Browser build only; the desktop starts real processes.
namespace imgit::platform
{
    // Runs with the arguments after the program's name and returns the exit code; output goes to `out`, in order.
    using Program = std::function<int(const std::string& workdir, std::span<const std::string> args,
                                      const std::string& input, const Environment& environment, const OutputSink& out)>;

    void register_program(const std::string& name, Program program);
} // namespace imgit::platform
