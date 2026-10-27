# Porting ImGit off Windows

## Where platform code lives

Since 2026-10-25 every OS call sits behind a header in `src/platform/`, with one implementation per platform chosen in
`CMakeLists.txt` (`if(WIN32)` → `*_win32.cpp`, else `*_posix.cpp`, plus `window_mac.mm` on macOS for the one Cocoa
call). No other source has an `#ifdef _WIN32` or `__APPLE__`. The only remaining ones are inside `platform/` and the
GL calling-convention macro in `gfx/lcd_renderer.cpp`; the UI asks ImGui's `io.ConfigMacOSXBehaviors` where macOS
differs at run time (shortcut labels, the terminal's modifiers).

| Header | Windows | POSIX (Linux / macOS) |
|---|---|---|
| `platform/process.hpp`: `run_process` (args, stdin, env, merged output, exit code), `spawn_detached`, `executable_path` | `CreateProcessW` with pipes, `CREATE_NO_WINDOW`, an environment block, CommandLineToArgv-correct quoting | `posix_spawnp` + pipes + `addchdir_np`, argv passed verbatim (no shell), `WEXITSTATUS`; `/proc/self/exe` or `_NSGetExecutablePath` |
| `platform/pseudo_console.hpp`: the terminal panel's shell | ConPTY (`CreatePseudoConsole`, looked up at run time) | `forkpty` + `execve` (`TERM=xterm-256color`), `TIOCSWINSZ`, `waitpid(WNOHANG)`, SIGHUP on close |
| `platform/shell.hpp`: open file manager / terminal / editor / URL / path, show in folder, file dialogs | `ShellExecuteW`, `explorer /select,` | `xdg-open` / `open`, `open -R`, `$TERMINAL` → `x-terminal-emulator` → gnome-terminal → konsole → xterm |
| ↳ file dialogs (`shell_dialogs.cpp`, shared) | nativefiledialog-extended, parented to the main window | the same (GTK or portal on Linux, NSOpenPanel on macOS) |
| `platform/system.hpp`: home, app data and config folders, `local_time`, menu show delay, system font files, default shell, system proxy | `%USERPROFILE%`, `%APPDATA%`, `localtime_s`, `SPI_GETMENUSHOWDELAY`, `%WINDIR%\Fonts`, PowerShell, `WinHttpGetIEProxyConfigForCurrentUser` | `$HOME`, `$XDG_CONFIG_HOME`, `localtime_r`, 400 ms, *no fonts yet*, `$SHELL -l`, none (curl reads `HTTPS_PROXY` itself) |
| `platform/window.hpp`: `apply_window_hints`, `init_window`, `layout_scale`, `set_cursor` / `reset_cursor`, `native_window` | no extra hints; dark caption (DWM), Chromium's resize cursors through a `WM_SETCURSOR` hook, ole32's drag-and-drop cursors; the monitor's content scale | `GLFW_OPENGL_FORWARD_COMPAT` on macOS; the window icon (Linux) or the dark title bar (`window_mac.mm`); GLFW standard cursors; X11 window or NSWindow handle; the content scale on X11, 1 on macOS (points; ImGui 1.92 rasterises the fonts at the Retina framebuffer's density on its own) |
| `gfx/native_font_loader.hpp` | DirectWrite ClearType (`native_font_loader_dwrite.cpp`) | none (`native_font_loader_none.cpp`): FreeType with the gamma loader |
| `platform/main_loop.hpp`: the frame loop, `wait_for_events`, the GLSL version, `relaunch` | `main_loop_desktop.cpp` (shared): a `while` loop, `glfwWaitEventsTimeout`, `#version 330`, `spawn_detached(executable_path())` | the same |
| `platform/http.hpp`: `http_get` (the Gravatars) | `http_cpr.cpp` (shared): cpr, one keep-alive session per thread, the system proxy | the same |

The browser build (`*_web.cpp`, `main_loop_web.cpp`, `http_web.cpp`) has its own column in `docs/WEB.md`.

The portable replacements are cross-platform libraries, the same on every OS:

- **stb_image + stb_image_resize2** (`gfx/image.*`) replace WIC. The box filter on alpha-weighted pixels gives the
  same pixels as WIC's Fant scaler at the sizes ImGit draws.
- **cpr** (libcurl, Schannel on Windows) replaces `URLDownloadToFileW` for Gravatars. It keeps one keep-alive
  session per worker thread and uses the system proxy on Windows.
- **A portable md5** (`app/md5.*`) replaces BCrypt.

Verified on 2026-10-25:

- A Windows debug build; scripted UI runs compared with the build before the change.
  - The main view and the Profiles page are pixel-identical.
  - The cached Gravatars are byte-identical.
  - The terminal runs PowerShell through ConPTY.
- 21 Windows checks (`win_test.cpp` in the session scratchpad): quoting, environment, stdin, exit codes, git CLI,
  ConPTY, proxy.
- 33 Linux checks in WSL against the POSIX sources: processes, quoting with `$` and backticks, forkpty, md5 vectors
  and image decode.
- A `g++ -fsyntax-only` pass over every source a Linux build compiles.

Not verified:

- The file dialogs were not clicked through.
- Nothing was built on macOS by hand. The Linux build is described below, the macOS port after it.

## Lost in the move

- **The folder picker title.** "Select a repository to open" is gone, because nativefiledialog-extended has no
  title parameter.

## Linux build (2026-10-25)

`cmake --preset linux-debug` / `cmake --build --preset linux-debug` builds and links with g++ 15 on Ubuntu 26.04 (WSL),
vcpkg triplet `x64-linux` (installed into `build/vcpkg_installed_linux`, apart from the Windows tree). No source
changes were needed beyond the platform layer.

The dependencies needed one fix. nativefiledialog-extended's portal backend needs libdbus, and the dbus port's
systemd default pulls in libsystemd → libxcrypt, which needs autotools from apt. vcpkg does not let a top-level
`default-features: false` override that, so `vcpkg-overlays/dbus` (registered in `vcpkg-configuration.json`) is the
baseline port with the default removed.

A scripted run under WSLg rendered the same screens as Windows:

- Graph, Gravatars (cpr with OpenSSL), the Profiles page and the Add Profile icon carousel (stb_image).
- The terminal panel running `bash -l` through forkpty.
- Settings migrated from `~/.config/imgit` into `~/.imgit`.

`system_font_file` asks `fc-match`. `ui::font_em_scale` now reads (usWinAscent + usWinDescent) / unitsPerEm from
each loaded font; this reproduces the old hard-coded Windows values exactly, and the Windows screenshots are unchanged.

Differences seen on Linux:

- **Text.** It is FreeType grey-scale, not ClearType. The glyphs come out slightly wider, so "main" in the
  checked-out branch pill truncates to "ma…".
- **File dialogs.** WSL has no `xdg-desktop-portal`, so they cannot open there; not tried on a desktop Linux.
- **WSL speed.** A repository under `/mnt/c` sits on WSL's 9P share, where one status read took about 3 s, because
  libgit2 rehashes the files when the index's stat data, written by Windows git, does not match. The 2-second status
  poll used to run on the UI thread and blocked it almost all the time. It now runs on a worker thread
  (`RepoState::poll_status`), and only the initial load still blocks. WSLg renders with llvmpipe (software) by
  default. `GALLIUM_DRIVER=d3d12` reaches the GPU but was slower here, because each frame is copied back for
  presenting.
- **Not exercised.** A network fetch from the UI (the script's Ctrl+L does nothing on either platform), and the
  release preset.

## macOS (2026-10-25)

Added without a Mac at hand: the sources were syntax-checked on Linux and the macOS job of `build.yml` is the first
real build. What the port consists of:

- **Build.** `macos-debug` / `macos-release` presets (vcpkg's default `arm64-osx` / `x64-osx` triplet, overridden by
  `vcpkg-triplets/` to carry `VCPKG_OSX_DEPLOYMENT_TARGET 12.0`, the same release as `CMAKE_OSX_DEPLOYMENT_TARGET`),
  and the `ci-macos` preset (`arm64-osx`) for the `macos-15` runner. The target is an app bundle, `ImGit.app`
  (`res/Info.plist.in`, `res/imgit.icns` from `tools/app_icon.py`), so Finder and the Dock show the icon; the
  package step strips, ad-hoc signs (`codesign --sign -`) and zips it with `ditto`.
- **GL.** `apply_window_hints()` sets `GLFW_OPENGL_FORWARD_COMPAT`, which Cocoa requires for the 3.3 core profile the
  app asks for. GLFW's header includes the legacy `<OpenGL/gl.h>` with `GL_SILENCE_DEPRECATION`; `lcd_renderer`
  loads the 2.0+ entry points itself, and the `#version 330 core` shaders run on Apple's 3.3 / 4.1 core contexts.
- **Retina.** Cocoa windows are measured in points, and `GLFW_SCALE_TO_MONITOR` does nothing there, so
  `layout_scale()` returns 1 on macOS (the content scale elsewhere) and the window's size limits stay in points.
  ImGui's GLFW backend reports the 2x framebuffer through `DisplayFramebufferScale`, which ImGui 1.92 uses as the
  fonts' rasteriser density, and `lcd_renderer` scales its viewport and scissor rectangles by it.
- **Keyboard.** The GLFW backend sets `io.ConfigMacOSXBehaviors`, and ImGui then swaps Ctrl and Super as a backend's
  key events arrive: every `ImGuiMod_Ctrl` chord in the app means ⌘, `io.KeyCtrl` is ⌘ held and `io.KeySuper` the
  Control key. `ui::shortcut_label()` turns the Windows labels (`Ctrl`, `Alt`) into `Cmd` and `Option` on macOS at the
  places they are shown (menu accelerators, tooltips, the shortcuts dialog, the rebase hint, two filter
  placeholders); `ui::shortcut_modifier_event()` is the modifier the menus feed `io.AddKeyEvent()` for a synthesized
  chord (Super on macOS, which the swap turns into Ctrl). The terminal panel takes the Control key from `io.KeySuper`
  there for its control sequences and ⌘ (`io.KeyCtrl`) for copy / paste and the app's chords; its menu's *Clear*
  keeps the `Ctrl+L` label because that is the shell's own key.
- **Window.** `window_mac.mm` gives the NSWindow the dark appearance (`NSAppearanceNameDarkAqua`), so the title bar
  matches the dark UI whatever the system setting. GLFW ignores `glfwSetWindowIcon` on macOS; the bundle's icon is
  used instead.
- **Title bar.** The system title bar is hidden (`titlebarAppearsTransparent`, `NSWindowStyleMaskFullSizeContentView`,
  an empty compact-style toolbar for the traffic lights' inset, as Electron's `hiddenInset`), so the tabs bar is
  the top of the window with the traffic lights over its left end; `platform::title_bar_inset()` (72 px) keeps the
  folder icon and the tabs clear of them. The bar publishes its empty part (from the + to the right-hand icons)
  through `platform::set_title_bar_drag_region()` each frame; a transparent `ImGitDragView` over GLFW's content
  view hit-tests that region and starts `performWindowDragWithEvent:` from the press itself, or zooms / minimises
  on a double-click per `AppleActionOnDoubleClick`. (Handing AppKit a later "current event" from the ImGui frame
  instead did nothing once the window had been tiled or zoomed.) The presses there never reach ImGui. In full
  screen the toolbar is taken off the window (`NSWindowWillEnterFullScreenNotification`; macOS would keep it as
  an opaque band over the tabs bar) and the inset is 0, the lights being hidden there.
- **Menu bar.** `platform/app_menu.hpp`: on macOS (`app_menu_mac.mm`) the File / Edit / View / Help menus are
  NSMenus between the application menu and the Window menu GLFW creates, and the app draws no bar of its own
  (`has_native_menu_bar()`; the prompt bars and the minimum window height follow the viewport's work area).
  Each menu is rebuilt from `Application::bar_menu()` as it opens (`menuNeedsUpdate:`), every item disabled while
  a modal dialog is up; a click is queued and run by `draw_menu_bar` on the next frame, like the in-app menus'
  deferred clicks. The items show their shortcuts as key equivalents but `menuHasKeyEquivalent:` returns NO, so
  the keystrokes still reach the window and the app's own shortcut handling, which knows about focused fields and
  dialogs. Elsewhere `app_menu_none.cpp` does nothing.
- **Text.** `gfx::set_subpixel_atlas()` tells `lcd_renderer` whether the atlas is the native loader's 3x1
  sub-pixel coverage. Before it, the renderer sampled every atlas per sub-pixel, and a FreeType atlas (one
  grey coverage per texel) came out smeared a third of a pixel to each side with red and cyan fringes, most
  visibly on bold text; it is now drawn like any other texture (this also applies to Linux). The FreeType
  coverage gamma follows the framebuffer density (`freetype_gamma()` in `main.cpp`: 3.0 at 1x, 1.5 on Retina,
  `IMGIT_FREETYPE_GAMMA` to try others), since the 1x boost makes bold stems soft at 2x. ImGui 1.92 bakes a font
  at the framebuffer's density only while it is the current font; the fonts `ui::draw_text` hands to
  `ImDrawList::AddText` (bold, semibold) kept density 1 and came out pixel-doubled on Retina, so `main.cpp` sets
  every font's `CurrentRasterizerDensity` to the framebuffer scale each frame.
- **System.** `system_font_file` tries San Francisco (`SFNS.ttf`), Helvetica Neue and Helvetica for the UI face,
  Menlo, SF Mono and Monaco for the monospace one, and Arial (Supplemental) for the tooltips. The shell is `$SHELL
  -l` (zsh), *Open External Terminal* is `open -a Terminal`, *Show in Folder* is `open -R`. Settings live in
  `~/.imgit`.

Not verified on macOS: everything above beyond compiling the portable sources on Linux. The CI smoke test on the
macOS runner is `continue-on-error`, because the virtual GPU's GL support is not guaranteed; its screenshot is
uploaded when it works.

## Browser (2026-10-27)

The `web-debug` / `web-release` presets build `imgit.html` / `.js` / `.wasm` with emsdk; `docs/WEB.md` covers the
build, the hosting headers and what works. In short:

- **Libraries.** vcpkg has no wasm builds of the GLFW binding the app needs, so `cmake/WebDependencies.cmake` builds
  ImGui, FreeType, libgit2 (no HTTPS / SSH) and stb from source at the baseline's versions. GLFW is emsdk's
  `contrib.glfw3` port (GLFW 3.4). cpr and nativefiledialog-extended are left out.
- **GL.** WebGL 2 via `GLFW_INCLUDE_ES3`, and ImGui's shaders use `#version 300 es`. `lcd_renderer` needs
  `glDrawElementsBaseVertex` and dual-source blending, which WebGL lacks, so its init fails and ImGui's own OpenGL 3
  renderer draws (the FreeType path, as on Linux).
- **Loop.** `emscripten_set_main_loop_arg` with `simulate_infinite_loop`, so main never returns and its locals (the
  `Application`) live for the page. The state is saved every 10 s and when the page is hidden, then written to
  IndexedDB.
- **Threads.** `-pthread` with a pool of 8 workers started before main(), so the background jobs start at once. A
  thread beyond the pool waits for a new web worker to load.
- **Processes.** There are none: `cli::available()` is false, the terminal cannot start, and the shell actions do
  nothing. The folder picker is the browser's `prompt()`.

Verified on 2026-10-27, without a current emsdk (only Ubuntu's emscripten 3.1.6 was installable here):

- libgit2 1.9.4, FreeType 2.14.3 and ImGui 1.92.8 compiled with emcc 3.1.6. ImGui used emscripten-glfw's headers
  in place of the port, which emcc 3.1.6 predates.
- Every ImGit source passed a wasm32 syntax check with `-Wall -Wextra -Wpedantic -Werror`, using clang 18,
  libc++ 18 and the Emscripten sysroot. emcc 3.1.6's own libc++ has no `<expected>`.
- The first link and the first run of the page are CI's `Browser (WebAssembly)` job (`build.yml`).

## Still to do

1. **Text on Linux and macOS.** A FreeType LCD loader (`FT_RENDER_MODE_LCD`) feeding `lcd_renderer`, and checking
   glyph advances against Chromium on Linux. On macOS, Core Text's grey-scale rendering with font smoothing as the
   native loader.
2. **macOS.**
   - A universal (arm64 + x86_64) build or an `x64-osx` release package (`macos-15-intel` runner).
   - The system proxy (`CFNetworkCopySystemProxySettings`); curl only reads `HTTPS_PROXY` today.
   - ⌘ glyphs in the in-app labels (Open Sans has none); the NSMenus show the real key equivalents.
   - Code signing with a Developer ID and notarisation for the release package.
3. **Windows-only content.**
   - The Windows 11 menu bar.
   - `C:\projects` placeholders, the `sh.exe` / Git for Windows preference rows, and `core.longpaths`.
   - The PowerShell terminal menu (`menus.cpp`).
4. **CI.** Done: a windows / ubuntu / macos matrix (`build.yml`); Linux runs `--screenshot` under `xvfb-run`.
