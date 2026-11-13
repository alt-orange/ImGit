//! Opening things outside the app: "Open in File Manager", "Open External Terminal", "Open Repo in External
//! Editor", links, and the system's file dialogs.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
#[path = "shell/unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "shell/windows.rs"]
mod imp;

/// Opens the folder `path` in the system's file manager.
///
/// # Errors
/// The file manager could not be started.
pub fn open_in_file_manager(path: &Path) -> io::Result<()> {
    imp::open_in_file_manager(path)
}

/// Opens a terminal window in the folder `path`: PowerShell on Windows, Terminal on macOS, elsewhere `$TERMINAL`,
/// then the Debian alternative (`x-terminal-emulator`) and gnome-terminal, konsole, xterm.
///
/// # Errors
/// No terminal could be started.
pub fn open_external_terminal(path: &Path) -> io::Result<()> {
    imp::open_external_terminal(path)
}

/// Opens `path` in an editor: tries `code`, then the system default for folders.
///
/// # Errors
/// Neither could be started.
pub fn open_in_editor(path: &Path) -> io::Result<()> {
    imp::open_in_editor(path)
}

/// Opens `url` in the default browser.
///
/// # Errors
/// The system has nothing to open it with.
pub fn open_url(url: &str) -> io::Result<()> {
    imp::open_url(url)
}

/// Starts `args` (`args[0]` looked up on PATH; on Windows also a .cmd / .bat script, as `code` is) in `workdir`
/// without waiting. `hide_console`: a script's console window stays hidden (Windows only).
///
/// On Windows the arguments go through the shell, each wrapped in plain quotes: one that itself holds a quote,
/// or ends in a backslash, does not arrive verbatim.
///
/// # Errors
/// It could not be started.
pub fn launch(args: &[String], workdir: Option<&Path>, hide_console: bool) -> io::Result<()> {
    if args.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no program to run",
        ));
    }
    imp::launch(
        args,
        workdir.filter(|path| !path.as_os_str().is_empty()),
        hide_console,
    )
}

/// Opens a file with its default program.
///
/// # Errors
/// The system has nothing to open it with.
pub fn open_path(path: &Path) -> io::Result<()> {
    imp::open_path(path)
}

/// Shows `path` selected in the file manager (Explorer, Finder); elsewhere opens the folder it is in, as file
/// managers differ in how they select an item.
///
/// # Errors
/// The file manager could not be started.
pub fn show_in_folder(path: &Path) -> io::Result<()> {
    imp::show_in_folder(path)
}

/// A window a file dialog can be parented to (it stays above that window and blocks it): anything that hands
/// out raw window and display handles, as the windowing crates' windows do.
pub trait DialogParent: HasWindowHandle + HasDisplayHandle {}

impl<T: HasWindowHandle + HasDisplayHandle + ?Sized> DialogParent for T {}

fn file_dialog(parent: Option<&dyn DialogParent>, extension: Option<&str>) -> rfd::FileDialog {
    let mut dialog = rfd::FileDialog::new();
    if let Some(parent) = parent {
        dialog = dialog.set_parent(parent);
    }
    let extension = extension
        .map(|extension| extension.trim_start_matches('.'))
        .filter(|extension| !extension.is_empty());
    if let Some(extension) = extension {
        dialog = dialog.add_filter(format!("{extension} files"), &[extension]);
    }
    dialog
}

/// A native "save file" dialog filtered to `extension` ("patch"; any file when `None`), with `default_name`
/// filled in. `None` when cancelled.
///
/// The dialogs block until they are closed, and macOS only allows them on the main thread.
#[must_use]
pub fn save_file_dialog(
    parent: Option<&dyn DialogParent>,
    default_name: Option<&str>,
    extension: Option<&str>,
) -> Option<PathBuf> {
    let mut dialog = file_dialog(parent, extension);
    if let Some(name) = default_name.filter(|name| !name.is_empty()) {
        dialog = dialog.set_file_name(name);
    }
    dialog.save_file()
}

/// A native "open file" dialog filtered to `extension` (any file when `None`). `None` when cancelled.
#[must_use]
pub fn open_file_dialog(
    parent: Option<&dyn DialogParent>,
    extension: Option<&str>,
) -> Option<PathBuf> {
    file_dialog(parent, extension).pick_file()
}

/// A native folder picker that starts in `start` when that folder exists. `None` when cancelled.
#[must_use]
pub fn pick_folder_dialog(
    parent: Option<&dyn DialogParent>,
    start: Option<&Path>,
) -> Option<PathBuf> {
    let mut dialog = file_dialog(parent, None);
    if let Some(start) = start.and_then(dialog_folder) {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_folder()
}

/// `start` the way the system's dialogs take a folder (the Windows shell rejects "C:/repo/"): native separators,
/// no trailing one. `None` when it is not an existing folder.
fn dialog_folder(start: &Path) -> Option<PathBuf> {
    start.is_dir().then(|| start.components().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_the_folder_a_dialog_starts_in() {
        let folder = tempfile::tempdir().unwrap();
        let native = folder.path().components().collect::<PathBuf>();
        let with_slashes = format!("{}/", folder.path().to_str().unwrap().replace('\\', "/"));
        assert_eq!(
            dialog_folder(Path::new(&with_slashes)),
            Some(native.clone())
        );
        let prepared = dialog_folder(Path::new(&with_slashes)).unwrap();
        let text = prepared.to_str().unwrap();
        assert!(!text.ends_with(['/', '\\']));
        assert!(!cfg!(windows) || !text.contains('/'));
        assert_eq!(dialog_folder(&folder.path().join("missing")), None);
        assert_eq!(dialog_folder(Path::new("")), None);
    }

    #[test]
    fn refuses_to_launch_nothing() {
        assert!(launch(&[], None, true).is_err());
    }
}
