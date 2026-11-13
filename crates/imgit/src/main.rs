//! ImGit Desktop, the Rust port: a Git client on Dear ImGui.
//!
//! This crate is the application shell and the application itself: the main window, its OpenGL context, Dear
//! ImGui with the app's fonts and text rendering, the development options (`--screenshot`, `--script`), and on
//! top of them the [`app`] layer (tabs, dialogs, settings, toasts, the frame) drawing the panels of
//! [`ui::panels`] through [`shell::Frame`].

// A GUI-subsystem program in release builds (no console window), like the C++ build; debug builds keep the
// console for logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// The shell's API is laid out for the UI port that follows; not all of it has a caller yet.
#![allow(dead_code)]

mod app;
mod app_menu;
mod assets;
mod demo;
mod dev;
mod fonts;
mod gfx;
mod shell;
mod theme;
mod ui;
mod window;

use std::env;
use std::process::ExitCode;

use imgit_app::preferences::{pref, preferences};
use imgit_platform::system::monospace_font_file;

use crate::app::Application;
use crate::app::frame_impl::AppFrame;
use crate::dev::DevOptions;
use crate::fonts::{FontRequest, TextSettings};

/// `--gallery`: the widget gallery instead of the application.
const GALLERY_FLAG: &str = "--gallery";

fn main() -> anyhow::Result<ExitCode> {
    // `git` runs ImGit as its GIT_ASKPASS with the prompt as the only argument: answer it and exit before any
    // window exists.
    if env::args_os().count() == 2
        && imgit_git::cli::is_askpass_invocation()
        && let Some(prompt) = env::args().nth(1)
    {
        return Ok(imgit_git::cli::askpass_main(&prompt));
    }
    let args: Vec<String> = env::args().skip(1).collect();
    let gallery = args.iter().any(|arg| arg == GALLERY_FLAG);
    let options = DevOptions::parse(args.into_iter().filter(|arg| arg != GALLERY_FLAG));
    let text = TextSettings::from_env();

    if gallery {
        let config = shell::Config {
            options,
            fonts: FontRequest::default(),
            text,
        };
        shell::run(config, demo::DemoFrame::new)?;
        return Ok(ExitCode::SUCCESS);
    }

    // The application first: it loads the profile's preferences, which name the Editor and Terminal fonts.
    let app = Application::new();
    let font_file = |preference| monospace_font_file(&preferences().get(preference));
    let config = shell::Config {
        options,
        fonts: FontRequest {
            editor: font_file(pref::EDITOR_FONT),
            terminal: font_file(pref::TERMINAL_FONT),
        },
        text,
    };
    shell::run(config, move |shell| AppFrame::new(app, shell))?;
    Ok(ExitCode::SUCCESS)
}
