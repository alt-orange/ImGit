# 📥Installation Guide

ImGit needs `git` on your `PATH` for network operations (fetch, pull, push, clone, rebase, remote refs, LFS); your
credential helpers and SSH setup apply as usual. Everything else is built into the executable.

## <img width="28px" src="https://github.githubassets.com/favicons/favicon.svg" /> Using prebuilt binaries (GitHub Releases)

**Note**: This is the fastest option if you don’t want to build from source.

1. **Go to the [Releases](https://github.com/alt-orange/ImGit/releases) page** and choose the latest version.

2. **Download the asset for your platform**

   | Platform              | Asset                            |
   |-----------------------|----------------------------------|
   | Windows x64           | `imgit-<tag>-windows-x64.zip`    |
   | Linux x64             | `imgit-<tag>-linux-x64.tar.gz`   |
   | macOS (Apple silicon) | `imgit-<tag>-macos-arm64.zip`    |

   `SHA256SUMS.txt` next to them has the checksums: `sha256sum -c SHA256SUMS.txt` (or `certutil -hashfile` on Windows).

3. **Extract and run**

   - **Windows**: unzip and start `imgit.exe`, optionally with a repository path:
     ```
     imgit.exe path\to\repository
     ```
   - **Linux**: `tar -xzf imgit-<tag>-linux-x64.tar.gz`, then `./imgit path/to/repository`. File dialogs use the
     xdg desktop portal, so install `xdg-desktop-portal` and a backend such as `xdg-desktop-portal-gtk` if your
     desktop does not ship one.
   - **macOS**: unzip to get `ImGit.app` and drop it into *Applications*. The app is only ad-hoc signed, so the first
     launch needs a right-click → *Open*, or:
     ```bash
     xattr -d com.apple.quarantine ImGit.app
     ```

**Note**: The Windows and Linux executables are compressed with UPX. Some antivirus scanners flag UPX-packed files;
`upx -d imgit.exe` restores the original binary.

Settings and profiles live in `%APPDATA%\.imgit` on Windows and `~/.imgit` elsewhere.

## <img width="28px" src="https://upload.wikimedia.org/wikipedia/commons/e/ef/CMake_logo.svg?" /> Build from source using CMake

1. **Preparation**

   Install the tools: a C++23 compiler, CMake 3.25+, Ninja, Git and vcpkg. The dependencies (imgui with the
   GLFW / OpenGL 3 bindings, glfw3, libgit2, cpr, nativefiledialog-extended and stb) come from `vcpkg.json`.

   1. **Windows:**

      Install Visual Studio 2022 or later with the C++ workload, clang-cl, CMake and vcpkg from
      [here](https://visualstudio.microsoft.com/downloads/) and Git from [here](https://git-scm.com/downloads).

      Use the x64 Native Tools shell to execute the commands below. `CMakePresets.json` points
      `CMAKE_TOOLCHAIN_FILE` at the Visual Studio vcpkg bundle; change it (or use
      `$env{VCPKG_ROOT}/scripts/buildsystems/vcpkg.cmake`) for another install.
   2. **Linux:**
      ```bash
      sudo pacman -Sy cmake ninja clang git pkg-config
      # GLFW's X11 packages, Debian / Ubuntu names:
      sudo apt install libxinerama-dev libxcursor-dev xorg-dev libglu1-mesa-dev
      ```
      g++ 13+ or clang 17+ and a vcpkg checkout in `VCPKG_ROOT`.
   3. **macOS:**
      ```bash
      brew install cmake ninja pkg-config autoconf automake libtool
      ```
      Xcode (or the Command Line Tools) 15+ and a vcpkg checkout in `VCPKG_ROOT`.

2. **Clone the repository:**
   ```bash
   git clone https://github.com/alt-orange/ImGit.git
   ```
3. **Navigate to the project directory:**
   ```bash
   cd ImGit
   ```
4. **Build the project using CMake:**
   ```bash
   cmake --preset x64-release
   cmake --build --preset x64-release
   ```
   Use the **\<platform\>-\<build configuration\>** preset for your machine, like **x64-release** or
   **linux-debug**; the debug presets keep a console for logs on Windows.

   | Platform Name | Build Config  |
   |---------------|---------------|
   | x64 (Windows) | release/debug |
   | linux         | release/debug |
   | macos         | release/debug |

5. **Run it:**
   ```bash
   build/x64-release/imgit.exe path/to/repository                        # Windows
   build/linux-release/imgit path/to/repository                          # Linux
   build/macos-release/ImGit.app/Contents/MacOS/ImGit path/to/repository # macOS, or open build/macos-release/ImGit.app
   ```

   The macOS build is an app bundle; vcpkg picks `arm64-osx` on Apple silicon and `x64-osx` on Intel, both with the
   macOS 12 deployment target from `vcpkg-triplets/`. `vcpkg-overlays/dbus` is the stock dbus port without its
   systemd default, for Linux. See `PORTING.md` for what each platform implements.
