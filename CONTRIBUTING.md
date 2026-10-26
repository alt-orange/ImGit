# Contributing

## Prerequisites

- A C++23 compiler: clang-cl or MSVC from Visual Studio 2022+ on Windows, g++ 13+ or clang 17+ on Linux, Xcode (or
  the Command Line Tools) 15+ on macOS
- CMake 3.25+ and Ninja
- [vcpkg](https://github.com/microsoft/vcpkg) (the Visual Studio bundle on Windows, a checkout in `VCPKG_ROOT`
  elsewhere); the dependencies come from `vcpkg.json`
- Git, on `PATH` (ImGit runs it for fetch, pull, push, clone and rebase)
- Familiarity with the codebase (see `INSTALL.md` for setup and the *Source map* in `README.md` for where things live)

For questions, create a draft PR or reach out via [Telegram](https://t.me/orange_cpp).

## Workflow

1. [Fork](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/working-with-forks/fork-a-repo) the repository.
2. Create a feature branch from `github-main`.
3. Make your changes and make sure the build stays warning-free (see *Tests* below).
4. Open a [pull request](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/proposing-changes-to-your-work-with-pull-requests/creating-a-pull-request-from-a-fork) against `github-main`.
   CI builds the PR on Windows, Linux and macOS and takes a headless screenshot as a smoke test.

Keep a pull request to one change. Describe what it does and how you verified it; a screenshot helps for anything
visible in the UI.

## Code Style

Follow the project `.clang-format` (LLVM base, Stroustrup braces, 4-space indent, 120 columns). Run `clang-format`
before committing.

Anything that differs per OS goes behind a header in `src/platform/`, with one implementation per platform chosen in
`CMakeLists.txt`; no `#ifdef _WIN32` or `__APPLE__` outside that directory. `PORTING.md` lists what each platform
implements.

## Building

Use one of the CMake presets defined in `CMakePresets.json`:

```bash
cmake --preset <preset-name>
cmake --build --preset <preset-name>
```

The presets are `x64-debug` / `x64-release` (Windows), `linux-debug` / `linux-release` and `macos-debug` /
`macos-release`. Run `cmake --list-presets` to see the ones available on your machine; the `ci-*` presets are what the
workflows use.

## Tests

The build treats every warning as an error on every platform (`/W4 /WX` on MSVC and clang-cl,
`-Wall -Wextra -Wpedantic -Werror` elsewhere), so a change must compile cleanly on all three before it can merge.

The unit tests (`test/`, GoogleTest) cover the plain C++ code; build them with `-DIMGIT_BUILD_TESTS=ON` and run
`ctest --test-dir build/<preset>`.

### Memory checks

CI hunts memory errors and leaks on Linux with two tools, over both the unit tests and the app itself (run headless
through a screenshot, the File History view with blame, and the scripted tour in `tools/memcheck/session.txt`):

- `linux-asan` builds everything with AddressSanitizer, LeakSanitizer and UBSan: fast enough for the whole tour.
- `linux-valgrind` runs the same binaries, uninstrumented, under Valgrind memcheck: slower, so the app runs without
  the tour, but it also sees reads of uninitialised memory.

Either fails on an invalid access, a definite or indirect leak, or (ASan) undefined behaviour. To run them locally:

```bash
cmake --preset linux-asan               # or linux-valgrind
cmake --build --preset linux-asan
ctest --preset linux-asan
tools/memcheck/run_app.sh asan build/linux-asan/imgit .            # valgrind build/linux-valgrind/imgit
```

A dynamic check only sees the code a run reaches, so when you add a view or a dialog that allocates, add a few steps
that open and close it to `session.txt`. Suppressions for reports in system libraries (Mesa, X11, fontconfig, ...)
go in `tools/memcheck/lsan.supp` and `valgrind.supp`; never suppress a stack that is ImGit's own.

For everything else, verify your change by running it against a real repository, and for the UI use the headless mode
CI relies on:

```bash
imgit path/to/repository --screenshot out.bmp [--size WxH] [--select-row N]
imgit path/to/repository --script steps.txt --shots dir
```

The first renders a few frames and exits; the second replays clicks and keys and saves a shot after each step. If you
change what a platform implements, update `PORTING.md`; if you change how the app is built or packaged, update
`INSTALL.md` and the *Building* section of `README.md`.
