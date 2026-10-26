# ImGit Desktop: a Git client on Dear ImGui (GLFW + OpenGL 3) and libgit2.
#
#   imgit    the application
#
# Standalone: `cmake --preset x64-debug` (CMakePresets.json; vcpkg through the Visual Studio bundle), then
# `cmake --build --preset x64-debug`; `linux-debug` and `macos-debug` likewise with a vcpkg checkout in
# VCPKG_ROOT.  Everything in assets/ is compiled into the executable (cmake/EmbedAssets.cmake), so imgit.exe
# (imgit on Linux, ImGit.app on macOS) runs on its own.
#
#   imgit_tests  the unit tests (test/, GoogleTest), with -DIMGIT_BUILD_TESTS=ON; then `ctest --test-dir <build>`
#                (-DIMGIT_VALGRIND=ON also runs them under Valgrind: `ctest -L valgrind`, or the linux-valgrind preset)

cmake_minimum_required(VERSION 3.25)

# The tests need GoogleTest: vcpkg installs it with the manifest's "tests" feature, which must be chosen before
# project().
option(IMGIT_BUILD_TESTS "Build the unit tests (test/)" OFF)
option(IMGIT_VALGRIND "Also run the unit tests under Valgrind memcheck (ctest -L valgrind)" OFF)
if(IMGIT_BUILD_TESTS)
    list(APPEND VCPKG_MANIFEST_FEATURES tests)
endif()

# The oldest macOS the build runs on; must be set before project(). vcpkg-triplets/*-osx.cmake builds the
# libraries for the same release (keep the two in step).
if(CMAKE_HOST_APPLE AND NOT DEFINED CMAKE_OSX_DEPLOYMENT_TARGET)
    set(CMAKE_OSX_DEPLOYMENT_TARGET 12.0 CACHE STRING "Minimum macOS version")
endif()

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

# The syntax highlighter: plain C++ (no ImGui), shared by the app and the tests.
add_library(imgit_syntax STATIC
    src/ui/syntax/languages.cpp
    src/ui/syntax/syntax.cpp
    src/ui/syntax/tokenizer.cpp)
target_include_directories(imgit_syntax PUBLIC src)

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
    src/ui/file_history.cpp
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

# Platform code lives in src/platform (one header per service, a _win32 / _posix source each, plus the .mm
# sources for the Cocoa calls) and in the native font loader; everything else is portable.
if(WIN32)
    target_sources(imgit PRIVATE
        src/platform/app_menu_none.cpp
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
    if(APPLE)
        # An app bundle (ImGit.app) with the Dock icon; the GL 1.1 header's deprecation warnings are silenced.
        enable_language(OBJCXX)
        target_sources(imgit PRIVATE src/platform/app_menu_mac.mm src/platform/window_mac.mm res/imgit.icns)
        set_source_files_properties(res/imgit.icns PROPERTIES MACOSX_PACKAGE_LOCATION Resources)
        set_target_properties(imgit PROPERTIES
            OUTPUT_NAME ImGit
            MACOSX_BUNDLE TRUE
            MACOSX_BUNDLE_INFO_PLIST ${CMAKE_SOURCE_DIR}/res/Info.plist.in
            MACOSX_BUNDLE_BUNDLE_NAME ImGit
            MACOSX_BUNDLE_GUI_IDENTIFIER com.alt-orange.imgit
            MACOSX_BUNDLE_ICON_FILE imgit.icns
            MACOSX_BUNDLE_SHORT_VERSION_STRING ${PROJECT_VERSION}
            MACOSX_BUNDLE_BUNDLE_VERSION ${PROJECT_VERSION})
        target_compile_definitions(imgit PRIVATE GL_SILENCE_DEPRECATION)
        target_link_libraries(imgit PRIVATE "-framework Cocoa")
    else()
        target_sources(imgit PRIVATE src/platform/app_menu_none.cpp)
        target_link_libraries(imgit PRIVATE util) # forkpty
    endif()
endif()

target_include_directories(imgit PRIVATE src ${Stb_INCLUDE_DIR})
target_link_libraries(imgit PRIVATE imgit_syntax imgui::imgui glfw libgit2::libgit2package cpr::cpr nfd::nfd
                                    OpenGL::GL)

# Every warning the compilers offer short of the style-level ones, and all of them errors: /W4 /WX on MSVC and
# clang-cl, -Wall -Wextra -Wpedantic -Werror elsewhere. clang-cl takes the GNU flags too, for what /W4 leaves
# out, through /clang: (a bare -Wall there is MSVC's /Wall, which clang-cl reads as -Weverything).
if(MSVC)
    target_compile_options(imgit PRIVATE /utf-8 /EHsc /W4 /WX)
    target_compile_options(imgit_syntax PRIVATE /utf-8 /EHsc /W4 /WX)
    if(CMAKE_CXX_COMPILER_ID MATCHES "Clang")
        target_compile_options(imgit PRIVATE /clang:-Wall /clang:-Wextra /clang:-Wpedantic)
        target_compile_options(imgit_syntax PRIVATE /clang:-Wall /clang:-Wextra /clang:-Wpedantic)
    endif()
    # a GUI subsystem in Release keeps the console window away; Debug keeps it for logs
    set_target_properties(imgit PROPERTIES WIN32_EXECUTABLE $<CONFIG:Release>)
    # A windowed (no console) release build that still enters through main().
    target_link_options(imgit PRIVATE $<$<CONFIG:Release>:/ENTRY:mainCRTStartup>)
else()
    target_compile_options(imgit PRIVATE -Wall -Wextra -Wpedantic -Werror)
    target_compile_options(imgit_syntax PRIVATE -Wall -Wextra -Wpedantic -Werror)
endif()

include(cmake/EmbedAssets.cmake)
imgit_embed_assets(imgit ${CMAKE_CURRENT_SOURCE_DIR}/assets)

if(IMGIT_BUILD_TESTS)
    enable_testing()
    add_subdirectory(test)
endif()
