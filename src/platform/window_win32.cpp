#include "app/assets.hpp"
#include "platform/win32_util.hpp"
#include "platform/window.hpp"
#include <GLFW/glfw3.h>
#define GLFW_EXPOSE_NATIVE_WIN32
#include <GLFW/glfw3native.h>
#include <algorithm>
#include <cstdint>
#include <dwmapi.h>
#include <span>
#include <vector>

namespace imgit::platform
{
    namespace
    {
        GLFWwindow* g_window = nullptr;
        // The cursor WM_SETCURSOR shows instead of GLFW's, while set: GLFW's RGBA cursors cannot invert the screen.
        HCURSOR g_custom_cursor = nullptr;
        WNDPROC g_glfw_window_proc = nullptr;

        LRESULT CALLBACK cursor_window_proc(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
        {
            if (message == WM_SETCURSOR && LOWORD(lparam) == HTCLIENT && g_custom_cursor != nullptr)
            {
                SetCursor(g_custom_cursor);
                return TRUE;
            }
            return CallWindowProcW(g_glfw_window_proc, hwnd, message, wparam, lparam);
        }

        // One of Chromium's cursors (assets/cursors: col_resize.cur, row_resize.cur, shown by
        // the resize handles), stretched to the system cursor size as LoadCursor does. CreateIconFromResourceEx takes a cursor
        // resource: the .cur entry's hotspot (two WORDs) followed by its DIB. `fallback` is the system cursor's id.
        HCURSOR chromium_cursor(const char* asset, int fallback)
        {
            const std::span<const unsigned char> file = assets::data(asset);
            const auto word = [&](std::size_t at)
            {
                return static_cast<std::uint32_t>(file[at] | file[at + 1] << 8);
            };
            if (file.size() >= 22)
            {
                const std::size_t bytes = word(14) | word(16) << 16;
                const std::size_t offset = word(18) | word(20) << 16;
                if (offset + bytes <= file.size())
                {
                    std::vector<BYTE> resource(4 + bytes);
                    std::copy_n(file.data() + 10, 4, resource.data()); // hotspot x, y
                    std::copy_n(file.data() + offset, bytes, resource.data() + 4);
                    const int size = GetSystemMetrics(SM_CXCURSOR);
                    if (HICON cursor = CreateIconFromResourceEx(resource.data(), static_cast<DWORD>(resource.size()),
                                                                FALSE, 0x00030000, size, size, LR_DEFAULTCOLOR);
                        cursor != nullptr)
                    {
                        return cursor;
                    }
                }
            }
            return LoadCursorW(nullptr, MAKEINTRESOURCEW(fallback));
        }

        // The system drag-and-drop cursor `id` (ole32.dll's resources: 1 no drop, 3 move) as a GLFW cursor: drawn
        // on black and on white to recover its colour and alpha; inverting pixels come out white.
        GLFWcursor* drag_drop_cursor(int id)
        {
            HMODULE ole = LoadLibraryExW(L"ole32.dll", nullptr, LOAD_LIBRARY_AS_DATAFILE);
            HCURSOR cursor = ole != nullptr ? static_cast<HCURSOR>(LoadImageW(ole, MAKEINTRESOURCEW(id), IMAGE_CURSOR,
                                                                              0, 0, LR_DEFAULTSIZE))
                                            : nullptr;
            if (cursor == nullptr)
            {
                return glfwCreateStandardCursor(id == 1 ? GLFW_NOT_ALLOWED_CURSOR : GLFW_ARROW_CURSOR);
            }
            ICONINFO info{};
            GetIconInfo(cursor, &info);
            const int size = GetSystemMetrics(SM_CXCURSOR);
            BITMAPINFO bitmap_info{};
            bitmap_info.bmiHeader = {sizeof(BITMAPINFOHEADER), size, -size, 1, 32, BI_RGB};
            HDC screen = GetDC(nullptr);
            HDC dc = CreateCompatibleDC(screen);
            const unsigned char* pixels[2] = {};
            HBITMAP bitmaps[2] = {};
            for (int k = 0; k < 2; ++k)
            {
                void* bits = nullptr;
                bitmaps[k] = CreateDIBSection(dc, &bitmap_info, DIB_RGB_COLORS, &bits, nullptr, 0);
                SelectObject(dc, bitmaps[k]);
                PatBlt(dc, 0, 0, size, size, k == 0 ? BLACKNESS : WHITENESS);
                DrawIconEx(dc, 0, 0, cursor, size, size, 0, nullptr, DI_NORMAL);
                pixels[k] = static_cast<const unsigned char*>(bits);
            }
            GdiFlush();
            std::vector<unsigned char> rgba(static_cast<std::size_t>(size * size * 4));
            for (int i = 0; i < size * size; ++i)
            {
                const unsigned char* on_black = pixels[0] + i * 4;
                const unsigned char* on_white = pixels[1] + i * 4;
                unsigned char* out = rgba.data() + i * 4;
                if (on_white[1] < on_black[1])
                {
                    out[0] = out[1] = out[2] = out[3] = 255;
                    continue;
                }
                const int alpha = 255 - (on_white[1] - on_black[1]);
                out[3] = static_cast<unsigned char>(alpha);
                for (int c = 0; c < 3; ++c)
                {
                    out[c] = static_cast<unsigned char>(alpha > 0 ? std::min(255, on_black[2 - c] * 255 / alpha) : 0);
                }
            }
            DeleteDC(dc);
            ReleaseDC(nullptr, screen);
            DeleteObject(bitmaps[0]);
            DeleteObject(bitmaps[1]);
            DeleteObject(info.hbmMask);
            DeleteObject(info.hbmColor);
            FreeLibrary(ole);
            const GLFWimage image{size, size, rgba.data()};
            return glfwCreateCursor(&image, static_cast<int>(info.xHotspot), static_cast<int>(info.yHotspot));
        }
    } // namespace

    void init_window(GLFWwindow* window)
    {
        g_window = window;
        // The window frame is the dark Windows caption.
        const BOOL dark = TRUE;
        constexpr DWORD USE_IMMERSIVE_DARK_MODE = 20;
        HWND hwnd = glfwGetWin32Window(window);
        DwmSetWindowAttribute(hwnd, USE_IMMERSIVE_DARK_MODE, &dark, sizeof(dark));
        g_glfw_window_proc = reinterpret_cast<WNDPROC>(
                SetWindowLongPtrW(hwnd, GWLP_WNDPROC, reinterpret_cast<LONG_PTR>(cursor_window_proc)));
    }

    GLFWwindow* main_window()
    {
        return g_window;
    }

    void set_cursor(GLFWwindow* window, CursorShape shape)
    {
        if (shape == CursorShape::COL_RESIZE || shape == CursorShape::ROW_RESIZE)
        {
            // IDC_SIZEWE / IDC_SIZENS (the macros are the ANSI ones here)
            static const HCURSOR col_resize = chromium_cursor("cursors/col_resize.cur", 32644);
            static const HCURSOR row_resize = chromium_cursor("cursors/row_resize.cur", 32645);
            g_custom_cursor = shape == CursorShape::COL_RESIZE ? col_resize : row_resize;
            SetCursor(g_custom_cursor);
            return;
        }
        reset_cursor(window);
        static GLFWcursor* move = drag_drop_cursor(3);
        static GLFWcursor* no_drop = drag_drop_cursor(1);
        glfwSetCursor(window, shape == CursorShape::MOVE ? move : no_drop);
    }

    void reset_cursor(GLFWwindow* window)
    {
        if (g_custom_cursor != nullptr)
        {
            // Let GLFW put its cursor (the one ImGui last asked for) back.
            g_custom_cursor = nullptr;
            HWND hwnd = glfwGetWin32Window(window);
            SendMessageW(hwnd, WM_SETCURSOR, reinterpret_cast<WPARAM>(hwnd), MAKELPARAM(HTCLIENT, WM_MOUSEMOVE));
        }
    }

    NativeWindow native_window(GLFWwindow* window)
    {
        if (HWND hwnd = window != nullptr ? glfwGetWin32Window(window) : nullptr; hwnd != nullptr)
        {
            return {NativeWindow::Kind::WINDOWS_HWND, hwnd};
        }
        return {};
    }
} // namespace imgit::platform
