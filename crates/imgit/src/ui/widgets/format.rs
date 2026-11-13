//! Text for the UI: dates in the user's formats, relative times, shortcut labels and the small string helpers
//! (which live in [`imgit_app::text`] and are re-exported here, where the C++ `ui::` had them).

use std::time::{SystemTime, UNIX_EPOCH};

use imgit_app::date_format::{DateTime, locale_code, moment_format};
use imgit_app::preferences::{
    DEFAULT_DATE_FORMAT, DEFAULT_DATE_TIME_FORMAT, DEFAULT_DATE_VERBOSE_FORMAT,
    DEFAULT_DATE_WORD_FORMAT,
};
use imgit_app::{Preference, pref, preferences};

pub use imgit_app::text::{contains_ci, directory_name, file_name, fuzzy_score, to_upper};

use crate::ui::Cx;

/// Keyboard shortcuts are written for Windows ("Ctrl+Shift+T", "Ctrl + Alt + F"); ImGui reads the same chords
/// as Cmd and Option on macOS, so with `macos` the words Ctrl and Alt in `label` become Cmd and Option.
#[must_use]
pub fn shortcut_label(label: &str, macos: bool) -> String {
    if !macos {
        return label.to_owned();
    }
    let mut result = label.to_owned();
    for (word, replacement) in [("Ctrl", "Cmd"), ("Alt", "Option")] {
        let mut at = 0;
        while let Some(found) = result[at..].find(word).map(|offset| at + offset) {
            let bytes = result.as_bytes();
            let end = found + word.len();
            let starts = found == 0 || !bytes[found - 1].is_ascii_alphanumeric();
            let ends = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
            if starts && ends {
                result.replace_range(found..end, replacement);
                at = found + replacement.len();
            } else {
                at = end;
            }
        }
    }
    result
}

/// How long ago `delta_seconds` was, in words: "just now", "5 minutes ago", "yesterday", "3 weeks ago"...
#[must_use]
pub fn relative_time(delta_seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 3600;
    const DAY: i64 = 86400;
    let delta = delta_seconds.max(0);
    let counted = |count: i64, one: &str, unit: &str| {
        if count == 1 {
            one.to_owned()
        } else {
            format!("{count} {unit} ago")
        }
    };
    if delta < MINUTE {
        "just now".to_owned()
    } else if delta < HOUR {
        counted(delta / MINUTE, "1 minute ago", "minutes")
    } else if delta < DAY {
        counted(delta / HOUR, "1 hour ago", "hours")
    } else if delta < DAY * 2 {
        "yesterday".to_owned()
    } else if delta < DAY * 7 {
        format!("{} days ago", delta / DAY)
    } else if delta < DAY * 30 {
        counted(delta / (DAY * 7), "a week ago", "weeks")
    } else if delta < DAY * 365 {
        counted(delta / (DAY * 30), "a month ago", "months")
    } else {
        counted(delta / (DAY * 365), "a year ago", "years")
    }
}

/// How long before now `unix_seconds` was, in words ([`relative_time`]).
#[must_use]
pub fn format_relative(unix_seconds: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        });
    relative_to(now, unix_seconds)
}

/// [`relative_time`] of `unix_seconds` as seen at `now`; a commit date at either end of the `i64` range gives
/// the nearest extreme instead of overflowing.
fn relative_to(now: i64, unix_seconds: i64) -> String {
    relative_time(now.saturating_sub(unix_seconds))
}

impl Cx<'_> {
    /// [`shortcut_label`] for the platform ImGui runs on (`io.ConfigMacOSXBehaviors`).
    #[must_use]
    pub fn shortcut_label(&self, label: &str) -> String {
        shortcut_label(label, self.macos_behaviors())
    }

    /// `unix_seconds` as local time in the Date/Time preference `format` (`fallback` while it is empty), in
    /// the chosen locale. Empty for a time the system cannot represent.
    fn format_preference(&self, unix_seconds: i64, format: Preference, fallback: &str) -> String {
        let Some(local) = imgit_platform::system::local_time(unix_seconds) else {
            return String::new();
        };
        let preferences = preferences();
        let chosen = preferences.get(format);
        let locale = locale_code(
            &preferences.get(pref::LOCALE),
            self.state.system_locale.as_deref(),
        );
        moment_format(
            if chosen.is_empty() { fallback } else { &chosen },
            &DateTime::from(local),
            &locale,
        )
    }

    // Dates in the Preferences → UI Customization → Date/Time formats and locale.

    /// The Date/Time Format: "10/24/2026 @ 4:43 PM".
    #[must_use]
    pub fn format_date_time(&self, unix_seconds: i64) -> String {
        self.format_preference(
            unix_seconds,
            pref::DATE_TIME_FORMAT,
            DEFAULT_DATE_TIME_FORMAT,
        )
    }

    /// The Date Format, the date alone: "10/25/2026".
    #[must_use]
    pub fn format_date(&self, unix_seconds: i64) -> String {
        self.format_preference(unix_seconds, pref::DATE_FORMAT, DEFAULT_DATE_FORMAT)
    }

    /// The Date word Format: "October 25, 2026".
    #[must_use]
    pub fn format_date_word(&self, unix_seconds: i64) -> String {
        self.format_preference(
            unix_seconds,
            pref::DATE_WORD_FORMAT,
            DEFAULT_DATE_WORD_FORMAT,
        )
    }

    /// The Date verbose Format: "October 25, 2026 4:43 PM".
    #[must_use]
    pub fn format_date_long(&self, unix_seconds: i64) -> String {
        self.format_preference(
            unix_seconds,
            pref::DATE_VERBOSE_FORMAT,
            DEFAULT_DATE_VERBOSE_FORMAT,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_labels_follow_the_platform() {
        assert_eq!(shortcut_label("Ctrl+Shift+T", false), "Ctrl+Shift+T");
        assert_eq!(shortcut_label("Ctrl+Shift+T", true), "Cmd+Shift+T");
        assert_eq!(shortcut_label("Ctrl + Alt + F", true), "Cmd + Option + F");
        assert_eq!(
            shortcut_label("Undo (Ctrl+Z), redo (Ctrl+Y)", true),
            "Undo (Cmd+Z), redo (Cmd+Y)"
        );
        // Whole words only.
        assert_eq!(
            shortcut_label("Control Alternate", true),
            "Control Alternate"
        );
        assert_eq!(shortcut_label("AltGr Ctrl2 xAlt", true), "AltGr Ctrl2 xAlt");
        assert_eq!(shortcut_label("Alt", true), "Option");
        assert_eq!(shortcut_label("", true), "");
        assert_eq!(shortcut_label("Größe Alt+é", true), "Größe Option+é");
    }

    #[test]
    fn relative_times() {
        let day = 86400;
        for (delta, expected) in [
            (-5, "just now"),
            (59, "just now"),
            (60, "1 minute ago"),
            (150, "2 minutes ago"),
            (3600, "1 hour ago"),
            (3 * 3600 + 5, "3 hours ago"),
            (day, "yesterday"),
            (2 * day, "2 days ago"),
            (6 * day, "6 days ago"),
            (7 * day, "a week ago"),
            (29 * day, "4 weeks ago"),
            (30 * day, "a month ago"),
            (200 * day, "6 months ago"),
            (365 * day, "a year ago"),
            (800 * day, "2 years ago"),
        ] {
            assert_eq!(relative_time(delta), expected, "{delta}");
        }
        assert_eq!(format_relative(i64::MAX), "just now");
    }

    #[test]
    fn absurd_timestamps_do_not_overflow() {
        // Both of these panicked in a debug build: `now - unix_seconds` left the range.
        assert!(relative_to(1_000, i64::MIN).ends_with("years ago"));
        assert_eq!(relative_to(-1_000, i64::MAX), "just now");
        assert_eq!(relative_to(i64::MAX, i64::MIN), relative_time(i64::MAX));
        assert!(format_relative(i64::MIN).ends_with("years ago"));
        // The date formats give an empty string for a time the system cannot represent.
        assert!(imgit_platform::system::local_time(i64::MIN).is_none());
        assert!(imgit_platform::system::local_time(i64::MAX).is_none());
    }

    #[test]
    fn string_helpers_come_from_the_app_crate() {
        assert_eq!(file_name("src/ui/widgets.rs"), "widgets.rs");
        assert_eq!(directory_name("src/ui/widgets.rs"), "src/ui");
        assert_eq!(to_upper("staged"), "STAGED");
        assert!(contains_ci("Fix Parser", "parser"));
        assert!(fuzzy_score("src/main.rs", "smr") > 0);
        assert!(fuzzy_score("src/main.rs", "xyz") < 0);
    }
}
