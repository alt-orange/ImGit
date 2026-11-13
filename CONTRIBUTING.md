# Contributing

## Prerequisites

- A stable Rust toolchain (1.90 or later) with `rustfmt` and `clippy`
- A C and C++ compiler (MSVC from Visual Studio 2022+ on Windows, gcc or clang on Linux, Xcode or the Command Line
  Tools on macOS): the crates build libgit2 and Dear ImGui from source
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

Format with `cargo fmt --all` and keep `cargo clippy --workspace --all-targets` free of warnings before committing.

Anything that differs per OS goes into `crates/imgit-platform` (or `crates/imgit/src/window/` for the window itself),
behind `cfg` attributes; the rest of the code stays portable.

## Building

```bash
cargo build -p imgit            # debug, with a console for logs
cargo build --release -p imgit
```

## Tests

```bash
cargo test --workspace
```

The library crates have unit and integration tests; the Git ones build scratch repositories in temporary folders.
For the UI, also verify your change by running it against a real repository, with the headless mode CI relies on:

```bash
imgit path/to/repository --screenshot out.bmp [--size WxH] [--select-row N]
imgit path/to/repository --script steps.txt --shots dir
```

The first renders a few frames and exits; the second replays clicks and keys and saves a shot after each step. If you
change how the app is built or packaged, update `INSTALL.md` and the *Building* section of `README.md`.
