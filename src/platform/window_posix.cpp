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

#ifdef __APPLE__
    namespace detail
    {
        // window_mac.mm: the dark appearance whatever the system setting, and the title bar hidden under the
        // content (the traffic lights stay, over the tabs bar).
        void configure_window(void* ns_window);
        void drag_window(void* ns_window);
        void title_bar_double_click(void* ns_window);
    } // namespace detail
#endif

    void apply_window_hints()
    {
#ifdef __APPLE__
        // macOS only gives a core profile context (3.2 to 4.1) when it is forward-compatible.
        glfwWindowHint(GLFW_OPENGL_FORWARD_COMPAT, GLFW_TRUE);
#endif
    }

    void init_window(GLFWwindow* window)
    {
        g_window = window;
#ifdef __APPLE__
        // The app icon comes from the bundle; the window frame is dark like the rest of the window.
        detail::configure_window(static_cast<void*>(glfwGetCocoaWindow(window)));
#else
        // The frame belongs to the window manager.
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

    float title_bar_inset()
    {
#ifdef __APPLE__
        return 72.f; // the three traffic lights (inset as in a toolbar window, they end near 70) and a gap
#else
        return 0.f;
#endif
    }

    void drag_window([[maybe_unused]] GLFWwindow* window)
    {
#ifdef __APPLE__
        if (window != nullptr)
        {
            detail::drag_window(static_cast<void*>(glfwGetCocoaWindow(window)));
        }
#endif
    }

    void title_bar_double_click([[maybe_unused]] GLFWwindow* window)
    {
#ifdef __APPLE__
        if (window != nullptr)
        {
            detail::title_bar_double_click(static_cast<void*>(glfwGetCocoaWindow(window)));
        }
#endif
    }

    float layout_scale([[maybe_unused]] GLFWwindow* window)
    {
#ifdef __APPLE__
        // Cocoa windows are measured in points; GLFW_SCALE_TO_MONITOR does nothing there and the Retina
        // framebuffer is reported through the framebuffer scale instead.
        return 1.f;
#else
        float scale = 1.f;
        glfwGetWindowContentScale(window, &scale, nullptr);
        return scale > 0.f ? scale : 1.f;
#endif
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
