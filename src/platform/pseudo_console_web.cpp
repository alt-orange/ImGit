#include "platform/pseudo_console.hpp"

// The browser has no shell to attach: start() fails and the terminal panel says so.
namespace imgit::platform
{
    struct PseudoConsole::Impl
    {
    };

    PseudoConsole::PseudoConsole() = default;

    PseudoConsole::~PseudoConsole() = default;

    bool PseudoConsole::start(const std::vector<std::string>&, const std::string&, int, int)
    {
        return false;
    }

    void PseudoConsole::write(std::string_view)
    {
    }

    void PseudoConsole::resize(int, int)
    {
    }

    std::string PseudoConsole::read()
    {
        return {};
    }

    bool PseudoConsole::exited() const
    {
        return true;
    }
} // namespace imgit::platform
