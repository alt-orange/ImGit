//! The text formats of the profile's small state files, byte-compatible with the C++ build: `tabs.txt`,
//! `recent.txt`, `aliases.txt` and `hidden_refs.txt`. Pure parsing and formatting; the application reads and
//! writes the files.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::app::settings::atoi;
use crate::app::tabs::TabGroup;

/// The key of a repository in the aliases and hidden refs: its path with forward slashes and no trailing one.
#[must_use]
pub fn alias_key(path: &str) -> String {
    let mut key = path.replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    key
}

/// One tab of `tabs.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabEntry {
    pub path: String,
    /// The tab's own name.
    pub name: Option<String>,
    /// The tab's group, one of the groups declared before the tab.
    pub group: Option<String>,
}

/// `tabs.txt`: one line per tab, its path then (tab-separated) `name=` / `group=` fields when set; a line
/// `#group=<colour>\t<collapsed>\t<name>` per tab group and `#active=<index>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TabsFile {
    pub groups: Vec<TabGroup>,
    pub tabs: Vec<TabEntry>,
    /// The active tab; `-1` is the New Tab page. `None` when the file does not say.
    pub active: Option<i32>,
}

impl TabsFile {
    /// Reads the file's text. A tab line's `group=` naming a group not declared above it is ignored.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut file = Self::default();
        for line in text.lines() {
            if let Some(index) = line.strip_prefix("#active=") {
                file.active = Some(atoi(index));
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if let Some(color) = line.strip_prefix("#group=") {
                let color = color.split('\t').next().unwrap_or_default();
                if let [_, collapsed, name, ..] = fields.as_slice()
                    && !name.is_empty()
                    && !file.groups.iter().any(|group| group.name == *name)
                {
                    file.groups.push(TabGroup {
                        name: (*name).to_owned(),
                        color: usize::try_from(atoi(color).max(0)).unwrap_or_default(),
                        collapsed: *collapsed == "1",
                    });
                }
                continue;
            }
            let path = fields.first().map_or("", |path| path.trim());
            if path.is_empty() {
                continue;
            }
            let mut entry = TabEntry {
                path: path.to_owned(),
                name: None,
                group: None,
            };
            for field in &fields[1..] {
                if let Some(name) = field.strip_prefix("name=") {
                    entry.name = Some(name.to_owned());
                } else if let Some(group) = field.strip_prefix("group=")
                    && file.groups.iter().any(|known| known.name == group)
                {
                    entry.group = Some(group.to_owned());
                }
            }
            file.tabs.push(entry);
        }
        file
    }

    /// The file's text.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for group in &self.groups {
            text.push_str(&format!(
                "#group={}\t{}\t{}\n",
                group.color,
                if group.collapsed { "1" } else { "0" },
                group.name
            ));
        }
        for tab in &self.tabs {
            text.push_str(&tab.path);
            if let Some(name) = tab.name.as_deref().filter(|name| !name.is_empty()) {
                text.push_str(&format!("\tname={name}"));
            }
            if let Some(group) = tab.group.as_deref().filter(|group| !group.is_empty()) {
                text.push_str(&format!("\tgroup={group}"));
            }
            text.push('\n');
        }
        text.push_str(&format!("#active={}\n", self.active.unwrap_or(-1)));
        text
    }
}

/// `recent.txt`: one path per line, newest first; the ones that no longer exist are dropped.
#[must_use]
pub fn parse_recent(text: &str, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| exists(path))
        .collect()
}

#[must_use]
pub fn format_recent(recent: &[PathBuf]) -> String {
    recent
        .iter()
        .map(|path| format!("{}\n", path.display()))
        .collect()
}

/// `<path>\t<value>` lines whose two parts are not empty (the value is the rest of the line), keyed by
/// [`alias_key`].
fn key_value_lines(text: &str) -> impl Iterator<Item = (String, &str)> {
    text.lines().filter_map(|line| {
        let (path, value) = line.split_once('\t')?;
        (!path.is_empty() && !value.is_empty()).then(|| (alias_key(path), value))
    })
}

/// `aliases.txt`: `<path>\t<alias>` lines.
#[must_use]
pub fn parse_aliases(text: &str) -> BTreeMap<String, String> {
    key_value_lines(text)
        .map(|(path, alias)| (path, alias.to_owned()))
        .collect()
}

#[must_use]
pub fn format_aliases(aliases: &BTreeMap<String, String>) -> String {
    aliases
        .iter()
        .map(|(path, alias)| format!("{path}\t{alias}\n"))
        .collect()
}

/// `hidden_refs.txt`: `<path>\t<full ref name>` lines.
#[must_use]
pub fn parse_hidden_refs(text: &str) -> BTreeMap<String, HashSet<String>> {
    let mut hidden: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for (path, name) in key_value_lines(text) {
        hidden.entry(path).or_default().insert(name.to_owned());
    }
    hidden
}

#[must_use]
pub fn format_hidden_refs(hidden: &BTreeMap<String, HashSet<String>>) -> String {
    let mut text = String::new();
    for (path, refs) in hidden {
        let mut sorted: Vec<&String> = refs.iter().collect();
        sorted.sort();
        for name in sorted {
            text.push_str(&format!("{path}\t{name}\n"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_use_forward_slashes_and_no_trailing_one() {
        assert_eq!(alias_key("C:\\Users\\me\\repo\\"), "C:/Users/me/repo");
        assert_eq!(alias_key("C:/Users/me/repo/"), "C:/Users/me/repo");
        assert_eq!(alias_key("/"), "/");
        assert_eq!(alias_key("/home/me/repo"), "/home/me/repo");
    }

    const TABS: &str = "#group=2\t1\tWork\n\
#group=5\t0\tPlay\n\
C:/Users/me/one/\tname=Mine\tgroup=Work\n\
C:/Users/me/two/\n\
C:/Users/me/three/\tgroup=Play\n\
#active=1\n";

    #[test]
    fn tabs_parse_and_format_back() {
        let file = TabsFile::parse(TABS);
        assert_eq!(file.groups.len(), 2);
        assert_eq!(file.groups[0].name, "Work");
        assert_eq!(file.groups[0].color, 2);
        assert!(file.groups[0].collapsed);
        assert!(!file.groups[1].collapsed);
        assert_eq!(file.tabs.len(), 3);
        assert_eq!(file.tabs[0].name.as_deref(), Some("Mine"));
        assert_eq!(file.tabs[0].group.as_deref(), Some("Work"));
        assert_eq!(file.tabs[1].group, None);
        assert_eq!(file.active, Some(1));
        assert_eq!(file.to_text(), TABS);
    }

    #[test]
    fn tabs_files_are_read_forgivingly() {
        let file = TabsFile::parse(
            "C:/a/\tgroup=Nowhere\r\n\r\n#group=x\t0\t\n#group=-4\t0\tG\n#group=1\t0\tG\n#active=-1\n",
        );
        assert_eq!(file.tabs.len(), 1);
        // A group that is not declared (yet) is not joined; empty names and repeated groups are skipped.
        assert_eq!(file.tabs[0].group, None);
        assert_eq!(file.groups.len(), 1);
        assert_eq!(file.groups[0].color, 0);
        assert_eq!(file.active, Some(-1));
        assert_eq!(TabsFile::default().to_text(), "#active=-1\n");
    }

    #[test]
    fn recent_repositories_keep_the_existing_ones() {
        let recent = parse_recent(" /a \n\n/gone\n/b\r\n", |path| path != Path::new("/gone"));
        assert_eq!(recent, [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(format_recent(&recent), "/a\n/b\n");
    }

    #[test]
    fn aliases_round_trip() {
        let text = "C:\\Users\\me\\repo\\\tMy repo\n/home/x\tAnother\n\tno path\nno-tab\n/y\t\n";
        let aliases = parse_aliases(text);
        assert_eq!(aliases.len(), 2);
        assert_eq!(aliases["C:/Users/me/repo"], "My repo");
        assert_eq!(
            format_aliases(&aliases),
            "/home/x\tAnother\nC:/Users/me/repo\tMy repo\n"
        );
    }

    #[test]
    fn hidden_refs_round_trip_sorted() {
        let hidden =
            parse_hidden_refs("/r\trefs/heads/b\n/r\trefs/heads/a\n/s/\trefs/tags/v1\nbroken\n");
        assert_eq!(hidden.len(), 2);
        assert!(hidden["/r"].contains("refs/heads/a"));
        assert_eq!(
            format_hidden_refs(&hidden),
            "/r\trefs/heads/a\n/r\trefs/heads/b\n/s\trefs/tags/v1\n"
        );
    }
}
