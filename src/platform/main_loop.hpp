#pragma once

#include <functional>

struct GLFWwindow;

// How the app's frames are driven, which differs between a desktop process (a loop in main) and a browser page
// (the page's animation frames call in, and main returns).
namespace imgit::platform
{
    // The GLSL version line of ImGui's OpenGL 3 shaders: GL 3.3 core on the desktop, GLSL ES 3.0 for WebGL 2.
    [[nodiscard]] const char* glsl_version();

    // Waits up to `timeout` seconds for input while the app is idle. The browser schedules the frames itself, so
    // there it only takes the input that arrived.
    void wait_for_events(double timeout);

    // Calls `frame` until `window` is asked to close. In the browser it returns only when the page goes away:
    // main's locals stay alive for the page's lifetime, and `save` is called whenever the page is hidden and every
    // few seconds, after which the app's files are written to the browser's storage. On the desktop `save` is not
    // called; the caller saves once this returns.
    void run_main_loop(GLFWwindow* window, const std::function<void()>& frame, const std::function<void()>& save);

    // Starts a new instance of the app for "Relaunch", after which the caller closes this one; false when it cannot.
    // The browser reloads the page once the files are stored.
    bool relaunch();
} // namespace imgit::platform
