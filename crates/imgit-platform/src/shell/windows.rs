//! The Windows side of [`super`]: `ShellExecuteW`.

use crate::process::imp::wide;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
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
    // `code` is a script (code.cmd): its console window stays hidden. A path cmd.exe would rewrite (`%NAME%`)
    // is not handed to it: the system's own opener gets that one.
    check_batch_arguments(OsStr::new("code"), &[path.as_os_str()], None)
        .and_then(|()| {
            shell_execute(
                "open",
                OsStr::new("code"),
                Some(&quoted("", path.as_os_str())),
                None,
                SW_HIDE,
            )
        })
        .or_else(|_| open_path(path))
}

/// Whether a UTF-16 unit cannot travel to a `.cmd` / `.bat` script inside quotes. ShellExecute starts such a
/// script through `cmd.exe /c`, which expands `%NAME%` on the command line before it looks at quotes (a caret
/// does not escape it there, and doubling it is only for a script's own text), so no quoting passes a `%`
/// through unchanged; a `"` ends the quoting, after which `&` and `|` are commands; a line break ends the
/// command line.
fn unsafe_for_batch(unit: u16) -> bool {
    matches!(unit, 0x25 | 0x22 | 0x0A | 0x0D)
}

/// The extensions a program name is completed with, in the order the shell tries them.
const PROGRAM_EXTENSIONS: [&str; 4] = ["com", "exe", "bat", "cmd"];

/// Whether `program` starts a `.cmd` / `.bat` script: it says so, or (no extension of a program) the first
/// file `program` + an extension finds in `directories` is one. A `program` with a folder is looked up there
/// alone.
fn is_batch_script(program: &OsStr, directories: &[PathBuf]) -> bool {
    let program = Path::new(program);
    let extension = program
        .extension()
        .map(|extension| extension.to_ascii_lowercase());
    match extension.as_deref().and_then(OsStr::to_str) {
        Some("cmd" | "bat") => return true,
        Some("exe" | "com") => return false,
        _ => {}
    }
    let own_folder;
    let directories = match program
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        Some(folder) => {
            own_folder = [folder.to_path_buf()];
            &own_folder[..]
        }
        None => directories,
    };
    let Some(file_name) = program.file_name() else {
        return false;
    };
    for directory in directories {
        for extension in PROGRAM_EXTENSIONS {
            let mut name = file_name.to_os_string();
            name.push(".");
            name.push(extension);
            if directory.join(&name).is_file() {
                return matches!(extension, "bat" | "cmd");
            }
        }
    }
    false
}

/// Where the shell looks for a program: the working directory it is started in, then `PATH`.
fn search_directories(workdir: Option<&Path>) -> Vec<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    workdir
        .map(Path::to_path_buf)
        .into_iter()
        .chain(std::env::split_paths(&path))
        .collect()
}

/// Refuses arguments `cmd.exe` would rewrite or that break out of their quotes when `program` is a script.
/// `directories`: where to look for the program; `None`: `PATH`.
fn check_batch_arguments(
    program: &OsStr,
    arguments: &[&OsStr],
    directories: Option<&[PathBuf]>,
) -> io::Result<()> {
    let found;
    let directories = match directories {
        Some(directories) => directories,
        None => {
            found = search_directories(None);
            &found
        }
    };
    if arguments
        .iter()
        .any(|argument| argument.encode_wide().any(unsafe_for_batch))
        && is_batch_script(program, directories)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an argument with %, a quote or a line break cannot be passed to a .cmd script: cmd.exe would change it",
        ));
    }
    Ok(())
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
    let arguments: Vec<&OsStr> = args[1..].iter().map(OsStr::new).collect();
    check_batch_arguments(
        OsStr::new(&args[0]),
        &arguments,
        Some(&search_directories(workdir)),
    )?;
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

    /// A folder with the given (empty) files in it.
    fn folder_with(files: &[&str]) -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        for file in files {
            std::fs::write(folder.path().join(file), "").unwrap();
        }
        folder
    }

    #[test]
    fn finds_out_whether_a_program_is_a_script() {
        let folder = folder_with(&["code.cmd", "tool.exe", "tool.cmd", "both.bat", "both.com"]);
        let directories = [folder.path().to_path_buf()];
        let script = |program: &str| is_batch_script(OsStr::new(program), &directories);
        assert!(script("code"));
        assert!(script("CODE.CMD"));
        assert!(script("anything.bat")); // says so itself, exists or not
        assert!(!script("tool")); // .exe before .cmd
        assert!(!script("tool.exe"));
        assert!(!script("both")); // .com before .bat
        assert!(!script("missing"));
        // A program with a folder is looked up there alone.
        let elsewhere = [folder_with(&[]).path().to_path_buf()];
        assert!(is_batch_script(
            folder.path().join("code").as_os_str(),
            &elsewhere
        ));
        assert!(!is_batch_script(OsStr::new("code"), &elsewhere));
    }

    #[test]
    fn scripts_get_no_argument_cmd_would_change() {
        let folder = folder_with(&["code.cmd", "tool.exe"]);
        let directories = [folder.path().to_path_buf()];
        let check = |program: &str, argument: &str| {
            check_batch_arguments(
                OsStr::new(program),
                &[OsStr::new("--goto"), OsStr::new(argument)],
                Some(&directories),
            )
        };
        assert!(check("code", r"C:\my repo\a&b (1)").is_ok());
        for argument in [r"C:\100%\x", "%PATH%", "a\"b", "a\nb", "a\rb"] {
            let error = check("code", argument).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{argument:?}");
        }
        // A program is left alone.
        assert!(check("tool", "%PATH%").is_ok());
    }

    #[test]
    fn a_script_gets_its_argument_and_refuses_percent_signs() {
        let folder = tempfile::tempdir().unwrap();
        let script = folder.path().join("echo arg.cmd");
        let output = folder.path().join("out.txt");
        // `%~1` is the argument without its quotes.
        std::fs::write(
            &script,
            format!(
                "@echo off\r\n>\"{}\" <nul set /p =%~1\r\n",
                output.display()
            ),
        )
        .unwrap();
        let script_arg = script.to_string_lossy().into_owned();

        // A value cmd.exe would expand, were the argument let through.
        // SAFETY: no other test of this crate reads or writes this variable.
        unsafe { std::env::set_var("IMGIT_TEST_PERCENT", "EXPANDED") };
        let hostile = launch(
            &[script_arg.clone(), "a %IMGIT_TEST_PERCENT% b".to_owned()],
            Some(folder.path()),
            true,
        );
        assert_eq!(hostile.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!output.exists());

        launch(
            &[script_arg, r"C:\my repo\x (1)".to_owned()],
            Some(folder.path()),
            true,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let text = loop {
            if let Ok(text) = std::fs::read_to_string(&output)
                && !text.is_empty()
            {
                break text;
            }
            assert!(std::time::Instant::now() < deadline, "the script never ran");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(text.trim_end(), r"C:\my repo\x (1)");
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
