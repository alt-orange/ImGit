#include "platform/window.hpp"
#include <GLFW/glfw3.h>

// Windows, Linux and macOS: what GLFW does the same way on each.
namespace imgit::platform
{
    void toggle_full_screen(GLFWwindow* window)
    {
        static int saved[4] = {};
        if (glfwGetWindowMonitor(window) != nullptr)
        {
            glfwSetWindowMonitor(window, nullptr, saved[0], saved[1], saved[2], saved[3], 0);
            return;
        }
        glfwGetWindowPos(window, &saved[0], &saved[1]);
        glfwGetWindowSize(window, &saved[2], &saved[3]);
        GLFWmonitor* monitor = glfwGetPrimaryMonitor();
        const GLFWvidmode* mode = monitor != nullptr ? glfwGetVideoMode(monitor) : nullptr;
        if (mode == nullptr)
        {
            return; // no monitor to go full screen on (disconnected meanwhile)
        }
        glfwSetWindowMonitor(window, monitor, 0, 0, mode->width, mode->height, mode->refreshRate);
    }
} // namespace imgit::platform
