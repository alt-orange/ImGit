//! The Windows side of [`super`]: `ShellExecuteW`.

use crate::process::imp::wide;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

/// `ShellExecuteW(verb, file, parameters, directory, show)`.
fn shell_execute(
    verb: &str,
    file: &OsStr,
    parameters: Option<&OsStr>,
    directory: Option<&Path>,
    show: i32,
) -> io::Result<()> {
    let verb = wide(verb);
    let file = wide(file);
    let parameters = parameters.map(wide);
    let directory = directory.map(wide);
    let pointer =
        |text: &Option<Vec<u16>>| text.as_ref().map_or(std::ptr::null(), |text| text.as_ptr());
    // SAFETY: every string is NUL-terminated UTF-16 that lives across the call (or null, where optional); no
    // parent window is given.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            pointer(&parameters),
            pointer(&directory),
            show,
        )
    };
    // Not a real instance handle: a value above 32 means success, anything else is an error code.
    match result as isize {
        32.. => Ok(()),
        // The ones that are plain Win32 error codes: file / path not found, access denied, out of memory.
        code @ (2 | 3 | 5 | 8) => Err(io::Error::from_raw_os_error(code as i32)),
        code => Err(io::Error::other(format!(
            "the shell could not open it (error {code})"
        ))),
    }
}

/// Backslashes, without a trailing one: the shell rejects "C:/repo/".
fn native_path(path: &Path) -> OsString {
    const SLASH: u16 = b'/' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    let mut native: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .map(|unit| if unit == SLASH { BACKSLASH } else { unit })
        .collect();
    while native.len() > 3 && native.last() == Some(&BACKSLASH) {
        native.pop();
    }
    OsString::from_wide(&native)
}

fn quoted(prefix: &str, text: &OsStr) -> OsString {
    let mut quoted = OsString::from(format!("{prefix}\""));
    quoted.push(text);
    quoted.push("\"");
    quoted
}

pub(super) fn open_in_file_manager(path: &Path) -> io::Result<()> {
    shell_execute("explore", path.as_os_str(), None, None, SW_SHOWNORMAL)
}

pub(super) fn open_external_terminal(path: &Path) -> io::Result<()> {
    shell_execute(
        "open",
        OsStr::new("powershell.exe"),
        None,
        Some(path),
        SW_SHOWNORMAL,
    )
}

pub(super) fn open_in_editor(path: &Path) -> io::Result<()> {
    // `code` is a script (code.cmd): its console window stays hidden.
    shell_execute(
        "open",
        OsStr::new("code"),
        Some(&quoted("", path.as_os_str())),
        None,
        SW_HIDE,
    )
    .or_else(|_| open_path(path))
}

pub(super) fn open_url(url: &str) -> io::Result<()> {
    shell_execute("open", OsStr::new(url), None, None, SW_SHOWNORMAL)
}

pub(super) fn launch(
    args: &[String],
    workdir: Option<&Path>,
    hide_console: bool,
) -> io::Result<()> {
    // ShellExecute finds scripts (code.cmd) on PATH as well as programs; CreateProcess only takes programs.
    let parameters = args[1..]
        .iter()
        .map(|arg| format!("\"{arg}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let show = if hide_console { SW_HIDE } else { SW_SHOWNORMAL };
    shell_execute(
        "open",
        OsStr::new(&args[0]),
        Some(OsStr::new(&parameters)),
        workdir,
        show,
    )
}

pub(super) fn open_path(path: &Path) -> io::Result<()> {
    shell_execute("open", path.as_os_str(), None, None, SW_SHOWNORMAL)
}

pub(super) fn show_in_folder(path: &Path) -> io::Result<()> {
    let parameters = quoted("/select,", &native_path(path));
    shell_execute(
        "open",
        OsStr::new("explorer.exe"),
        Some(&parameters),
        None,
        SW_SHOWNORMAL,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_paths_native() {
        assert_eq!(
            native_path(Path::new("C:/repo/sub/")),
            OsStr::new(r"C:\repo\sub")
        );
        assert_eq!(native_path(Path::new(r"C:\repo\\")), OsStr::new(r"C:\repo"));
        assert_eq!(native_path(Path::new("C:/")), OsStr::new(r"C:\"));
        assert_eq!(
            native_path(Path::new("C:/file.txt")),
            OsStr::new(r"C:\file.txt")
        );
    }

    #[test]
    fn quotes_shell_parameters() {
        assert_eq!(
            quoted("", OsStr::new(r"C:\my repo")),
            OsStr::new("\"C:\\my repo\"")
        );
        assert_eq!(
            quoted("/select,", OsStr::new(r"C:\a b\c.txt")),
            OsStr::new("/select,\"C:\\a b\\c.txt\"")
        );
    }
}
