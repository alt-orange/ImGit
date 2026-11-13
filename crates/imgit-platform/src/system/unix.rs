//! The POSIX (Linux, macOS) side of [`super`].

use super::{MonospaceFont, Shell, SystemFont};
use std::ffi::{CStr, OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Looks up an environment variable; `None` when it is unset or empty.
pub(super) type Environment<'a> = &'a dyn Fn(&str) -> Option<OsString>;

pub(super) fn home_directory(environment: Environment) -> PathBuf {
    environment("HOME")
        .map(PathBuf::from)
        .or_else(password_database_home)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// The user's home folder according to the password database.
fn password_database_home() -> Option<PathBuf> {
    let mut buffer = vec![0 as libc::c_char; 4096];
    loop {
        // SAFETY: `passwd` is plain integers and pointers, for which all zeroes is a valid value.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: `entry`, `buffer` (for the length passed) and `found` are live and writable; getpwuid_r is the
        // thread-safe variant and keeps none of the pointers.
        let error = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut found,
            )
        };
        if error == libc::ERANGE && buffer.len() < (1 << 20) {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if error != 0 || found.is_null() || entry.pw_dir.is_null() {
            return None;
        }
        // SAFETY: on success pw_dir points to a NUL-terminated string inside `buffer`, which is still alive.
        let home = unsafe { CStr::from_ptr(entry.pw_dir) };
        return (!home.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(home.to_bytes())));
    }
}

pub(super) fn app_data_directory(environment: Environment) -> PathBuf {
    home_directory(environment)
}

pub(super) fn config_directory(environment: Environment) -> PathBuf {
    environment("XDG_CONFIG_HOME").map_or_else(
        || home_directory(environment).join(".config"),
        PathBuf::from,
    )
}

pub(super) fn local_tm(unix_seconds: i64) -> Option<libc::tm> {
    let time = libc::time_t::try_from(unix_seconds).ok()?;
    // SAFETY: `tm` is plain integers and a pointer, for which all zeroes is a valid value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers refer to live, properly aligned values of the types localtime_r expects.
    let result = unsafe { libc::localtime_r(&time, &mut tm) };
    (!result.is_null()).then_some(tm)
}

pub(super) fn system_locale(environment: Environment) -> Option<String> {
    // LC_ALL, LC_TIME, then LANG: "de_DE.UTF-8" -> "de_DE".
    let value = ["LC_ALL", "LC_TIME", "LANG"]
        .into_iter()
        .find_map(environment)?;
    let value = value.to_string_lossy();
    let locale = value.split(['.', '@']).next().unwrap_or_default();
    (!locale.is_empty() && locale != "C" && locale != "POSIX").then(|| locale.to_owned())
}

pub(super) fn menu_show_delay() -> Duration {
    Duration::from_millis(400) // Windows' default
}

#[cfg(target_os = "macos")]
pub(super) fn system_font_file(font: SystemFont) -> Option<PathBuf> {
    // The system faces in the roles Windows fills with Segoe UI, Consolas and Arial, oldest macOS release last:
    // San Francisco (the variable SFNS.ttf, since Big Sur), Menlo and Arial (Supplemental since Catalina).
    let candidates = match font {
        SystemFont::Ui => [
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            "/System/Library/Fonts/Helvetica.ttc",
        ],
        SystemFont::Monospace => [
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/SFNSMono.ttf",
            "/System/Library/Fonts/Monaco.ttf",
        ],
        SystemFont::Arial => [
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/Library/Fonts/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ],
    };
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn system_font_file(font: SystemFont) -> Option<PathBuf> {
    use crate::process::{RunOptions, run_process};
    // fontconfig's choice for the role (Arial usually maps to Liberation Sans or DejaVu Sans).
    let pattern = match font {
        SystemFont::Ui => "sans-serif",
        SystemFont::Monospace => "monospace",
        SystemFont::Arial => "Arial",
    };
    let args = ["fc-match", "--format=%{file}", pattern].map(str::to_owned);
    let matched = run_process(Path::new(""), &args, RunOptions::default()).ok()?;
    if matched.exit_code != 0 {
        return None;
    }
    let path = PathBuf::from(matched.text);
    // TrueType / OpenType outlines only: ImGui's loaders take no Type 1 or bitmap fonts.
    let outline = matches!(
        path.extension().and_then(OsStr::to_str),
        Some("ttf" | "otf" | "ttc")
    );
    (outline && path.exists()).then_some(path)
}

/// The faces macOS ships, then popular ones a user installs (for everyone, or for themselves).
#[cfg(target_os = "macos")]
pub(super) fn installed_monospace_fonts() -> Vec<MonospaceFont> {
    const KNOWN: [(&str, &str); 10] = [
        ("Menlo", "Menlo.ttc"),
        ("Monaco", "Monaco.ttf"),
        ("SF Mono", "SFNSMono.ttf"),
        ("Courier New", "Courier New.ttf"),
        ("Andale Mono", "Andale Mono.ttf"),
        ("PT Mono", "PTMono.ttc"),
        ("Fira Code", "FiraCode-Regular.ttf"),
        ("Hack", "Hack-Regular.ttf"),
        ("JetBrains Mono", "JetBrainsMono-Regular.ttf"),
        ("Source Code Pro", "SourceCodePro-Regular.ttf"),
    ];
    let folders = [
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/System/Library/Fonts/Supplemental"),
        PathBuf::from("/Library/Fonts"),
        super::home_directory().join("Library/Fonts"),
    ];
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

/// Distributions file fonts in different subfolders (truetype/dejavu, TTF, dejavu-sans-mono-fonts...), so the
/// font folders are searched for the files' names.
#[cfg(not(target_os = "macos"))]
pub(super) fn installed_monospace_fonts() -> Vec<MonospaceFont> {
    const KNOWN: [(&str, &str); 10] = [
        ("DejaVu Sans Mono", "DejaVuSansMono.ttf"),
        ("Liberation Mono", "LiberationMono-Regular.ttf"),
        ("Noto Sans Mono", "NotoSansMono-Regular.ttf"),
        ("Ubuntu Mono", "UbuntuMono-R.ttf"),
        ("Cascadia Code", "CascadiaCode.ttf"),
        ("Cascadia Mono", "CascadiaMono.ttf"),
        ("Fira Code", "FiraCode-Regular.ttf"),
        ("Hack", "Hack-Regular.ttf"),
        ("JetBrains Mono", "JetBrainsMono-Regular.ttf"),
        ("Source Code Pro", "SourceCodePro-Regular.ttf"),
    ];
    let home = super::home_directory();
    let folders = [
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
        home.join(".local/share/fonts"),
        home.join(".fonts"),
    ];
    let mut files: [Option<PathBuf>; KNOWN.len()] = Default::default();
    for folder in &folders {
        visit_files(folder, 0, &mut |path| {
            let known = KNOWN
                .iter()
                .position(|(_, file)| path.file_name() == Some(OsStr::new(file)));
            if let Some(index) = known
                && files[index].is_none()
            {
                files[index] = Some(path.to_owned());
            }
        });
    }
    KNOWN
        .into_iter()
        .zip(files)
        .filter_map(|((name, _), file)| Some(MonospaceFont { name, file: file? }))
        .collect()
}

/// Calls `visit` for every entry below `folder` that is not a directory, skipping what cannot be read. Symbolic
/// links to directories are not followed.
#[cfg(not(target_os = "macos"))]
fn visit_files(folder: &Path, depth: usize, visit: &mut dyn FnMut(&Path)) {
    const MAX_DEPTH: usize = 16;
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if depth < MAX_DEPTH {
                visit_files(&path, depth + 1, visit);
            }
        } else {
            visit(&path);
        }
    }
}

const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "sh"];

/// A shell by its file name in the usual folders; `None` when it is not installed.
fn find_shell(name: &str) -> Option<String> {
    [
        "/bin/",
        "/usr/bin/",
        "/usr/local/bin/",
        "/opt/homebrew/bin/",
    ]
    .into_iter()
    .map(|folder| format!("{folder}{name}"))
    .find(|path| Path::new(path).exists())
}

pub(super) fn default_shell(environment: Environment) -> Shell {
    let shell = environment("SHELL").map_or_else(
        || "/bin/sh".to_owned(),
        |shell| shell.to_string_lossy().into_owned(),
    );
    let label = Path::new(&shell)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A login shell, as terminal emulators start it, so the profile scripts run.
    Shell {
        args: vec![shell, "-l".to_owned()],
        label,
    }
}

pub(super) fn terminal_shell_names() -> Vec<String> {
    let installed = SHELLS
        .into_iter()
        .filter(|shell| find_shell(shell).is_some());
    std::iter::once("System Default")
        .chain(installed)
        .map(str::to_owned)
        .collect()
}

pub(super) fn terminal_shell(name: &str) -> Option<Shell> {
    let shell = SHELLS.into_iter().find(|&shell| shell == name)?;
    Some(Shell {
        args: vec![find_shell(shell)?, "-l".to_owned()],
        label: shell.to_owned(),
    })
}

pub(super) fn system_proxy() -> Option<String> {
    None // curl reads HTTPS_PROXY itself
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
        let xdg = fake(&[("HOME", "/home/ada"), ("XDG_CONFIG_HOME", "/etc/xdg-ada")]);
        assert_eq!(home_directory(&xdg), Path::new("/home/ada"));
        assert_eq!(app_data_directory(&xdg), Path::new("/home/ada"));
        assert_eq!(config_directory(&xdg), Path::new("/etc/xdg-ada"));

        let plain = fake(&[("HOME", "/home/ada")]);
        assert_eq!(config_directory(&plain), Path::new("/home/ada/.config"));

        // Without HOME the password database (or the current directory) answers.
        assert!(!home_directory(&fake(&[])).as_os_str().is_empty());
    }

    #[test]
    fn reads_the_locale_from_the_environment() {
        assert_eq!(
            system_locale(&fake(&[("LANG", "de_DE.UTF-8")])).as_deref(),
            Some("de_DE")
        );
        assert_eq!(
            system_locale(&fake(&[("LANG", "en_US"), ("LC_TIME", "sr_RS@latin")])).as_deref(),
            Some("sr_RS")
        );
        assert_eq!(
            system_locale(&fake(&[("LANG", "en_US"), ("LC_ALL", "C.UTF-8")])),
            None
        );
        assert_eq!(system_locale(&fake(&[("LANG", "POSIX")])), None);
        assert_eq!(system_locale(&fake(&[])), None);
    }

    #[test]
    fn names_the_shells() {
        let zsh = default_shell(&fake(&[("SHELL", "/usr/bin/zsh")]));
        assert_eq!(
            (zsh.args, zsh.label.as_str()),
            (vec!["/usr/bin/zsh".to_owned(), "-l".to_owned()], "zsh")
        );
        assert_eq!(default_shell(&fake(&[])).args[0], "/bin/sh");
        assert_eq!(terminal_shell_names()[0], "System Default");
        assert_eq!(terminal_shell("System Default"), None);
        if let Some(shell) = terminal_shell("sh") {
            assert!(shell.args[0].ends_with("/sh") && shell.label == "sh");
        }
    }
}
