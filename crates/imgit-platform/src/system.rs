//! Facts about the user's system that differ per platform.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

#[cfg(unix)]
#[path = "system/unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "system/windows.rs"]
mod imp;

/// An environment variable's value; `None` when it is unset or empty.
fn environment(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

/// The user's home folder (`USERPROFILE` on Windows, `HOME` elsewhere); the current directory when the system
/// names none.
#[must_use]
pub fn home_directory() -> PathBuf {
    imp::home_directory(&environment)
}

/// Where ImGit keeps its `.imgit` folder: `%APPDATA%` on Windows, the home folder elsewhere.
#[must_use]
pub fn app_data_directory() -> PathBuf {
    imp::app_data_directory(&environment)
}

/// The per-user configuration folder: `%APPDATA%` on Windows, `$XDG_CONFIG_HOME` (or `~/.config`) elsewhere.
#[must_use]
pub fn config_directory() -> PathBuf {
    imp::config_directory(&environment)
}

/// A point in time broken down into the fields of the user's time zone (C's `struct tm`, with a real year and a
/// 1-based month).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalTime {
    /// The calendar year (2026).
    pub year: i32,
    /// 1 (January) to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
    /// 0 to 23.
    pub hour: u8,
    /// 0 to 59.
    pub minute: u8,
    /// 0 to 60 (a leap second).
    pub second: u8,
    /// Days since Sunday: 0 (Sunday) to 6.
    pub weekday: u8,
    /// Days since January 1st: 0 to 365.
    pub day_of_year: u16,
    /// Whether daylight saving time is in effect.
    pub is_dst: bool,
}

/// `unix_seconds` (seconds since 1970-01-01 UTC) as local time; `None` for a time the C runtime cannot represent.
#[must_use]
pub fn local_time(unix_seconds: i64) -> Option<LocalTime> {
    let tm = imp::local_tm(unix_seconds)?;
    Some(LocalTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u8,
        day: tm.tm_mday as u8,
        hour: tm.tm_hour as u8,
        minute: tm.tm_min as u8,
        second: tm.tm_sec as u8,
        weekday: tm.tm_wday as u8,
        day_of_year: tm.tm_yday as u16,
        is_dst: tm.tm_isdst > 0,
    })
}

/// The user's locale for dates as the system spells it: "en-US" on Windows, "de_DE" elsewhere (from `LC_ALL`,
/// `LC_TIME`, then `LANG`, without the encoding). `None` when unknown (or "C" / "POSIX").
#[must_use]
pub fn system_locale() -> Option<String> {
    imp::system_locale(&environment)
}

/// How long a hovered submenu waits before it opens (the system's menu show delay; 400 ms where the system has
/// no such setting). Looked up once.
#[must_use]
pub fn menu_show_delay() -> Duration {
    static DELAY: OnceLock<Duration> = OnceLock::new();
    *DELAY.get_or_init(imp::menu_show_delay)
}

/// The roles ImGit fills with a system font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemFont {
    /// The native menus' face (Segoe UI, San Francisco, fontconfig's sans-serif).
    Ui,
    /// The diff and the terminal (Consolas, Menlo, fontconfig's monospace).
    Monospace,
    /// Arial, or what the system substitutes for it.
    Arial,
}

/// The font file for `font`; `None` when the system has none.
#[must_use]
pub fn system_font_file(font: SystemFont) -> Option<PathBuf> {
    imp::system_font_file(font)
}

/// An installed monospace face.
struct MonospaceFont {
    name: &'static str,
    file: PathBuf,
}

fn installed_monospace_fonts() -> &'static [MonospaceFont] {
    static FONTS: OnceLock<Vec<MonospaceFont>> = OnceLock::new();
    FONTS.get_or_init(imp::installed_monospace_fonts)
}

/// The monospace faces installed on this system that Preferences → Editor / In-App Terminal offer, by name
/// ("Consolas", "Menlo", "DejaVu Sans Mono"...). Looked up once.
#[must_use]
pub fn monospace_font_names() -> Vec<&'static str> {
    installed_monospace_fonts()
        .iter()
        .map(|font| font.name)
        .collect()
}

/// The file of one of those faces; `None` when it is unknown or not installed.
#[must_use]
pub fn monospace_font_file(name: &str) -> Option<PathBuf> {
    installed_monospace_fonts()
        .iter()
        .find(|font| font.name == name)
        .map(|font| font.file.clone())
}

/// A shell the terminal panel can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// The command line: the program (looked up on PATH unless it is a path) and its arguments.
    pub args: Vec<String>,
    /// The terminal tab's title.
    pub label: String,
}

#[cfg(windows)]
impl Shell {
    fn new(args: &[&str], label: &str) -> Self {
        Self {
            args: args.iter().map(|&arg| arg.to_owned()).collect(),
            label: label.to_owned(),
        }
    }
}

/// The interactive shell the terminal panel runs: PowerShell on Windows, `$SHELL -l` (a login shell, as terminal
/// emulators start it, so the profile scripts run) elsewhere.
#[must_use]
pub fn default_shell() -> Shell {
    imp::default_shell(&environment)
}

/// The shells the terminal panel can run, by name ("PowerShell", "Git Bash", "zsh"...). Looked up once.
#[must_use]
pub fn terminal_shell_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(imp::terminal_shell_names)
}

/// The shell for one of those names; [`default_shell`] for an empty or unknown one.
#[must_use]
pub fn terminal_shell(name: &str) -> Shell {
    imp::terminal_shell(name).unwrap_or_else(default_shell)
}

/// The proxy the system routes HTTPS through ("host:port"); `None` for a direct connection. Environment proxies
/// (`HTTPS_PROXY`, ...) are honoured by the HTTP client itself.
///
/// Windows: the Internet Options proxy (a PAC script is not evaluated). Elsewhere: always `None`.
#[must_use]
pub fn system_proxy() -> Option<String> {
    imp::system_proxy()
}

/// The HTTPS proxy in an Internet Options proxy setting: "host:port", or per scheme
/// ("http=host:port;https=host:port"), where the HTTP one stands in for a missing HTTPS one.
#[cfg(any(windows, test))]
fn https_proxy_of(setting: &str) -> Option<String> {
    let proxy = if setting.contains('=') {
        let mut entries = setting.split([';', ' ']);
        entries
            .clone()
            .find_map(|entry| entry.strip_prefix("https="))
            .or_else(|| entries.find_map(|entry| entry.strip_prefix("http=")))
            .unwrap_or_default()
    } else {
        setting
    };
    (!proxy.is_empty()).then(|| proxy.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn breaks_down_local_time() {
        // Noon UTC is the same calendar day in (nearly) every time zone; only the hour depends on the zone.
        let noon = local_time(1_700_049_600).unwrap(); // 2023-11-15 12:00:00 UTC, a Wednesday
        assert_eq!(noon.year, 2023);
        assert_eq!(noon.month, 11);
        assert!((15..=16).contains(&noon.day));
        assert_eq!(noon.second, 0);
        assert_eq!(noon.weekday, if noon.day == 15 { 3 } else { 4 });
        assert_eq!(noon.day_of_year, 318 + u16::from(noon.day - 15));

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let local = local_time(now).unwrap();
        assert!(local.year >= 2025 && (1..=12).contains(&local.month) && local.hour < 24);
        // One hour later is one hour later (or a daylight saving switch).
        let later = local_time(now + 3600).unwrap();
        assert!([0, 1, 2].contains(&((later.hour + 24 - local.hour) % 24)));
    }

    #[test]
    fn picks_the_https_proxy() {
        assert_eq!(https_proxy_of(""), None);
        assert_eq!(https_proxy_of("proxy:8080").as_deref(), Some("proxy:8080"));
        assert_eq!(
            https_proxy_of("http=a:1;https=b:2;ftp=c:3").as_deref(),
            Some("b:2")
        );
        assert_eq!(https_proxy_of("https=b:2 http=a:1").as_deref(), Some("b:2"));
        assert_eq!(https_proxy_of("ftp=c:3;http=a:1").as_deref(), Some("a:1"));
        assert_eq!(https_proxy_of("ftp=c:3"), None);
    }

    #[test]
    fn the_real_lookups_answer() {
        assert!(!home_directory().as_os_str().is_empty());
        assert!(!app_data_directory().as_os_str().is_empty());
        assert!(!config_directory().as_os_str().is_empty());
        assert!(menu_show_delay() <= Duration::from_secs(60));
        let _ = system_locale();
        let _ = system_proxy();
        for font in [SystemFont::Ui, SystemFont::Monospace, SystemFont::Arial] {
            if let Some(file) = system_font_file(font) {
                assert!(file.is_file(), "{}", file.display());
            }
        }
        for name in monospace_font_names() {
            assert!(
                monospace_font_file(name).is_some_and(|file| file.is_file()),
                "{name}"
            );
        }
        assert_eq!(monospace_font_file("No Such Face"), None);
    }

    #[test]
    fn offers_shells() {
        let default = default_shell();
        assert!(!default.args.is_empty() && !default.label.is_empty());
        assert_eq!(terminal_shell(""), default);
        assert_eq!(terminal_shell("no such shell"), default);
        let names = terminal_shell_names();
        assert!(!names.is_empty());
        for name in names {
            assert!(!terminal_shell(name).args.is_empty(), "{name}");
        }
    }
}
