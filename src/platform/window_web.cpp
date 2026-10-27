#include "platform/window.hpp"
#include <GLFW/emscripten_glfw3.h>
#include <GLFW/glfw3.h>
#include <emscripten/emscripten.h>

// The canvas' CSS cursor (at global scope, as EM_JS requires): the browser has the drag-and-drop and resize shapes
// GLFW does not name.
EM_JS_DEPS(imgit_window, "$UTF8ToString");
EM_JS(void, imgit_set_canvas_cursor, (const char* cursor), {
    if (Module.canvas) {
        Module.canvas.style.cursor = UTF8ToString(cursor);
    }
});

// The page's full screen (the whole page, so the canvas keeps following the window's size). The key or click that
// asked for it still counts as the user's gesture the browser wants.
EM_JS(void, imgit_toggle_page_full_screen, (), {
    if (document.fullscreenElement) {
        document.exitFullscreen();
    } else if (document.documentElement.requestFullscreen) {
        document.documentElement.requestFullscreen().catch(function (error) {
            console.warn('ImGit: full screen refused', error);
        });
    }
});

// The browser: the window is the page's canvas, sized to the browser window.
namespace imgit::platform
{
    namespace
    {
        GLFWwindow* g_window = nullptr;
    } // namespace

    void apply_window_hints()
    {
    }

    void init_window(GLFWwindow* window)
    {
        g_window = window;
        // The canvas fills the page and follows the browser window's size; the page's title and icon are the
        // window's (res/web/shell.html).
        emscripten_glfw_make_canvas_resizable(window, "window", nullptr);
    }

    GLFWwindow* main_window()
    {
        return g_window;
    }

    float title_bar_inset()
    {
        return 0.f;
    }

    void set_title_bar_drag_region(float, float, float, float)
    {
    }

    float layout_scale(GLFWwindow*)
    {
        // The window is measured in CSS pixels; the device pixel ratio comes through the framebuffer scale, at
        // which ImGui rasterises the fonts (as on macOS).
        return 1.f;
    }

    GLFWcursor* grab_cursor()
    {
        // set_cursor() shows the browser's own grab cursor; this is the nearest GLFW shape.
        static GLFWcursor* hand = glfwCreateStandardCursor(GLFW_POINTING_HAND_CURSOR);
        return hand;
    }

    void set_cursor(GLFWwindow*, CursorShape shape)
    {
        imgit_set_canvas_cursor(shape == CursorShape::MOVE         ? "default"
                                : shape == CursorShape::NO_DROP    ? "no-drop"
                                : shape == CursorShape::COL_RESIZE ? "col-resize"
                                : shape == CursorShape::ROW_RESIZE ? "row-resize"
                                                                   : "grab");
    }

    void reset_cursor(GLFWwindow* window)
    {
        // Back to GLFW's cursor: cleared here, so the one ImGui asks for next frame is applied again.
        imgit_set_canvas_cursor("");
        glfwSetCursor(window, nullptr);
    }

    void toggle_full_screen(GLFWwindow*)
    {
        imgit_toggle_page_full_screen();
    }

    NativeWindow native_window(GLFWwindow*)
    {
        return {};
    }
} // namespace imgit::platform
