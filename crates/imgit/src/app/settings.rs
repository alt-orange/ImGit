//! The preferences persisted as `key=value` lines in the profile's `settings.txt` (the C++ `Settings`).
//!
//! The on-disk format is the C++ app's, byte for byte: floats are written the way a C++ `ostream` prints them
//! (six significant digits), booleans as `0` / `1`, and what is read is as forgiving as `atoi` / `atof` were.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use imgit_app::profiles::write_text_file;

use crate::theme::GRAPH_COLOR_COUNT;

/// The order of the right panel's file lists (the Sort button).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileSort {
    #[default]
    Path,
    PathDescending,
    Name,
    Status,
}

impl FileSort {
    /// The value `settings.txt` holds.
    #[must_use]
    pub const fn index(self) -> i32 {
        match self {
            Self::Path => 0,
            Self::PathDescending => 1,
            Self::Name => 2,
            Self::Status => 3,
        }
    }

    /// The sort a stored value names (anything out of range is clamped).
    #[must_use]
    pub const fn from_index(index: i32) -> Self {
        match index {
            i32::MIN..=0 => Self::Path,
            1 => Self::PathDescending,
            2 => Self::Name,
            _ => Self::Status,
        }
    }
}

/// What the toolbar's pull button does by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PullMode {
    FetchAll,
    #[default]
    FastForwardIfPossible,
    FastForwardOnly,
    Rebase,
}

impl PullMode {
    /// The value `settings.txt` holds.
    #[must_use]
    pub const fn index(self) -> i32 {
        match self {
            Self::FetchAll => 0,
            Self::FastForwardIfPossible => 1,
            Self::FastForwardOnly => 2,
            Self::Rebase => 3,
        }
    }

    /// The mode a stored value names (anything out of range is clamped).
    #[must_use]
    pub const fn from_index(index: i32) -> Self {
        match index {
            i32::MIN..=0 => Self::FetchAll,
            1 => Self::FastForwardIfPossible,
            2 => Self::FastForwardOnly,
            _ => Self::Rebase,
        }
    }
}

/// Preferences, persisted as `key=value` lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub auto_fetch_minutes: i32,
    pub initial_commits: i32,
    pub show_toolbar_labels: bool,
    pub author_initials: bool,
    pub show_author: bool,
    pub show_date: bool,
    pub show_sha: bool,
    pub show_changes: bool,
    pub show_description: bool,
    /// With `show_description`: only the row under the mouse shows it.
    pub description_on_hover: bool,
    pub show_ghost_refs: bool,
    pub highlight_rows_on_ref_hover: bool,
    pub remember_tabs: bool,
    pub push_after_commit: bool,
    pub compact_graph: bool,
    /// The graph shows only the checked-out branch, its upstream and the default branch (unless a ref is
    /// soloed).
    pub smart_branch_visibility: bool,
    /// The colour (a graph lane colour index) a new tab group takes; `None`: the next one after the groups
    /// already there.
    pub tab_group_color: Option<usize>,
    /// The diff and merge editors colour code by its language.
    pub syntax_highlighting: bool,
    /// The diff / file view wraps long lines (Preferences > Editor, the toolbar toggle).
    pub word_wrap: bool,
    /// The diff view's whitespace toggle: on, changes in whitespace only are shown and the whitespace of changed
    /// lines is drawn; off, they are left out.
    pub diff_whitespace: bool,
    pub file_sort: FileSort,
    pub zoom: f32,
    /// A theme name (`UiState::theme_names`).
    pub theme: String,
    pub default_branch: String,
    pub pull_mode: PullMode,
    /// Left panel sections switched off from the section header's context menu (ImGit hides Worktrees by
    /// default).
    pub hidden_left_sections: BTreeSet<String>,
    /// Panel sizes and the graph columns' preferred widths dragged by the user, in CSS px.
    pub left_panel_width: f32,
    pub detail_panel_width: f32,
    pub terminal_height: f32,
    /// `layout.CommitMessage.rightPanelHeight`.
    pub commit_message_height: f32,
    /// The merge tool's dragged splits: A's share of the width beside B ...
    pub merge_a_fraction: f32,
    /// ... and A / B's share of the height above the Output.
    pub merge_top_fraction: f32,
    /// By zone: ref, graph, message, author...
    pub graph_column_widths: BTreeMap<String, f32>,
    /// The zones' keys as dragged into place; empty: the default.
    pub graph_column_order: Vec<String>,
    /// The graph columns without a `show_*` flag of their own (ref, graph, message) switched off from the
    /// header's menu.
    pub hidden_graph_columns: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_fetch_minutes: 1,
            initial_commits: 2000,
            show_toolbar_labels: true,
            author_initials: true,
            show_author: false,
            show_date: false,
            show_sha: false,
            show_changes: false,
            show_description: true,
            description_on_hover: false,
            show_ghost_refs: true,
            highlight_rows_on_ref_hover: true,
            remember_tabs: true,
            push_after_commit: false,
            compact_graph: false,
            smart_branch_visibility: false,
            tab_group_color: None,
            syntax_highlighting: true,
            word_wrap: false,
            diff_whitespace: true,
            file_sort: FileSort::Path,
            zoom: 1.0,
            theme: "ImGit Dark".to_owned(),
            default_branch: "main".to_owned(),
            pull_mode: PullMode::FastForwardIfPossible,
            hidden_left_sections: BTreeSet::from(["Worktrees".to_owned()]),
            left_panel_width: 208.0,
            detail_panel_width: 438.0,
            terminal_height: 275.0,
            commit_message_height: 150.0,
            merge_a_fraction: 0.5,
            merge_top_fraction: 0.5,
            graph_column_widths: BTreeMap::new(),
            graph_column_order: Vec::new(),
            hidden_graph_columns: BTreeSet::new(),
        }
    }
}

/// C's `atoi`: the integer at the start of `text` after any white space (0 when there is none), saturating.
#[must_use]
pub fn atoi(text: &str) -> i32 {
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i64 = 0;
    for byte in digits.bytes().take_while(u8::is_ascii_digit) {
        value = (value * 10 + i64::from(byte - b'0')).min(i64::from(i32::MAX) + 1);
    }
    let value = if negative { -value } else { value };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// C's `atof` for the plain decimal notation settings files hold: the longest number at the start of `text`
/// after any white space (0 when there is none); `inf` and `nan` read as what they spell.
#[must_use]
pub fn atof(text: &str) -> f64 {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end += 1;
    }
    let lower = text[end..].to_ascii_lowercase();
    if lower.starts_with("nan") {
        return f64::NAN;
    }
    if lower.starts_with("inf") {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let digits_start = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
    }
    if end == digits_start || (end == digits_start + 1 && bytes[digits_start] == b'.') {
        return 0.0;
    }
    // An exponent only counts with digits after it.
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent = end + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            while bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
                exponent += 1;
            }
            end = exponent;
        }
    }
    text[..end].parse().unwrap_or(0.0)
}

/// A float from a settings value; `fallback` for anything that is not a finite number (a hand-edited "nan"
/// would otherwise poison the layout).
#[must_use]
pub fn finite_or(value: &str, fallback: f32) -> f32 {
    let parsed = atof(value) as f32;
    if parsed.is_finite() { parsed } else { fallback }
}

/// A float as a C++ `ostream` prints it by default (`%g`, six significant digits): `208`, `0.5`, `1.1`,
/// `1e-05`, `1.5e+06`.
#[must_use]
pub fn format_float(value: f32) -> String {
    let value = f64::from(value);
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    if !value.is_finite() {
        return if value.is_nan() {
            "nan"
        } else if value > 0.0 {
            "inf"
        } else {
            "-inf"
        }
        .to_owned();
    }
    // `{:.5e}` rounds to six significant digits and tells the decimal exponent of the rounded value.
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let strip = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text.to_owned()
        }
    };
    if !(-4..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", strip(mantissa), exponent.abs());
    }
    let decimals = usize::try_from(5 - exponent).unwrap_or(0);
    strip(&format!("{value:.decimals$}"))
}

/// The comma-separated entries of a value, without the empty ones.
fn list(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').filter(|entry| !entry.is_empty())
}

fn join<'a>(entries: impl IntoIterator<Item = &'a String>) -> String {
    entries
        .into_iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

impl Settings {
    /// Reads `path`; a missing or unreadable file leaves the defaults. Unknown keys and malformed lines are
    /// skipped.
    pub fn load(&mut self, path: &Path) {
        let text = fs::read(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let (syntax_saved, wrap_saved) = self.parse(&text);
        if !syntax_saved || !wrap_saved {
            // Before they were settings, Preferences kept the Syntax Highlighting and Word Wrap checkboxes in
            // preferences.txt.
            let legacy = path
                .parent()
                .map(|folder| folder.join("preferences.txt"))
                .and_then(|file| fs::read(file).ok())
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            for line in legacy.lines() {
                if !syntax_saved && line == "Editor/Syntax Highlighting\t0" {
                    self.syntax_highlighting = false;
                }
                if !wrap_saved && line == "Editor/Word Wrap\t1" {
                    self.word_wrap = true;
                }
            }
        }
    }

    /// Applies the lines of a settings file; whether it held `syntax_highlighting` and `word_wrap`.
    fn parse(&mut self, text: &str) -> (bool, bool) {
        let mut syntax_saved = false;
        let mut wrap_saved = false;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            let truthy = value == "1" || value == "true";
            match key {
                "auto_fetch_minutes" => self.auto_fetch_minutes = atoi(value).clamp(0, 60),
                "initial_commits" => self.initial_commits = atoi(value).max(500),
                "show_toolbar_labels" => self.show_toolbar_labels = truthy,
                "syntax_highlighting" => {
                    self.syntax_highlighting = truthy;
                    syntax_saved = true;
                }
                "word_wrap" => {
                    self.word_wrap = truthy;
                    wrap_saved = true;
                }
                "diff_whitespace" => self.diff_whitespace = truthy,
                "file_sort" => self.file_sort = FileSort::from_index(atoi(value)),
                "merge_a_fraction" => {
                    self.merge_a_fraction = finite_or(value, self.merge_a_fraction).clamp(0.1, 0.9);
                }
                "merge_top_fraction" => {
                    self.merge_top_fraction =
                        finite_or(value, self.merge_top_fraction).clamp(0.1, 0.9);
                }
                "author_initials" => self.author_initials = truthy,
                "show_author" => self.show_author = truthy,
                "show_date" => self.show_date = truthy,
                "show_sha" => self.show_sha = truthy,
                "show_changes" => self.show_changes = truthy,
                "show_description" => self.show_description = truthy,
                "description_on_hover" => self.description_on_hover = truthy,
                "show_ghost_refs" => self.show_ghost_refs = truthy,
                "highlight_rows_on_ref_hover" => self.highlight_rows_on_ref_hover = truthy,
                "remember_tabs" => self.remember_tabs = truthy,
                "push_after_commit" => self.push_after_commit = truthy,
                "compact_graph" => self.compact_graph = truthy,
                "smart_branch_visibility" => self.smart_branch_visibility = truthy,
                "tab_group_color" => {
                    let last = GRAPH_COLOR_COUNT as i32 - 1;
                    self.tab_group_color = usize::try_from(atoi(value).clamp(-1, last)).ok();
                }
                "zoom" => self.zoom = finite_or(value, self.zoom).clamp(0.5, 2.0),
                "theme" => self.theme = value.to_owned(),
                "default_branch" => self.default_branch = value.to_owned(),
                "pull_mode" => self.pull_mode = PullMode::from_index(atoi(value)),
                "left_panel_width" => {
                    self.left_panel_width = finite_or(value, self.left_panel_width)
                }
                "detail_panel_width" => {
                    self.detail_panel_width = finite_or(value, self.detail_panel_width);
                }
                "terminal_height" => self.terminal_height = finite_or(value, self.terminal_height),
                "commit_message_height" => {
                    self.commit_message_height = finite_or(value, self.commit_message_height);
                }
                "graph_column_widths" => {
                    self.graph_column_widths = list(value)
                        .filter_map(|column| column.split_once(':'))
                        .map(|(zone, width)| (zone.to_owned(), finite_or(width, 0.0)))
                        .collect();
                }
                "graph_column_order" => {
                    self.graph_column_order = list(value).map(str::to_owned).collect();
                }
                "hidden_graph_columns" => {
                    self.hidden_graph_columns = list(value).map(str::to_owned).collect();
                }
                "hidden_left_sections" => {
                    self.hidden_left_sections = list(value).map(str::to_owned).collect();
                }
                _ => {}
            }
        }
        (syntax_saved, wrap_saved)
    }

    /// The file's text.
    #[must_use]
    pub fn to_text(&self) -> String {
        let flag = |value: bool| u8::from(value);
        let mut text = String::new();
        let mut line = |key: &str, value: &dyn std::fmt::Display| {
            let _ = writeln!(text, "{key}={value}");
        };
        line("hidden_left_sections", &join(&self.hidden_left_sections));
        line("pull_mode", &self.pull_mode.index());
        line("left_panel_width", &format_float(self.left_panel_width));
        line("detail_panel_width", &format_float(self.detail_panel_width));
        line("terminal_height", &format_float(self.terminal_height));
        line(
            "commit_message_height",
            &format_float(self.commit_message_height),
        );
        line("merge_a_fraction", &format_float(self.merge_a_fraction));
        line("merge_top_fraction", &format_float(self.merge_top_fraction));
        let widths = self
            .graph_column_widths
            .iter()
            .map(|(zone, width)| format!("{zone}:{}", format_float(*width)))
            .collect::<Vec<_>>()
            .join(",");
        line("graph_column_widths", &widths);
        line("graph_column_order", &join(&self.graph_column_order));
        line("hidden_graph_columns", &join(&self.hidden_graph_columns));
        line("auto_fetch_minutes", &self.auto_fetch_minutes);
        line("initial_commits", &self.initial_commits);
        line("show_toolbar_labels", &flag(self.show_toolbar_labels));
        line("author_initials", &flag(self.author_initials));
        line("show_author", &flag(self.show_author));
        line("show_date", &flag(self.show_date));
        line("show_sha", &flag(self.show_sha));
        line("show_changes", &flag(self.show_changes));
        line("show_description", &flag(self.show_description));
        line("description_on_hover", &flag(self.description_on_hover));
        line("show_ghost_refs", &flag(self.show_ghost_refs));
        line(
            "highlight_rows_on_ref_hover",
            &flag(self.highlight_rows_on_ref_hover),
        );
        line("remember_tabs", &flag(self.remember_tabs));
        line("push_after_commit", &flag(self.push_after_commit));
        line("compact_graph", &flag(self.compact_graph));
        line(
            "smart_branch_visibility",
            &flag(self.smart_branch_visibility),
        );
        let color = self.tab_group_color.map_or(-1, |color| color as i64);
        line("tab_group_color", &color);
        line("syntax_highlighting", &flag(self.syntax_highlighting));
        line("word_wrap", &flag(self.word_wrap));
        line("diff_whitespace", &flag(self.diff_whitespace));
        line("file_sort", &self.file_sort.index());
        line("zoom", &format_float(self.zoom));
        line("theme", &self.theme);
        line("default_branch", &self.default_branch);
        text
    }

    /// Writes the file (through a temporary one next to it). Failures are ignored, as they were: the settings
    /// are written again at the next change.
    pub fn save(&self, path: &Path) {
        let _ = write_text_file(path, &self.to_text());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the C++ build wrote.
    const CPP_FILE: &str = "hidden_left_sections=Worktrees\n\
pull_mode=1\n\
left_panel_width=208\n\
detail_panel_width=438.5\n\
terminal_height=275\n\
commit_message_height=150\n\
merge_a_fraction=0.5\n\
merge_top_fraction=0.35\n\
graph_column_widths=author:120,ref:129.5\n\
graph_column_order=ref,graph,message\n\
hidden_graph_columns=message\n\
auto_fetch_minutes=5\n\
initial_commits=2000\n\
show_toolbar_labels=1\n\
author_initials=1\n\
show_author=1\n\
show_date=0\n\
show_sha=0\n\
show_changes=0\n\
show_description=1\n\
description_on_hover=0\n\
show_ghost_refs=1\n\
highlight_rows_on_ref_hover=1\n\
remember_tabs=1\n\
push_after_commit=0\n\
compact_graph=0\n\
smart_branch_visibility=0\n\
tab_group_color=-1\n\
syntax_highlighting=1\n\
word_wrap=0\n\
diff_whitespace=1\n\
file_sort=2\n\
zoom=1.1\n\
theme=ImGit Light\n\
default_branch=main\n";

    #[test]
    fn a_cpp_settings_file_round_trips_byte_for_byte() {
        let mut settings = Settings::default();
        let (syntax, wrap) = settings.parse(CPP_FILE);
        assert!(syntax && wrap);
        assert_eq!(settings.detail_panel_width, 438.5);
        assert_eq!(settings.merge_top_fraction, 0.35);
        assert_eq!(settings.file_sort, FileSort::Name);
        assert_eq!(settings.theme, "ImGit Light");
        assert_eq!(settings.auto_fetch_minutes, 5);
        assert!(settings.show_author);
        assert_eq!(settings.graph_column_widths["ref"], 129.5);
        assert_eq!(settings.graph_column_order, ["ref", "graph", "message"]);
        assert!(settings.hidden_graph_columns.contains("message"));
        assert_eq!(settings.tab_group_color, None);
        assert_eq!(settings.to_text(), CPP_FILE);
    }

    #[test]
    fn settings_survive_the_disk() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("settings.txt");
        let mut settings = Settings {
            zoom: 1.25,
            tab_group_color: Some(3),
            pull_mode: PullMode::Rebase,
            ..Settings::default()
        };
        settings.hidden_left_sections.insert("Stashes".to_owned());
        settings.graph_column_widths.insert("date".to_owned(), 88.0);
        settings.save(&path);
        let mut loaded = Settings::default();
        loaded.load(&path);
        assert_eq!(loaded, settings);
    }

    #[test]
    fn values_are_clamped_and_bad_numbers_ignored() {
        let mut settings = Settings::default();
        settings.parse(
            "auto_fetch_minutes=99\ninitial_commits=10\nzoom=nan\nmerge_a_fraction=7\n\
tab_group_color=99\npull_mode=9\nfile_sort=-3\nno equals sign\nleft_panel_width = 300 \n",
        );
        assert_eq!(settings.auto_fetch_minutes, 60);
        assert_eq!(settings.initial_commits, 500);
        assert_eq!(settings.zoom, 1.0);
        assert_eq!(settings.merge_a_fraction, 0.9);
        assert_eq!(settings.tab_group_color, Some(GRAPH_COLOR_COUNT - 1));
        assert_eq!(settings.pull_mode, PullMode::Rebase);
        assert_eq!(settings.file_sort, FileSort::Path);
        assert_eq!(settings.left_panel_width, 300.0);
    }

    #[test]
    fn syntax_and_wrap_come_from_the_legacy_preferences_when_unsaved() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("settings.txt"), "zoom=1\n").unwrap();
        fs::write(
            folder.path().join("preferences.txt"),
            "Editor/Syntax Highlighting\t0\nEditor/Word Wrap\t1\n",
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.load(&folder.path().join("settings.txt"));
        assert!(!settings.syntax_highlighting);
        assert!(settings.word_wrap);
        // Saved values win.
        fs::write(
            folder.path().join("settings.txt"),
            "syntax_highlighting=1\nword_wrap=0\n",
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.load(&folder.path().join("settings.txt"));
        assert!(settings.syntax_highlighting);
        assert!(!settings.word_wrap);
    }

    #[test]
    fn floats_print_like_an_ostream() {
        for (value, text) in [
            (208.0, "208"),
            (0.5, "0.5"),
            (1.1, "1.1"),
            (0.1, "0.1"),
            (123456.0, "123456"),
            (1234567.0, "1.23457e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.25, "-2.25"),
            (0.0, "0"),
            (99.999_99, "100"),
        ] {
            assert_eq!(format_float(value), text, "{value}");
        }
    }

    #[test]
    fn atoi_and_atof_read_prefixes() {
        assert_eq!(atoi("  42abc"), 42);
        assert_eq!(atoi("-7"), -7);
        assert_eq!(atoi("abc"), 0);
        assert_eq!(atoi("99999999999"), i32::MAX);
        assert_eq!(atof("1.5px"), 1.5);
        assert_eq!(atof(".5"), 0.5);
        assert_eq!(atof("1e3"), 1000.0);
        assert_eq!(atof("1e"), 1.0);
        assert_eq!(atof("-"), 0.0);
        assert!(atof("nan").is_nan());
        assert_eq!(atof("-inf"), f64::NEG_INFINITY);
    }
}
