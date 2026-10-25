# Porting ImGit off Windows

## Where platform code lives

Since 2026-10-25 every OS call sits behind a header in `src/platform/`, with one implementation per platform chosen in
`CMakeLists.txt` (`if(WIN32)` → `*_win32.cpp`, else `*_posix.cpp`). No other source has an `#ifdef _WIN32`. The only
remaining ones are inside `platform/` and the GL calling-convention macro in `gfx/lcd_renderer.cpp`.

| Header | Windows | POSIX (Linux / macOS) |
|---|---|---|
| `platform/process.hpp`: `run_process` (args, stdin, env, merged output, exit code), `spawn_detached`, `executable_path` | `CreateProcessW` with pipes, `CREATE_NO_WINDOW`, an environment block, CommandLineToArgv-correct quoting | `posix_spawnp` + pipes + `addchdir_np`, argv passed verbatim (no shell), `WEXITSTATUS`; `/proc/self/exe` or `_NSGetExecutablePath` |
| `platform/pseudo_console.hpp`: the terminal panel's shell | ConPTY (`CreatePseudoConsole`, looked up at run time) | `forkpty` + `execve` (`TERM=xterm-256color`), `TIOCSWINSZ`, `waitpid(WNOHANG)`, SIGHUP on close |
| `platform/shell.hpp`: open file manager / terminal / editor / URL / path, show in folder, file dialogs | `ShellExecuteW`, `explorer /select,` | `xdg-open` / `open`, `open -R`, `$TERMINAL` → `x-terminal-emulator` → gnome-terminal → konsole → xterm |
| ↳ file dialogs (`shell_dialogs.cpp`, shared) | nativefiledialog-extended, parented to the main window | the same (GTK or portal on Linux, NSOpenPanel on macOS) |
| `platform/system.hpp`: home, app data and config folders, `local_time`, menu show delay, system font files, default shell, system proxy | `%USERPROFILE%`, `%APPDATA%`, `localtime_s`, `SPI_GETMENUSHOWDELAY`, `%WINDIR%\Fonts`, PowerShell, `WinHttpGetIEProxyConfigForCurrentUser` | `$HOME`, `$XDG_CONFIG_HOME`, `localtime_r`, 400 ms, *no fonts yet*, `$SHELL -l`, none (curl reads `HTTPS_PROXY` itself) |
| `platform/window.hpp`: `init_window`, `set_cursor` / `reset_cursor`, `native_window` | dark caption (DWM), Chromium's resize cursors through a `WM_SETCURSOR` hook, ole32's drag-and-drop cursors | GLFW standard cursors; X11 window or NSWindow handle |
| `gfx/native_font_loader.hpp` | DirectWrite ClearType (`native_font_loader_dwrite.cpp`) | none (`native_font_loader_none.cpp`): FreeType with the gamma loader |

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
- Nothing was built on macOS. The Linux build is described below.

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

## Still to do

1. **Text on Linux.** A FreeType LCD loader (`FT_RENDER_MODE_LCD`) feeding `lcd_renderer`, and checking glyph
   advances against Chromium on Linux.
2. **macOS.**
   - `GLFW_OPENGL_FORWARD_COMPAT`.
   - `<OpenGL/gl3.h>` in `lcd_renderer`.
   - The dark appearance (`window_mac.mm`).
   - An app bundle.
   - ⌘ in the accelerator labels.
3. **Windows-only content.**
   - The `Ctrl+…` labels.
   - The Windows 11 menu bar.
   - `C:\projects` placeholders, the `sh.exe` / Git for Windows preference rows, and `core.longpaths`.
   - The PowerShell terminal menu (`menus.cpp`).
4. **CI.** A windows / ubuntu / macos matrix; Linux needs the X11 dev packages plus `xvfb-run` for `--screenshot`.
