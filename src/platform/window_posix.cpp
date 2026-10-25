#include "app/assets.hpp"
#include "gfx/image.hpp"
#include "platform/window.hpp"
#include <GLFW/glfw3.h>
#ifdef __APPLE__
#define GLFW_EXPOSE_NATIVE_COCOA
#else
#define GLFW_EXPOSE_NATIVE_X11
#endif
#include <GLFW/glfw3native.h>
#undef CursorShape // X.h's QueryBestSize class

namespace imgit::platform
{
    namespace
    {
        GLFWwindow* g_window = nullptr;
    } // namespace

    void init_window(GLFWwindow* window)
    {
        // The frame belongs to the window manager (Linux) or follows the system appearance (macOS).
        g_window = window;
#ifndef __APPLE__
        // The app icon (macOS takes it from the bundle instead).
        if (const std::optional<gfx::Image> icon = gfx::decode_image(assets::data("images/app_icon.png")); icon)
        {
            GLFWimage image{icon->width, icon->height, const_cast<unsigned char*>(icon->rgba.data())};
            glfwSetWindowIcon(window, 1, &image);
        }
#endif
    }

    GLFWwindow* main_window()
    {
        return g_window;
    }

    void set_cursor(GLFWwindow* window, CursorShape shape)
    {
        static GLFWcursor* move = glfwCreateStandardCursor(GLFW_ARROW_CURSOR);
        static GLFWcursor* no_drop = glfwCreateStandardCursor(GLFW_NOT_ALLOWED_CURSOR);
        static GLFWcursor* col_resize = glfwCreateStandardCursor(GLFW_HRESIZE_CURSOR);
        static GLFWcursor* row_resize = glfwCreateStandardCursor(GLFW_VRESIZE_CURSOR);
        GLFWcursor* cursor = shape == CursorShape::MOVE         ? move
                             : shape == CursorShape::NO_DROP    ? no_drop
                             : shape == CursorShape::COL_RESIZE ? col_resize
                                                                : row_resize;
        glfwSetCursor(window, cursor);
    }

    void reset_cursor(GLFWwindow*)
    {
        // ImGui's GLFW backend sets its cursor again on the next frame.
    }

    NativeWindow native_window(GLFWwindow* window)
    {
        if (window == nullptr)
        {
            return {};
        }
#ifdef __APPLE__
        return {NativeWindow::Kind::COCOA_NSWINDOW, static_cast<void*>(glfwGetCocoaWindow(window))};
#else
        // Under Wayland there is no X11 window to hand out.
        if (glfwGetPlatform() != GLFW_PLATFORM_X11)
        {
            return {};
        }
        return {NativeWindow::Kind::X11_WINDOW, reinterpret_cast<void*>(glfwGetX11Window(window))};
#endif
    }
} // namespace imgit::platform
