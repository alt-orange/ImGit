#pragma once

struct GLFWwindow;

// Platform touches on the GLFW main window: its frame, cursors and native handle.
namespace imgit::platform
{
    // Called once the main window exists: the dark caption on Windows; remembers the window for main_window().
    void init_window(GLFWwindow* window);

    // The window init_window() was given (the parent of file dialogs), null before.
    [[nodiscard]] GLFWwindow* main_window();

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
