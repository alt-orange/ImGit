//! The Windows side of [`super`].

use super::{MonospaceFont, Shell, SystemFont, https_proxy_of};
use crate::process::{RunOptions, run_process};
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use windows_sys::Win32::Foundation::{GlobalFree, MAX_PATH};
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WinHttpGetIEProxyConfigForCurrentUser,
};
use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETMENUSHOWDELAY, SystemParametersInfoW};

/// Looks up an environment variable; `None` when it is unset or empty.
pub(super) type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;

pub(super) fn home_directory(environment: Environment) -> PathBuf {
    ["USERPROFILE", "HOME"]
        .into_iter()
        .find_map(environment)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

pub(super) fn app_data_directory(environment: Environment) -> PathBuf {
    // The variable rather than the known folder, so a test run can point it elsewhere.
    environment("APPDATA").map_or_else(|| home_directory(environment), PathBuf::from)
}

pub(super) fn config_directory(environment: Environment) -> PathBuf {
    app_data_directory(environment)
}

pub(super) fn local_tm(unix_seconds: i64) -> Option<libc::tm> {
    // SAFETY: `tm` is plain integers, for which all zeroes is a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers refer to live, properly aligned values of the types localtime_s expects.
    let error = unsafe { libc::localtime_s(&mut tm, &unix_seconds) };
    (error == 0).then_some(tm)
}

pub(super) fn system_locale(_environment: Environment) -> Option<String> {
    const LOCALE_NAME_MAX_LENGTH: usize = 85;
    let mut name = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: the buffer is writable for the length passed.
    let length = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    // The length counts the terminating NUL.
    (length > 1).then(|| String::from_utf16_lossy(&name[..length as usize - 1]))
}

pub(super) fn menu_show_delay() -> Duration {
    let mut milliseconds: u32 = 400;
    // SAFETY: SPI_GETMENUSHOWDELAY writes one DWORD through the pointer, which refers to a live u32.
    unsafe {
        SystemParametersInfoW(
            SPI_GETMENUSHOWDELAY,
            0,
            (&raw mut milliseconds).cast::<c_void>(),
            0,
        );
    }
    Duration::from_millis(u64::from(milliseconds))
}

/// `%WINDIR%`, as the system reports it.
fn windows_directory() -> Option<PathBuf> {
    let mut buffer = [0u16; MAX_PATH as usize];
    // SAFETY: the buffer is writable for the length passed.
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    (length > 0 && length < buffer.len())
        .then(|| PathBuf::from(OsString::from_wide(&buffer[..length])))
}

pub(super) fn system_font_file(font: SystemFont) -> Option<PathBuf> {
    let file = match font {
        SystemFont::Ui => "segoeui.ttf",
        SystemFont::Monospace => "consola.ttf",
        SystemFont::Arial => "arial.ttf",
    };
    let path = windows_directory()?.join("Fonts").join(file);
    path.exists().then_some(path)
}

/// The faces Windows ships, then popular ones a user installs (for everyone, or for themselves only).
pub(super) fn installed_monospace_fonts() -> Vec<MonospaceFont> {
    const KNOWN: [(&str, &str); 9] = [
        ("Cascadia Code", "CascadiaCode.ttf"),
        ("Cascadia Mono", "CascadiaMono.ttf"),
        ("Consolas", "consola.ttf"),
        ("Courier New", "cour.ttf"),
        ("Lucida Console", "lucon.ttf"),
        ("Fira Code", "FiraCode-Regular.ttf"),
        ("Hack", "Hack-Regular.ttf"),
        ("JetBrains Mono", "JetBrainsMono-Regular.ttf"),
        ("Source Code Pro", "SourceCodePro-Regular.ttf"),
    ];
    let mut folders = Vec::new();
    if let Some(windows) = windows_directory() {
        folders.push(windows.join("Fonts"));
    }
    if let Some(local) = super::environment("LOCALAPPDATA") {
        folders.push(
            Path::new(&local)
                .join("Microsoft")
                .join("Windows")
                .join("Fonts"),
        );
    }
    KNOWN
        .into_iter()
        .filter_map(|(name, file)| {
            let file = folders
                .iter()
                .map(|folder| folder.join(file))
                .find(|path| path.exists())?;
            Some(MonospaceFont { name, file })
        })
        .collect()
}

/// Git for Windows' bash: next to the git on PATH (`<git>/mingw64/libexec/git-core` -> `<git>/bin/bash.exe`),
/// else where its installer puts it. `None` when there is none. Looked up once.
fn git_bash() -> Option<&'static Path> {
    static BASH: OnceLock<Option<PathBuf>> = OnceLock::new();
    BASH.get_or_init(|| {
        let mut candidates = Vec::new();
        let exec_path = run_process(
            Path::new(""),
            &["git".to_owned(), "--exec-path".to_owned()],
            RunOptions::default(),
        );
        if let Ok(output) = exec_path
            && output.exit_code == 0
            && let Some(root) = Path::new(output.text.trim_end()).ancestors().nth(3)
        {
            candidates.push(root.join("bin").join("bash.exe"));
        }
        for variable in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
            if let Some(folder) = super::environment(variable) {
                let mut base = PathBuf::from(folder);
                if variable == "LOCALAPPDATA" {
                    base.push("Programs");
                }
                candidates.push(base.join("Git").join("bin").join("bash.exe"));
            }
        }
        candidates.into_iter().find(|candidate| candidate.is_file())
    })
    .as_deref()
}

pub(super) fn default_shell(_environment: Environment) -> Shell {
    // The default terminal on Windows.
    Shell::new(
        &[
            "powershell.exe",
            "-NoLogo",
            "-ExecutionPolicy",
            "RemoteSigned",
        ],
        "powershell.exe",
    )
}

pub(super) fn terminal_shell_names() -> Vec<String> {
    let mut names = vec!["PowerShell".to_owned(), "Command Prompt".to_owned()];
    if git_bash().is_some() {
        names.push("Git Bash".to_owned());
    }
    names
}

pub(super) fn terminal_shell(name: &str) -> Option<Shell> {
    match name {
        "Command Prompt" => Some(Shell::new(&["cmd.exe"], "cmd.exe")),
        // A login shell, as Git Bash's own shortcut starts it, so ~/.bash_profile runs.
        "Git Bash" => {
            git_bash().map(|bash| Shell::new(&[&bash.to_string_lossy(), "--login", "-i"], "bash"))
        }
        _ => None,
    }
}

pub(super) fn system_proxy() -> Option<String> {
    // The Internet Options proxy, which URLDownloadToFile (and Chromium) use. A PAC script is not evaluated.
    let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    // SAFETY: `config` is a live, writable structure of the type the function fills in.
    if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) } == 0 {
        return None;
    }
    let proxy = if config.lpszProxy.is_null() {
        String::new()
    } else {
        // SAFETY: on success a non-null lpszProxy points to a NUL-terminated UTF-16 string, so every offset up
        // to and including the terminator is readable.
        let text = unsafe {
            let length = (0..).take_while(|&i| *config.lpszProxy.add(i) != 0).count();
            std::slice::from_raw_parts(config.lpszProxy, length)
        };
        String::from_utf16_lossy(text)
    };
    for text in [
        config.lpszProxy,
        config.lpszProxyBypass,
        config.lpszAutoConfigUrl,
    ] {
        if !text.is_null() {
            // SAFETY: the function allocated each non-null string with GlobalAlloc and hands its ownership to the
            // caller; none is used after this.
            unsafe {
                GlobalFree(text.cast::<c_void>());
            }
        }
    }
    https_proxy_of(&proxy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake<'a>(variables: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            variables
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn folders_follow_the_environment() {
        let both = fake(&[
            ("USERPROFILE", r"C:\Users\ada"),
            ("HOME", r"C:\msys\home\ada"),
            ("APPDATA", r"D:\Roaming"),
        ]);
        assert_eq!(home_directory(&both), Path::new(r"C:\Users\ada"));
        assert_eq!(app_data_directory(&both), Path::new(r"D:\Roaming"));
        assert_eq!(config_directory(&both), Path::new(r"D:\Roaming"));

        let home_only = fake(&[("HOME", r"C:\msys\home\ada")]);
        assert_eq!(home_directory(&home_only), Path::new(r"C:\msys\home\ada"));
        // Without APPDATA the home folder stands in.
        assert_eq!(
            app_data_directory(&home_only),
            Path::new(r"C:\msys\home\ada")
        );
        assert_eq!(config_directory(&home_only), Path::new(r"C:\msys\home\ada"));

        let nothing = fake(&[]);
        assert_eq!(home_directory(&nothing), std::env::current_dir().unwrap());
    }

    #[test]
    fn knows_the_windows_fonts() {
        assert!(windows_directory().is_some_and(|folder| folder.is_dir()));
        // Every Windows installation ships these.
        assert!(system_font_file(SystemFont::Ui).is_some());
        assert!(system_font_file(SystemFont::Monospace).is_some());
        assert!(super::super::monospace_font_names().contains(&"Consolas"));
    }

    #[test]
    fn names_the_shells() {
        assert_eq!(default_shell(&fake(&[])).args[0], "powershell.exe");
        assert_eq!(terminal_shell("Command Prompt").unwrap().args, ["cmd.exe"]);
        assert_eq!(terminal_shell("PowerShell"), None); // the default
        let names = terminal_shell_names();
        assert_eq!(names[..2], ["PowerShell", "Command Prompt"]);
        assert_eq!(
            names.iter().any(|name| name == "Git Bash"),
            git_bash().is_some()
        );
        if let Some(shell) = terminal_shell("Git Bash") {
            assert!(Path::new(&shell.args[0]).is_file());
        }
    }
}
