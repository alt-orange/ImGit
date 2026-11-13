//! The rows of the graph: keyboard navigation, the ref zone's popover, timeline markers and every row's zones.

use std::collections::HashSet;

use dear_imgui_rs::{FocusedFlags, Key, ListClipper, MouseButton, StyleVar, WindowHoveredFlags};
use imgit_app::repo_state::InlineRef;
use imgit_app::{RepoState, pref, preferences};
use imgit_git::repository::short_sha;
use imgit_git::{Commit, NodeType, Sha};

use super::changes::{FileCounts, draw_file_counts};
use super::lanes::{draw_edges, draw_node, fade_vertices, gutter_shadow};
use super::refs::{
    RefGroup, RefGroupKey, RefNode, overflow_count, overflow_count_width, ref_groups, ref_node,
    ref_node_width, row_misses_group, row_ref_groups,
};
use super::zones::{GraphLanes, Metrics, ZoneLayout, ZoneType, Zones};
use super::{Deferred, Geometry, State, draw_scroll_markers, style_sizes, timeline_bucket};
use crate::app::application::Application;
use crate::theme::{Color, FONT_SIZE_ROW};
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};
use crate::ui::menu::popup_menu;
use crate::ui::panels::file_editor::navigate_from_editor;
use crate::ui::panels::menus::{commit_menu, ref_drop_menu, ref_menu};
use crate::ui::widgets::format_relative;
use crate::ui::{Cx, panels};
use crate::window::CursorShape;

/// A selection a row asked for; made once the rows are drawn (they read what it changes).
enum Pick {
    /// The commit alone, closing the open diff when `close_diff`.
    Only { sha: Sha, close_diff: bool },
    /// A click: Ctrl toggles, Shift ranges.
    Click { sha: Sha, toggle: bool, range: bool },
}

/// What the rows of one frame share: the measures and mouse state they read, and what they report back.
struct RowFrame {
    m: Metrics,
    z: Zones,
    origin: Vec2,
    size: Vec2,
    scrollbar: f32,
    row_h: f32,
    zones_x: f32,
    mouse: Vec2,
    window_hovered: bool,
    /// A pressed pill is the active item: the ref zone stays hovered through it, so a double click lands on the
    /// popover it was aimed at.
    zone_window_hovered: bool,
    line_h: f32,
    searching: bool,
    search_rows: HashSet<usize>,
    /// `.dimmed-row`'s progress, 0..1, for the rows that miss `dim_key`.
    dim: f32,
    dim_key: Option<RefGroupKey>,
    // What the rows report.
    hovered_group: Option<RefGroupKey>,
    hovered_zone: Option<Sha>,
    picks: Vec<Pick>,
    deferred: Deferred,
    open_ref_menu: bool,
    open_commit_menu: bool,
    /// Rows on screen whose CHANGES counts are not known yet.
    counts_wanted: Vec<Sha>,
}

/// A ref line's extent and colour.
struct RefLine {
    min: Vec2,
    max: Vec2,
    color: Color,
}

fn lerp(a: Color, to: Color, t: f32) -> Color {
    Color::new(
        a.r + (to.r - a.r) * t,
        a.g + (to.g - a.g) * t,
        a.b + (to.b - a.b) * t,
        a.a + (to.a - a.a) * t,
    )
}

/// The zones but the ref and graph ones get the row background over the inner 22px (GRAPH_ROW_INNER_HEIGHT); the
/// last one carries it to the panel's edge.
fn fill_row(
    draw: DrawList<'_>,
    layout: &ZoneLayout,
    f: &RowFrame,
    (top, bottom): (f32, f32),
    color: Color,
) {
    for (i, zone) in layout.zones.iter().enumerate() {
        if matches!(zone.zone, ZoneType::Ref | ZoneType::Graph) {
            continue;
        }
        let x = f.z.x(zone.zone);
        let right = if i + 1 == layout.zones.len() {
            f.origin.x + f.size.x
        } else {
            x + f.z.w(zone.zone)
        };
        draw.add_rect_filled(vec2(x, top), vec2(right, bottom), color, 0.0);
    }
}

impl State {
    /// The rows child window (scrolling vertically), below the header.
    pub(super) fn draw_rows(
        &mut self,
        cx: &mut Cx<'_>,
        app: &mut Application,
        state: &mut RepoState,
        g: &mut Geometry,
        rows_top: f32,
    ) -> Deferred {
        let ui = cx.ui;
        let p = cx.palette;
        let m = g.m;
        let (origin, size, scrollbar) = (g.origin, g.size, g.scrollbar);
        let row_h = m.px(m.row_height);
        let mut z = g.z;
        // Before the rows, so the zone edges win the hover over them.
        self.zone_handles(cx, g.zones_x, rows_top, origin.y + size.y, false);
        // The graph zone scrolls sideways when narrower than its lanes: a horizontal wheel, Shift + wheel, or its
        // scrollbar (.scrollbar-outer.horizontal, along the zone's bottom over the rows).
        let (bar_h, spacing_x) = style_sizes(cx);
        let bar_min = vec2(z.lanes_x(&m), origin.y + size.y - bar_h);
        let bar_max = vec2(bar_min.x + z.w(ZoneType::Graph), origin.y + size.y);
        {
            let io = ui.io();
            let mouse = cx.mouse_pos();
            if ui.is_window_hovered() && mouse.x >= bar_min.x && mouse.x < bar_max.x {
                let wheel = if io.mouse_wheel_h() != 0.0 {
                    -io.mouse_wheel_h()
                } else if io.key_shift() {
                    -io.mouse_wheel()
                } else {
                    0.0
                };
                self.graph_scroll += wheel * 100.0; // Chromium scrolls 100px a notch
            }
        }
        let max_columns = state.layout.max_columns;
        z.lanes = GraphLanes::new(&self.zones, &mut self.graph_scroll, max_columns, &m);
        let graph_bar = cx.scrollbar_x(
            "##graph_scroll_x",
            bar_min,
            bar_max,
            &mut self.graph_scroll,
            z.lanes.content,
            z.lanes.width,
        );
        z.lanes = GraphLanes::new(&self.zones, &mut self.graph_scroll, max_columns, &m);
        // The rows abut.
        let spacing = ui.push_style_var(StyleVar::ItemSpacing([spacing_x, 0.0]));
        let rows_draw = cx.window_draw_list();
        rows_draw.set_grayscale_text(true); // grey-scale text, like a composited layer
        let row_count = state.history.commits.len();

        self.navigate(cx, state, row_count);
        self.scroll_to_row(cx, state, row_h);

        // A pressed pill becomes a drag 4px away; Esc cancels it.
        if self.refs.drag.pressed && self.refs.drag.repo != state.repo.workdir() {
            self.refs.drag = Default::default();
        }
        if self.refs.drag.pressed && !self.refs.drag.active && ui.is_mouse_down(MouseButton::Left) {
            let mouse = cx.mouse_pos();
            let press = self.refs.drag.press;
            if (mouse.x - press.x).abs() >= cx.px(4.0) || (mouse.y - press.y).abs() >= cx.px(4.0) {
                self.refs.drag.active = true;
            }
        }
        if self.refs.drag.active && ui.is_key_pressed(Key::Escape) {
            self.refs.drag = Default::default();
        }
        self.refs.drop_target = None;

        let rows_origin = cx.cursor();
        let searching = state.search_open && !state.search_text.is_empty();
        let dim_key = self.dim_key(cx, app, state);
        if dim_key != self.dimming_for {
            self.dimming_for.clone_from(&dim_key);
            self.dim_since = cx.time();
        }
        let mut dim = 0.0;
        if dim_key.is_some() {
            let t = (((cx.time() - self.dim_since - 1.0) / 0.5) as f32).clamp(0.0, 1.0);
            dim = 1.0 - (1.0 - t) * (1.0 - t); // ease-out
        }
        state.hovered_sha = None;
        let mut f = RowFrame {
            m,
            z,
            origin,
            size,
            scrollbar,
            row_h,
            zones_x: g.zones_x,
            mouse: cx.mouse_pos(),
            window_hovered: ui.is_window_hovered_with_flags(WindowHoveredFlags::CHILD_WINDOWS),
            zone_window_hovered: ui.is_window_hovered_with_flags(
                WindowHoveredFlags::CHILD_WINDOWS
                    | WindowHoveredFlags::ALLOW_WHEN_BLOCKED_BY_ACTIVE_ITEM,
            ),
            line_h: cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, "A").y,
            searching,
            search_rows: state.search_matches.iter().copied().collect(),
            dim,
            dim_key,
            hovered_group: None,
            hovered_zone: None,
            picks: Vec::new(),
            deferred: Deferred::default(),
            open_ref_menu: false,
            open_commit_menu: false,
            counts_wanted: Vec::new(),
        };

        // The hovered ref zone's popover (.ref-zone ul): every group at its natural width, 22px apart.
        rows_draw.channels_split(2);
        rows_draw.channels_set_current(1);
        self.draw_zone_popover(cx, state, &mut f, rows_draw, rows_origin.y);
        rows_draw.channels_set_current(0);
        cx.set_cursor(rows_origin);

        let timeline = timeline_markers(state);

        let footer = usize::from(state.history.has_more);
        let clipper = ListClipper::new(row_count + footer)
            .items_height(row_h)
            .begin(ui);
        for i in clipper.iter() {
            self.draw_row(cx, app, state, &mut f, rows_draw, i);
        }
        if f.z.w(ZoneType::Changes) > 0.0 {
            self.changes.update(state.repo.workdir(), &f.counts_wanted);
            // The counts arrive from another thread: keep drawing until they are in.
            if self.changes.busy() {
                cx.keep_animating();
            }
        }
        for pick in std::mem::take(&mut f.picks) {
            match pick {
                Pick::Only { sha, close_diff } => {
                    if close_diff {
                        // Unsaved text in the file editor is asked about before the commit changes.
                        navigate_from_editor(app, state, move |_app, state| {
                            state.select_only(&sha);
                            let _ = state.close_diff();
                        });
                    } else {
                        state.select_only(&sha);
                    }
                }
                Pick::Click { sha, toggle, range } => {
                    navigate_from_editor(app, state, move |_app, state| {
                        state.select(&sha, toggle, range);
                        let _ = state.close_diff();
                    });
                }
            }
        }
        // Over the rows, under the ref zone's popover.
        rows_draw.channels_set_current(0);
        self.draw_timeline(cx, &f, rows_draw, rows_origin.y, &timeline);
        rows_draw.channels_merge();
        draw_scroll_markers(cx, state, row_h, row_count + footer);
        drop(spacing);
        if !self.refs.drag.active {
            self.hovered_group = f.hovered_group.take();
            if f.hovered_zone.is_none() {
                self.zone_popover_row = None;
            }
            self.hovered_zone_sha = f.hovered_zone.take();
        }

        self.finish_ref_drag(cx, app, state, &f);

        if graph_bar.thumb_w > 0.0 {
            // .pad-for-horizontal-scrollbar: the last row clears the scrollbar.
            ui.dummy([1.0, bar_h]);
        }
        cx.draw_scrollbar_x(rows_draw, bar_min, bar_max, &graph_bar);
        if row_count == 0 {
            cx.set_cursor(vec2(
                origin.x + cx.px(20.0),
                g.header_y + g.header_h + cx.px(20.0),
            ));
            cx.secondary_text(if state.history.unborn {
                "No commits yet. Stage files and make the first commit."
            } else {
                "No commits"
            });
        }

        if f.open_ref_menu
            && let Some(context) = self.context_ref.clone()
        {
            let items = ref_menu(cx, app, state, &context, false);
            popup_menu(cx, items, None);
        } else if f.open_commit_menu {
            let items = commit_menu(cx, app, state, &self.context_sha);
            popup_menu(cx, items, None);
        }
        let _ = p;
        f.deferred
    }

    /// The commit the Ctrl-held dimming and the hovered ref group dim the other rows for.
    fn dim_key(&self, cx: &Cx<'_>, app: &Application, state: &RepoState) -> Option<RefGroupKey> {
        // .dimmed-row: rows that miss the hovered ref group (or, while Ctrl is held, the selected commit's first
        // group) fade their text to --text-dimmed after 1s over 0.5s ease-out; the class leaves instantly.
        if cx.ui.io().key_ctrl()
            && cx
                .ui
                .is_window_focused_with_flags(FocusedFlags::ROOT_AND_CHILD_WINDOWS)
            && let Some(single) = state.single_selected()
            && let Some(&row) = state.history.row_by_sha.get(&single.sha)
            && let Some(first) = row_ref_groups(state, row).first()
        {
            return Some(first.key());
        }
        if app.settings.highlight_rows_on_ref_hover {
            self.hovered_group.clone()
        } else {
            None
        }
    }

    /// Up / Down / J / K, Home, End and Shift ranges move the selection.
    fn navigate(&self, cx: &Cx<'_>, state: &mut RepoState, row_count: usize) {
        let ui = cx.ui;
        if !ui.is_window_focused_with_flags(FocusedFlags::CHILD_WINDOWS | FocusedFlags::ROOT_WINDOW)
            || ui.io().want_text_input()
            || row_count == 0
        {
            return;
        }
        let io = ui.io();
        // The letters alone: Ctrl+J / Ctrl+K toggle the panels.
        let plain = !io.key_ctrl() && !io.key_alt() && !io.key_super();
        let mut step = 0isize;
        if ui.is_key_pressed(Key::DownArrow) || (plain && ui.is_key_pressed(Key::J)) {
            step = 1;
        }
        if ui.is_key_pressed(Key::UpArrow) || (plain && ui.is_key_pressed(Key::K)) {
            step = -1;
        }
        if ui.is_key_pressed(Key::Home) {
            let sha = state.history.commits[0].sha.clone();
            state.select_only(&sha);
            state.scroll_to_row = Some(0);
        }
        if ui.is_key_pressed(Key::End) {
            let sha = state.history.commits[row_count - 1].sha.clone();
            state.select_only(&sha);
            state.scroll_to_row = Some(row_count - 1);
        }
        if step == 0 {
            return;
        }
        // A Shift range grows from its moving end; `focused` stays its anchor.
        let range = io.key_shift();
        let from = state
            .range_cursor
            .as_ref()
            .filter(|sha| range && state.history.row_by_sha.contains_key(*sha))
            .or(state.focused.as_ref());
        let index = from
            .and_then(|sha| state.history.row_by_sha.get(sha))
            .map_or(0, |&row| row.saturating_add_signed(step).min(row_count - 1));
        let target = state.history.commits[index].sha.clone();
        if range {
            state.select(&target, false, true);
            state.range_cursor = Some(target);
        } else {
            state.select_only(&target);
        }
        state.scroll_to_row = Some(index);
    }

    /// Scrolls the requested row into view (centred when it is off screen).
    fn scroll_to_row(&self, cx: &Cx<'_>, state: &mut RepoState, row_h: f32) {
        let Some(row) = state.scroll_to_row.take() else {
            return;
        };
        let ui = cx.ui;
        let target = row as f32 * row_h;
        let scroll = ui.scroll_y();
        let visible = ui.window_height();
        if target < scroll || target + row_h > scroll + visible {
            ui.set_scroll_y((target - visible / 2.0 + row_h / 2.0).max(0.0));
        }
    }

    /// The popover of the hovered ref zone, over the rows below.
    fn draw_zone_popover(
        &mut self,
        cx: &mut Cx<'_>,
        state: &RepoState,
        f: &mut RowFrame,
        draw: DrawList<'_>,
        rows_origin_y: f32,
    ) {
        let commits = &state.history.commits;
        let (Some(sha), Some(row)) = (self.hovered_zone_sha.clone(), self.zone_popover_row) else {
            return;
        };
        if commits.get(row).is_none_or(|commit| commit.sha != sha) {
            return;
        }
        let source = if self.zone_popover_ghost {
            state.ghost_ref_rows.get(row).copied().flatten()
        } else {
            Some(row)
        };
        let groups = source.map_or_else(Vec::new, |source| {
            ref_groups(&state.refs_at(&commits[source].sha))
        });
        let lane = cx.palette.graph_color(state.layout.rows[row].column as i32);
        let m = f.m;
        let mut top = rows_origin_y + row as f32 * f.row_h + m.px(m.margin);
        for (g, group) in groups.iter().enumerate().take(50) {
            let width = ref_node_width(cx, group, &m);
            let pos = vec2(f.z.x(ZoneType::Ref), top);
            let node = ref_node(
                cx,
                &mut self.refs,
                state,
                &m,
                draw,
                &RefNode {
                    group,
                    pos,
                    width,
                    lane,
                    opacity: if self.zone_popover_ghost { 0.65 } else { 1.0 },
                    id: &format!("popover{g}"),
                },
            );
            if node.hovered {
                f.hovered_group = Some(group.key());
            }
            if node.double_clicked {
                f.deferred.checkout = Some(group.primary().clone());
            } else if node.clicked {
                f.picks.push(Pick::Only {
                    sha: sha.clone(),
                    close_diff: true,
                });
            }
            if node.right_clicked {
                self.context_ref = Some(group.primary().clone());
                f.open_ref_menu = true;
            }
            if f.zone_window_hovered
                && f.mouse.x >= pos.x
                && f.mouse.x < pos.x + width
                && f.mouse.y >= top
                && f.mouse.y < top + m.px(m.ref_height)
            {
                f.hovered_zone = Some(sha.clone());
            }
            top += m.px(m.ref_height);
        }
    }

    /// Timeline markers: the first row past each age step ("1 hour ago", "2 hours ago", "yesterday"...) gets a
    /// line along its top edge and the age in a label at the rows' right end.
    fn draw_timeline(
        &self,
        cx: &Cx<'_>,
        f: &RowFrame,
        draw: DrawList<'_>,
        rows_origin_y: f32,
        timeline: &[(usize, String)],
    ) {
        let p = cx.palette;
        let m = &f.m;
        let visible_top = cx.ui.window_pos()[1];
        let visible_bottom = visible_top + cx.ui.window_height();
        let rows_right = f.origin.x + f.size.x - f.scrollbar;
        // The label keeps clear of the ref pills: before the ref zone when that is the last one.
        let label_right = if self
            .zones
            .zones
            .last()
            .is_some_and(|z| z.zone == ZoneType::Ref)
        {
            f.z.x(ZoneType::Ref)
        } else {
            rows_right
        };
        let label_h = m.px(12.0);
        for (row, label) in timeline {
            let y = (rows_origin_y + *row as f32 * f.row_h).round();
            if y + label_h < visible_top || y - label_h > visible_bottom {
                continue;
            }
            // The first row has no row above to part from, and its label stays inside the rows.
            if *row > 0 {
                draw.add_line(
                    vec2(f.origin.x, y + 0.5),
                    vec2(rows_right, y + 0.5),
                    p.ink(0.08),
                    1.0,
                );
            }
            let label_y = if *row > 0 { y } else { y + label_h / 2.0 };
            let label_size = cx.text_size(cx.fonts.regular, 10.0, label);
            let label_max = vec2(label_right - m.px(8.0), (label_y + label_h / 2.0).round());
            let label_min = vec2(
                (label_max.x - label_size.x - m.px(10.0)).round(),
                label_max.y - label_h,
            );
            draw.add_rect_filled(label_min, label_max, p.panel_bg0, m.px(3.0));
            cx.draw_text(
                draw,
                cx.fonts.regular,
                10.0,
                vec2(
                    label_min.x + m.px(5.0),
                    (label_min.y + (label_h - label_size.y) / 2.0).round(),
                ),
                p.text_secondary,
                label,
            );
        }
    }

    /// The dragged ref pill, and the drop on release.
    fn finish_ref_drag(
        &mut self,
        cx: &mut Cx<'_>,
        app: &mut Application,
        state: &mut RepoState,
        f: &RowFrame,
    ) {
        let ui = cx.ui;
        if self.refs.drag.active {
            let drag = &self.refs.drag;
            let dragged = RefGroup {
                name: drag.name.clone(),
                refs: drag.refs.clone(),
                current: drag.current,
            };
            // The drag image was captured on dragstart: hovered (45%), or the checked-out branch's 50%.
            let min = f.mouse - drag.grab;
            let weight = if drag.current { 0.50 } else { 0.45 };
            let (width, lane) = (drag.width, drag.lane);
            let front = cx.foreground_draw_list();
            super::refs::draw_ref_pill(
                cx,
                front,
                &f.m,
                &dragged,
                (min, vec2(min.x + width, min.y + f.m.px(f.m.ref_height))),
                super::refs::PillStyle {
                    lane,
                    opacity: 0.75,
                    weight,
                    show_check: true,
                    tooltips: false,
                },
            );
            cx.set_drag_cursor(if self.refs.drop_target.is_some() {
                CursorShape::Move
            } else {
                CursorShape::NoDrop
            });
        }
        if self.refs.drag.pressed && !ui.is_mouse_down(MouseButton::Left) {
            if self.refs.drag.active
                && let (Some(target), Some(source)) = (
                    self.refs.drop_target.clone(),
                    self.refs.drag.refs.first().cloned(),
                )
            {
                let items = ref_drop_menu(cx, app, state, &source, &target);
                popup_menu(cx, items, None);
            }
            self.refs.drag = Default::default();
        }
    }

    /// One row, or the "Show more commits" footer after the last.
    fn draw_row(
        &mut self,
        cx: &mut Cx<'_>,
        app: &mut Application,
        state: &mut RepoState,
        f: &mut RowFrame,
        rows_draw: DrawList<'_>,
        i: usize,
    ) {
        let ui = cx.ui;
        let p = cx.palette;
        let m = f.m;
        let z = f.z;
        let (origin, size, row_h) = (f.origin, f.size, f.row_h);
        let row_count = state.history.commits.len();
        let y0 = cx.cursor().y;
        if i >= row_count {
            cx.set_cursor(vec2(
                (if z.w(ZoneType::Message) > 0.0 {
                    z.x(ZoneType::Message)
                } else {
                    f.zones_x
                }) + cx.px(10.0),
                y0 + cx.px(3.0),
            ));
            cx.secondary_text(&format!("Displaying {row_count} commits."));
            ui.same_line();
            if cx.link_text("Show more commits") {
                f.deferred.show_more = true;
            }
            // Lazy Load Commits in Graph: reaching the end loads the next ones (once per load: the limit is past
            // what is shown until the history is read again).
            if preferences().flag(pref::LAZY_LOAD_COMMITS) && row_count >= state.commit_limit {
                f.deferred.show_more = true;
            }
            cx.set_cursor(vec2(origin.x, y0));
            ui.dummy([size.x, row_h]);
            return;
        }
        let commit = &state.history.commits[i];
        let row = &state.layout.rows[i];
        let selected = state.is_selected(&commit.sha);
        let is_wip = commit.node_type == NodeType::WorkDir;
        let dimmed = f.searching && !f.search_rows.contains(&i);
        let alpha = if dimmed { 0.3 } else { 1.0 };
        // The work-dir row has its own renderer without .dimmed-row.
        let row_dim = match &f.dim_key {
            Some(key) if f.dim > 0.0 && !is_wip && row_misses_group(state, i, key) => f.dim,
            _ => 0.0,
        };
        // --text-dimmed (20% white), --text-dimmed-selected (50%) on a selected row.
        let dim_text = |color: Color| {
            let to = if selected { p.ink(0.5) } else { p.ink(0.2) };
            lerp(color, to, row_dim)
        };
        // The message fades its colour by `alpha`; the author, date and SHA texts *set* the alpha, as the C++
        // `theme::with_alpha` does, so they show at the ink's full opacity (`alpha` on a dimmed search row).
        let faded = |color: Color, factor: f32| color.with_alpha(color.a * factor);

        // Hover is known before any item is submitted so the ref pills (submitted first, so they win the hover
        // test) can be drawn, and the row button goes last to leave the cursor on the next row.
        let hovered = f.window_hovered
            && !self.refs.drag.active
            && f.mouse.x >= origin.x
            && f.mouse.x < origin.x + size.x - f.scrollbar
            && f.mouse.y >= y0
            && f.mouse.y < y0 + row_h;
        // The ref zone does not hover the row (ImGit tracks row hover in the graph / message zones).
        let ref_zone = (
            z.x(ZoneType::Ref) - m.px(3.0),
            z.x(ZoneType::Ref) + z.w(ZoneType::Ref) - m.px(3.0),
        );
        let over_ref_zone = f.mouse.x >= ref_zone.0 && f.mouse.x < ref_zone.1;
        let row_hovered = hovered && !over_ref_zone;
        let single_selected = selected && state.selected.len() == 1;
        if hovered {
            state.hovered_sha = Some(commit.sha.clone());
        }
        let _id = ui.push_id(i);
        // The row background over the inner 22px.
        let inner_top = y0 + m.px(m.margin);
        let inner_bottom = inner_top + m.px(m.row_inner_height);
        if selected {
            fill_row(
                rows_draw,
                &self.zones,
                f,
                (inner_top, inner_bottom),
                p.selected_row,
            );
        } else if row_hovered {
            fill_row(
                rows_draw,
                &self.zones,
                f,
                (inner_top, inner_bottom),
                p.hover_row,
            );
        }
        if f.searching && !dimmed && !selected {
            fill_row(
                rows_draw,
                &self.zones,
                f,
                (inner_top, inner_bottom),
                p.accent.with_alpha(0.08),
            );
        }

        // Ref zone: the first ref group and a "+N" count; hovering the zone lists every group (drawn after the
        // rows). Rows without refs show their nearest descendant's refs dimmed while hovered or selected (ghost
        // refs).
        let lane_color = p.graph_color(row.column as i32);
        let groups = ref_groups(&state.refs_at(&commit.sha));
        let ghost_groups = if groups.is_empty()
            && app.settings.show_ghost_refs
            && (row_hovered || single_selected)
        {
            state
                .ghost_ref_rows
                .get(i)
                .copied()
                .flatten()
                .map(|source| ref_groups(&state.refs_at(&state.history.commits[source].sha)))
                .filter(|groups| !groups.is_empty())
        } else {
            None
        };
        let ghost = ghost_groups.is_some();
        let shown_groups: &[RefGroup] = ghost_groups.as_deref().unwrap_or(&groups);
        let zone_hovered = self.hovered_zone_sha.as_deref() == Some(commit.sha.as_str());
        let creating = state
            .inline_ref
            .as_ref()
            .is_some_and(|r| r.sha == commit.sha);
        let mut ref_line = None;
        if (!groups.is_empty() || creating)
            && z.w(ZoneType::Ref) > 0.0
            && z.w(ZoneType::Graph) > 0.0
        {
            // .ref-line: the lane colour from the zone (+3px) to the node, 1px at 25% opacity; 2px opaque on the
            // row of the checked-out branch.
            let head_row = groups.iter().any(|g| g.current);
            let mut node_edge = z.node_x(row.column, &m);
            let node_half = if commit.node_type == NodeType::Merge {
                m.px(m.merge_node_diameter) / 2.0
            } else {
                m.px(m.node_diameter) / 2.0
            };
            // From the zone to the node's near edge, whichever side of the graph the zone is on; with other
            // zones between them it stops at the graph zone's edge instead of crossing their text.
            let ref_left = z.x(ZoneType::Ref) < z.x(ZoneType::Graph);
            let ref_adjacent = matches!(
                (self.zones.index_of(ZoneType::Ref), self.zones.index_of(ZoneType::Graph)),
                (Some(a), Some(b)) if a.abs_diff(b) == 1
            );
            node_edge += if ref_left { -node_half } else { node_half };
            let line_y =
                inner_top + m.px(m.row_inner_height / 2.0 - if head_row { 1.0 } else { 0.0 });
            let body_left = z.lanes_x(&m);
            let body_right = body_left + z.w(ZoneType::Graph);
            let line_min_x = if ref_left {
                if ref_adjacent {
                    z.x(ZoneType::Ref)
                } else {
                    body_left
                }
            } else {
                node_edge
            };
            let line_max_x = if ref_left {
                node_edge
            } else if ref_adjacent {
                z.x(ZoneType::Ref)
            } else {
                body_right
            };
            let line = RefLine {
                min: vec2(line_min_x, line_y),
                max: vec2(line_max_x, line_y + m.px(if head_row { 2.0 } else { 1.0 })),
                color: lane_color.with_alpha(if head_row { alpha } else { 0.25 * alpha }),
            };
            // Outside the graph zone now; over it after the gutters, which it crosses (.ref-line is z4).
            if line_min_x < body_left {
                rows_draw.add_rect_filled(
                    line.min,
                    vec2(line_max_x.min(body_left), line.max.y),
                    line.color,
                    0.0,
                );
            }
            if line_max_x > body_right {
                rows_draw.add_rect_filled(
                    vec2(line_min_x.max(body_right), line_y),
                    line.max,
                    line.color,
                    0.0,
                );
            }
            ref_line = Some(line);
        }
        if creating {
            self.draw_inline_ref(cx, &mut state.inline_ref, f, rows_draw, y0);
        } else if !shown_groups.is_empty() && !zone_hovered && z.w(ZoneType::Ref) > 0.0 {
            let opacity = if ghost {
                if zone_hovered { 0.65 } else { 0.5 }
            } else {
                alpha
            };
            let more = shown_groups.len() - 1;
            let more_w = if more > 0 {
                overflow_count_width(cx, &m, more) + m.px(5.0)
            } else {
                0.0
            };
            let room = z.w(ZoneType::Ref) - m.px(8.0) - more_w;
            let width = ref_node_width(cx, &shown_groups[0], &m).min(room);
            if width > m.px(20.0) {
                let node = ref_node(
                    cx,
                    &mut self.refs,
                    state,
                    &m,
                    rows_draw,
                    &RefNode {
                        group: &shown_groups[0],
                        pos: vec2(z.x(ZoneType::Ref), inner_top),
                        width,
                        lane: lane_color,
                        opacity,
                        id: "row",
                    },
                );
                if node.hovered {
                    f.hovered_group = Some(shown_groups[0].key());
                }
                if node.double_clicked {
                    f.deferred.checkout = Some(shown_groups[0].primary().clone());
                } else if node.clicked {
                    f.picks.push(Pick::Only {
                        sha: commit.sha.clone(),
                        close_diff: true,
                    });
                }
                if node.right_clicked {
                    self.context_ref = Some(shown_groups[0].primary().clone());
                    f.open_ref_menu = true;
                }
                if more > 0 {
                    overflow_count(
                        cx,
                        rows_draw,
                        &m,
                        vec2(z.x(ZoneType::Ref) + node.width + m.px(5.0), inner_top),
                        more,
                        lane_color,
                        opacity,
                    );
                }
            }
        }
        // A popover item over this row keeps its own zone (else this row's ghost refs would replace it).
        if f.window_hovered
            && !self.refs.drag.active
            && f.hovered_zone.is_none()
            && f.mouse.x >= ref_zone.0
            && f.mouse.x < ref_zone.1
            && f.mouse.y >= y0
            && f.mouse.y < y0 + row_h
            && !shown_groups.is_empty()
        {
            f.hovered_zone = Some(commit.sha.clone());
            self.zone_popover_row = Some(i);
            self.zone_popover_ghost = ghost;
        }

        // Graph zone: the commit's background streak from its node to the zone's end (.commit-bg-color, lane
        // colour at 10%, 50% when selected) ending in the 2px .color-strip.
        if z.w(ZoneType::Graph) > 0.0 {
            let node_center = z.column_x(row.column, &m);
            let graph_end = z.x(ZoneType::Graph) + z.w(ZoneType::Graph);
            let strip_x = graph_end - m.px(2.0);
            // A lane scrolled past the zone's end has no streak in it.
            if node_center < strip_x {
                rows_draw.add_rect_filled(
                    vec2(node_center, inner_top),
                    vec2(strip_x, inner_bottom),
                    lane_color.with_alpha((if selected { 0.5 } else { 0.1 }) * alpha),
                    0.0,
                );
            }
            if !is_wip {
                rows_draw.add_rect_filled(
                    vec2(strip_x, inner_top),
                    vec2(graph_end, inner_bottom),
                    lane_color.with_alpha(alpha),
                    0.0,
                );
            }
        }
        let body_x = z.lanes_x(&m);
        let graph_w = z.w(ZoneType::Graph);
        rows_draw.push_clip_rect(vec2(body_x, y0), vec2(body_x + graph_w, y0 + row_h), true);
        draw_edges(cx, rows_draw, &z, &m, row, y0, alpha);
        {
            // The gutters (.app-bg0 over the scrolled lanes): the left one LeftGutterWidth wide from the zone's
            // start, the right one ending at the zone's end and carrying the row's streak.
            let gutter = m.gutter_width;
            let l = &z.lanes;
            let left_end = body_x + ((gutter + gutter / 2.0 * l.factor) * m.scale).round();
            rows_draw.add_rect_filled(vec2(body_x, y0), vec2(left_end, y0 + row_h), p.app_bg0, 0.0);
            let left_alpha = 0.4
                * (1.0
                    - l.factor
                        .max((1.0 - l.scroll / m.column_width).clamp(0.0, 1.0)));
            gutter_shadow(
                rows_draw,
                m.scale,
                left_end,
                1.0,
                y0,
                y0 + row_h,
                left_alpha,
            );
            let right_w = (1.0 - l.slide_out) * gutter;
            if right_w > 0.0 {
                let right_end = body_x + graph_w;
                let right_start = right_end - (right_w * m.scale).round();
                rows_draw.add_rect_filled(
                    vec2(right_start, y0),
                    vec2(right_end, y0 + row_h),
                    p.app_bg0,
                    0.0,
                );
                rows_draw.add_rect_filled(
                    vec2(right_start, inner_top),
                    vec2(right_end, inner_bottom),
                    lane_color.with_alpha((if selected { 0.5 } else { 0.1 }) * alpha),
                    0.0,
                );
                gutter_shadow(
                    rows_draw,
                    m.scale,
                    right_start,
                    -1.0,
                    y0,
                    y0 + row_h,
                    0.4 * (1.0 - l.factor.max(l.slide_out)),
                );
            }
        }
        if let Some(line) = &ref_line
            && line.max.x > body_x
            && line.min.x < body_x + graph_w
        {
            rows_draw.add_rect_filled(
                vec2(body_x.max(line.min.x), line.min.y),
                vec2((body_x + graph_w).min(line.max.x), line.max.y),
                line.color,
                0.0,
            );
        }
        let node_start = rows_draw.vertex_count();
        draw_node(cx, rows_draw, &z, &m, commit, row, y0, alpha);
        fade_vertices(rows_draw, node_start, z.node_opacity(row.column, &m));
        rows_draw.pop_clip_rect();

        let line_h = f.line_h;
        let text_y = y0 + (row_h - line_h) / 2.0;
        // Message
        if z.w(ZoneType::Message) > 0.0 {
            let message_x = z.x(ZoneType::Message);
            let message_w = z.w(ZoneType::Message);
            let ty = inner_top + (m.px(m.row_inner_height) - line_h) / 2.0;
            let text_x = message_x + m.px(8.0); // .graph-zone { margin-left: 8px }
            if is_wip {
                // .work-dir-input: a 100px cell holding a form-control (rgba(0,0,0,.2), 1px 8% white border,
                // radius 4, 12px horizontal padding) bound to the commit summary; the stats follow 15px on.
                let box_min = vec2(message_x + m.px(1.0), inner_top);
                let box_max = vec2(box_min.x + m.px(97.0), inner_bottom);
                // The zone hides what overflows it (a narrow graph cuts the input and the stats).
                let _clip =
                    ui.push_clip_rect([message_x, y0], [message_x + message_w, y0 + row_h], true);
                cx.set_cursor(box_min);
                let editing = {
                    use dear_imgui_rs::StyleColor;
                    let _pad = ui.push_style_var(StyleVar::FramePadding([
                        m.px(13.0),
                        // (the compact rows are barely taller than the text: ImGui's padding is not negative)
                        ((box_max.y - box_min.y - line_h) / 2.0).max(0.0),
                    ]));
                    let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
                    let _bg = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
                    let _bg_hovered =
                        ui.push_style_color(StyleColor::FrameBgHovered, Color::TRANSPARENT);
                    let _bg_active =
                        ui.push_style_color(StyleColor::FrameBgActive, Color::TRANSPARENT);
                    let _text = ui.push_style_color(StyleColor::Text, p.text_normal);
                    let _font = cx.push_font(cx.fonts.regular, FONT_SIZE_ROW);
                    rows_draw.add_rect_filled(box_min, box_max, p.input_bg, m.px(4.0));
                    cx.input_text(
                        "##wip_summary",
                        &mut state.commit_summary,
                        None,
                        dear_imgui_rs::InputTextFlags::NONE,
                        box_max.x - box_min.x,
                    );
                    ui.is_item_active()
                };
                rows_draw.add_rect(
                    box_min,
                    box_max,
                    if editing { p.accent } else { p.ink_255(20) },
                    m.px(4.0),
                    1.0,
                );
                if state.commit_summary.is_empty() && !editing {
                    cx.draw_text(
                        rows_draw,
                        cx.fonts.regular,
                        FONT_SIZE_ROW,
                        vec2(box_min.x + m.px(13.0), ty),
                        p.text_disabled,
                        "// WIP",
                    );
                }
                self.draw_wip_stats(cx, rows_draw, vec2(message_x + m.px(113.0), ty), state);
            } else {
                let base = dim_text(if selected {
                    p.text_selected
                } else {
                    p.text_normal
                });
                let used = cx.draw_text_ellipsis(
                    rows_draw,
                    cx.fonts.regular,
                    FONT_SIZE_ROW,
                    vec2(text_x, ty),
                    message_w - m.px(13.0),
                    faded(base, alpha),
                    &commit.summary,
                );
                // Show commit description in graph: always, or on the row under the mouse.
                if app.settings.show_description
                    && !commit.body.is_empty()
                    && (!app.settings.description_on_hover
                        || state.hovered_sha.as_deref() == Some(commit.sha.as_str()))
                {
                    // .message-zone--body: the rest of the message on one line, 8px after the summary at 60%
                    // opacity.
                    let body = commit.body.replace('\n', " ");
                    let remaining = message_w - m.px(21.0) - used;
                    if remaining > m.px(20.0) {
                        cx.draw_text_ellipsis(
                            rows_draw,
                            cx.fonts.regular,
                            FONT_SIZE_ROW,
                            vec2(text_x + used + m.px(8.0), ty),
                            remaining,
                            faded(base, 0.6 * alpha),
                            &body,
                        );
                    }
                }
            }
        }
        // Author
        if z.w(ZoneType::Author) > 0.0 {
            let author_x = z.x(ZoneType::Author);
            let author_w = z.w(ZoneType::Author);
            let avatar = m.px(m.avatar_diameter);
            cx.draw_avatar(
                rows_draw,
                vec2(author_x + m.px(10.0) + avatar / 2.0, y0 + row_h / 2.0),
                avatar,
                &commit.author,
                None,
            );
            if author_w > m.px(60.0) {
                cx.draw_text_ellipsis(
                    rows_draw,
                    cx.fonts.regular,
                    FONT_SIZE_ROW,
                    vec2(author_x + m.px(16.0) + avatar, text_y),
                    author_w - avatar - m.px(24.0),
                    dim_text(p.text_normal).with_alpha(alpha),
                    &commit.author.name,
                );
            } else if hovered {
                ui.set_tooltip(format!("{} <{}>", commit.author.name, commit.author.email));
            }
        }
        // Date
        if z.w(ZoneType::DateTime) > 0.0 {
            let date = if is_wip {
                format_relative(commit.time)
            } else {
                cx.format_date_time(commit.time)
            };
            cx.draw_text_ellipsis(
                rows_draw,
                cx.fonts.regular,
                FONT_SIZE_ROW,
                vec2(z.x(ZoneType::DateTime) + m.px(10.0), text_y),
                z.w(ZoneType::DateTime) - m.px(16.0),
                dim_text(p.text_normal).with_alpha(alpha),
                &date,
            );
        }
        // Sha
        if z.w(ZoneType::Sha) > 0.0 && !is_wip {
            let mono_h = cx.text_size(cx.fonts.mono, FONT_SIZE_ROW, "A").y;
            cx.draw_text(
                rows_draw,
                cx.fonts.mono,
                FONT_SIZE_ROW,
                vec2(z.x(ZoneType::Sha) + m.px(10.0), y0 + (row_h - mono_h) / 2.0),
                p.text_secondary.with_alpha(alpha),
                short_sha(&commit.sha),
            );
        }
        // Changes: the working directory's, or the commit's once the background count has it.
        if z.w(ZoneType::Changes) > 0.0 {
            let changes_x = z.x(ZoneType::Changes);
            let at = vec2(changes_x + m.px(8.0), text_y);
            rows_draw.push_clip_rect(
                vec2(changes_x, y0),
                vec2(changes_x + z.w(ZoneType::Changes), y0 + row_h),
                true,
            );
            if is_wip {
                self.draw_wip_stats(cx, rows_draw, at, state);
            } else if let Some(counts) = self.changes.get(state.repo.workdir(), &commit.sha) {
                draw_file_counts(cx, rows_draw, at, counts, alpha);
            } else {
                f.counts_wanted.push(commit.sha.clone());
            }
            rows_draw.pop_clip_rect();
        }

        // The row itself, last: pills submitted above keep the hover when the mouse is on them.
        let row_button_x = (if self
            .zones
            .zones
            .first()
            .is_some_and(|z| z.zone == ZoneType::Ref)
        {
            z.x(ZoneType::Ref) + z.w(ZoneType::Ref)
        } else {
            f.zones_x
        }) - m.px(3.0);
        cx.set_cursor(vec2(row_button_x, y0));
        ui.invisible_button(
            "##row",
            vec2(origin.x + size.x - f.scrollbar - row_button_x, row_h),
        );
        if ui.is_item_clicked() {
            let io = ui.io();
            f.picks.push(Pick::Click {
                sha: commit.sha.clone(),
                toggle: io.key_ctrl(),
                range: io.key_shift(),
            });
        }
        if ui.is_item_clicked_with_button(MouseButton::Right) && !selected {
            f.picks.push(Pick::Only {
                sha: commit.sha.clone(),
                close_diff: false,
            });
        }
        // Windows opens context menus when the right button is released.
        if ui.is_item_hovered() && ui.is_mouse_released(MouseButton::Right) && !f.open_ref_menu {
            self.context_sha.clone_from(&commit.sha);
            f.open_commit_menu = true;
        }
        if ui.is_item_hovered() && ui.is_mouse_double_clicked(MouseButton::Left) && !is_wip {
            app.show_detail_panel = true;
        }
    }

    /// The WIP row's counts: every staged, unstaged and conflicted file.
    fn draw_wip_stats(&self, cx: &mut Cx<'_>, draw: DrawList<'_>, pos: Vec2, state: &RepoState) {
        let counts = FileCounts::of_status(&state.status);
        draw_file_counts(cx, draw, pos, &counts, 1.0);
    }

    /// The inline "enter branch name" / "enter tag name" input in the ref zone of the row at `y0`.
    fn draw_inline_ref(
        &self,
        cx: &mut Cx<'_>,
        slot: &mut Option<InlineRef>,
        f: &mut RowFrame,
        rows_draw: DrawList<'_>,
        y0: f32,
    ) {
        use dear_imgui_rs::{InputTextFlags, StyleColor};
        let ui = cx.ui;
        let p = cx.palette;
        let m = f.m;
        let z = f.z;
        let Some(inline_ref) = slot.as_mut() else {
            return;
        };
        // .form-control in the ref zone: the whole zone and row, rgba(0,0,0,.2), a 1px #4D88FF focus border,
        // radius 4, 12px text 12px in. With the ref zone hidden, the input takes the first column's place.
        let box_zone = if z.w(ZoneType::Ref) > 0.0 {
            ZoneType::Ref
        } else {
            self.zones
                .zones
                .first()
                .map_or(ZoneType::Ref, |zone| zone.zone)
        };
        let min = vec2(z.x(box_zone) - m.px(2.0), y0);
        let max = vec2(min.x + z.w(box_zone) + m.px(1.0), y0 + f.row_h);
        rows_draw.add_rect_filled(min, max, p.app_bg0, m.px(4.0));
        rows_draw.add_rect_filled(min, max, p.input_bg, m.px(4.0));
        cx.set_cursor(min);
        let text_h = f.line_h;
        let submitted;
        let cancelled;
        {
            let _pad = ui.push_style_var(StyleVar::FramePadding([
                m.px(13.0),
                ((f.row_h - text_h) / 2.0).max(0.0),
            ]));
            let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
            let _bg = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
            let _bg_hovered = ui.push_style_color(StyleColor::FrameBgHovered, Color::TRANSPARENT);
            let _bg_active = ui.push_style_color(StyleColor::FrameBgActive, Color::TRANSPARENT);
            let _text = ui.push_style_color(StyleColor::Text, p.text_normal);
            let _font = cx.push_font(cx.fonts.regular, FONT_SIZE_ROW);
            if inline_ref.focus {
                ui.set_keyboard_focus_here();
                inline_ref.focus = false;
            }
            submitted = cx.input_text(
                "##inline_ref",
                &mut inline_ref.name,
                None,
                InputTextFlags::ENTER_RETURNS_TRUE,
                max.x - min.x,
            );
            cancelled = ui.is_item_deactivated() && !submitted;
        }
        // A name git would read as an option on a command line is refused here, as in the dialogs: the input
        // stays open with a red border and says why.
        let error = panels::dialogs::leading_dash_error(
            if inline_ref.tag { "tag" } else { "branch" },
            &inline_ref.name,
        );
        if let Some(error) = &error {
            cx.styled_tooltip(min, max, error, false);
        }
        rows_draw.add_rect(
            min,
            max,
            if error.is_some() { p.red } else { p.accent },
            m.px(4.0),
            1.0,
        );
        if inline_ref.name.is_empty() {
            rows_draw.push_clip_rect(min, vec2(max.x - m.px(13.0), max.y), true);
            cx.draw_text(
                rows_draw,
                cx.fonts.regular,
                FONT_SIZE_ROW,
                vec2(min.x + m.px(13.0), y0 + (f.row_h - text_h) / 2.0),
                p.text_secondary,
                if inline_ref.tag {
                    "enter tag name"
                } else {
                    "enter branch name"
                },
            );
            rows_draw.pop_clip_rect();
        }
        if submitted && error.is_some() {
            inline_ref.focus = true;
        } else if submitted && !inline_ref.name.is_empty() {
            f.deferred.inline_done = Some(inline_ref.clone());
            *slot = None;
        } else if cancelled || submitted || ui.is_key_pressed(Key::Escape) {
            *slot = None;
        }
    }
}

/// The first row past each age step, with the age in words. Rows out of date order never step back.
fn timeline_markers(state: &RepoState) -> Vec<(usize, String)> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        });
    markers_at(&state.history.commits, now, format_relative)
}

/// [`timeline_markers`] for the time `now`, labelling with `label`.
fn markers_at(commits: &[Commit], now: i64, label: impl Fn(i64) -> String) -> Vec<(usize, String)> {
    let mut reached = 0;
    let mut markers = Vec::new();
    for (row, commit) in commits.iter().enumerate() {
        if commit.node_type == NodeType::WorkDir {
            continue;
        }
        let bucket = timeline_bucket((now - commit.time).max(0));
        if bucket > reached {
            reached = bucket;
            markers.push((row, label(commit.time)));
        }
    }
    markers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(time: i64, node_type: NodeType) -> Commit {
        Commit {
            time,
            node_type,
            ..Commit::default()
        }
    }

    #[test]
    fn markers_start_at_the_first_row_of_each_age_step() {
        const HOUR: i64 = 3600;
        let now = 100 * HOUR;
        let commits = [
            commit(now, NodeType::WorkDir),
            commit(now - 10, NodeType::Commit),
            commit(now - 20, NodeType::Commit),
            commit(now - 2 * HOUR - 5, NodeType::Commit),
            commit(now - 2 * HOUR - 6, NodeType::Commit),
            // Out of date order: never steps back.
            commit(now - HOUR - 1, NodeType::Commit),
            commit(now - 5 * HOUR, NodeType::Commit),
        ];
        let rows: Vec<usize> = markers_at(&commits, now, |t| t.to_string())
            .into_iter()
            .map(|(row, _)| row)
            .collect();
        assert_eq!(rows, [3, 6]);
    }

    #[test]
    fn lerp_blends_every_channel() {
        let mixed = lerp(
            Color::new(0.0, 0.0, 0.0, 0.0),
            Color::new(1.0, 0.5, 0.25, 1.0),
            0.5,
        );
        assert_eq!(mixed, Color::new(0.5, 0.25, 0.125, 0.5));
        let same = lerp(Color::WHITE, Color::BLACK, 0.0);
        assert_eq!(same, Color::WHITE);
    }
}
