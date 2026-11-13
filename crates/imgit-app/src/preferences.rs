//! The Preferences ImGit keeps itself (the profile's `preferences.txt`): what the Preferences pages show and what
//! the app reads, from one store. Settings git has its own key for (`commit.gpgsign`, `core.autocrlf`,
//! `core.hooksPath`...) are not kept here: the pages read and write the git configuration instead.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::{fs, io};

use crate::profiles::write_text_file;

/// A key and the value it has until the user sets one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Preference {
    pub key: &'static str,
    pub fallback: &'static str,
}

impl Preference {
    #[must_use]
    pub const fn new(key: &'static str, fallback: &'static str) -> Self {
        Self { key, fallback }
    }
}

/// Every preference ImGit keeps.
pub mod pref {
    use super::Preference;

    // General
    /// Fetches prune deleted remote branches.
    pub const AUTO_PRUNE: Preference = Preference::new("General/Auto-Prune", "1");
    pub const UPDATE_SUBMODULES: Preference =
        Preference::new("General/Keep submodules up to date", "1");
    pub const SHOW_ALL_COMMITS: Preference =
        Preference::new("General/Show All Commits in Graph", "0");
    pub const LAZY_LOAD_COMMITS: Preference =
        Preference::new("General/Lazy Load Commits in Graph", "1");
    pub const EXTENDED_LOGGING: Preference = Preference::new("General/Extended logging", "0");

    // SSH
    pub const SSH_AGENT: Preference = Preference::new("SSH/Use local SSH agent", "0");
    /// Empty: ssh's own default keys.
    pub const SSH_PRIVATE_KEY: Preference = Preference::new("SSH/Private key", "");
    pub const SSH_PUBLIC_KEY: Preference = Preference::new("SSH/Public key", "");
    pub const CREDENTIAL_MANAGER: Preference =
        Preference::new("SSH/Use Git Credential Manager", "0");

    // External Tools
    pub const MERGE_TOOL: Preference =
        Preference::new("External Tools/Merge Tool", "Git Config Default");
    pub const DIFF_TOOL: Preference =
        Preference::new("External Tools/Diff Tool", "<Use Merge Tool>");
    pub const EDITOR: Preference = Preference::new("External Tools/Editor", "System Default");
    pub const EDITOR_COMMAND: Preference = Preference::new("External Tools/Editor command", "");
    pub const TERMINAL: Preference = Preference::new("External Tools/Terminal", "System Default");
    pub const TERMINAL_COMMAND: Preference = Preference::new("External Tools/Terminal command", "");

    // Notifications
    pub const DESKTOP_NOTIFICATIONS: Preference = Preference::new("Notifications/Desktop", "0");

    // UI Customization
    pub const NOTIFICATION_LOCATION: Preference =
        Preference::new("UI Customization/Notification Location", "Bottom Left");
    pub const LOCALE: Preference = Preference::new("UI Customization/Locale", "System [system]");
    /// A moment.js format; empty: the placeholder ([`DEFAULT_DATE_FORMAT`](super::DEFAULT_DATE_FORMAT)).
    pub const DATE_FORMAT: Preference = Preference::new("UI Customization/Date Format", "");
    /// Empty: [`DEFAULT_DATE_WORD_FORMAT`](super::DEFAULT_DATE_WORD_FORMAT).
    pub const DATE_WORD_FORMAT: Preference =
        Preference::new("UI Customization/Date word Format", "");
    /// Empty: [`DEFAULT_DATE_TIME_FORMAT`](super::DEFAULT_DATE_TIME_FORMAT).
    pub const DATE_TIME_FORMAT: Preference =
        Preference::new("UI Customization/Date/Time Format", "");
    /// Empty: [`DEFAULT_DATE_VERBOSE_FORMAT`](super::DEFAULT_DATE_VERBOSE_FORMAT).
    pub const DATE_VERBOSE_FORMAT: Preference =
        Preference::new("UI Customization/Date verbose Format", "");
    pub const GROUP_WORKTREES: Preference =
        Preference::new("UI Customization/Group worktree tabs", "1");

    // Editor (the file view, the diff and the merge tool)
    /// Empty: the system's monospace face.
    pub const EDITOR_FONT: Preference = Preference::new("Editor/Font", "");
    pub const EDITOR_FONT_SIZE: Preference = Preference::new("Editor/Font Size", "12");
    pub const EDITOR_TAB_SIZE: Preference = Preference::new("Editor/Tab Size", "4");
    pub const EDITOR_EOL: Preference = Preference::new(
        "Editor/End of Line",
        if cfg!(windows) { "CRLF" } else { "LF" },
    );
    pub const EDITOR_LINE_NUMBERS: Preference = Preference::new("Editor/Show Line Numbers", "1");

    // In-App Terminal
    pub const TERMINAL_FONT: Preference = Preference::new("In-App Terminal/Font", "");
    pub const TERMINAL_FONT_SIZE: Preference = Preference::new("In-App Terminal/Font Size", "12");
    pub const TERMINAL_LINE_HEIGHT: Preference =
        Preference::new("In-App Terminal/Line Height", "1.2");
    pub const TERMINAL_CURSOR: Preference = Preference::new("In-App Terminal/Cursor Style", "Bar");
    pub const TERMINAL_DIM: Preference = Preference::new("In-App Terminal/Dim when unfocused", "1");
    /// Empty: the platform's shell.
    pub const TERMINAL_SHELL: Preference = Preference::new("In-App Terminal/Default", "");
}

pub const DEFAULT_DATE_FORMAT: &str = "MM/DD/YYYY";
pub const DEFAULT_DATE_WORD_FORMAT: &str = "MMMM D, YYYY";
pub const DEFAULT_DATE_TIME_FORMAT: &str = "MM/DD/YYYY @ h:mm A";
pub const DEFAULT_DATE_VERBOSE_FORMAT: &str = "MMMM D, YYYY h:mm A";

/// The line break the file is written with: the platform's own, as the files written so far have. Either is
/// read.
const LINE_BREAK: &str = if cfg!(windows) { "\r\n" } else { "\n" };

#[derive(Debug, Default)]
struct Store {
    /// Where saves go; `None` until a file was loaded.
    path: Option<PathBuf>,
    /// The file exists but could not be read: nothing is saved over it (it may be fine again, or hold what the
    /// user has to recover), so changes live in memory only until the next [`Preferences::load`].
    unreadable: bool,
    /// Sorted by key, the order the file is written in.
    values: BTreeMap<String, String>,
}

/// The store: read and written on the UI thread, read by worker threads too (each call locks).
#[derive(Debug, Default)]
pub struct Preferences {
    store: Mutex<Store>,
}

impl Preferences {
    /// An empty store that saves nowhere until [`load`](Self::load) names its file.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            store: Mutex::new(Store {
                path: None,
                unreadable: false,
                values: BTreeMap::new(),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        // The store holds plain strings: one a panicking thread left behind is still whole.
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads `path` (key, tab, value lines), replacing what was loaded before; later saves go there. A file that
    /// does not exist yet is an empty store.
    ///
    /// # Errors
    /// The file exists and could not be read: the store is empty then and does not save (it would replace the
    /// file with a near-empty one): [`set`](Self::set) keeps its values in memory and reports the error.
    pub fn load(&self, path: impl Into<PathBuf>) -> io::Result<()> {
        let path = path.into();
        let read = fs::read(&path);
        let mut store = self.lock();
        store.path = Some(path);
        store.values.clear();
        store.unreadable = false;
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                store.unreadable = true;
                return Err(error);
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        // A UTF-8 byte order mark (an editor's) is not part of the first key.
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if let Some((key, value)) = line.split_once('\t') {
                store.values.insert(key.to_owned(), value.to_owned());
            }
        }
        Ok(())
    }

    /// The value the user set, else the preference's fallback.
    #[must_use]
    pub fn get(&self, preference: Preference) -> String {
        self.lock()
            .values
            .get(preference.key)
            .map_or_else(|| preference.fallback.to_owned(), Clone::clone)
    }

    /// Whether the value is "1".
    #[must_use]
    pub fn flag(&self, preference: Preference) -> bool {
        self.lock()
            .values
            .get(preference.key)
            .map_or(preference.fallback, String::as_str)
            == "1"
    }

    /// The value as a whole number in `range`; the fallback's (brought into the range) when it is not one.
    #[must_use]
    pub fn number(&self, preference: Preference, range: RangeInclusive<u32>) -> u32 {
        /// Digits only, and few enough of them to fit.
        fn parse(text: &str) -> Option<u32> {
            if text.len() > 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            text.parse().ok()
        }
        let (low, high) = (*range.start(), *range.end());
        parse(&self.get(preference))
            .filter(|value| range.contains(value))
            .unwrap_or_else(|| parse(preference.fallback).unwrap_or(low).max(low).min(high))
    }

    /// Sets the value and writes the file at once. A tab or line break in `value` becomes a space (it would
    /// split the line it is saved on).
    ///
    /// # Errors
    /// The file could not be written, or could not be read by [`load`](Self::load) and is left alone; the store
    /// has the new value all the same.
    pub fn set(&self, preference: Preference, value: &str) -> io::Result<()> {
        let value = value.replace(['\t', '\n', '\r'], " ");
        let mut store = self.lock();
        store.values.insert(preference.key.to_owned(), value);
        store.save()
    }

    /// Sets the value to "1" or "0" and writes the file at once.
    ///
    /// # Errors
    /// As [`set`](Self::set).
    pub fn set_flag(&self, preference: Preference, value: bool) -> io::Result<()> {
        self.set(preference, if value { "1" } else { "0" })
    }

    /// The file loaded and saved to, `None` before the first [`load`](Self::load).
    #[must_use]
    pub fn path(&self) -> Option<PathBuf> {
        self.lock().path.clone()
    }

    /// External Tools: whether `git mergetool` is offered (`merge.tool` decides which tool git starts).
    #[must_use]
    pub fn external_merge_tool_enabled(&self) -> bool {
        self.get(pref::MERGE_TOOL) != "<None>"
    }

    /// External Tools: whether `git difftool` is offered (`diff.tool` decides which tool git starts; the diff
    /// tool's `<Use Merge Tool>` follows the merge tool's choice, as git falls back to it).
    #[must_use]
    pub fn external_diff_tool_enabled(&self) -> bool {
        match self.get(pref::DIFF_TOOL).as_str() {
            "<Use Merge Tool>" => self.external_merge_tool_enabled(),
            tool => tool != "<None>",
        }
    }
}

impl Store {
    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if self.unreadable {
            return Err(io::Error::other(
                "the preferences file could not be read, so it is not overwritten",
            ));
        }
        let text: String = self
            .values
            .iter()
            .flat_map(|(key, value)| [key.as_str(), "\t", value.as_str(), LINE_BREAK])
            .collect();
        write_text_file(path, &text)
    }
}

/// The current profile's preferences.
#[must_use]
pub fn preferences() -> &'static Preferences {
    static INSTANCE: Preferences = Preferences::new();
    &INSTANCE
}

// Pure helpers of what the preferences drive (unit tested).

/// `GIT_SSH_COMMAND` for a chosen private key: ssh with only that key, unless the SSH agent is used or there is
/// no key (then `None`: ssh's own configuration, agent and default keys decide). The path is quoted for git's
/// shell.
#[must_use]
pub fn ssh_command(private_key: &str, use_agent: bool) -> Option<String> {
    if use_agent || private_key.is_empty() {
        return None;
    }
    // git runs it through sh: forward slashes (ssh on Windows takes them) and single quotes, with a quote in
    // the path closed, escaped and reopened.
    let mut quoted = String::with_capacity(private_key.len() + 2);
    for c in private_key.chars() {
        match c {
            '\'' => quoted.push_str("'\\''"),
            '\\' => quoted.push('/'),
            c => quoted.push(c),
        }
    }
    Some(format!("ssh -i '{quoted}' -o IdentitiesOnly=yes"))
}

/// `git fetch ...` with the Auto-Prune preference applied (its own `--prune` / `--no-prune` / `-p` replaced);
/// other commands unchanged.
#[must_use]
pub fn with_prune_preference(mut args: Vec<String>, prune: bool) -> Vec<String> {
    // The subcommand: the first argument that is not a `-c key=value` pair.
    let mut command = 0;
    while command + 1 < args.len() && args[command] == "-c" {
        command += 2;
    }
    if args.get(command).map(String::as_str) != Some("fetch") {
        return args;
    }
    // Nothing up to the subcommand is one of these, so it stays where it is.
    args.retain(|arg| !matches!(arg.as_str(), "--prune" | "--no-prune" | "-p"));
    let flag = if prune { "--prune" } else { "--no-prune" };
    args.insert(command + 1, flag.to_owned());
    args
}

/// "1.2" -> 12 (tenths), for the line height field: digits, optionally a point or comma and more digits ("1",
/// "1.2", "2,5"; "1.25" rounds down to 1.2). `None` when it is not a number in `range` tenths.
#[must_use]
pub fn parse_tenths(text: &str, range: RangeInclusive<u32>) -> Option<u32> {
    let bytes = text.as_bytes();
    let digit = |i: usize| {
        bytes
            .get(i)
            .filter(|b| b.is_ascii_digit())
            .map(|b| u32::from(b - b'0'))
    };
    let mut whole = 0;
    let mut tenths = 0;
    let mut i = 0;
    let mut digits = false;
    // Longer numbers than any range has stop here, and fail below for the digits left over.
    while whole < 1000
        && let Some(d) = digit(i)
    {
        whole = whole * 10 + d;
        digits = true;
        i += 1;
    }
    if matches!(bytes.get(i), Some(b'.' | b',')) {
        i += 1;
        if let Some(d) = digit(i) {
            tenths = d;
            digits = true;
            i += 1;
        }
        while digit(i).is_some() {
            i += 1;
        }
    }
    let value = whole * 10 + tenths;
    (digits && i == bytes.len() && range.contains(&value)).then_some(value)
}

/// The command line of a `<Custom>` external tool: `command` split into words (double quotes group), with
/// `path` in place of each `%path%`, or appended when there is none. Empty for a blank command.
#[must_use]
pub fn custom_tool_command(command: &str, path: &Path) -> Vec<String> {
    const PLACEHOLDER: &str = "%path%";
    let path = path.to_string_lossy();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut in_word = false;
    for c in command.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            ' ' | '\t' if !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    let mut placed = false;
    for word in &mut words {
        if word.contains(PLACEHOLDER) {
            *word = word.replace(PLACEHOLDER, &path);
            placed = true;
        }
    }
    if !placed && !words.is_empty() {
        words.push(path.into_owned());
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|&arg| arg.to_owned()).collect()
    }

    #[test]
    fn ssh_command_key_without_agent() {
        assert_eq!(
            ssh_command("C:\\Users\\me\\.ssh\\id_ed25519", false).as_deref(),
            Some("ssh -i 'C:/Users/me/.ssh/id_ed25519' -o IdentitiesOnly=yes")
        );
        assert_eq!(
            ssh_command("/home/o'neil/.ssh/key", false).as_deref(),
            Some("ssh -i '/home/o'\\''neil/.ssh/key' -o IdentitiesOnly=yes")
        );
    }

    #[test]
    fn ssh_command_agent_or_no_key() {
        assert_eq!(ssh_command("/home/me/.ssh/key", true), None);
        assert_eq!(ssh_command("", false), None);
    }

    #[test]
    fn prune_fetch() {
        assert_eq!(
            with_prune_preference(strings(&["fetch", "--all", "--prune"]), false),
            strings(&["fetch", "--no-prune", "--all"])
        );
        assert_eq!(
            with_prune_preference(strings(&["fetch", "origin"]), true),
            strings(&["fetch", "--prune", "origin"])
        );
        assert_eq!(
            with_prune_preference(strings(&["-c", "a=b", "fetch", "-p"]), true),
            strings(&["-c", "a=b", "fetch", "--prune"])
        );
        assert_eq!(
            with_prune_preference(strings(&["push", "--prune"]), false),
            strings(&["push", "--prune"])
        );
        assert_eq!(
            with_prune_preference(Vec::new(), true),
            Vec::<String>::new()
        );
        assert_eq!(
            with_prune_preference(strings(&["-c"]), true),
            strings(&["-c"])
        );
    }

    #[test]
    fn tenths_parse() {
        assert_eq!(parse_tenths("1.2", 10..=30), Some(12));
        assert_eq!(parse_tenths("1", 10..=30), Some(10));
        assert_eq!(parse_tenths("1.25", 10..=30), Some(12));
        assert_eq!(parse_tenths("2,5", 10..=30), Some(25));
        assert_eq!(parse_tenths("abc", 10..=30), None);
        assert_eq!(parse_tenths("9", 10..=30), None);
        assert_eq!(parse_tenths("", 10..=30), None);
        assert_eq!(parse_tenths("2.", 10..=30), Some(20));
        assert_eq!(parse_tenths(".", 0..=30), None);
        assert_eq!(parse_tenths(".5", 0..=30), Some(5));
        assert_eq!(parse_tenths("1.2x", 10..=30), None);
        assert_eq!(parse_tenths("99999999999999999999", 0..=u32::MAX), None);
    }

    #[test]
    fn custom_tool_command_words() {
        assert_eq!(
            custom_tool_command("subl -n", Path::new("/r/a b")),
            strings(&["subl", "-n", "/r/a b"])
        );
        assert_eq!(
            custom_tool_command(
                "\"C:\\Program Files\\Ed\\ed.exe\" --open=%path% -w",
                Path::new("C:/r")
            ),
            strings(&["C:\\Program Files\\Ed\\ed.exe", "--open=C:/r", "-w"])
        );
        assert!(custom_tool_command("   ", Path::new("/r")).is_empty());
        // A path that holds the placeholder itself is not expanded again; "" is a word.
        assert_eq!(
            custom_tool_command("ed %path%:%path% \"\"", Path::new("a%path%")),
            strings(&["ed", "a%path%:a%path%", ""])
        );
    }

    const FLAG: Preference = Preference::new("Test/Flag", "1");
    const SIZE: Preference = Preference::new("Test/Size", "12");
    const TEXT: Preference = Preference::new("Test/Text", "");

    #[test]
    fn preference_store_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.txt");
        {
            let store = Preferences::new();
            store.load(&path).unwrap();
            assert!(store.flag(FLAG));
            assert_eq!(store.number(SIZE, 6..=40), 12);
            store.set_flag(FLAG, false).unwrap();
            store.set(SIZE, "99").unwrap();
            store.set(TEXT, "a\tb\nc").unwrap();
        }
        let store = Preferences::new();
        store.load(&path).unwrap();
        assert!(!store.flag(FLAG));
        assert_eq!(store.number(SIZE, 6..=40), 12); // out of range: the fallback
        assert_eq!(store.get(TEXT), "a b c");
        assert_eq!(store.path().as_deref(), Some(path.as_path()));
    }

    #[test]
    fn preference_store_file_format() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.txt");
        // What an editor may leave: a byte order mark, either line break, lines that are not preferences.
        fs::write(
            &path,
            "\u{FEFF}Test/Flag\t0\r\nnot a preference\nTest/Text\tkeeps\tits tabs\n\nTest/Size\t7",
        )
        .unwrap();
        let store = Preferences::new();
        store.load(&path).unwrap();
        assert!(!store.flag(FLAG));
        assert_eq!(store.get(TEXT), "keeps\tits tabs");
        assert_eq!(store.number(SIZE, 6..=40), 7);

        // Written sorted by key, one "key<tab>value" per line.
        store.set(Preference::new("A/First", ""), "x").unwrap();
        let written = fs::read_to_string(&path).unwrap();
        let expected = [
            "A/First\tx",
            "Test/Flag\t0",
            "Test/Size\t7",
            "Test/Text\tkeeps\tits tabs",
            "",
        ];
        assert_eq!(written, expected.join(LINE_BREAK));
    }

    #[test]
    fn an_unreadable_file_is_not_saved_over() {
        let directory = tempfile::tempdir().unwrap();
        // A folder where the file should be: reading fails with something other than "not found".
        let path = directory.path().join("preferences.txt");
        fs::create_dir(&path).unwrap();
        let store = Preferences::new();
        assert!(store.load(&path).is_err());
        assert!(store.set(SIZE, "20").is_err());
        assert_eq!(store.number(SIZE, 6..=40), 20); // kept in memory
        assert!(path.is_dir());

        // Loading a readable file again saves normally, and leaves no temporary file.
        let file = directory.path().join("other.txt");
        store.load(&file).unwrap();
        store.set(SIZE, "21").unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            format!("Test/Size	21{LINE_BREAK}")
        );
        assert!(!directory.path().join("other.txt.tmp").exists());
    }

    #[test]
    fn preference_store_numbers() {
        let store = Preferences::new();
        assert_eq!(store.path(), None);
        // Without a file, values are kept in memory only.
        store.set(SIZE, "0020").unwrap();
        assert_eq!(store.number(SIZE, 6..=40), 20);
        store.set(SIZE, "+20").unwrap();
        assert_eq!(store.number(SIZE, 6..=40), 12);
        store.set(SIZE, "1234567").unwrap();
        assert_eq!(store.number(SIZE, 0..=u32::MAX), 12);
        // The fallback itself is brought into the range, or is the low end when it is no number.
        assert_eq!(store.number(SIZE, 20..=40), 20);
        assert_eq!(store.number(SIZE, 1..=8), 8);
        assert_eq!(store.number(TEXT, 3..=8), 3);
    }

    #[test]
    fn external_tools() {
        let store = Preferences::new();
        assert!(store.external_merge_tool_enabled());
        assert!(store.external_diff_tool_enabled());
        store.set(pref::MERGE_TOOL, "<None>").unwrap();
        assert!(!store.external_merge_tool_enabled());
        assert!(!store.external_diff_tool_enabled()); // follows the merge tool
        store.set(pref::DIFF_TOOL, "meld").unwrap();
        assert!(store.external_diff_tool_enabled());
        store.set(pref::DIFF_TOOL, "<None>").unwrap();
        store.set(pref::MERGE_TOOL, "meld").unwrap();
        assert!(!store.external_diff_tool_enabled());
    }

    #[test]
    fn the_global_store_is_one_store() {
        assert!(std::ptr::eq(preferences(), preferences()));
    }
}
