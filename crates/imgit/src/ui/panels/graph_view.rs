//! The commit graph: the column header (drag to reorder / resize, the header menu), the rows with their ref
//! pills, lanes and nodes, the commit search bar, selection, hover highlights, timeline markers and the
//! sideways scroll of the graph zone.
//!
//! Ports `src/ui/graph_view.cpp`.

mod changes;
mod lanes;
mod refs;
mod rows;
mod search_bar;
mod zones;

use dear_imgui_rs::{ChildFlags, MouseButton, StyleColor, WindowFlags, sys};
use imgit_app::RepoState;
use imgit_git::Ref;

use self::changes::ChangeCounts;
use self::refs::{RefGroupKey, RefInteraction};
use self::zones::{Metrics, ZoneLayout, ZoneType, Zones};
use crate::app::application::Application;
use crate::theme::{GRAPH_HEADER_HEIGHT, GRAPH_HEADER_MARGIN_BOTTOM, GRAPH_LEFT_MARGIN};
use crate::ui::draw::DrawList;
use crate::ui::geom::{Rect, Vec2, vec2};
use crate::ui::menu::popup_menu;
use crate::ui::panels::menus::graph_header_menu;
use crate::ui::widgets::ResizeAxis;
use crate::ui::{Cx, panels};
use crate::window::CursorShape;

/// The zone whose right edge is being dragged (re-resizable: its width when pressed plus the mouse's travel).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ZoneDrag {
    index: usize,
    start_width: f32,
    start_mouse: f32,
}

/// A header cell held and moved 4px becomes a column drag: the cell follows the mouse as a floating copy and the
/// zone swaps places with a neighbour once the cell's leading edge passes that neighbour's centre (its centre
/// would have to travel half its own width, far for the message column); the order is saved on release.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ZoneMove {
    zone: ZoneType,
    active: bool,
    press_x: f32,
    /// The press, from the cell's left edge.
    grab: f32,
    /// The cell.
    width: f32,
}

/// The graph's state between frames (what the C++ kept in file-level and function-local statics).
#[derive(Default)]
pub struct State {
    zones: ZoneLayout,
    zone_drag: Option<ZoneDrag>,
    zone_move: Option<ZoneMove>,
    /// The graph zone's scrollLeft, CSS px.
    graph_scroll: f32,
    refs: RefInteraction,
    context_sha: String,
    context_ref: Option<Ref>,
    /// For dimming the rows it does not reach.
    hovered_group: Option<RefGroupKey>,
    /// The row whose ref zone is hovered lists all its ref groups in a popover over the rows below.
    hovered_zone_sha: Option<String>,
    zone_popover_ghost: bool,
    zone_popover_row: Option<usize>,
    dimming_for: Option<RefGroupKey>,
    dim_since: f64,
    /// The search input takes the focus back after Enter.
    search_refocus: bool,
    changes: ChangeCounts,
}

/// What the graph asks for once its rows are drawn: they read the history, and these change it.
#[derive(Default)]
struct Deferred {
    checkout: Option<Ref>,
    show_more: bool,
    inline_done: Option<imgit_app::repo_state::InlineRef>,
}

/// The measures of a frame of the header: where everything is, in device px.
struct Geometry {
    m: Metrics,
    origin: Vec2,
    size: Vec2,
    scrollbar: f32,
    gear_w: f32,
    header_y: f32,
    header_h: f32,
    zones_x: f32,
    z: Zones,
}

/// The graph, `size` device px.
pub fn draw_graph_view(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, size: Vec2) {
    app.with_panel_state(
        |panels| &mut panels.graph_view,
        |app, graph| graph.draw(cx, app, state, size),
    );
}

/// `ImGui::GetStyle().ScrollbarSize` and `ItemSpacing.x`.
fn style_sizes(cx: &Cx<'_>) -> (f32, f32) {
    let style = cx.ui.clone_style();
    (style.scrollbar_size(), style.item_spacing()[0])
}

impl State {
    /// Whether background work a screenshot should wait for is running (the CHANGES column's counts).
    #[must_use]
    pub fn busy(&self) -> bool {
        self.changes.busy()
    }

    fn draw(&mut self, cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, size: Vec2) {
        let ui = cx.ui;
        let origin = cx.cursor();
        let deferred = ui
            .child_window("##graph_view")
            .size(size.into())
            .child_flags(ChildFlags::NONE)
            .flags(WindowFlags::NO_SCROLLBAR)
            .build(ui, || self.draw_content(cx, app, state, origin, size))
            .flatten();
        // Without a load error: the search bar floats over the graph.
        let Some(deferred) = deferred else {
            return;
        };
        search_bar::commit_search_bar(cx, state, &mut self.search_refocus, origin, size);

        if let Some(r) = deferred.checkout {
            let _ = panels::checkout_ref(cx, app, state, &r);
        }
        if deferred.show_more {
            state.commit_limit += 1000;
            state.refresh();
        }
        if let Some(inline) = deferred.inline_done {
            if inline.tag {
                panels::create_tag_at(cx, app, state, &inline.name, &inline.sha, "");
            } else {
                panels::create_branch_at(
                    cx,
                    app,
                    state,
                    &inline.name,
                    &inline.sha,
                    inline.checkout,
                );
            }
        }
    }

    fn draw_content(
        &mut self,
        cx: &mut Cx<'_>,
        app: &mut Application,
        state: &mut RepoState,
        origin: Vec2,
        size: Vec2,
    ) -> Option<Deferred> {
        let ui = cx.ui;
        let p = cx.palette;
        if let Some(error) = &state.load_error {
            cx.set_cursor(origin + vec2(cx.px(20.0), cx.px(20.0)));
            let _color = ui.push_style_color(StyleColor::Text, p.red);
            ui.text_wrapped(error);
            return None;
        }

        let m = Metrics::new(app.settings.compact_graph, cx.scale);
        let header_h = cx.px(GRAPH_HEADER_HEIGHT);
        let (scrollbar, _) = style_sizes(cx);
        let gear_w = cx.px(26.0);
        // The zones fill the graph from the rows' left edge, 4px left of the first header cell, to the panel's
        // right edge; the last one ends at the column settings button for the headers and rows drawn here.
        let zones_x = origin.x + cx.px(GRAPH_LEFT_MARGIN);
        let width = ((size.x - cx.px(GRAPH_LEFT_MARGIN - 4.0)) / cx.scale).floor();
        if self
            .zones
            .update(&app.settings, width, state.layout.max_columns, &m)
        {
            self.zone_drag = None;
        }
        self.apply_zone_drag(cx, app);
        let mut z = Zones::place(&self.zones, zones_x, cx.scale);
        z.extend_last(
            &self.zones,
            origin.x + size.x - scrollbar - gear_w,
            cx.scale,
        );
        let mut g = Geometry {
            m,
            origin,
            size,
            scrollbar,
            gear_w,
            header_y: origin.y,
            header_h,
            zones_x,
            z,
        };
        self.draw_header(cx, app, &mut g);

        let rows_top = g.header_y + header_h + cx.px(GRAPH_HEADER_MARGIN_BOTTOM);
        cx.set_cursor(vec2(origin.x, rows_top));
        ui.child_window("##graph_rows")
            .size([size.x, origin.y + size.y - rows_top])
            .build(ui, || self.draw_rows(cx, app, state, &mut g, rows_top))
    }

    /// Follows the mouse while a zone's edge is held; on release the zone keeps the width as its preferred one
    /// and the column resize saves it.
    fn apply_zone_drag(&mut self, cx: &Cx<'_>, app: &mut Application) {
        let Some(drag) = self.zone_drag else {
            return;
        };
        if drag.index + 1 >= self.zones.zones.len() {
            self.zone_drag = None;
            return;
        }
        let zone = self.zones.zones[drag.index];
        if cx.ui.is_mouse_down(MouseButton::Left) {
            let travel = (cx.mouse_pos().x - drag.start_mouse) / cx.scale;
            let width = zone
                .minimum
                .max((drag.start_width + travel).min(self.zones.max_zone_width(drag.index)));
            if width != zone.current {
                self.zones.resize_zone(drag.index, width, false);
            }
            return;
        }
        if zone.current != drag.start_width {
            self.zones.resize_zone(drag.index, zone.current, true);
            app.settings
                .graph_column_widths
                .insert(zone.zone.key().to_owned(), zone.current);
            app.settings.save(&app.config_path("settings.txt"));
        }
        self.zone_drag = None;
    }

    /// The handles on the right edge of every zone but the last: `header` ones fill the header cell's last 10px
    /// and reach 4px over the border into the next cell, so hovering the border shows the resize cursor rather
    /// than the next cell's grab hand; the others, over the rows, are 10px centred 4px left of it (the rows sit
    /// 4px left of the header).
    fn zone_handles(&mut self, cx: &mut Cx<'_>, x: f32, top: f32, bottom: f32, header: bool) {
        for i in 0..self.zones.zones.len().saturating_sub(1) {
            let right =
                zone_right(&self.zones, x, i, cx.scale) - if header { 0.0 } else { cx.px(4.0) };
            let left_reach = if header { cx.px(10.0) } else { cx.px(5.0) };
            let right_reach = if header { cx.px(4.0) } else { cx.px(5.0) };
            let key = self.zones.zones[i].zone.key();
            let id = format!(
                "{}{key}",
                if header {
                    "##zone_header_resize_"
                } else {
                    "##zone_resize_"
                }
            );
            let handle = cx.resize_handle(
                &id,
                vec2(right - left_reach, top),
                vec2(right + right_reach, bottom),
                ResizeAxis::Columns,
            );
            if handle.activated {
                self.zone_drag = Some(ZoneDrag {
                    index: i,
                    start_width: self.zones.zones[i].current,
                    start_mouse: cx.mouse_pos().x,
                });
            }
        }
    }

    fn draw_header_cells(&self, cx: &Cx<'_>, draw: DrawList<'_>, g: &Geometry) {
        draw.add_rect_filled(
            vec2(g.origin.x, g.header_y),
            vec2(g.origin.x + g.size.x, g.header_y + g.header_h),
            cx.palette.panel_bg0,
            0.0,
        );
        for (i, zone) in self.zones.zones.iter().enumerate() {
            let ty = zone.zone;
            header_cell(
                cx,
                draw,
                (g.z.x(ty), g.z.w(ty)),
                (g.header_y, g.header_h),
                header_label(&g.z, &g.m, ty),
                i > 0,
            );
        }
    }

    fn draw_header(&mut self, cx: &mut Cx<'_>, app: &mut Application, g: &mut Geometry) {
        let ui = cx.ui;
        let p = cx.palette;
        let draw = cx.window_draw_list();
        let (origin, size, header_y, header_h) = (g.origin, g.size, g.header_y, g.header_h);
        // Header (.panel-bg0 .graph-header)
        self.draw_header_cells(cx, draw, g);
        self.zone_handles(cx, g.zones_x, header_y, header_y + header_h, true);
        {
            // .columns-settings: far:cog (12px, text-disabled) 9px from the right edge; it opens the same native
            // menu as a right-click on the header.
            let gear_min = vec2(origin.x + size.x - cx.px(16.0), header_y);
            cx.set_cursor(gear_min);
            let gear = ui.invisible_button("##column_settings", vec2(cx.px(16.0), header_h));
            let gear_hovered = ui.is_item_hovered();
            cx.draw_fa_icon(
                draw,
                "far:cog",
                vec2(origin.x + size.x - cx.px(9.0), header_y + cx.px(12.17)),
                cx.px(12.0),
                if gear_hovered {
                    p.text_selected
                } else {
                    p.text_disabled
                },
            );
            if gear_hovered {
                cx.styled_tooltip(
                    gear_min,
                    vec2(gear_min.x + cx.px(16.0), header_y + header_h),
                    "Column settings",
                    true,
                );
            }
            if gear {
                let items = graph_header_menu(cx, app);
                popup_menu(cx, items, None);
            }
        }
        // The header cells, after the resize handles and the gear so those win the hover: pressing one and moving
        // 4px drags the column.
        {
            let mouse = cx.mouse_pos();
            for i in 0..self.zones.zones.len() {
                let ty = self.zones.zones[i].zone;
                let (x, w) = (g.z.x(ty), g.z.w(ty));
                if w <= 0.0 {
                    continue;
                }
                cx.set_cursor(vec2(x, header_y));
                ui.invisible_button(format!("##zone_header_{}", ty.key()), vec2(w, header_h));
                // cursor: grab over the header cells. `is_item_hovered()` goes by the rectangle alone, so it would
                // also say yes over the resize handle at the cell's edge, which was submitted first and owns the
                // hover: only the cell that owns it shows the hand.
                if ui.is_item_hovered() && cx.hovered_id() == ui.item_id().raw() {
                    cx.set_drag_cursor(CursorShape::Grab);
                }
                if ui.is_item_activated() && self.zone_move.is_none() {
                    self.zone_move = Some(ZoneMove {
                        zone: ty,
                        active: false,
                        press_x: mouse.x,
                        grab: mouse.x - x,
                        width: w,
                    });
                }
            }
            if self.zone_move.is_some() && !ui.is_mouse_down(MouseButton::Left) {
                if self.zone_move.is_some_and(|zone_move| zone_move.active) {
                    app.settings.save(&app.config_path("settings.txt"));
                }
                self.zone_move = None;
            }
            if let Some(zone_move) = &mut self.zone_move {
                if !zone_move.active && (mouse.x - zone_move.press_x).abs() >= cx.px(4.0) {
                    zone_move.active = true;
                }
                cx.set_drag_cursor(CursorShape::Grab);
            }
            if let Some(zone_move) = self.zone_move.filter(|zone_move| zone_move.active) {
                // The floating cell's edges against the neighbours' centres: past one, the zones swap and the
                // layout is placed again for the rows below.
                let left_edge = mouse.x - zone_move.grab;
                let right_edge = left_edge + zone_move.width;
                let mut moved = false;
                for _ in 0..self.zones.zones.len() {
                    let Some(index) = self.zones.index_of(zone_move.zone) else {
                        break;
                    };
                    let placed = Zones::place(&self.zones, g.zones_x, cx.scale);
                    let centre = |ty: ZoneType| placed.x(ty) + placed.w(ty) / 2.0;
                    let mut to = index;
                    if index > 0 && left_edge < centre(self.zones.zones[index - 1].zone) {
                        to = index - 1;
                    }
                    if to == index
                        && index + 1 < self.zones.zones.len()
                        && right_edge > centre(self.zones.zones[index + 1].zone)
                    {
                        to = index + 1;
                    }
                    if to == index {
                        break;
                    }
                    if let Some(order) = self.zones.move_zone(&app.settings, index, to) {
                        app.settings.graph_column_order = order;
                        moved = true;
                    }
                }
                if moved {
                    // Placed again, with the last zone's trailing room as above.
                    g.z = Zones::place(&self.zones, g.zones_x, cx.scale);
                    g.z.extend_last(
                        &self.zones,
                        origin.x + size.x - g.scrollbar - g.gear_w,
                        cx.scale,
                    );
                    self.draw_header_cells(cx, draw, g);
                    cx.draw_fa_icon(
                        draw,
                        "far:cog",
                        vec2(origin.x + size.x - cx.px(9.0), header_y + cx.px(12.17)),
                        cx.px(12.0),
                        p.text_disabled,
                    );
                }
                // The dragged cell, over everything: the header's background and label following the mouse.
                let front = cx.foreground_draw_list();
                let fx = (mouse.x - zone_move.grab).round();
                front.add_rect_filled(
                    vec2(fx, header_y),
                    vec2(fx + zone_move.width, header_y + header_h),
                    p.panel_bg2.with_alpha(0.95),
                    cx.px(2.0),
                );
                front.add_rect(
                    vec2(fx, header_y),
                    vec2(fx + zone_move.width, header_y + header_h),
                    p.section_border,
                    cx.px(2.0),
                    1.0,
                );
                header_cell(
                    cx,
                    front,
                    (fx, zone_move.width),
                    (header_y, header_h),
                    header_label(&g.z, &g.m, zone_move.zone),
                    false,
                );
            }
        }

        if ui.is_window_hovered_with_flags(dear_imgui_rs::WindowHoveredFlags::CHILD_WINDOWS)
            && ui.is_mouse_released(MouseButton::Right)
            && Rect::new(
                vec2(origin.x, header_y),
                vec2(origin.x + size.x, header_y + header_h),
            )
            .contains(cx.mouse_pos())
        {
            let items = graph_header_menu(cx, app);
            popup_menu(cx, items, None);
        }
    }
}

/// The right edge of the shown zone `index`, in device px from `x`.
fn zone_right(layout: &ZoneLayout, x: f32, index: usize, scale: f32) -> f32 {
    let edge: f32 = layout.zones[..=index].iter().map(|zone| zone.current).sum();
    x + (scale * edge).round()
}

/// The label of a header cell: the AUTHOR one goes when the zone is too narrow for more than the avatar.
fn header_label(z: &Zones, m: &Metrics, zone: ZoneType) -> &'static str {
    if zone == ZoneType::Author && z.w(zone) <= m.px(60.0) {
        ""
    } else {
        zone.label()
    }
}

/// A graph header cell: ".fs-1 .ml1 .font-monospace" in text-disabled, a 1px section border on its left.
fn header_cell(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    (x, w): (f32, f32),
    (y, h): (f32, f32),
    label: &str,
    border: bool,
) {
    let p = cx.palette;
    if w <= 0.0 {
        return;
    }
    // Baseline 15.5px below the header's top edge, for the 10px Consolas line.
    let ascent = (cx.px(10.0) * 1884.0 / 2048.0).ceil();
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.mono,
        10.0,
        vec2(x + cx.px(6.0), y + cx.px(15.5) - ascent),
        w - cx.px(10.0),
        p.text_disabled,
        label,
    );
    if border {
        // The previous zone's resize handle has the 1px border-right, just left of this cell.
        draw.add_rect_filled(
            vec2(x - cx.px(1.0), y),
            vec2(x, y + h),
            p.section_border,
            0.0,
        );
    }
}

/// The timeline bucket of a commit `age` seconds old, on format_relative's steps: 0 within the hour, then rising
/// with each hour, "yesterday", day, week, month and year it reads.
fn timeline_bucket(age: i64) -> i64 {
    const HOUR: i64 = 3600;
    const DAY: i64 = 86400;
    if age < HOUR {
        0
    } else if age < DAY {
        age / HOUR // 1..23
    } else if age < DAY * 2 {
        24 // yesterday
    } else if age < DAY * 7 {
        23 + age / DAY // 25..29
    } else if age < DAY * 30 {
        29 + age / (DAY * 7) // 30..33
    } else if age < DAY * 365 {
        33 + age / (DAY * 30) // 34..45
    } else {
        45 + (age / (DAY * 365)).min(1000)
    }
}

/// The marks along the rows' vertical scrollbar where the branches sit, at their rows' share of the list: a short
/// tick in the lane colour per local branch (half as strong for remote ones), the checked-out commit a full-width
/// bar in the accent and the selected commits a thin bar in the text colour. Call inside the rows' window, after
/// its rows.
fn draw_scroll_markers(cx: &Cx<'_>, state: &RepoState, row_h: f32, list_rows: usize) {
    // SAFETY: a window is current while its contents are built, which is where this is called; the window is only
    // read (its scrollbar flag), and the rectangle is returned by value.
    let track = unsafe {
        let window = sys::igGetCurrentWindow();
        if window.is_null() || !(*window).ScrollbarY || list_rows == 0 {
            return;
        }
        sys::igGetWindowScrollbarRect(window, sys::ImGuiAxis_Y as sys::ImGuiAxis)
    };
    let (track_min, track_max) = (
        vec2(track.Min.x, track.Min.y),
        vec2(track.Max.x, track.Max.y),
    );
    let p = cx.palette;
    let height = track_max.y - track_min.y;
    let content = (list_rows as f32 * row_h).max(height);
    let scale = height / content;
    let draw = cx.window_draw_list();
    draw.push_clip_rect(track_min, track_max, false);
    let mark_at = |row: usize, inset: f32, mark_height: f32, color| {
        let centre = track_min.y + (row as f32 + 0.5) * row_h * scale;
        let top = (centre - mark_height / 2.0)
            .clamp(track_min.y, (track_max.y - mark_height).max(track_min.y))
            .round();
        draw.add_rect_filled(
            vec2(track_min.x + inset, top),
            vec2(track_max.x - inset, top + mark_height),
            color,
            0.0,
        );
    };
    let row_of = |sha: &str| state.history.row_by_sha.get(sha).copied();
    for r in &state.history.refs {
        use imgit_git::RefType;
        if r.ref_type != RefType::LocalBranch && r.ref_type != RefType::RemoteBranch {
            continue;
        }
        let Some(row) = row_of(&r.target).filter(|&row| row < state.layout.rows.len()) else {
            continue;
        };
        let lane = p.graph_color(state.layout.rows[row].column as i32);
        let local = r.ref_type == RefType::LocalBranch;
        mark_at(
            row,
            cx.px(2.0),
            cx.px(2.0),
            lane.with_alpha(if local { 1.0 } else { 0.5 }),
        );
    }
    for sha in &state.selected {
        if let Some(row) = row_of(sha) {
            mark_at(row, 0.0, cx.px(1.0), p.text_normal);
        }
    }
    if let Some(head) = state.history.head_sha.as_deref().and_then(row_of) {
        mark_at(head, 0.0, cx.px(3.0), p.accent);
    }
    draw.pop_clip_rect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_buckets_step_with_the_age() {
        const HOUR: i64 = 3600;
        const DAY: i64 = 86400;
        assert_eq!(timeline_bucket(0), 0);
        assert_eq!(timeline_bucket(HOUR - 1), 0);
        assert_eq!(timeline_bucket(HOUR), 1);
        assert_eq!(timeline_bucket(DAY - 1), 23);
        assert_eq!(timeline_bucket(DAY), 24);
        assert_eq!(timeline_bucket(2 * DAY - 1), 24);
        assert_eq!(timeline_bucket(2 * DAY), 25);
        assert_eq!(timeline_bucket(6 * DAY), 29);
        assert_eq!(timeline_bucket(7 * DAY), 30);
        assert_eq!(timeline_bucket(29 * DAY), 33);
        assert_eq!(timeline_bucket(30 * DAY), 34);
        assert_eq!(timeline_bucket(364 * DAY), 45);
        assert_eq!(timeline_bucket(365 * DAY), 46);
        assert_eq!(timeline_bucket(i64::MAX / 2), 45 + 1000);
        // Buckets never fall as the age grows.
        let mut last = 0;
        for age in (0..400 * DAY).step_by(3600) {
            let bucket = timeline_bucket(age);
            assert!(bucket >= last);
            last = bucket;
        }
    }

    #[test]
    fn zone_edges_accumulate_the_widths() {
        let layout = ZoneLayout {
            zones: [(ZoneType::Ref, 100.0), (ZoneType::Graph, 50.5)]
                .iter()
                .map(|&(zone, current)| zones::GraphZone {
                    zone,
                    minimum: 0.0,
                    maximum: 0.0,
                    current,
                    preferred: current,
                })
                .collect(),
            width: 150.5,
            content: 0.0,
        };
        assert_eq!(zone_right(&layout, 10.0, 0, 1.5), 160.0);
        assert_eq!(
            zone_right(&layout, 10.0, 1, 1.5),
            10.0 + (1.5f32 * 150.5).round()
        );
    }

    #[test]
    fn narrow_author_zones_lose_their_label() {
        let m = Metrics::rich(1.0);
        let mut z = Zones::default();
        z.rect_mut(ZoneType::Author).w = 60.0;
        assert_eq!(header_label(&z, &m, ZoneType::Author), "");
        z.rect_mut(ZoneType::Author).w = 61.0;
        assert_eq!(header_label(&z, &m, ZoneType::Author), "AUTHOR");
        assert_eq!(header_label(&z, &m, ZoneType::Sha), "SHA");
    }
}
