#include "platform/process.hpp"

// The browser runs no other programs: no git executable (cli::available() is false, so the network operations and
// the few commands libgit2 lacks report that they cannot run) and no second instance.
namespace imgit::platform
{
    ProcessOutput run_process(const std::string&, const std::vector<std::string>&, const std::string&,
                              const Environment&, const OutputSink&)
    {
        return {};
    }

    bool spawn_detached(const std::vector<std::string>&, const std::string&)
    {
        return false;
    }

    std::string executable_path()
    {
        return {};
    }
} // namespace imgit::platform
