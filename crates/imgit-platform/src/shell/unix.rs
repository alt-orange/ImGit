//! The POSIX side of [`super`]: `xdg-open` on Linux, `open` on macOS.

use crate::process::imp::spawn_detached_os;
use std::ffi::OsStr;
use std::io;
use std::path::Path;

/// The desktop's "open this" command.
fn open_with_desktop(target: &OsStr) -> io::Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    spawn_detached_os(&[OsStr::new(opener), target], None)
}

pub(super) fn open_in_file_manager(path: &Path) -> io::Result<()> {
    open_with_desktop(path.as_os_str())
}

#[cfg(target_os = "macos")]
pub(super) fn open_external_terminal(path: &Path) -> io::Result<()> {
    spawn_detached_os(
        &[
            OsStr::new("open"),
            OsStr::new("-a"),
            OsStr::new("Terminal"),
            path.as_os_str(),
        ],
        None,
    )
}

#[cfg(not(target_os = "macos"))]
pub(super) fn open_external_terminal(path: &Path) -> io::Result<()> {
    // $TERMINAL first, then the Debian alternative and the two big desktops' terminals.
    let preferred = std::env::var_os("TERMINAL").filter(|terminal| !terminal.is_empty());
    let known = ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"].map(Into::into);
    let mut result = Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no terminal emulator found",
    ));
    for terminal in preferred.into_iter().chain(known) {
        result = spawn_detached_os(&[terminal], Some(path));
        if result.is_ok() {
            break;
        }
    }
    result
}

pub(super) fn open_in_editor(path: &Path) -> io::Result<()> {
    spawn_detached_os(&[OsStr::new("code"), path.as_os_str()], None)
        .or_else(|_| open_with_desktop(path.as_os_str()))
}

pub(super) fn open_url(url: &str) -> io::Result<()> {
    open_with_desktop(OsStr::new(url))
}

pub(super) fn launch(
    args: &[String],
    workdir: Option<&Path>,
    _hide_console: bool,
) -> io::Result<()> {
    spawn_detached_os(args, workdir)
}

pub(super) fn open_path(path: &Path) -> io::Result<()> {
    open_with_desktop(path.as_os_str())
}

#[cfg(target_os = "macos")]
pub(super) fn show_in_folder(path: &Path) -> io::Result<()> {
    spawn_detached_os(
        &[OsStr::new("open"), OsStr::new("-R"), path.as_os_str()],
        None,
    )
}

#[cfg(not(target_os = "macos"))]
pub(super) fn show_in_folder(path: &Path) -> io::Result<()> {
    // File managers differ in how they select an item; open the containing folder.
    open_with_desktop(path.parent().unwrap_or(path).as_os_str())
}
