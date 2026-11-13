//! The file lists of every view: files grouped into folders for the "Tree" view with the per-folder counts, the
//! sort orders, the file and folder rows and the events a list reports.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use dear_imgui_rs::{MouseButton, WindowHoveredFlags};
use imgit_app::repo_state::{DiffSource, DiffTarget};
use imgit_git::{FileChange, FileStatus};

use super::controls::{css_text_y, tone_button, tone_button_width};
use crate::app::application::Application;
use crate::app::settings::FileSort;
use crate::theme::{Color, FONT_SIZE_ROW, FONT_SIZE_TINY};
use crate::ui::menu::popup_menu;
use crate::ui::panels::file_editor::navigate_from_editor;
use crate::ui::panels::menus;
use crate::ui::widgets::file_name;
use crate::ui::{Cx, DrawList, Icon, Rect, Vec2, vec2};
use imgit_app::RepoState;

/// Files grouped into folders for the "Tree" view, with the per-folder counts shown next to the folder name
/// (modified / added / deleted files).
#[derive(Debug, Default)]
pub struct FileNode<'a> {
    pub name: String,
    pub path: String,
    pub children: BTreeMap<String, FileNode<'a>>,
    pub file: Option<&'a FileChange>,
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    pub additions: usize,
    pub deletions: usize,
}

fn count_file(node: &mut FileNode<'_>, change: &FileChange) {
    match change.status {
        FileStatus::Added | FileStatus::Untracked => node.added += 1,
        FileStatus::Deleted => node.deleted += 1,
        _ => node.modified += 1,
    }
    let stats = change.stats.unwrap_or_default();
    node.additions += stats.additions;
    node.deletions += stats.deletions;
}

/// The tree of `changes`: folders by path segment in the Tree view, a flat list of full paths otherwise.
pub fn build_tree(changes: &[FileChange], tree_view: bool) -> FileNode<'_> {
    let mut root = FileNode::default();
    for change in changes {
        count_file(&mut root, change);
        let mut node = &mut root;
        if tree_view {
            let mut start = 0;
            while let Some(slash) = change.path[start..].find('/') {
                let slash = start + slash;
                let segment = &change.path[start..slash];
                let child = node.children.entry(segment.to_owned()).or_default();
                child.name = segment.to_owned();
                child.path = change.path[..slash].to_owned();
                count_file(child, change);
                node = child;
                start = slash + 1;
            }
        }
        let name = if tree_view {
            file_name(&change.path)
        } else {
            &change.path
        };
        // The \x01 sorts a file after a folder of the same name.
        let leaf = node.children.entry(format!("{name}\x01")).or_default();
        leaf.name = name.to_owned();
        leaf.path = change.path.clone();
        leaf.file = Some(change);
        let stats = change.stats.unwrap_or_default();
        leaf.additions = stats.additions;
        leaf.deletions = stats.deletions;
    }
    root
}

/// The order of a file's status under Sort by status: conflicts first, then added, modified, renamed and deleted
/// files; files of an unchanged tree (View all files) last.
pub fn status_rank(status: FileStatus) -> u8 {
    match status {
        FileStatus::Conflicted => 0,
        FileStatus::Added | FileStatus::Untracked => 1,
        FileStatus::Modified | FileStatus::TypeChange => 2,
        FileStatus::Renamed => 3,
        FileStatus::Deleted => 4,
        FileStatus::Unknown => 5,
    }
}

/// `a` before `b` comparing letters regardless of case (byte-wise, ASCII).
fn compare_ignoring_case(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(b.bytes().map(|byte| byte.to_ascii_lowercase()))
}

/// A node's children in the Sort button's order. By path they keep the tree's own order (folders and files by
/// name, as git sorts paths); by name or status the folders come first, by name, then the files.
pub fn sorted_children<'n, 'a>(node: &'n FileNode<'a>, sort: FileSort) -> Vec<&'n FileNode<'a>> {
    let mut children: Vec<&FileNode<'a>> = node.children.values().collect();
    match sort {
        FileSort::Path => {}
        FileSort::PathDescending => children.reverse(),
        FileSort::Name | FileSort::Status => children.sort_by(|a, b| {
            match (a.file, b.file) {
                (None, Some(_)) => return Ordering::Less,
                (Some(_), None) => return Ordering::Greater,
                (Some(a_file), Some(b_file)) if sort == FileSort::Status => {
                    let by_status = status_rank(a_file.status).cmp(&status_rank(b_file.status));
                    if by_status != Ordering::Equal {
                        return by_status;
                    }
                }
                _ => {}
            }
            let a_name = if a.file.is_none() {
                a.name.as_str()
            } else {
                file_name(&a.path)
            };
            let b_name = if b.file.is_none() {
                b.name.as_str()
            } else {
                file_name(&b.path)
            };
            compare_ignoring_case(a_name, b_name).then_with(|| a.path.cmp(&b.path))
        }),
    }
    children
}

/// The paths of every file under `node`.
pub fn collect_paths(node: &FileNode<'_>, out: &mut Vec<String>) {
    for child in node.children.values() {
        match child.file {
            Some(file) => out.push(file.path.clone()),
            None => collect_paths(child, out),
        }
    }
}

/// "✎ 63  + 34  − 3" after a folder, or "+12 −3" after a file; draws right to left from `right_limit` at height
/// `y` and returns the width used.
#[allow(clippy::too_many_arguments)]
fn draw_counts(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    y: f32,
    right_limit: f32,
    node: &FileNode<'_>,
    folder: bool,
) -> f32 {
    let p = cx.palette;
    let mut x = right_limit;
    let mut item_right = |cx: &mut Cx<'_>, text: &str, color: Color, icon: Option<Icon>| {
        let size = cx.text_size(cx.fonts.regular, FONT_SIZE_TINY, text);
        x -= size.x;
        cx.draw_text(
            draw,
            cx.fonts.regular,
            FONT_SIZE_TINY,
            vec2(x, y),
            color,
            text,
        );
        if let Some(icon) = icon {
            x -= cx.px(12.0);
            cx.draw_icon(
                draw,
                icon,
                vec2(x + cx.px(5.0), y + size.y / 2.0),
                cx.px(9.0),
                color,
            );
        }
        x -= cx.px(8.0);
    };
    if folder {
        if node.deleted > 0 {
            item_right(cx, &node.deleted.to_string(), p.red, Some(Icon::Minus));
        }
        if node.added > 0 {
            item_right(cx, &node.added.to_string(), p.green, Some(Icon::Plus));
        }
        if node.modified > 0 {
            item_right(
                cx,
                &node.modified.to_string(),
                p.stats_files,
                Some(Icon::Pencil),
            );
        }
    } else {
        if node.deletions > 0 {
            item_right(cx, &format!("\u{2212}{}", node.deletions), p.red, None);
        }
        if node.additions > 0 {
            item_right(cx, &format!("+{}", node.additions), p.green, None);
        }
    }
    right_limit - x
}

/// The Font Awesome icon of a file's status and its colour.
fn status_fa_icon(cx: &Cx<'_>, status: FileStatus) -> (&'static str, Color) {
    let p = cx.palette;
    match status {
        FileStatus::Added | FileStatus::Untracked => ("fas:plus", p.green),
        FileStatus::Deleted => ("fas:minus", p.red),
        FileStatus::Conflicted => ("fas:exclamation-triangle", p.red),
        // An unchanged file of View all files.
        FileStatus::Unknown => ("far:file", p.text_secondary),
        _ => ("fas:pencil", p.orange),
    }
}

/// The button a hovered file row of the working directory shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    /// "Stage File"
    Stage,
    /// "Mark resolved" (a conflicted file): the same green button.
    MarkResolved,
    /// "Unstage File"
    Unstage,
}

#[derive(Debug, Default)]
struct FileRowResult {
    clicked: bool,
    action: bool,
    right_clicked: bool,
}

/// .file-node (32px): the 10px solid status icon in a 12.5px fa-fw box 10px in, the 14px name 5px after;
/// hovered, the "Stage File" / "Unstage File" xs button 10px from the right edge.
#[allow(clippy::too_many_arguments)]
fn file_row(
    cx: &mut Cx<'_>,
    change: &FileChange,
    label: &str,
    width: f32,
    depth: usize,
    action: Option<RowAction>,
    active: bool,
) -> FileRowResult {
    let ui = cx.ui;
    let p = cx.palette;
    let height = cx.px(32.0);
    let pos = cx.cursor();
    let _id = ui.push_id(change.path.as_str());
    let mut result = FileRowResult::default();
    let hovered = ui
        .is_window_hovered_with_flags(WindowHoveredFlags::ALLOW_WHEN_BLOCKED_BY_ACTIVE_ITEM)
        && Rect::from_pos_size(pos, vec2(width, height)).contains(cx.mouse_pos());
    let draw = cx.window_draw_list();
    if active {
        draw.add_rect_filled(pos, pos + vec2(width, height), p.selected_row, 0.0);
    } else if hovered {
        draw.add_rect_filled(pos, pos + vec2(width, height), p.hover_row, 0.0);
    }
    let mut right = pos.x + width;
    if let (true, Some(action)) = (hovered, action) {
        let unstage = action == RowAction::Unstage;
        let button = if unstage {
            "Unstage File"
        } else {
            "Stage File"
        };
        let w = tone_button_width(cx, button);
        right = pos.x + width - cx.px(10.0) - w;
        if tone_button(
            cx,
            "##action",
            button,
            vec2(right, pos.y + cx.px(6.0)),
            cx.px(22.0),
            if unstage { p.red } else { p.green },
            true,
        ) {
            result.action = true;
        }
    }
    let indent = cx.px(14.0) * depth.saturating_sub(1) as f32;
    let (icon, tone) = status_fa_icon(cx, change.status);
    cx.draw_fa_icon(
        draw,
        icon,
        vec2(pos.x + indent + cx.px(16.25), pos.y + cx.px(16.0)),
        cx.px(10.0),
        tone,
    );
    let name_x = pos.x + indent + cx.px(27.5);
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        14.0,
        vec2(name_x, css_text_y(cx, 14.0, pos.y + cx.px(6.0))),
        right - cx.px(10.0) - name_x,
        p.text_normal,
        label,
    );

    cx.set_cursor(pos);
    ui.invisible_button("##file", vec2(width, height));
    result.clicked = ui.is_item_clicked_with_button(MouseButton::Left);
    result.right_clicked = ui.is_item_hovered() && ui.is_mouse_released(MouseButton::Right);
    if let Some(old_path) = &change.old_path {
        cx.tooltip(&format!("{old_path} \u{2192} {}", change.path));
    }
    result
}

/// What clicking a folder row did.
struct FolderRowResult {
    open: bool,
    right_clicked: bool,
}

fn folder_row(
    cx: &mut Cx<'_>,
    expanded: &mut HashSet<String>,
    node: &FileNode<'_>,
    width: f32,
    depth: usize,
    key: &str,
) -> FolderRowResult {
    let ui = cx.ui;
    let p = cx.palette;
    let height = cx.px(24.0);
    let pos = cx.cursor();
    let collapsed = !expanded.contains(key);
    let _id = ui.push_id(key);
    ui.invisible_button("##folder", vec2(width, height));
    let draw = cx.window_draw_list();
    let hovered = ui.is_item_hovered();
    if hovered {
        draw.add_rect_filled(pos, pos + vec2(width, height), p.hover_row, 0.0);
    }
    if ui.is_item_clicked_with_button(MouseButton::Left) && !expanded.remove(key) {
        expanded.insert(key.to_owned());
    }
    let right_clicked = hovered && ui.is_mouse_released(MouseButton::Right);
    let indent = cx.px(10.0) + cx.px(14.0) * depth as f32;
    cx.draw_icon(
        draw,
        if collapsed {
            Icon::ChevronRight
        } else {
            Icon::ChevronDown
        },
        vec2(pos.x + indent - cx.px(2.0), pos.y + height / 2.0),
        cx.px(9.0),
        p.text_secondary,
    );
    let text_h = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, "A").y;
    let counts_w = draw_counts(
        cx,
        draw,
        pos.y + (height - text_h) / 2.0,
        pos.x + width - cx.px(10.0),
        node,
        true,
    );
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        FONT_SIZE_ROW,
        vec2(
            pos.x + indent + cx.px(10.0),
            pos.y + (height - text_h) / 2.0,
        ),
        width - indent - cx.px(24.0) - counts_w,
        p.text_normal,
        &node.name,
    );
    FolderRowResult {
        open: !collapsed,
        right_clicked,
    }
}

/// Which list a [`FileList`] shows: where its rows' diffs come from and which menus and buttons they get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListKind {
    Unstaged,
    Staged,
    /// A commit's files.
    Commit(String),
    /// A commit compared against the working directory.
    Compare(String),
}

impl ListKind {
    /// The diff a file of this list opens.
    pub fn diff_source(&self) -> DiffSource {
        match self {
            Self::Unstaged => DiffSource::Unstaged,
            Self::Staged => DiffSource::Staged,
            Self::Commit(sha) => DiffSource::Commit(sha.clone()),
            Self::Compare(sha) => DiffSource::Compare(sha.clone()),
        }
    }

    /// A commit's files and a commit compared against the working directory: no staging actions.
    pub fn commit_side(&self) -> bool {
        matches!(self, Self::Commit(_) | Self::Compare(_))
    }
}

/// Whether two diff sources are of the same kind (the commit's sha is not compared).
fn same_source_kind(a: &DiffSource, b: &DiffSource) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// What the user did in a file list; applied after the list is drawn.
#[derive(Debug)]
pub enum ListEvent {
    /// A file row was clicked: open its diff (the merge tool of a conflict).
    Open(FileChange),
    /// The hovered row's Stage / Unstage button.
    Action(FileChange),
    /// A file row was right-clicked.
    FileMenu(FileChange),
    /// A folder row was right-clicked.
    FolderMenu { folder: String, files: Vec<String> },
}

/// One file list being drawn.
pub struct FileList<'a> {
    pub width: f32,
    pub kind: ListKind,
    pub action: Option<RowAction>,
    pub file_sort: FileSort,
    pub diff_target: Option<&'a DiffTarget>,
    pub expanded: &'a mut HashSet<String>,
    pub events: Vec<ListEvent>,
}

impl FileList<'_> {
    /// Draws `node`'s children (folders open when expanded) at the layout cursor.
    pub fn draw(&mut self, cx: &mut Cx<'_>, node: &FileNode<'_>, depth: usize, prefix: &str) {
        let source = self.kind.diff_source();
        for child in sorted_children(node, self.file_sort) {
            if let Some(change) = child.file {
                let active = self.diff_target.is_some_and(|target| {
                    same_source_kind(&target.source, &source) && target.path == change.path
                });
                let row = file_row(
                    cx,
                    change,
                    &child.name,
                    self.width,
                    depth,
                    self.action,
                    active,
                );
                if row.clicked {
                    self.events.push(ListEvent::Open(change.clone()));
                }
                if row.action {
                    self.events.push(ListEvent::Action(change.clone()));
                }
                if row.right_clicked {
                    self.events.push(ListEvent::FileMenu(change.clone()));
                }
                continue;
            }
            let folder_key = format!("{prefix}{}", child.path);
            let row = folder_row(cx, self.expanded, child, self.width, depth, &folder_key);
            if row.right_clicked {
                let mut files = Vec::new();
                collect_paths(child, &mut files);
                self.events.push(ListEvent::FolderMenu {
                    folder: child.path.clone(),
                    files,
                });
            }
            if row.open {
                self.draw(cx, child, depth + 1, prefix);
            }
        }
    }
}

/// Carries out what the user did in a list of `kind`: opens diffs, shows the menus, and returns the rows whose
/// Stage / Unstage button was pressed.
pub fn apply_events(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    kind: &ListKind,
    events: Vec<ListEvent>,
) -> Vec<FileChange> {
    let mut actions = Vec::new();
    for event in events {
        match event {
            ListEvent::Open(change) => open_file(app, state, kind, change),
            ListEvent::Action(change) => actions.push(change),
            ListEvent::FileMenu(change) => {
                let items = match kind {
                    ListKind::Commit(sha) | ListKind::Compare(sha) => {
                        menus::commit_file_menu(cx, app, state, sha, &change)
                    }
                    ListKind::Unstaged | ListKind::Staged => {
                        let staged = *kind == ListKind::Staged;
                        menus::workdir_file_menu(cx, app, state, &change, staged)
                    }
                };
                popup_menu(cx, items, None);
            }
            ListEvent::FolderMenu { folder, files } => {
                let staged = *kind == ListKind::Staged;
                let items =
                    menus::folder_menu(cx, app, state, &folder, files, staged, !kind.commit_side());
                popup_menu(cx, items, None);
            }
        }
    }
    actions
}

/// A click on a file row: a conflicted file opens in the merge tool (its plain diff when its sides cannot be
/// read), a file a commit did not change (View all files) in the file history as the commit left it.
fn open_file(app: &mut Application, state: &mut RepoState, kind: &ListKind, change: FileChange) {
    if let ListKind::Commit(sha) = kind
        && change.status == FileStatus::Unknown
    {
        state.open_file_history(change.path, Some(sha.clone()), Some(sha.clone()), false);
        return;
    }
    let source = kind.diff_source();
    // Unsaved text in the file editor is asked about first.
    navigate_from_editor(app, state, move |_app, state| {
        if change.status != FileStatus::Conflicted || state.open_merge_tool(&change.path).is_err() {
            let _ = state.open_diff(DiffTarget {
                source,
                path: change.path,
                status: change.status,
            });
        }
    });
}

/// A file list's section header ("Unstaged Files (2)"): the 14px caret 15px in, the 14px label 5px after it and
/// the xs button 15px from the right; `top_margin` is 4px for every section after the first.
pub struct SectionHeader<'a> {
    pub label: &'a str,
    pub count: usize,
    pub pos: Vec2,
    pub width: f32,
    pub top_margin: f32,
    pub button_label: Option<&'a str>,
    pub danger: bool,
}

impl SectionHeader<'_> {
    /// Returns whether the section is open and whether its button was clicked.
    pub fn draw(self, cx: &mut Cx<'_>, collapsed_sections: &mut HashSet<String>) -> (bool, bool) {
        let ui = cx.ui;
        let p = cx.palette;
        let key = format!("files:{}", self.label);
        let collapsed = collapsed_sections.contains(&key);
        let _id = ui.push_id(self.label);
        let draw = cx.window_draw_list();
        let (pos, width) = (self.pos, self.width);
        let y = pos.y + self.top_margin;
        let mut button_clicked = false;
        if let (Some(button_label), true) = (self.button_label, self.count > 0) {
            let w = tone_button_width(cx, button_label);
            button_clicked = tone_button(
                cx,
                "##all",
                button_label,
                vec2(pos.x + width - cx.px(15.0) - w, y + cx.px(12.0)),
                cx.px(22.0),
                if self.danger { p.red } else { p.green },
                true,
            );
        }
        let text = format!("{} ({})", self.label, self.count);
        let text_w = cx.text_size(cx.fonts.regular, 14.0, &text).x;
        cx.set_cursor(vec2(pos.x + cx.px(15.0), y + cx.px(10.0)));
        if ui.invisible_button("##section", vec2(cx.px(13.75) + text_w, cx.px(26.0)))
            && !collapsed_sections.remove(&key)
        {
            collapsed_sections.insert(key);
        }
        cx.draw_fa_icon(
            draw,
            if collapsed {
                "fas:caret-right"
            } else {
                "fas:caret-down"
            },
            vec2(pos.x + cx.px(19.4), y + cx.px(22.8)),
            cx.px(14.0),
            p.text_normal,
        );
        super::controls::css_text(
            cx,
            draw,
            14.0,
            vec2(pos.x + cx.px(28.75), y + cx.px(13.0)),
            p.text_normal,
            &text,
        );
        (!collapsed, button_clicked)
    }
}

/// The commit's changes plus the files of its tree it left as they were (status Unknown), sorted by path.
pub fn with_unchanged_files(changed: &[FileChange], tree_paths: Vec<String>) -> Vec<FileChange> {
    let known: HashSet<&str> = changed.iter().map(|change| change.path.as_str()).collect();
    let mut files = changed.to_vec();
    files.extend(
        tree_paths
            .into_iter()
            .filter(|path| !known.contains(path.as_str()))
            .map(|path| FileChange {
                path,
                ..FileChange::default()
            }),
    );
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use imgit_git::LineStats;

    fn change(path: &str, status: FileStatus, additions: usize, deletions: usize) -> FileChange {
        FileChange {
            path: path.to_owned(),
            status,
            stats: Some(LineStats {
                additions,
                deletions,
            }),
            ..FileChange::default()
        }
    }

    fn sample() -> Vec<FileChange> {
        vec![
            change("src/b.rs", FileStatus::Modified, 3, 1),
            change("src/ui/a.rs", FileStatus::Added, 10, 0),
            change("README.md", FileStatus::Deleted, 0, 7),
            change("src/Z.rs", FileStatus::Renamed, 0, 0),
        ]
    }

    fn names(children: &[&FileNode<'_>]) -> Vec<String> {
        children.iter().map(|node| node.name.clone()).collect()
    }

    #[test]
    fn tree_groups_folders_and_counts() {
        let changes = sample();
        let root = build_tree(&changes, true);
        assert_eq!((root.modified, root.added, root.deleted), (2, 1, 1));
        assert_eq!((root.additions, root.deletions), (13, 8));
        let src = &root.children["src"];
        assert_eq!((src.modified, src.added, src.deleted), (2, 1, 0));
        assert_eq!(src.path, "src");
        let ui = &src.children["ui"];
        assert_eq!(ui.path, "src/ui");
        assert!(ui.children["a.rs\u{1}"].file.is_some());
    }

    #[test]
    fn flat_list_keeps_full_paths() {
        let changes = sample();
        let root = build_tree(&changes, false);
        assert_eq!(root.children.len(), 4);
        assert!(root.children.values().all(|node| node.file.is_some()));
        assert!(root.children.contains_key("src/ui/a.rs\u{1}"));
    }

    #[test]
    fn sorts_by_path_name_and_status() {
        let changes = sample();
        let root = build_tree(&changes, true);
        assert_eq!(
            names(&sorted_children(&root, FileSort::Path)),
            ["README.md", "src"]
        );
        assert_eq!(
            names(&sorted_children(&root, FileSort::PathDescending)),
            ["src", "README.md"]
        );
        // Folders first, then the files.
        assert_eq!(
            names(&sorted_children(&root, FileSort::Name)),
            ["src", "README.md"]
        );
        let src = &root.children["src"];
        // Folder `ui` first, then b.rs and Z.rs ignoring case.
        assert_eq!(
            names(&sorted_children(src, FileSort::Name)),
            ["ui", "b.rs", "Z.rs"]
        );
        // By status: modified before renamed.
        assert_eq!(
            names(&sorted_children(src, FileSort::Status)),
            ["ui", "b.rs", "Z.rs"]
        );
    }

    #[test]
    fn status_order() {
        let ranks: Vec<u8> = [
            FileStatus::Conflicted,
            FileStatus::Untracked,
            FileStatus::TypeChange,
            FileStatus::Renamed,
            FileStatus::Deleted,
            FileStatus::Unknown,
        ]
        .into_iter()
        .map(status_rank)
        .collect();
        assert_eq!(ranks, [0, 1, 2, 3, 4, 5]);
        assert_eq!(
            status_rank(FileStatus::Added),
            status_rank(FileStatus::Untracked)
        );
    }

    #[test]
    fn case_insensitive_names() {
        assert_eq!(compare_ignoring_case("abc", "ABD"), Ordering::Less);
        assert_eq!(compare_ignoring_case("B", "a"), Ordering::Greater);
        assert_eq!(compare_ignoring_case("Same", "sAME"), Ordering::Equal);
    }

    #[test]
    fn collects_paths_under_a_folder() {
        let changes = sample();
        let root = build_tree(&changes, true);
        let mut paths = Vec::new();
        collect_paths(&root.children["src"], &mut paths);
        paths.sort();
        assert_eq!(paths, ["src/Z.rs", "src/b.rs", "src/ui/a.rs"]);
    }

    #[test]
    fn unchanged_files_join_the_changes() {
        let changed = vec![change("b.txt", FileStatus::Modified, 1, 0)];
        let files = with_unchanged_files(
            &changed,
            vec!["c.txt".into(), "b.txt".into(), "a.txt".into()],
        );
        let listed: Vec<(&str, FileStatus)> = files
            .iter()
            .map(|file| (file.path.as_str(), file.status))
            .collect();
        assert_eq!(
            listed,
            [
                ("a.txt", FileStatus::Unknown),
                ("b.txt", FileStatus::Modified),
                ("c.txt", FileStatus::Unknown),
            ]
        );
    }

    #[test]
    fn list_kinds() {
        assert_eq!(ListKind::Staged.diff_source(), DiffSource::Staged);
        assert_eq!(
            ListKind::Compare("abc".into()).diff_source(),
            DiffSource::Compare("abc".into())
        );
        assert!(ListKind::Commit("x".into()).commit_side());
        assert!(!ListKind::Unstaged.commit_side());
        assert!(same_source_kind(
            &DiffSource::Commit("a".into()),
            &DiffSource::Commit("b".into())
        ));
        assert!(!same_source_kind(
            &DiffSource::Staged,
            &DiffSource::Unstaged
        ));
    }
}
