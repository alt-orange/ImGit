#include "platform/main_loop.hpp"
#include "platform/process.hpp"
#include <GLFW/glfw3.h>

// Windows, Linux and macOS: a plain loop on the main thread.
namespace imgit::platform
{
    const char* glsl_version()
    {
        return "#version 330";
    }

    void wait_for_events(double timeout)
    {
        glfwWaitEventsTimeout(timeout);
    }

    void run_main_loop(GLFWwindow* window, const std::function<void()>& frame, const std::function<void()>&)
    {
        while (!glfwWindowShouldClose(window))
        {
            frame();
        }
    }

    bool relaunch()
    {
        return spawn_detached({executable_path()});
    }
} // namespace imgit::platform
