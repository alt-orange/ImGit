# ImGit Desktop: a Git client on Dear ImGui (GLFW + OpenGL 3) and libgit2.
#
#   imgit    the application
#
# Standalone: `cmake --preset x64-debug` (CMakePresets.json; vcpkg through the Visual Studio bundle), then
# `cmake --build --preset x64-debug`.  Everything in assets/ is compiled into the executable
# (cmake/EmbedAssets.cmake), so imgit.exe runs on its own.

cmake_minimum_required(VERSION 3.25)

project(imgit VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
# the static CRT of the x64-windows-static triplet (/MT, /MTd)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")

find_package(imgui CONFIG REQUIRED)
find_package(glfw3 CONFIG REQUIRED)
find_package(libgit2 CONFIG REQUIRED)
find_package(cpr CONFIG REQUIRED)
find_package(nfd CONFIG REQUIRED)
find_package(Stb REQUIRED)
find_package(OpenGL REQUIRED)

add_executable(imgit
    src/main.cpp
    src/app/application.cpp
    src/app/profiles.cpp
    src/app/assets.cpp
    src/app/md5.cpp
    src/app/repo_state.cpp
    src/app/merge_tool.cpp
    src/app/theme.cpp
    src/app/toasts.cpp
    src/app/vt_screen.cpp
    src/git/git_cli.cpp
    src/git/repository.cpp
    src/gfx/image.cpp
    src/gfx/lcd_renderer.cpp
    src/graph/graph_layout.cpp
    src/platform/shell_dialogs.cpp
    src/ui/tabs_bar.cpp
    src/ui/toolbar.cpp
    src/ui/left_panel.cpp
    src/ui/graph_view.cpp
    src/ui/detail_panel.cpp
    src/ui/diff_view.cpp
    src/ui/dialogs.cpp
    src/ui/menus.cpp
    src/ui/native_menu.cpp
    src/ui/preferences.cpp
    src/ui/popover.cpp
    src/ui/tab_popovers.cpp
    src/ui/profile_widgets.cpp
    src/ui/repo_management.cpp
    src/ui/terminal_panel.cpp
    src/ui/fuzzy_finder.cpp
    src/ui/rebase_panel.cpp
    src/ui/merge_view.cpp
    src/ui/avatars.cpp
    src/ui/vector_icons.cpp
    src/ui/widgets.cpp)

# Platform code lives in src/platform (one header per service, a _win32 / _posix source each) and in the native
# font loader; everything else is portable.
if(WIN32)
    target_sources(imgit PRIVATE
        src/platform/process_win32.cpp
        src/platform/pseudo_console_win32.cpp
        src/platform/shell_win32.cpp
        src/platform/system_win32.cpp
        src/platform/window_win32.cpp
        src/gfx/native_font_loader_dwrite.cpp
        res/imgit.rc) # the app icon (res/imgit.ico, drawn by tools/app_icon.py)
    target_compile_definitions(imgit PRIVATE _CRT_SECURE_NO_WARNINGS NOMINMAX WIN32_LEAN_AND_MEAN)
    target_link_libraries(imgit PRIVATE shell32 dwmapi dwrite winhttp)
else()
    target_sources(imgit PRIVATE
        src/platform/process_posix.cpp
        src/platform/pseudo_console_posix.cpp
        src/platform/shell_posix.cpp
        src/platform/system_posix.cpp
        src/platform/window_posix.cpp
        src/gfx/native_font_loader_none.cpp)
    if(NOT APPLE)
        target_link_libraries(imgit PRIVATE util) # forkpty
    endif()
endif()

target_include_directories(imgit PRIVATE src ${Stb_INCLUDE_DIR})
target_link_libraries(imgit PRIVATE imgui::imgui glfw libgit2::libgit2package cpr::cpr nfd::nfd OpenGL::GL)

if(MSVC)
    target_compile_options(imgit PRIVATE /utf-8 /EHsc /W4)
    if(CMAKE_CXX_COMPILER_ID MATCHES "Clang")
        target_compile_options(imgit PRIVATE -Wno-unused-parameter -Wno-missing-field-initializers)
    endif()
    # a GUI subsystem in Release keeps the console window away; Debug keeps it for logs
    set_target_properties(imgit PROPERTIES WIN32_EXECUTABLE $<CONFIG:Release>)
    # A windowed (no console) release build that still enters through main().
    target_link_options(imgit PRIVATE $<$<CONFIG:Release>:/ENTRY:mainCRTStartup>)
endif()

include(cmake/EmbedAssets.cmake)
imgit_embed_assets(imgit ${CMAKE_SOURCE_DIR}/assets)
