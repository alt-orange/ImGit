#include "platform/programs.hpp"
#include <map>
#include <mutex>

// The browser runs no other programs: run_process() runs the ones the app registered (its own `git`), and
// nothing can be started detached.
namespace imgit::platform
{
    namespace
    {
        struct Registry
        {
            std::mutex mutex;
            std::map<std::string, Program> programs;
        };

        // A function-local static: programs register from static constructors in other files.
        Registry& registry()
        {
            static Registry instance;
            return instance;
        }
    } // namespace

    void register_program(const std::string& name, Program program)
    {
        Registry& programs = registry();
        const std::lock_guard lock(programs.mutex);
        programs.programs[name] = std::move(program);
    }

    ProcessOutput run_process(const std::string& workdir, const std::vector<std::string>& args,
                              const std::string& input, const Environment& environment, const OutputSink& on_output)
    {
        if (args.empty())
        {
            return {};
        }
        Program program;
        {
            Registry& programs = registry();
            const std::lock_guard lock(programs.mutex);
            const auto found = programs.programs.find(args.front());
            if (found == programs.programs.end())
            {
                return {};
            }
            program = found->second;
        }
        ProcessOutput result;
        result.started = true;
        result.exit_code = program(workdir, std::span<const std::string>(args).subspan(1), input, environment,
                                   [&](std::string_view chunk)
                                   {
                                       result.text.append(chunk);
                                       if (on_output)
                                       {
                                           on_output(chunk);
                                       }
                                   });
        return result;
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
