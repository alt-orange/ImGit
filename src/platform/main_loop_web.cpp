#include "platform/main_loop.hpp"
#include <GLFW/glfw3.h>
#include <emscripten/emscripten.h>
#include <emscripten/html5.h>
#include <imgui_impl_glfw.h>

// The browser: the page's animation frames call the app, and the home folder is written back to IndexedDB
// (Module.imgitSyncFiles in res/web/pre.js) while the page runs.
namespace imgit::platform
{
    namespace detail
    {
        // shell_web.cpp: the file dialogs' answers, and the saved files that become downloads.
        void run_dialog_answers();
        void deliver_downloads();
    } // namespace detail

    namespace
    {
        // Seconds between two saves while the page is open: what a closed tab can lose.
        constexpr double SAVE_INTERVAL = 10.0;

        struct Loop
        {
            GLFWwindow* window = nullptr;
            std::function<void()> frame;
            std::function<void()> save;
            double last_save = 0.0;
        };
        Loop g_loop;

        double now()
        {
            return emscripten_get_now() / 1000.0;
        }

        void save_and_store()
        {
            if (g_loop.save)
            {
                g_loop.save();
            }
            g_loop.last_save = now();
            emscripten_run_script("Module.imgitSyncFiles()");
        }

        void run_frame(void*)
        {
            if (glfwWindowShouldClose(g_loop.window))
            {
                emscripten_cancel_main_loop();
                save_and_store();
                return;
            }
            detail::run_dialog_answers();
            g_loop.frame();
            detail::deliver_downloads();
            if (now() - g_loop.last_save >= SAVE_INTERVAL)
            {
                save_and_store();
            }
        }

        // A hidden page may be closed or discarded without another frame: store everything now.
        EM_BOOL on_visibility_change(int, const EmscriptenVisibilityChangeEvent* event, void*)
        {
            if (event->hidden)
            {
                save_and_store();
            }
            return EM_FALSE;
        }
    } // namespace

    const char* glsl_version()
    {
        return "#version 300 es";
    }

    void wait_for_events(double)
    {
        glfwPollEvents();
    }

    void run_main_loop(GLFWwindow* window, const std::function<void()>& frame, const std::function<void()>& save)
    {
        g_loop = {window, frame, save, now()};
        ImGui_ImplGlfw_InstallEmscriptenCallbacks(window, "#canvas");
        emscripten_set_visibilitychange_callback(nullptr, false, on_visibility_change);
        // Never returns: main's stack is left in place for the frames (simulate_infinite_loop).
        emscripten_set_main_loop_arg(run_frame, nullptr, 0, true);
    }

    bool relaunch()
    {
        emscripten_run_script("Module.imgitSyncFiles(() => location.reload())");
        return true;
    }
} // namespace imgit::platform
