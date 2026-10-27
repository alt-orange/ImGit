# The libraries of a browser build (Emscripten), which vcpkg does not provide: built from source at the versions
# vcpkg.json's builtin-baseline resolves to, so the browser runs the same ImGui, FreeType and libgit2 as the desktop.
# GLFW is Emscripten's own port and WebGL 2 stands in for OpenGL; there is no cpr (avatars use the browser's fetch)
# and no nativefiledialog-extended.
#
# Sets IMGIT_DEPENDENCIES (the libraries imgit links) and Stb_INCLUDE_DIR, as find_package(Stb) does on the desktop.

include(FetchContent)

# GLFW 3.4 for the browser: emsdk's contrib.glfw3 port (emscripten-glfw, since emsdk 3.1.55), which has the cursors,
# Hi-DPI framebuffers and clipboard of a desktop GLFW. (Emscripten's older built-in GLFW is the 3.2 API.)
set(IMGIT_WEB_GLFW_FLAGS --use-port=contrib.glfw3)

# Every library runs on the worker threads too (the status poll, avatars, file history), so all of them are built
# for shared memory.
string(APPEND CMAKE_C_FLAGS " -pthread")
string(APPEND CMAKE_CXX_FLAGS " -pthread")
set(BUILD_SHARED_LIBS OFF)
# libgit2's overflow checks hand a size_t* (unsigned long) to the unsigned int builtins where both are 32 bits wide,
# as in wasm32: the same type in all but name.
string(APPEND CMAKE_C_FLAGS " -Wno-incompatible-pointer-types")
# FreeType's C90 sources use `inline`, which only the GNU dialect has (-std=gnu90, not -std=c90).
set(CMAKE_C_EXTENSIONS ON)

FetchContent_Declare(imgui
    GIT_REPOSITORY https://github.com/ocornut/imgui.git
    GIT_TAG v1.92.8
    GIT_SHALLOW TRUE)
FetchContent_Declare(freetype
    GIT_REPOSITORY https://github.com/freetype/freetype.git
    GIT_TAG VER-2-14-3
    GIT_SHALLOW TRUE)
FetchContent_Declare(libgit2
    GIT_REPOSITORY https://github.com/libgit2/libgit2.git
    GIT_TAG v1.9.4
    GIT_SHALLOW TRUE)
FetchContent_Declare(stb
    GIT_REPOSITORY https://github.com/nothings/stb.git
    GIT_TAG f75e8d1cad7d90d72ef7a4661f1b994ef78b4e31) # 2024-07-29, as vcpkg's stb port

# FreeType alone: ImGui only hands it TrueType / OpenType outlines.
set(FT_DISABLE_ZLIB ON CACHE BOOL "" FORCE)
set(FT_DISABLE_BZIP2 ON CACHE BOOL "" FORCE)
set(FT_DISABLE_PNG ON CACHE BOOL "" FORCE)
set(FT_DISABLE_HARFBUZZ ON CACHE BOOL "" FORCE)
set(FT_DISABLE_BROTLI ON CACHE BOOL "" FORCE)

# libgit2 for local repositories: the browser has no sockets, so no transports beyond what libgit2 has built in,
# and its own copies of zlib, the regex engine and the HTTP parser.
set(BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(BUILD_CLI OFF CACHE BOOL "" FORCE)
set(BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(BUILD_FUZZERS OFF CACHE BOOL "" FORCE)
set(USE_THREADS ON CACHE BOOL "" FORCE)
set(USE_HTTPS OFF CACHE STRING "" FORCE)
set(USE_SSH OFF CACHE STRING "" FORCE)
set(USE_GSSAPI OFF CACHE STRING "" FORCE)
set(USE_AUTH_NTLM OFF CACHE STRING "" FORCE)
set(USE_AUTH_NEGOTIATE OFF CACHE STRING "" FORCE)
set(USE_ICONV OFF CACHE BOOL "" FORCE)
set(USE_BUNDLED_ZLIB ON CACHE STRING "" FORCE)
set(REGEX_BACKEND builtin CACHE STRING "" FORCE)
set(USE_HTTP_PARSER builtin CACHE STRING "" FORCE)

FetchContent_MakeAvailable(freetype libgit2)
FetchContent_GetProperties(imgui)
if(NOT imgui_POPULATED)
    FetchContent_Populate(imgui)
endif()
FetchContent_GetProperties(stb)
if(NOT stb_POPULATED)
    FetchContent_Populate(stb)
endif()
set(Stb_INCLUDE_DIR ${stb_SOURCE_DIR})

# ImGui as vcpkg builds it (with imgui_stdlib) with the freetype, glfw-binding and opengl3-binding features.
add_library(imgui STATIC
    ${imgui_SOURCE_DIR}/imgui.cpp
    ${imgui_SOURCE_DIR}/imgui_demo.cpp
    ${imgui_SOURCE_DIR}/imgui_draw.cpp
    ${imgui_SOURCE_DIR}/imgui_tables.cpp
    ${imgui_SOURCE_DIR}/imgui_widgets.cpp
    ${imgui_SOURCE_DIR}/misc/cpp/imgui_stdlib.cpp
    ${imgui_SOURCE_DIR}/misc/freetype/imgui_freetype.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_glfw.cpp
    ${imgui_SOURCE_DIR}/backends/imgui_impl_opengl3.cpp)
target_include_directories(imgui PUBLIC ${imgui_SOURCE_DIR} ${imgui_SOURCE_DIR}/backends
                                        ${imgui_SOURCE_DIR}/misc/cpp ${imgui_SOURCE_DIR}/misc/freetype)
target_compile_definitions(imgui PUBLIC IMGUI_ENABLE_FREETYPE)
target_compile_options(imgui PUBLIC ${IMGIT_WEB_GLFW_FLAGS})
target_link_libraries(imgui PUBLIC freetype)
target_link_options(imgui PUBLIC ${IMGIT_WEB_GLFW_FLAGS})

# libgit2's static library names its headers for an installed copy only.
add_library(imgit_libgit2 INTERFACE)
target_include_directories(imgit_libgit2 INTERFACE ${libgit2_SOURCE_DIR}/include)
target_link_libraries(imgit_libgit2 INTERFACE libgit2package)

set(IMGIT_DEPENDENCIES imgui imgit_libgit2)
