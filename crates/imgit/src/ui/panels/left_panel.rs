//! The left panel: local branches, remotes, tags, stashes, submodules, worktrees.
//!
//! From the top: the collapse button and "Viewing N", the filter box, then the sections (LOCAL, REMOTE,
//! WORKTREES, STASHES, TAGS, SUBMODULES and, once Gitflow is initialized, GITFLOW). Each has a 30px header and,
//! when expanded, a scrolling body of 24px rows (folders of a `feature/x` name as a tree). The border between two
//! sections can be dragged. Collapsed, the panel is a 36px strip of section icons and counts.
//!
//! Ports `src/ui/left_panel.cpp`.

use std::cmp::Ordering;

use dear_imgui_rs::{
    InputTextFlags, Key, KeyChord, KeyMods, MouseButton, MouseCursor, StyleColor, StyleVar,
    WindowFlags, WindowHoveredFlags, sys,
};
use imgit_app::RepoState;
use imgit_app::text::contains_ci;
use imgit_git::repository::stash_summary;
use imgit_git::{Ref, RefType};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::theme::{Color, FONT_SIZE_ROW};
use crate::ui::geom::{Rect, vec2};
use crate::ui::menu::popup_menu;
use crate::ui::panels::menus::{
    checkout_ref, jump_to_commit, left_section_menu, local_folder_menu, ref_menu, remote_menu,
    worktree_menu,
};
use crate::ui::panels::workflows::{gitflow_branches, gitflow_initialized, gitflow_start_items};
use crate::ui::widgets::format::to_upper;
use crate::ui::{Cx, DrawList, Icon, Vec2, icons};

/// The left panel keeps everything it remembers in the repository tab's `RepoState` (the filter, the collapsed
/// folders, the expanded sections and their dragged ends, the selected row).
#[derive(Default)]
pub struct State {}

// The left panel constants: rows, headers, section sizes (logical px).
const REF_HEIGHT: f32 = 24.0;
const HEADER_HEIGHT: f32 = 30.0;
const SECTION_DEFAULT_HEIGHT: f32 = 132.0;
const SECTION_MINIMUM_HEIGHT: f32 = 108.0;
/// One `.width-15` spacer per level.
const DEPTH_INDENT: f32 = 15.0;

/// The sections, in order, as the section menu lists them.
const SECTIONS: [&str; 6] = [
    "Local",
    "Remote",
    "Worktrees",
    "Stashes",
    "Tags",
    "Submodules",
];

// ---------------------------------------------------------------------------------------------------------
// The trees

/// A ref, folder or remote in a section's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TreeNode {
    /// The path segment shown.
    name: String,
    /// The section-qualified folder path, for the collapsed set.
    key: String,
    /// Leaves only: the index into `history.refs`.
    ref_index: Option<usize>,
    children: Vec<TreeNode>,
}

impl TreeNode {
    fn root() -> Self {
        Self {
            name: String::new(),
            key: String::new(),
            ref_index: None,
            children: Vec::new(),
        }
    }

    fn folder(name: &str, key: String) -> Self {
        Self {
            name: name.to_owned(),
            key,
            ref_index: None,
            children: Vec::new(),
        }
    }

    /// Puts the ref `name` (its index `ref_index` into `history.refs`) into the tree, a folder per `/`
    /// segment; `prefix` qualifies the keys ("local:", "remote:", "tag:").
    fn insert(&mut self, name: &str, ref_index: usize, prefix: &str) {
        let mut node = self;
        let mut start = 0;
        while let Some(offset) = name[start..].find('/') {
            let slash = start + offset;
            let segment = &name[start..slash];
            let position = node
                .children
                .iter()
                .position(|child| child.ref_index.is_none() && child.name == segment);
            let position = position.unwrap_or_else(|| {
                node.children
                    .push(Self::folder(segment, format!("{prefix}{}", &name[..slash])));
                node.children.len() - 1
            });
            node = &mut node.children[position];
            start = slash + 1;
        }
        node.children.push(Self {
            name: name[start..].to_owned(),
            key: format!("{prefix}{name}"),
            ref_index: Some(ref_index),
            children: Vec::new(),
        });
    }

    fn sort(&mut self, tags: bool) {
        self.children.sort_by(|a, b| compare_nodes(a, b, tags));
        for child in &mut self.children {
            child.sort(tags);
        }
    }

    /// The indices of the refs at and below this node.
    fn collect_refs(&self, out: &mut Vec<usize>) {
        out.extend(self.ref_index);
        for child in &self.children {
            child.collect_refs(out);
        }
    }
}

/// Tag names compare as semver ranges (`minVersion` of `semver.validRange`): "v1.0" is ">=1.0.0 <1.1.0-0".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Version {
    /// A full x.y.z.
    exact: bool,
    part: [u32; 3],
    pre: String,
}

/// `None` for a name that is not a version.
fn parse_version(name: &str) -> Option<Version> {
    let bytes = name.as_bytes();
    let mut version = Version::default();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b'v' | b'=' | b' ') {
        i += 1;
    }
    let mut parts = 0;
    while parts < 3 && i < bytes.len() {
        match bytes[i] {
            b'x' | b'X' | b'*' => i += 1,
            b'0'..=b'9' => {
                let mut value = 0u32;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    value = value
                        .saturating_mul(10)
                        .saturating_add(u32::from(bytes[i] - b'0'));
                    i += 1;
                }
                version.part[parts] = value;
            }
            _ => return None,
        }
        parts += 1;
        if i < bytes.len() && bytes[i] == b'.' && parts < 3 {
            i += 1;
            continue;
        }
        break;
    }
    if parts == 0 {
        return None;
    }
    if i < bytes.len() && bytes[i] == b'-' {
        let end = name[i..].find('+').map_or(name.len(), |offset| i + offset);
        version.pre = name[i + 1..end].to_owned();
        i = end;
    }
    if i < bytes.len() && bytes[i] == b'+' {
        i = bytes.len();
    }
    if i != bytes.len() {
        return None;
    }
    version.exact = parts == 3;
    Some(version)
}

/// `defaultCompareFn`: case-insensitive by the name shown at this depth (folders and refs mixed);
/// `compareSemverOrAlpha` for tags: plain names first (alphabetically), then versions newest first.
fn node_less(a: &str, b: &str, tags: bool) -> bool {
    if tags {
        match (parse_version(a), parse_version(b)) {
            (None, Some(_)) => return true,
            (Some(_), None) => return false,
            (Some(va), Some(vb)) => {
                for (pa, pb) in va.part.iter().zip(&vb.part) {
                    if pa != pb {
                        return pa > pb;
                    }
                }
                if va.pre != vb.pre {
                    return va.pre.is_empty() || (!vb.pre.is_empty() && va.pre > vb.pre);
                }
                return va.exact && !vb.exact;
            }
            (None, None) => {}
        }
    }
    a.to_ascii_lowercase() < b.to_ascii_lowercase()
}

fn compare_nodes(a: &TreeNode, b: &TreeNode, tags: bool) -> Ordering {
    if node_less(&a.name, &b.name, tags) {
        Ordering::Less
    } else if node_less(&b.name, &a.name, tags) {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

fn ref_icon(ref_type: RefType) -> &'static str {
    match ref_type {
        RefType::Tag => "far:tag",
        RefType::Stash => "far:inbox",
        RefType::LocalBranch | RefType::RemoteBranch => "far:code-branch",
    }
}

// ---------------------------------------------------------------------------------------------------------
// The sections

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    Local,
    Remote,
    Worktrees,
    Stashes,
    Tags,
    Submodules,
    Gitflow,
}

impl SectionKind {
    fn name(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Remote => "Remote",
            Self::Worktrees => "Worktrees",
            Self::Stashes => "Stashes",
            Self::Tags => "Tags",
            Self::Submodules => "Submodules",
            Self::Gitflow => "Gitflow",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Local => "far:laptop",
            Self::Remote => "far:cloud",
            Self::Worktrees => "far:sitemap",
            Self::Stashes => "far:inbox",
            Self::Tags => "far:tag",
            Self::Submodules => "far:layer-group",
            Self::Gitflow => "fas:exchange",
        }
    }

    /// The tooltip of the "+" of the sections that have one: it adds a remote / a submodule, starts a gitflow
    /// branch.
    fn add_tip(self) -> Option<&'static str> {
        match self {
            Self::Remote => Some("Add Remote"),
            Self::Submodules => Some("Add Submodule"),
            Self::Gitflow => Some("Start Feature / Release / Hotfix"),
            _ => None,
        }
    }
}

/// The sizes the section heights are made of, in device px.
#[derive(Debug, Clone, Copy)]
struct SectionMetrics {
    header: f32,
    minimum: f32,
    default: f32,
}

/// `getLeftPanelHandleBounds` + `getBoundedSectionYsByKey`: each section ends at a Y; collapsed ones are exactly
/// the header, expanded ones at least header + `SECTION_MINIMUM_HEIGHT`; a section's preferred Y is its stored
/// one (device px) or the previous preferred Y + `SECTION_DEFAULT_HEIGHT` + `HEADER_HEIGHT`, then bounded so the
/// sections fill the panel. Returns the bounded Ys.
fn section_ys(
    expanded: &[bool],
    stored: &[Option<f32>],
    panel: f32,
    metrics: SectionMetrics,
) -> Vec<f32> {
    let n = expanded.len();
    if n == 0 {
        return Vec::new();
    }
    let header = metrics.header;
    let mins: Vec<f32> = expanded
        .iter()
        .map(|&e| if e { header + metrics.minimum } else { header })
        .collect();
    let maxs: Vec<f32> = expanded
        .iter()
        .map(|&e| if e { f32::INFINITY } else { header })
        .collect();
    // Bounds (ba).
    let mut lo = vec![0.0; n];
    let mut hi = vec![0.0; n];
    let mut sum_min = 0.0;
    let mut sum_max = 0.0;
    let mut unbounded = false;
    for i in 0..n {
        unbounded = unbounded || maxs[i].is_infinite();
        sum_min += mins[i];
        lo[i] = sum_min;
        hi[i] = f32::max(sum_min, panel);
        if !unbounded {
            sum_max += maxs[i];
            hi[i] = sum_max;
        }
    }
    if !unbounded && sum_max < panel {
        lo.clone_from(&hi);
    } else if sum_min >= panel {
        hi.clone_from(&lo);
    } else {
        let mut sum_min = 0.0;
        let mut sum_max = 0.0_f32;
        for i in 0..n {
            sum_min += mins[i];
            sum_max = (sum_max + maxs[i]).min(panel);
            lo[i] = sum_min;
            hi[i] = sum_max;
        }
        let mut from_end = Some(panel);
        let mut room = panel;
        lo[n - 1] = panel;
        hi[n - 1] = panel;
        for k in (0..n - 1).rev() {
            from_end = match from_end {
                Some(from_end) if maxs[k + 1].is_finite() => Some(from_end - maxs[k + 1]),
                _ => None,
            };
            room -= mins[k + 1];
            if let Some(from_end) = from_end {
                lo[k] = lo[k].max(from_end);
            }
            hi[k] = hi[k].min(room);
        }
    }
    let mut ys = vec![0.0; n];
    let mut previous = 0.0;
    for i in 0..n {
        let preferred = stored[i].unwrap_or(previous + metrics.default + header);
        previous = preferred;
        ys[i] = preferred.clamp(lo[i], hi[i].max(lo[i]));
    }
    for i in 1..n {
        let low = ys[i - 1] + mins[i];
        let high = if maxs[i].is_finite() {
            ys[i - 1] + maxs[i]
        } else {
            ys[i]
        };
        ys[i] = ys[i].clamp(low, high.max(low));
    }
    ys
}

// ---------------------------------------------------------------------------------------------------------
// Rows

/// What one frame's walk over the trees collects for after the panel is drawn.
#[derive(Default)]
struct Walk {
    row_width: f32,
    refresh_needed: bool,
    /// A double-clicked ref, checked out once the tree walk is over: the checkout refreshes the state, which
    /// frees the refs the walk reads.
    checkout_request: Option<Ref>,
}

/// Hides or shows `refs` (indices into `history.refs`) in the graph; the checked-out one stays.
fn set_hidden(state: &mut RepoState, walk: &mut Walk, refs: &[usize], hidden: bool) {
    state.solo_ref = None;
    for &index in refs {
        let r = &state.history.refs[index];
        if r.is_head {
            continue;
        }
        if hidden {
            state.hidden_refs.insert(r.full_name.clone());
        } else {
            state.hidden_refs.remove(&r.full_name);
        }
    }
    walk.refresh_needed = true;
}

/// Whether every ref of `refs` (the checked-out one aside) is hidden, and there is one.
fn all_hidden(state: &RepoState, refs: &[usize]) -> bool {
    let mut any = false;
    for &index in refs {
        let r = &state.history.refs[index];
        if r.is_head {
            continue;
        }
        if !r.hidden {
            return false;
        }
        any = true;
    }
    any
}

/// The text of a row: the name, or a stash's "message on: branch" with the branch in text-disabled.
struct RowLabel<'a> {
    text: &'a str,
    stash_branch: Option<&'a str>,
}

impl<'a> RowLabel<'a> {
    fn plain(text: &'a str) -> Self {
        Self {
            text,
            stash_branch: None,
        }
    }
}

/// One inline run of a row's text with its left margin.
struct TextRun<'a> {
    text: &'a str,
    color: Color,
    margin: f32,
}

/// Where the runs go: the kept text of each (the run's index, the text, x from the start) and, when something
/// had to be cut, x of the ellipsis.
#[derive(Debug, PartialEq)]
struct RunLayout<'a> {
    pieces: Vec<(usize, &'a str, f32)>,
    ellipsis: Option<f32>,
}

/// CSS `text-overflow: ellipsis` over inline runs (each with its left margin): glyphs are kept while they and
/// the ellipsis fit in `room`, and the ellipsis follows the last kept glyph. `measure` is the width of a text.
fn layout_runs<'a>(
    runs: &[TextRun<'a>],
    room: f32,
    ellipsis_width: f32,
    measure: &dyn Fn(&str) -> f32,
) -> RunLayout<'a> {
    let total: f32 = runs.iter().map(|run| run.margin + measure(run.text)).sum();
    let mut pieces = Vec::new();
    if total <= room {
        let mut x = 0.0;
        for (index, run) in runs.iter().enumerate() {
            x += run.margin;
            pieces.push((index, run.text, x));
            x += measure(run.text);
        }
        return RunLayout {
            pieces,
            ellipsis: None,
        };
    }
    let budget = room - ellipsis_width;
    let mut x = 0.0;
    let mut last_glyph_end = 0.0;
    for (index, run) in runs.iter().enumerate() {
        x += run.margin;
        let mut keep = 0;
        for (at, character) in run.text.char_indices() {
            let next = at + character.len_utf8();
            if x + measure(&run.text[..next]) > budget {
                break;
            }
            keep = next;
        }
        let kept = &run.text[..keep];
        if !kept.is_empty() {
            pieces.push((index, kept, x));
            last_glyph_end = x + measure(kept);
        }
        if keep < run.text.len() {
            return RunLayout {
                pieces,
                ellipsis: Some(last_glyph_end),
            };
        }
        x = last_glyph_end;
    }
    RunLayout {
        pieces,
        ellipsis: None,
    }
}

fn draw_runs_ellipsis(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    pos: Vec2,
    room: f32,
    ellipsis_color: Color,
    runs: &[TextRun<'_>],
) {
    let font = cx.fonts.regular;
    let measure = |text: &str| cx.text_size(font, FONT_SIZE_ROW, text).x;
    let layout = layout_runs(runs, room, measure("\u{2026}"), &measure);
    for (index, text, x) in layout.pieces {
        cx.draw_text(
            draw,
            font,
            FONT_SIZE_ROW,
            vec2(pos.x + x, pos.y),
            runs[index].color,
            text,
        );
    }
    if let Some(x) = layout.ellipsis {
        cx.draw_text(
            draw,
            font,
            FONT_SIZE_ROW,
            vec2(pos.x + x, pos.y),
            ellipsis_color,
            "\u{2026}",
        );
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct RowResult {
    clicked: bool,
    double_clicked: bool,
    /// Right-click or the ellipsis button.
    context: bool,
    eye_clicked: bool,
}

struct RowOptions<'a> {
    depth: usize,
    icon: Option<&'static str>,
    checked_out: bool,
    /// A submodule at the commit its repository records: a green check.
    synced: bool,
    hidden: bool,
    selected: bool,
    can_hide: bool,
    ahead: usize,
    behind: usize,
    tooltip: &'a str,
}

impl Default for RowOptions<'_> {
    fn default() -> Self {
        Self {
            depth: 0,
            icon: None,
            checked_out: false,
            synced: false,
            hidden: false,
            selected: false,
            can_hide: true,
            ahead: 0,
            behind: 0,
            tooltip: "",
        }
    }
}

/// The upstream status before the ellipsis slot: the arrow and the count (10px), growing leftwards from
/// `right`.
fn draw_upstream(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    right: &mut f32,
    mid: f32,
    count: usize,
    arrow: &str,
    color: Color,
) {
    if count == 0 {
        return;
    }
    let icon_w = cx.fa_icon_width(arrow, cx.px(10.0));
    *right -= icon_w;
    cx.draw_fa_icon(
        draw,
        arrow,
        vec2(*right + icon_w / 2.0, mid),
        cx.px(10.0),
        color,
    );
    let number = count.to_string();
    let regular = cx.fonts.regular;
    let size = cx.text_size(regular, 10.0, &number);
    *right -= cx.text_size(regular, 10.0, " ").x + size.x;
    cx.draw_text(
        draw,
        regular,
        10.0,
        vec2(*right, mid - size.y / 2.0),
        color,
        &number,
    );
    *right -= cx.px(5.0); // .ml1
}

/// A .hover-row (24px, 5px left padding): the 15px eye / checked-out slot, 15px per depth level, the 15px icon
/// slot in text-disabled, the label 5px on, then the upstream status and the 13.5px ellipsis button (hover only,
/// though it keeps its space). Hidden refs show the eye-slash and draw at half opacity.
fn panel_row(
    cx: &mut Cx<'_>,
    width: f32,
    id: &str,
    label: &RowLabel<'_>,
    options: &RowOptions<'_>,
) -> RowResult {
    let ui = cx.ui;
    let p = cx.palette;
    let h = cx.px(REF_HEIGHT);
    let w = width;
    let pos = cx.cursor();
    let draw = cx.window_draw_list();
    let mut result = RowResult::default();

    let mouse = cx.mouse_pos();
    let hovered = ui
        .is_window_hovered_with_flags(WindowHoveredFlags::ALLOW_WHEN_BLOCKED_BY_ACTIVE_ITEM)
        && Rect::from_pos_size(pos, vec2(w, h)).contains(mouse);
    // Slots submitted before the row so they win the hover test; the test still passes while a slot is held
    // down (blocked by its own active item), so its click lands on release.
    let eye_min = vec2(pos.x + cx.px(5.0), pos.y);
    let menu_x = pos.x + w + 1.0 - cx.px(13.5);
    {
        let _id = ui.push_id(id);
        if hovered && options.can_hide && !options.checked_out {
            cx.set_cursor(eye_min);
            if ui.invisible_button("##eye", [cx.px(15.0), h]) {
                result.eye_clicked = true;
            }
            if ui.is_item_hovered() {
                let rect = cx.item_rect();
                cx.styled_tooltip(
                    rect.min,
                    rect.max,
                    if options.hidden {
                        "Show in graph"
                    } else {
                        "Hide in graph"
                    },
                    false,
                );
            }
        }
        if hovered {
            cx.set_cursor(vec2(menu_x, pos.y));
            if ui.invisible_button("##menu", [cx.px(13.5), h]) {
                result.context = true;
            }
        }
        cx.set_cursor(pos);
        ui.invisible_button("##row", [w, h]);
        result.clicked = ui.is_item_clicked_with_button(MouseButton::Left);
        result.double_clicked =
            ui.is_item_hovered() && ui.is_mouse_double_clicked(MouseButton::Left);
        result.context =
            result.context || (ui.is_item_hovered() && ui.is_mouse_released(MouseButton::Right));
    }

    let row_rect = (pos, vec2(pos.x + w, pos.y + h));
    // .hover-row:hover:not(.selected-row) wins over .checked-out.
    if options.selected {
        draw.add_rect_filled(row_rect.0, row_rect.1, p.selected_row, 0.0);
    } else if hovered {
        draw.add_rect_filled(row_rect.0, row_rect.1, p.hover_row, 0.0);
    } else if options.checked_out {
        draw.add_rect_filled(row_rect.0, row_rect.1, p.checked_out, 0.0);
    }
    let alpha = if options.hidden { 0.5 } else { 1.0 };
    let color = |c: Color| c.with_alpha(c.a * alpha);
    let mid = pos.y + h / 2.0;
    let slot = eye_min.x + cx.px(7.5);
    if options.checked_out {
        cx.draw_fa_icon(
            draw,
            "fas:check-square",
            vec2(slot, mid),
            cx.px(14.0),
            color(p.green),
        );
    } else if options.synced {
        cx.draw_fa_icon(
            draw,
            "fas:check",
            vec2(slot, mid),
            cx.px(11.0),
            color(p.green),
        );
    } else if options.hidden {
        cx.draw_fa_icon(
            draw,
            "far:eye-slash",
            vec2(slot, mid),
            cx.px(12.0),
            color(p.text_normal),
        );
    } else if hovered && options.can_hide {
        cx.draw_fa_icon(
            draw,
            "far:eye",
            vec2(slot, mid),
            cx.px(12.0),
            color(p.green),
        );
    }
    let depth_x = cx.px(DEPTH_INDENT) * options.depth as f32;
    if let Some(icon) = options.icon {
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(pos.x + cx.px(32.5) + depth_x, mid),
            cx.px(12.0),
            color(p.text_disabled),
        );
    }

    // Right side: the upstream status (10px) before the ellipsis slot.
    let mut right = menu_x;
    let text = if options.checked_out {
        p.text_selected
    } else {
        p.text_normal
    };
    draw_upstream(
        cx,
        draw,
        &mut right,
        mid,
        options.behind,
        "far:long-arrow-down",
        color(text),
    );
    draw_upstream(
        cx,
        draw,
        &mut right,
        mid,
        options.ahead,
        "far:long-arrow-up",
        color(text),
    );
    if hovered {
        cx.draw_fa_icon(
            draw,
            "far:ellipsis-v",
            vec2(menu_x + cx.px(6.75), mid),
            cx.px(14.0),
            p.text_secondary,
        );
    }

    let regular = cx.fonts.regular;
    let label_x = pos.x + cx.px(45.0) + depth_x;
    let label_room = right - cx.px(5.0) - label_x;
    let line_h = cx.text_size(regular, FONT_SIZE_ROW, "A").y;
    let ty = mid - line_h / 2.0;
    match label.stash_branch {
        None => {
            cx.draw_text_ellipsis(
                draw,
                regular,
                FONT_SIZE_ROW,
                vec2(label_x, ty),
                label_room,
                color(text),
                label.text,
            );
        }
        Some(branch) => draw_runs_ellipsis(
            cx,
            draw,
            vec2(label_x, ty),
            label_room,
            color(text),
            &[
                TextRun {
                    text: label.text,
                    color: color(text),
                    margin: 0.0,
                },
                TextRun {
                    text: "on:",
                    color: color(text),
                    margin: cx.px(5.0),
                },
                TextRun {
                    text: branch,
                    color: color(p.text_disabled),
                    margin: cx.px(5.0),
                },
            ],
        ),
    }
    if hovered
        && !options.tooltip.is_empty()
        && mouse.x >= label_x
        && mouse.x < label_x + label_room
    {
        cx.styled_tooltip(
            vec2(label_x, pos.y),
            vec2(label_x + label_room, pos.y + h),
            options.tooltip,
            false,
        );
    }
    cx.set_cursor(vec2(pos.x, pos.y + h));
    result
}

/// The rows of one tree level: folders (collapsible, hide all their refs) and refs.
fn draw_tree(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    walk: &mut Walk,
    node: &TreeNode,
    depth: usize,
    remote_level: bool,
) {
    for child in &node.children {
        if let Some(index) = child.ref_index {
            let row = {
                let r = &state.history.refs[index];
                let is_local = r.ref_type == RefType::LocalBranch;
                let options = RowOptions {
                    depth,
                    icon: Some(ref_icon(r.ref_type)),
                    checked_out: r.is_head,
                    hidden: r.hidden,
                    selected: state.left_selected.as_deref() == Some(r.full_name.as_str())
                        && state.is_selected(&r.target)
                        && state.selected.len() == 1,
                    ahead: if is_local { r.ahead } else { 0 },
                    behind: if is_local { r.behind } else { 0 },
                    tooltip: &r.name,
                    ..RowOptions::default()
                };
                panel_row(
                    cx,
                    walk.row_width,
                    &r.full_name,
                    &RowLabel::plain(&child.name),
                    &options,
                )
            };
            if row.eye_clicked {
                let hidden = state.history.refs[index].hidden;
                set_hidden(state, walk, &[index], !hidden);
            } else if row.double_clicked {
                walk.checkout_request = Some(state.history.refs[index].clone());
            } else if row.clicked {
                let (full_name, target) = {
                    let r = &state.history.refs[index];
                    (r.full_name.clone(), r.target.clone())
                };
                state.left_selected = Some(full_name);
                jump_to_commit(state, &target);
            }
            if row.context {
                let r = state.history.refs[index].clone();
                let items = ref_menu(cx, app, state, &r, true);
                popup_menu(cx, items, None);
            }
            continue;
        }
        let mut refs = Vec::new();
        child.collect_refs(&mut refs);
        let collapsed = state.collapsed_nodes.contains(&child.key);
        let hidden = all_hidden(state, &refs);
        let options = RowOptions {
            depth,
            icon: Some(if remote_level {
                "far:globe"
            } else {
                "far:folder"
            }),
            hidden,
            ..RowOptions::default()
        };
        let row = panel_row(
            cx,
            walk.row_width,
            &child.key,
            &RowLabel::plain(&child.name),
            &options,
        );
        if row.eye_clicked {
            set_hidden(state, walk, &refs, !hidden);
        } else if row.clicked && !state.collapsed_nodes.remove(&child.key) {
            state.collapsed_nodes.insert(child.key.clone());
        }
        if row.context {
            if remote_level {
                let items = remote_menu(cx, app, state, &child.name, true);
                popup_menu(cx, items, None);
            } else if let Some(folder) = child.key.strip_prefix("local:") {
                let items = local_folder_menu(cx, app, state, folder);
                popup_menu(cx, items, None);
            }
        }
        if !collapsed {
            draw_tree(cx, app, state, walk, child, depth + 1, false);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Section headers

struct Header {
    kind: SectionKind,
    count: usize,
    expanded: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct HeaderResult {
    /// The header itself was clicked.
    clicked: bool,
    /// The "+" was clicked.
    add: bool,
}

/// The 30px sticky header: chevron (fa-fw), the section icon in text-disabled, the caps label and the count
/// (both 12px semibold); text-secondary turning white on hover. For the sections with an "add" tip, hovering
/// swaps the count for a 24px green "+" button 10px from the edge.
fn section_header(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    pos: Vec2,
    width: f32,
    header: &Header,
) -> HeaderResult {
    let ui = cx.ui;
    let p = cx.palette;
    let h = cx.px(HEADER_HEIGHT);
    let draw = cx.window_draw_list();
    let label = header.kind.name();
    let add_tip = header.kind.add_tip();
    let mut result = HeaderResult::default();
    let add_min = vec2(
        pos.x + width - cx.px(10.0) - cx.px(24.0),
        pos.y + cx.px(3.2),
    );
    let add_max = add_min + vec2(cx.px(24.0), cx.px(24.0));
    let mut add_hovered = false;
    let hovered;
    {
        cx.set_cursor(pos);
        let _id = ui.push_id(label);
        ui.set_next_item_allow_overlap();
        result.clicked = ui.invisible_button("##section", [width, h]);
        // While the "+" is held it is the active item; the header must stay hovered for it to finish the click.
        hovered = ui
            .is_window_hovered_with_flags(WindowHoveredFlags::ALLOW_WHEN_BLOCKED_BY_ACTIVE_ITEM)
            && ui.is_mouse_hovering_rect(pos.into(), (pos + vec2(width, h)).into());
        if let Some(tip) = add_tip
            && hovered
        {
            cx.set_cursor(add_min);
            result.add = ui.invisible_button("##add", (add_max - add_min).to_array());
            add_hovered = ui.is_item_hovered();
            if add_hovered {
                cx.tooltip(tip);
            }
        }
        if hovered && ui.is_mouse_released(MouseButton::Right) {
            let sections: Vec<String> = SECTIONS.iter().map(|&s| s.to_owned()).collect();
            let items = left_section_menu(cx, app, state, label, &sections);
            popup_menu(cx, items, None);
        }
    }
    draw.add_rect_filled(pos, pos + vec2(width, h), p.panel_bg0, 0.0);
    draw.add_rect_filled(pos, pos + vec2(width, 1.0), p.section_border, 0.0);
    let color = if hovered {
        p.text_selected
    } else {
        p.text_secondary
    };
    let mid = pos.y + 1.0 + (h - 1.0) / 2.0;
    cx.draw_fa_icon(
        draw,
        if header.expanded {
            "far:chevron-down"
        } else {
            "far:chevron-right"
        },
        vec2(pos.x + cx.px(12.5), mid),
        cx.px(12.0),
        color,
    );
    cx.draw_fa_icon(
        draw,
        header.kind.icon(),
        vec2(pos.x + cx.px(32.5), mid),
        cx.px(12.0),
        p.text_disabled,
    );
    let semibold = cx.fonts.semibold;
    let ascent = cx.font_px(semibold, FONT_SIZE_ROW) * 2189.0 / 2789.0;
    let baseline = pos.y + cx.px(19.6);
    cx.draw_text(
        draw,
        semibold,
        FONT_SIZE_ROW,
        vec2(pos.x + cx.px(45.0), baseline - ascent),
        color,
        &to_upper(label),
    );
    if add_tip.is_some() && hovered {
        draw.add_rect_filled(
            add_min,
            add_max,
            p.green.with_alpha(if add_hovered { 0.3 } else { 0.2 }),
            cx.px(2.0),
        );
        draw.add_rect(add_min, add_max, p.green, cx.px(2.0), 1.0);
        if let Some(path) = icons::svg_path("plus") {
            cx.draw_svg_path(
                draw,
                "plus",
                path,
                24.0,
                24.0,
                add_min + vec2(cx.px(4.0), cx.px(4.0)),
                add_max - vec2(cx.px(4.0), cx.px(4.0)),
                p.text_normal,
            );
        }
    } else {
        let count = header.count.to_string();
        let count_size = cx.text_size(semibold, FONT_SIZE_ROW, &count);
        cx.draw_text(
            draw,
            semibold,
            FONT_SIZE_ROW,
            vec2(
                pos.x + width - cx.px(10.0) - count_size.x,
                baseline - ascent,
            ),
            p.text_accent,
            &count,
        );
    }
    result
}

// ---------------------------------------------------------------------------------------------------------
// The sections' contents

/// What the sections show, built once a frame from the (filtered) refs.
struct Trees {
    local: TreeNode,
    remote: TreeNode,
    tags: TreeNode,
    /// Indices into `history.refs`.
    stashes: Vec<usize>,
    /// The gitflow branches (owned: the walk mutates the state they came from).
    flows: Vec<Ref>,
    local_count: usize,
    remote_count: usize,
    tag_count: usize,
}

fn build_trees(state: &RepoState, filter: &str) -> Trees {
    let filtering = !filter.is_empty();
    let mut trees = Trees {
        local: TreeNode::root(),
        remote: TreeNode::root(),
        tags: TreeNode::root(),
        stashes: Vec::new(),
        flows: Vec::new(),
        local_count: 0,
        remote_count: 0,
        tag_count: 0,
    };
    // Every configured remote gets its row, also one with nothing fetched yet (its branches fill it in).
    for remote in &state.history.remotes {
        // (A name with '/' is left to its branches: the tree splits ref names there.)
        if !remote.name.contains('/') && (!filtering || contains_ci(&remote.name, filter)) {
            trees.remote.children.push(TreeNode::folder(
                &remote.name,
                format!("remote:{}", remote.name),
            ));
        }
    }
    for (index, r) in state.history.refs.iter().enumerate() {
        if filtering && !contains_ci(&r.name, filter) && !contains_ci(&r.message, filter) {
            continue;
        }
        match r.ref_type {
            RefType::LocalBranch => {
                trees.local_count += 1;
                trees.local.insert(&r.name, index, "local:");
            }
            RefType::RemoteBranch => {
                trees.remote_count += 1;
                trees.remote.insert(&r.name, index, "remote:");
            }
            RefType::Tag => {
                trees.tag_count += 1;
                trees.tags.insert(&r.name, index, "tag:");
            }
            RefType::Stash => trees.stashes.push(index),
        }
    }
    trees.local.sort(false);
    trees.remote.sort(false);
    trees.tags.sort(true);
    if gitflow_initialized(state) {
        trees.flows = gitflow_branches(state).into_iter().cloned().collect();
    }
    trees
}

/// The branch a stash was made on: "On <branch>: message" / "WIP on <branch>: ..." gives message "on: branch".
fn stash_branch(message: &str) -> Option<&str> {
    ["WIP on ", "On "].iter().find_map(|prefix| {
        let rest = message.strip_prefix(prefix)?;
        Some(rest.find(": ").map(|colon| &rest[..colon]))
    })?
}

fn draw_worktrees(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, walk: &Walk) {
    for index in 0..state.history.worktrees.len() {
        let row = {
            let worktree = &state.history.worktrees[index];
            let options = RowOptions {
                icon: Some("far:sitemap"),
                can_hide: false,
                tooltip: &worktree.path,
                ..RowOptions::default()
            };
            panel_row(
                cx,
                walk.row_width,
                &worktree.path,
                &RowLabel::plain(&worktree.name),
                &options,
            )
        };
        if row.context {
            let worktree = state.history.worktrees[index].clone();
            let items = worktree_menu(cx, app, state, &worktree);
            popup_menu(cx, items, None);
        }
        if row.double_clicked {
            let path = state.history.worktrees[index].path.clone();
            app.open_repository(&path);
        }
    }
}

fn draw_stashes(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    walk: &mut Walk,
    stashes: &[usize],
) {
    for &index in stashes {
        let row = {
            let stash = &state.history.refs[index];
            let options = RowOptions {
                icon: Some("far:inbox"),
                hidden: stash.hidden,
                selected: state.left_selected.as_deref() == Some(stash.full_name.as_str())
                    && state.is_selected(&stash.target),
                ..RowOptions::default()
            };
            panel_row(
                cx,
                walk.row_width,
                &stash.full_name,
                &RowLabel {
                    text: stash_summary(&stash.message),
                    stash_branch: stash_branch(&stash.message),
                },
                &options,
            )
        };
        if row.eye_clicked {
            let hidden = state.history.refs[index].hidden;
            set_hidden(state, walk, &[index], !hidden);
        } else if row.clicked {
            let (full_name, target) = {
                let stash = &state.history.refs[index];
                (stash.full_name.clone(), stash.target.clone())
            };
            state.left_selected = Some(full_name);
            jump_to_commit(state, &target);
        }
        if row.context {
            let stash = state.history.refs[index].clone();
            let items = ref_menu(cx, app, state, &stash, true);
            popup_menu(cx, items, None);
        }
    }
}

fn draw_submodules(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, walk: &Walk) {
    for index in 0..state.history.submodules.len() {
        // Double-click: the Edit Submodule panel sliding out over the graph.
        let (key, row) = {
            let submodule = &state.history.submodules[index];
            let key = format!("submodule:{}", submodule.path);
            let tooltip = format!("Path: /{}\nRemote URL: {}", submodule.path, submodule.url);
            let options = RowOptions {
                icon: Some("far:layer-group"),
                can_hide: false,
                synced: submodule.in_sync(),
                selected: state.left_selected.as_deref() == Some(key.as_str()),
                tooltip: &tooltip,
                ..RowOptions::default()
            };
            let row = panel_row(
                cx,
                walk.row_width,
                &key,
                &RowLabel::plain(&submodule.name),
                &options,
            );
            (key, row)
        };
        if row.clicked {
            state.left_selected = Some(key);
        }
        if row.double_clicked {
            let submodule = &state.history.submodules[index];
            app.dialog.open(DialogKind::EditSubmodule);
            app.dialog.text_a.clone_from(&submodule.url);
            app.dialog.text_b.clone_from(&submodule.path);
        }
    }
}

/// GITFLOW: its feature / release / hotfix branches (right-click: Finish).
fn draw_gitflow(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    walk: &mut Walk,
    flows: &[Ref],
) {
    for flow in flows {
        let key = format!("gitflow:{}", flow.full_name);
        let options = RowOptions {
            icon: Some(ref_icon(flow.ref_type)),
            checked_out: flow.is_head,
            can_hide: false,
            selected: state.left_selected.as_deref() == Some(key.as_str())
                && state.is_selected(&flow.target),
            ahead: flow.ahead,
            behind: flow.behind,
            tooltip: &flow.name,
            ..RowOptions::default()
        };
        let row = panel_row(
            cx,
            walk.row_width,
            &key,
            &RowLabel::plain(&flow.name),
            &options,
        );
        if row.double_clicked {
            walk.checkout_request = Some(flow.clone());
        } else if row.clicked {
            state.left_selected = Some(key);
            jump_to_commit(state, &flow.target);
        }
        if row.context {
            let items = ref_menu(cx, app, state, flow, true);
            popup_menu(cx, items, None);
        }
    }
}

fn draw_section_body(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    walk: &mut Walk,
    kind: SectionKind,
    trees: &Trees,
) {
    match kind {
        SectionKind::Local => draw_tree(cx, app, state, walk, &trees.local, 0, false),
        SectionKind::Remote => draw_tree(cx, app, state, walk, &trees.remote, 0, true),
        SectionKind::Worktrees => draw_worktrees(cx, app, state, walk),
        SectionKind::Stashes => draw_stashes(cx, app, state, walk, &trees.stashes),
        SectionKind::Tags => draw_tree(cx, app, state, walk, &trees.tags, 0, false),
        SectionKind::Submodules => draw_submodules(cx, app, state, walk),
        SectionKind::Gitflow => draw_gitflow(cx, app, state, walk, &trees.flows),
    }
}

// ---------------------------------------------------------------------------------------------------------
// The panel

/// The left panel, `size` device px; `open`: expanded, else the collapsed strip.
pub fn draw_left_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    size: Vec2,
    open: bool,
) {
    let ui = cx.ui;
    let origin = cx.cursor();
    let mut walk = Walk::default();
    ui.child_window("##left_panel")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR)
        .build(ui, || {
            let draw = cx.window_draw_list();
            let p = cx.palette;
            // panel__bg0 with a 1px panel border (composited over the app background) on the right.
            draw.add_rect_filled(
                origin,
                vec2(origin.x + size.x - 1.0, origin.y + size.y),
                p.panel_bg0,
                0.0,
            );
            draw.add_rect_filled(
                vec2(origin.x + size.x - 1.0, origin.y),
                origin + size,
                p.panel_border.mix(p.app_bg0, p.panel_border.a),
                0.0,
            );
            let inner_w = size.x - 1.0;
            if open {
                draw_open_panel(cx, app, state, &mut walk, draw, origin, size, inner_w);
            } else {
                draw_collapsed_panel(cx, app, state, draw, origin, size);
            }
        });
    if walk.refresh_needed {
        state.refresh();
    }
    if let Some(request) = walk.checkout_request {
        checkout_ref(cx, app, state, &request);
    }
}

/// Opens the panel again (on the section, which is expanded).
fn open_panel(app: &mut Application, state: &mut RepoState, section: Option<&str>) {
    app.show_left_panel = true;
    state.left_open_in_diff = true;
    if let Some(section) = section {
        state.expanded_left_sections.insert(section.to_owned());
    }
}

/// The collapsed panel (`LEFT_PANEL_COLLAPSED_WIDTH` 36): the expand chevron in a 30px row, then each section's
/// 14px icon (text-secondary) over its 10px count (text-accent) in 36px rows; a row opens the panel on that
/// section.
fn draw_collapsed_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    draw: DrawList<'_>,
    origin: Vec2,
    size: Vec2,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let strip_w = cx.px(35.0);
    draw.add_rect_filled(
        origin,
        vec2(origin.x + strip_w, origin.y + size.y),
        p.panel_bg0,
        0.0,
    );
    cx.set_cursor(origin);
    if ui.invisible_button("##expand_left", [strip_w, cx.px(30.0)]) {
        open_panel(app, state, None);
    }
    if ui.is_item_hovered() {
        let max = vec2(origin.x + strip_w, origin.y + cx.px(30.0));
        draw.add_rect_filled(origin, max, p.hover_row, 0.0);
        cx.styled_tooltip(origin, max, "Expand Panel (Ctrl+J)", false);
    }
    cx.draw_fa_icon(
        draw,
        "fas:chevron-circle-right",
        vec2(origin.x + cx.px(17.5), origin.y + cx.px(15.0)),
        cx.px(14.0),
        p.text_normal,
    );
    let (mut local_n, mut remote_n, mut stash_n, mut tag_n) = (0, 0, 0, 0);
    for r in &state.history.refs {
        match r.ref_type {
            RefType::LocalBranch => local_n += 1,
            RefType::RemoteBranch => remote_n += 1,
            RefType::Stash => stash_n += 1,
            RefType::Tag => tag_n += 1,
        }
    }
    let hidden = &app.settings.hidden_left_sections;
    let mut items = vec![(SectionKind::Local, local_n)];
    if !hidden.contains("Remote") {
        items.push((SectionKind::Remote, remote_n));
    }
    if !hidden.contains("Worktrees") {
        items.push((SectionKind::Worktrees, state.history.worktrees.len()));
    }
    if stash_n > 0 && !hidden.contains("Stashes") {
        items.push((SectionKind::Stashes, stash_n));
    }
    if !hidden.contains("Tags") {
        items.push((SectionKind::Tags, tag_n));
    }
    if !hidden.contains("Submodules") {
        items.push((SectionKind::Submodules, state.history.submodules.len()));
    }
    if gitflow_initialized(state) {
        items.push((SectionKind::Gitflow, gitflow_branches(state).len()));
    }
    let regular = cx.fonts.regular;
    let row_h = cx.px(36.0);
    let mut y = origin.y + cx.px(30.0);
    for (kind, count) in items {
        cx.set_cursor(vec2(origin.x, y));
        let (clicked, hovered) = {
            let _id = ui.push_id(kind.name());
            let clicked = ui.invisible_button("##strip_section", [strip_w, row_h]);
            (clicked, ui.is_item_hovered())
        };
        if clicked {
            open_panel(app, state, Some(kind.name()));
        }
        if hovered {
            let (min, max) = (vec2(origin.x, y), vec2(origin.x + strip_w, y + row_h));
            draw.add_rect_filled(min, max, p.hover_row, 0.0);
            cx.styled_tooltip(min, max, kind.name(), false);
        }
        cx.draw_fa_icon(
            draw,
            kind.icon(),
            vec2(origin.x + cx.px(17.5), y + cx.px(13.4)),
            cx.px(14.0),
            if hovered {
                p.text_selected
            } else {
                p.text_secondary
            },
        );
        let number = count.to_string();
        let w = cx.text_size(regular, 10.0, &number).x;
        let ascent = 10.0 * 2189.0 / 2048.0 * cx.scale;
        cx.draw_text(
            draw,
            regular,
            10.0,
            vec2(
                origin.x + cx.px(17.5) - w / 2.0,
                (y + cx.px(20.4) + ascent.round()).round() - ascent.ceil(),
            ),
            p.text_accent,
            &number,
        );
        y += row_h;
    }
}

/// The expanded panel: the header row, the filter box and the sections.
#[allow(clippy::too_many_arguments)]
fn draw_open_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    walk: &mut Walk,
    draw: DrawList<'_>,
    origin: Vec2,
    size: Vec2,
    inner_w: f32,
) {
    let ui = cx.ui;
    let p = cx.palette;
    // Header row: the collapse button, then "Viewing N"; the filter box under it.
    {
        let center = vec2(origin.x + cx.px(18.5), origin.y + cx.px(17.0));
        let side = cx.px(22.0);
        cx.set_cursor(center - vec2(side / 2.0, side / 2.0));
        ui.invisible_button("##collapse_left", [side, side]);
        let hovered = ui.is_item_hovered();
        cx.draw_icon(
            draw,
            Icon::Back,
            center,
            cx.px(16.0),
            if hovered { p.text_selected } else { p.ink(0.7) },
        );
        cx.tooltip("Collapse Panel (Ctrl+J)");
        if ui.is_item_clicked_with_button(MouseButton::Left) {
            if state.diff_target.is_some() && state.left_open_in_diff {
                state.left_open_in_diff = false;
            } else {
                app.show_left_panel = false;
            }
        }
    }

    // "Viewing N": the branches and tags walked for the graph (not hidden; stashes do not count).
    let visible_refs = state
        .history
        .refs
        .iter()
        .filter(|r| !r.hidden && r.ref_type != RefType::Stash)
        .count();
    {
        let regular = cx.fonts.regular;
        let semibold = cx.fonts.semibold;
        let ascent = cx.font_px(regular, FONT_SIZE_ROW) * 2189.0 / 2789.0;
        let y = origin.y + cx.px(21.6) - ascent;
        let x = origin.x + cx.px(36.0);
        let w = cx.text_size(regular, FONT_SIZE_ROW, "Viewing").x;
        cx.draw_text(
            draw,
            regular,
            FONT_SIZE_ROW,
            vec2(x, y),
            p.text_normal,
            "Viewing",
        );
        cx.draw_text(
            draw,
            semibold,
            FONT_SIZE_ROW,
            vec2(x + w + cx.px(5.0), y),
            p.text_accent,
            &visible_refs.to_string(),
        );
    }
    {
        let min = vec2(origin.x + cx.px(10.0), origin.y + cx.px(33.0));
        let max = vec2(origin.x + inner_w - cx.px(10.0), min.y + cx.px(21.0));
        draw.add_rect_filled(min, max, p.input_bg, cx.px(3.0));
        draw.add_rect(min, max, p.subtle_border, cx.px(3.0), 1.0);
        let text_h = cx.font_px(cx.fonts.regular, FONT_SIZE_ROW);
        cx.set_cursor(min);
        {
            let _padding = ui.push_style_var(StyleVar::FramePadding([
                cx.px(6.0),
                (max.y - min.y - text_h) / 2.0,
            ]));
            let _bg = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
            let _bg_hovered = ui.push_style_color(StyleColor::FrameBgHovered, Color::TRANSPARENT);
            let _bg_active = ui.push_style_color(StyleColor::FrameBgActive, Color::TRANSPARENT);
            let _font = cx.push_font(cx.fonts.regular, FONT_SIZE_ROW);
            let hint = cx.shortcut_label("Filter (Ctrl + Alt + f)");
            cx.input_text(
                "##left_filter",
                &mut state.left_filter,
                Some(&hint),
                InputTextFlags::NONE,
                max.x - min.x - cx.px(24.0),
            );
        }
        cx.draw_icon(
            draw,
            Icon::Search,
            vec2(origin.x + inner_w - cx.px(21.5), (min.y + max.y) / 2.0),
            cx.px(12.0),
            p.text_secondary,
        );
        if ui.is_key_chord_pressed(KeyChord::new(Key::F).with_mods(KeyMods::CTRL | KeyMods::ALT)) {
            ui.set_keyboard_focus_here_with_offset(-1);
        }
    }

    // The trees.
    let filtering = !state.left_filter.is_empty();
    let trees = build_trees(state, &state.left_filter);
    let hidden_sections = &app.settings.hidden_left_sections;
    let mut sections = vec![(SectionKind::Local, trees.local_count)];
    if !hidden_sections.contains("Remote") {
        sections.push((SectionKind::Remote, trees.remote_count));
    }
    if !hidden_sections.contains("Worktrees") {
        sections.push((SectionKind::Worktrees, state.history.worktrees.len()));
    }
    if !trees.stashes.is_empty() && !hidden_sections.contains("Stashes") {
        sections.push((SectionKind::Stashes, trees.stashes.len()));
    }
    if !hidden_sections.contains("Tags") {
        sections.push((SectionKind::Tags, trees.tag_count));
    }
    if !hidden_sections.contains("Submodules") {
        sections.push((SectionKind::Submodules, state.history.submodules.len()));
    }
    // GITFLOW, once Initialize Gitflow has set the repository up; the "+" starts a new branch.
    if gitflow_initialized(state) {
        sections.push((SectionKind::Gitflow, trees.flows.len()));
    }

    // Sections fill the panel under the filter box.
    let top = origin.y + cx.px(63.0);
    let panel_h = origin.y + size.y - top;
    let expanded: Vec<bool> = sections
        .iter()
        .map(|(kind, _)| filtering || state.expanded_left_sections.contains(kind.name()))
        .collect();
    let stored: Vec<Option<f32>> = sections
        .iter()
        .map(|(kind, _)| state.left_section_ys.get(kind.name()).map(|&y| cx.px(y)))
        .collect();
    let metrics = SectionMetrics {
        header: cx.px(HEADER_HEIGHT),
        minimum: cx.px(SECTION_MINIMUM_HEIGHT),
        default: cx.px(SECTION_DEFAULT_HEIGHT),
    };
    let ys = section_ys(&expanded, &stored, panel_h, metrics);
    walk.row_width = inner_w - cx.px(10.0);
    // SAFETY: `ui` proves a frame is open, so the style exists; one float is read.
    let spacing_x = unsafe { (*sys::igGetStyle()).ItemSpacing.x };
    let _spacing = ui.push_style_var(StyleVar::ItemSpacing([spacing_x, 0.0]));
    for (i, &(kind, count)) in sections.iter().enumerate() {
        let name = kind.name();
        let y0 = top + if i > 0 { ys[i - 1] } else { 0.0 };
        let y1 = top + ys[i];
        if expanded[i] && y1 - y0 > cx.px(HEADER_HEIGHT) {
            cx.set_cursor(vec2(origin.x, y0 + cx.px(HEADER_HEIGHT)));
            ui.child_window(format!("##section_{name}"))
                .size([inner_w, y1 - y0 - cx.px(HEADER_HEIGHT)])
                .build(ui, || draw_section_body(cx, app, state, walk, kind, &trees));
        }
        // The top edge of each later header drags the previous section's end (submitted first to win the hover
        // over the header).
        if i > 0 && (expanded[i - 1] || expanded[i]) {
            cx.set_cursor(vec2(origin.x, y0));
            let _id = ui.push_id(name);
            ui.invisible_button("##resize", [inner_w, cx.px(4.0)]);
            if ui.is_item_hovered() || ui.is_item_active() {
                ui.set_mouse_cursor(Some(MouseCursor::ResizeNS));
            }
            if ui.is_item_active() {
                let y = cx.mouse_pos().y - top;
                state
                    .left_section_ys
                    .insert(sections[i - 1].0.name().to_owned(), y / cx.px(1.0));
            }
        }
        let header = Header {
            kind,
            count,
            expanded: expanded[i],
        };
        let result = section_header(cx, app, state, vec2(origin.x, y0), inner_w, &header);
        if result.clicked && !state.expanded_left_sections.remove(name) {
            state.expanded_left_sections.insert(name.to_owned());
        }
        if result.add {
            match kind {
                SectionKind::Gitflow => {
                    let items = gitflow_start_items(cx, app, state);
                    popup_menu(cx, items, None);
                }
                SectionKind::Remote => app.dialog.open(DialogKind::AddRemote),
                _ => app.dialog.open(DialogKind::AddSubmodule),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_of(names: &[&str], prefix: &str, tags: bool) -> TreeNode {
        let mut root = TreeNode::root();
        for (index, name) in names.iter().enumerate() {
            root.insert(name, index, prefix);
        }
        root.sort(tags);
        root
    }

    fn names(node: &TreeNode) -> Vec<&str> {
        node.children.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn versions_parse_like_semver_ranges() {
        let v = parse_version("v1.10.0").unwrap();
        assert_eq!((v.part, v.exact), ([1, 10, 0], true));
        let v = parse_version("v1.0").unwrap();
        assert_eq!((v.part, v.exact), ([1, 0, 0], false));
        assert_eq!(parse_version("2.x").unwrap().part, [2, 0, 0]);
        let v = parse_version("1.2.3-rc.1+build5").unwrap();
        assert_eq!((v.pre.as_str(), v.exact), ("rc.1", true));
        assert_eq!(parse_version("1.2.3+build").unwrap().pre, "");
        assert!(parse_version("latest").is_none());
        assert!(parse_version("v").is_none());
        assert!(parse_version("1.2.3.4").is_none());
        assert!(parse_version("1.2.3foo").is_none());
    }

    #[test]
    fn tags_sort_plain_names_first_then_newest_version_first() {
        let tree = tree_of(
            &["v1.2.0", "v1.10.0", "nightly", "v1.10.0-rc1", "Alpha", "v2"],
            "tag:",
            true,
        );
        assert_eq!(
            names(&tree),
            ["Alpha", "nightly", "v2", "v1.10.0", "v1.10.0-rc1", "v1.2.0"]
        );
    }

    #[test]
    fn branches_nest_in_folders_sorted_case_insensitively() {
        let tree = tree_of(
            &[
                "main",
                "feature/b",
                "Zeta",
                "feature/a",
                "feature/deep/x",
                "alpha",
            ],
            "local:",
            false,
        );
        assert_eq!(names(&tree), ["alpha", "feature", "main", "Zeta"]);
        let feature = &tree.children[1];
        assert_eq!(feature.key, "local:feature");
        assert_eq!(feature.ref_index, None);
        assert_eq!(names(feature), ["a", "b", "deep"]);
        assert_eq!(feature.children[0].key, "local:feature/a");
        assert_eq!(feature.children[2].children[0].key, "local:feature/deep/x");
        let mut refs = Vec::new();
        tree.collect_refs(&mut refs);
        refs.sort_unstable();
        assert_eq!(refs, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn remote_branches_go_under_their_remote() {
        let mut root = TreeNode::root();
        root.children
            .push(TreeNode::folder("origin", "remote:origin".to_owned()));
        root.children
            .push(TreeNode::folder("empty", "remote:empty".to_owned()));
        root.insert("origin/feature/a", 0, "remote:");
        root.insert("origin/main", 1, "remote:");
        root.sort(false);
        assert_eq!(names(&root), ["empty", "origin"]);
        assert!(root.children[0].children.is_empty());
        let origin = &root.children[1];
        assert_eq!(names(origin), ["feature", "main"]);
        assert_eq!(origin.children[0].key, "remote:origin/feature");
    }

    fn metrics() -> SectionMetrics {
        SectionMetrics {
            header: 30.0,
            minimum: 108.0,
            default: 132.0,
        }
    }

    #[test]
    fn all_collapsed_sections_are_one_header_each() {
        let ys = section_ys(&[false; 3], &[None; 3], 600.0, metrics());
        // Collapsed sections are exactly their header high.
        assert_eq!(ys, [30.0, 60.0, 90.0]);
    }

    #[test]
    fn the_expanded_section_takes_what_the_collapsed_ones_leave() {
        // The sections fill the panel: the collapsed ones below keep their header, the expanded one the rest.
        let ys = section_ys(&[true, false, false], &[None; 3], 800.0, metrics());
        assert_eq!(ys, [740.0, 770.0, 800.0]);
        // Two expanded ones share it: the first prefers header + 132.
        let ys = section_ys(&[true, true], &[None; 2], 800.0, metrics());
        assert_eq!(ys, [162.0, 800.0]);
    }

    #[test]
    fn a_stored_end_is_bounded_by_the_minimum_height() {
        // Never less than header + minimum.
        let ys = section_ys(&[true, true], &[Some(10.0), None], 500.0, metrics());
        assert_eq!(ys, [138.0, 500.0]);
        // The second section keeps its minimum below.
        let ys = section_ys(&[true, true], &[Some(450.0), None], 500.0, metrics());
        assert_eq!(ys, [362.0, 500.0]);
    }

    #[test]
    fn no_sections_no_ends() {
        assert!(section_ys(&[], &[], 100.0, metrics()).is_empty());
    }

    #[test]
    fn stash_branches_are_read_from_the_message() {
        assert_eq!(stash_branch("WIP on main: 1234567 subject"), Some("main"));
        assert_eq!(stash_branch("On feature/x: my stash"), Some("feature/x"));
        assert_eq!(stash_branch("On main"), None);
        assert_eq!(stash_branch("custom"), None);
    }

    fn width(text: &str) -> f32 {
        text.chars().count() as f32
    }

    fn run(text: &str, margin: f32) -> TextRun<'_> {
        TextRun {
            text,
            color: Color::WHITE,
            margin,
        }
    }

    #[test]
    fn runs_that_fit_are_laid_out_with_their_margins() {
        let runs = [run("fix", 0.0), run("on:", 2.0), run("main", 2.0)];
        let layout = layout_runs(&runs, 30.0, 1.0, &width);
        assert_eq!(
            layout.pieces,
            [(0, "fix", 0.0), (1, "on:", 5.0), (2, "main", 10.0)]
        );
        assert_eq!(layout.ellipsis, None);
    }

    #[test]
    fn runs_that_do_not_fit_are_cut_with_an_ellipsis_after_the_last_glyph() {
        let runs = [run("message", 0.0), run("on:", 2.0), run("branch", 2.0)];
        // 7 + 2 + 3 + 2 + 6 = 20 > 12: room for 11 glyphs and the ellipsis; the 2nd run starts at 9.
        let layout = layout_runs(&runs, 12.0, 1.0, &width);
        assert_eq!(layout.pieces, [(0, "message", 0.0), (1, "on", 9.0)]);
        assert_eq!(layout.ellipsis, Some(11.0));
        // Cut inside the first run.
        let layout = layout_runs(&runs, 4.0, 1.0, &width);
        assert_eq!(layout.pieces, [(0, "mes", 0.0)]);
        assert_eq!(layout.ellipsis, Some(3.0));
    }
}
