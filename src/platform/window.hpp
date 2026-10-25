#pragma once

struct GLFWwindow;

// Platform touches on the GLFW main window: its frame, cursors and native handle.
namespace imgit::platform
{
    // GLFW window hints the platform needs, set before the main window is created: on macOS a forward-compatible
    // context, without which Cocoa gives no core profile.
    void apply_window_hints();

    // Called once the main window exists: the dark caption on Windows, the dark title bar on macOS; remembers the
    // window for main_window().
    void init_window(GLFWwindow* window);

    // The factor the UI's logical (CSS) pixels are multiplied by for `window`: the monitor's content scale where
    // window coordinates are physical pixels (Windows, X11), 1 where they are points and the framebuffer alone is
    // scaled (macOS: ImGui rasterises the fonts at the framebuffer's density).
    [[nodiscard]] float layout_scale(GLFWwindow* window);

    // The window init_window() was given (the parent of file dialogs), null before.
    [[nodiscard]] GLFWwindow* main_window();

    // Logical pixels the tabs bar leaves free at its left for the window's own controls: the traffic lights on
    // macOS, where the system title bar is hidden and the tabs bar stands in for it; 0 elsewhere.
    [[nodiscard]] float title_bar_inset();
    // Where the tabs bar is the title bar: the part of it (window coordinates, logical px) that acts as one, moving
    // the window when dragged and zooming or minimising on a double-click, as the system is set. Set every frame,
    // empty (max <= min) when there is none; the presses there never reach ImGui. A no-op elsewhere.
    void set_title_bar_drag_region(float min_x, float min_y, float max_x, float max_y);

    enum class CursorShape
    {
        MOVE, // drag-and-drop: drop allowed
        NO_DROP,
        COL_RESIZE,
        ROW_RESIZE,
    };
    // Shows `shape` over the window instead of ImGui's cursor until reset_cursor(): the system drag-and-drop
    // cursors, and on Windows Chromium's own resize cursors (inverting masks GLFW's RGBA cursors cannot show).
    void set_cursor(GLFWwindow* window, CursorShape shape);
    // Hands the cursor back to GLFW (the one ImGui last asked for).
    void reset_cursor(GLFWwindow* window);

    struct NativeWindow
    {
        enum class Kind
        {
            NONE,
            WINDOWS_HWND,
            COCOA_NSWINDOW,
            X11_WINDOW,
        };
        Kind kind = Kind::NONE;
        void* handle = nullptr;
    };
    // The window's native handle, for system dialogs that take a parent.
    [[nodiscard]] NativeWindow native_window(GLFWwindow* window);
} // namespace imgit::platform
