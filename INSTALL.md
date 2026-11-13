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

## Build from source using Cargo

1. **Preparation**

   Install a stable Rust toolchain (1.90 or later) with [rustup](https://rustup.rs), Git, and a C and C++ compiler:
   the crates build their two C libraries (libgit2 and Dear ImGui) from source.

   1. **Windows:**

      Install Visual Studio 2022 or later with the C++ workload from
      [here](https://visualstudio.microsoft.com/downloads/) and Git from [here](https://git-scm.com/downloads).
      This is the platform the Rust build is developed and tested on.
   2. **Linux:**
      ```bash
      # Debian / Ubuntu names
      sudo apt install build-essential pkg-config libssl-dev
      ```
      The Rust code type-checks for Linux, but this build has not been linked or run yet.
   3. **macOS:**

      Xcode (or the Command Line Tools) 15+. The Rust code type-checks for macOS, but this build has not been linked
      or run yet, and the app bundle and native menu bar are not ported.

2. **Clone the repository:**
   ```bash
   git clone https://github.com/alt-orange/ImGit.git
   ```
3. **Navigate to the project directory:**
   ```bash
   cd ImGit
   ```
4. **Build the project using Cargo:**
   ```bash
   cargo build --release -p imgit
   ```
   A debug build (`cargo build -p imgit`) keeps a console for logs on Windows.

5. **Run it:**
   ```bash
   target/release/imgit path/to/repository
   ```
