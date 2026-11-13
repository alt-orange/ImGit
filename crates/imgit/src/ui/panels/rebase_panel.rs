//! The pending interactive rebase, in place of the left panel and the graph: a 36px header ("Interactive
//! Rebase", "Rebasing <branch> onto <base>", Cancel Rebase), one 32px row per commit (newest first: the action
//! select, the commit's node and streak in its lane colour, the message) and a footer with the shortcuts and
//! Reset / Cancel Rebase / Start Rebase. Keys on the selected row: P / S / R / D set its action, Up / Down select,
//! Ctrl + Up / Down move it. Positions are CSS px.

use std::collections::HashMap;
use std::path::PathBuf;

use dear_imgui_rs::{
    Condition, InputTextFlags, Key, MouseButton, StyleColor, StyleVar, WindowFlags,
};
use imgit_app::RepoState;
use imgit_app::repo_state::{RebasePlan, UndoEntry};
use imgit_git::{RebaseAction, RebaseStep, ResetMode};

use crate::app::application::Application;
use crate::app::toasts::ToastKind;
use crate::fonts::Font;
use crate::theme::{Color, Palette};
use crate::ui::icons::svg_path;
use crate::ui::{Cx, DrawList, Vec2, raw, vec2};

const HEADER_HEIGHT: f32 = 36.0;
const FOOTER_HEIGHT: f32 = 57.140_625;
const PADDING: f32 = 20.0;
/// 4px padding around the 24px controls.
const ROW_HEIGHT: f32 = 32.0;
const SELECT_WIDTH: f32 = 90.0;
/// After the select and its 20px margin.
const NODE_X: f32 = 110.0;
/// The node zone is 60px wide.
const MESSAGE_X: f32 = 170.0;
const REWORD_WIDTH: f32 = 400.0;
const REWORD_HEIGHT: f32 = 200.0;

const ACTION_LABELS: [&str; 4] = ["Pick", "Reword", "Squash", "Drop"];
const MENU_ID: &str = "##rebase_action_menu";

/// The four things a plan step can do, without the message the reword and squash carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Pick,
    Reword,
    Squash,
    Drop,
}

const KINDS: [Kind; 4] = [Kind::Pick, Kind::Reword, Kind::Squash, Kind::Drop];

impl Kind {
    fn of(action: &RebaseAction) -> Self {
        match action {
            RebaseAction::Pick => Self::Pick,
            RebaseAction::Reword(_) => Self::Reword,
            RebaseAction::Squash(_) => Self::Squash,
            RebaseAction::Drop => Self::Drop,
        }
    }

    fn label(self) -> &'static str {
        ACTION_LABELS[self as usize]
    }

    /// The action of this kind carrying `message`.
    fn action(self, message: Option<String>) -> RebaseAction {
        match self {
            Self::Pick => RebaseAction::Pick,
            Self::Reword => RebaseAction::Reword(message),
            Self::Squash => RebaseAction::Squash(message),
            Self::Drop => RebaseAction::Drop,
        }
    }
}

/// The message a step's action carries.
fn action_message(action: &RebaseAction) -> Option<&str> {
    match action {
        RebaseAction::Reword(message) | RebaseAction::Squash(message) => message.as_deref(),
        RebaseAction::Pick | RebaseAction::Drop => None,
    }
}

/// The reword editor's summary / description, split from and joined into the step's message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RewordFields {
    /// The tab the fields belong to: another tab's plan is not written.
    workdir: PathBuf,
    step: usize,
    summary: String,
    description: String,
}

/// Splits a commit message into its summary and its description (the blank lines between them dropped).
fn split_message(message: &str) -> (String, String) {
    match message.split_once('\n') {
        Some((summary, rest)) => (
            summary.to_owned(),
            rest.trim_start_matches(['\n', '\r']).to_owned(),
        ),
        None => (message.to_owned(), String::new()),
    }
}

/// The summary and description joined into a commit message.
fn join_message(summary: &str, description: &str) -> String {
    if description.is_empty() {
        summary.to_owned()
    } else {
        format!("{summary}\n\n{description}")
    }
}

/// What the rebase panel remembers between frames (the C++ file-level statics).
#[derive(Default)]
pub struct State {
    /// The reword editor's fields while it is open.
    reword: Option<RewordFields>,
    /// The step the action menu is open for, and where.
    menu_step: Option<usize>,
    menu_pos: Vec2,
    /// A step's edited message while its action is one that carries none, by tab and commit: switching back to
    /// Reword finds it again.
    remembered: HashMap<(PathBuf, String), String>,
}

/// `text` in a `line_h` tall line box at `line_top` (device px), centred like Chromium.
#[allow(clippy::too_many_arguments)]
fn line_text(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    font: Font,
    size: f32,
    x: f32,
    line_top: f32,
    line_h: f32,
    color: Color,
    text: &str,
) {
    let content = (size * 2189.0 / 2048.0).round() + (size * 600.0 / 2048.0).round();
    let top = line_top + ((line_h - content) / 2.0).floor() * cx.scale;
    let ascent = size * 2189.0 / 2048.0 * cx.scale;
    cx.draw_text(
        draw,
        font,
        size,
        vec2(x.round(), (top + ascent.round()).round() - ascent.ceil()),
        color,
        text,
    );
}

fn text_w(cx: &Cx<'_>, font: Font, size: f32, text: &str) -> f32 {
    cx.text_size(font, size, text).x
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Primary,
    Danger,
    Success,
}

/// What a bootstrap button reports.
struct Pressed {
    clicked: bool,
    width: f32,
}

/// A bootstrap button (12px text, translucent fill, 1px border, radius 2) with its top-right corner at `right`,
/// `top`. `padding_x` / `h` follow .btn (12 / 31.14) or .btn-xs (5 / 22).
#[allow(clippy::too_many_arguments)]
fn button(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    id: &str,
    label: &str,
    right: f32,
    top: f32,
    h: f32,
    padding_x: f32,
    tone: Tone,
) -> Pressed {
    let p = cx.palette;
    let color = match tone {
        Tone::Primary => p.accent,
        Tone::Danger => p.red,
        Tone::Success => p.green,
    };
    let w = text_w(cx, cx.fonts.regular, 12.0, label) + cx.px(padding_x * 2.0 + 2.0);
    let min = vec2((right - w).round(), top);
    let max = vec2(min.x + w, top + h);
    cx.set_cursor(min);
    let clicked = cx.ui.invisible_button(id, [w, h]);
    let hovered = cx.ui.is_item_hovered();
    draw.add_rect_filled(
        min,
        max,
        color.with_alpha(if hovered { 0.3 } else { 0.2 }),
        cx.px(2.0),
    );
    draw.add_rect(min, max, color, cx.px(2.0), 1.0);
    line_text(
        cx,
        draw,
        cx.fonts.regular,
        12.0,
        min.x + cx.px(padding_x + 1.0),
        min.y,
        h,
        if hovered {
            p.text_selected
        } else {
            p.text_normal
        },
        label,
    );
    Pressed { clicked, width: w }
}

/// The lane colour of the commit's node in the graph.
fn lane_color(p: &Palette, state: &RepoState, sha: &str) -> Color {
    let column = state
        .history
        .row_by_sha
        .get(sha)
        .and_then(|&row| state.layout.rows.get(row))
        .map_or(0, |row| row.column);
    p.graph_color(column as i32)
}

fn open_reword(ui: &mut State, state: &mut RepoState, step: usize) {
    let workdir = state.repo.workdir().to_path_buf();
    state.rebase.editing = Some(step);
    let row = &state.rebase.steps[step];
    let mut message = action_message(&row.step.action)
        .unwrap_or_default()
        .to_owned();
    if message.is_empty() {
        message = match state.commit_for(&row.step.sha) {
            Some(commit) if commit.body.is_empty() => commit.summary.clone(),
            Some(commit) => format!("{}\n\n{}", commit.summary, commit.body),
            None => row.summary.clone(),
        };
    }
    let (summary, description) = split_message(&message);
    ui.reword = Some(RewordFields {
        workdir,
        step,
        summary,
        description,
    });
}

fn set_action(ui: &mut State, state: &mut RepoState, step: usize, kind: Kind) {
    let workdir = state.repo.workdir().to_path_buf();
    let row = &mut state.rebase.steps[step];
    // The message follows the step from one action to the next, as the C++ step kept it.
    let key = (workdir, row.step.sha.clone());
    let message = action_message(&row.step.action)
        .map(str::to_owned)
        .or_else(|| ui.remembered.remove(&key));
    if let (Some(message), Kind::Pick | Kind::Drop) = (&message, kind) {
        ui.remembered.insert(key, message.clone());
    }
    row.step.action = kind.action(message);
    if kind == Kind::Reword {
        open_reword(ui, state, step);
    } else if state.rebase.editing == Some(step) {
        state.rebase.editing = None;
    }
}

fn select_step(state: &mut RepoState, step: usize) {
    state.rebase.selected = Some(step);
    let sha = state.rebase.steps[step].step.sha.clone();
    state.select_only(&sha);
}

/// The floating reword editor under row `row_min`: panel-bg1, 5px 10px 10px padding, the summary with its
/// characters-left count over the description. Closes on Escape or a click outside it.
fn draw_reword_editor(cx: &mut Cx<'_>, ui_state: &mut State, state: &mut RepoState, pos: Vec2) {
    let ui = cx.ui;
    let p = cx.palette;
    let Some(step) = state.rebase.editing else {
        return;
    };
    let size = vec2(cx.px(REWORD_WIDTH), cx.px(REWORD_HEIGHT));
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
    ui.window("##reword_editor")
        .position(pos.into(), Condition::Always)
        .size(size.into(), Condition::Always)
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_BACKGROUND,
        )
        .build(|| {
            raw::bring_current_window_to_display_front(ui);
            let draw = cx.window_draw_list();
            let max = pos + size;
            draw.add_rect_filled(
                pos + vec2(2.0, 3.0),
                max + vec2(2.0, 3.0),
                Color::from_rgba8(0, 0, 0, 70),
                cx.px(2.0),
            );
            draw.add_rect_filled(pos, max, p.panel_bg1, cx.px(2.0));

            let workdir = state.repo.workdir();
            let other = ui_state
                .reword
                .as_ref()
                .is_none_or(|fields| fields.workdir != workdir || fields.step != step);
            let appearing = other || ui.is_window_appearing();
            if other {
                open_reword(ui_state, state, step);
            }
            let Some(fields) = ui_state.reword.as_mut() else {
                return;
            };
            let _frame = ui.push_style_color(StyleColor::FrameBg, p.input_bg);
            let _text = ui.push_style_color(StyleColor::Text, p.text_selected);
            let _font = cx.push_font(cx.fonts.regular, 14.0);
            // Summary: 5px 30px 5px 10px padding.
            let inner_w = size.x - cx.px(20.0);
            cx.set_cursor(pos + vec2(cx.px(10.0), cx.px(5.0)));
            let padding = ui.push_style_var(StyleVar::FramePadding([cx.px(10.0), cx.px(5.0)]));
            if appearing {
                ui.set_keyboard_focus_here();
            }
            let entered = cx.input_text(
                "##reword_summary",
                &mut fields.summary,
                Some("Commit summary"),
                InputTextFlags::ENTER_RETURNS_TRUE,
                inner_w - cx.px(20.0),
            );
            let summary_rect = cx.item_rect();
            drop(padding);
            let left = 72 - fields.summary.len() as i64;
            let count = left.to_string();
            let font_px = cx.font_px(cx.fonts.regular, 14.0);
            draw.add_text(
                cx.fonts.regular,
                font_px,
                vec2(
                    summary_rect.min.x + inner_w
                        - cx.px(10.0)
                        - text_w(cx, cx.fonts.regular, 14.0, &count),
                    summary_rect.min.y + (summary_rect.height() - font_px) / 2.0,
                ),
                if left < 0 { p.orange } else { p.text_disabled },
                &count,
            );
            // Description: 5px 10px padding, the rest of the editor.
            cx.set_cursor(vec2(pos.x + cx.px(10.0), summary_rect.max.y + cx.px(5.0)));
            let padding = ui.push_style_var(StyleVar::FramePadding([cx.px(10.0), cx.px(5.0)]));
            cx.input_multiline(
                "##reword_description",
                &mut fields.description,
                Some("Description"),
                vec2(
                    inner_w,
                    max.y - cx.px(10.0) - (summary_rect.max.y + cx.px(5.0)),
                ),
                false,
            );
            drop(padding);

            let message = join_message(&fields.summary, &fields.description);
            let row = &mut state.rebase.steps[step];
            if let RebaseAction::Reword(current) = &mut row.step.action {
                *current = Some(message).filter(|message| !message.is_empty());
            }

            let outside = ui.is_mouse_clicked(MouseButton::Left)
                && !ui.is_mouse_hovering_rect(pos.into(), max.into())
                && !appearing;
            if entered || outside || ui.is_key_pressed(Key::Escape) {
                state.rebase.editing = None;
                ui_state.reword = None;
            }
        });
}

/// The rebase panel for `state.rebase`, `size` device px.
pub fn draw_rebase_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    size: Vec2,
) {
    app.with_panel_state(
        |panels| &mut panels.rebase_panel,
        |app, ui_state| draw_panel(cx, app, ui_state, state, size),
    );
}

/// What the panel's buttons asked for.
#[derive(Default)]
struct Requests {
    cancel: bool,
    start: bool,
}

fn draw_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    ui_state: &mut State,
    state: &mut RepoState,
    size: Vec2,
) {
    let ui = cx.ui;
    let origin = cx.cursor();
    let mut requests = Requests::default();
    ui.child_window("##rebase_panel")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || {
            draw_contents(cx, app, ui_state, state, &mut requests, origin, size);
        });
    if requests.cancel {
        state.rebase = RebasePlan::default();
        ui_state.reword = None;
        return;
    }
    if requests.start {
        start_rebase(app, ui_state, state);
    }
}

/// Start Rebase: rewrites the history, with an undo entry that resets to where HEAD was.
fn start_rebase(app: &mut Application, ui_state: &mut State, state: &mut RepoState) {
    let base = state.rebase.base.clone();
    let steps: Vec<RebaseStep> = state
        .rebase
        .steps
        .iter()
        .map(|row| row.step.clone())
        .collect();
    let previous = state.history.head_sha.clone();
    match state.repo.rewrite_history(&base, &steps) {
        Err(error) => {
            app.toasts.push_with_detail(
                ToastKind::Error,
                "Interactive rebase failed",
                error.to_string(),
            );
        }
        Ok(()) => {
            if let Some(previous) = previous {
                state.push_undo(UndoEntry::new("interactive rebase", move |repo| {
                    repo.reset(&previous, ResetMode::Hard)
                }));
            }
            app.toasts.push(ToastKind::Success, "Rebase complete");
            state.rebase = RebasePlan::default();
            ui_state.reword = None;
        }
    }
    state.refresh();
}

#[allow(clippy::too_many_lines)]
fn draw_contents(
    cx: &mut Cx<'_>,
    app: &mut Application,
    ui_state: &mut State,
    state: &mut RepoState,
    requests: &mut Requests,
    origin: Vec2,
    size: Vec2,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let count = state.rebase.steps.len();
    let draw = cx.window_draw_list();
    draw.add_rect_filled(origin, origin + size, p.app_bg0, 0.0);
    if state.rebase.selected.is_none() && count > 0 {
        select_step(state, count - 1); // the newest, at the top
    }

    // Header: panel-bg0, 0 20px padding, a 1px section border under it.
    {
        let hmax = vec2(origin.x + size.x, origin.y + cx.px(HEADER_HEIGHT));
        draw.add_rect_filled(origin, hmax, p.panel_bg0, 0.0);
        draw.add_rect_filled(vec2(origin.x, hmax.y - 1.0), hmax, p.section_border, 0.0);
        let title = "Interactive Rebase";
        let mut x = origin.x + cx.px(PADDING);
        line_text(
            cx,
            draw,
            cx.fonts.semibold,
            14.0,
            x,
            origin.y + cx.px(7.5),
            cx.px(20.0),
            p.text_normal,
            title,
        );
        x += text_w(cx, cx.fonts.semibold, 14.0, title) + cx.px(20.0);
        let line_top = origin.y + cx.px(8.92);
        let line_h = cx.px(17.140_625);
        let words = |x: &mut f32, label: &str| {
            line_text(
                cx,
                draw,
                cx.fonts.regular,
                12.0,
                *x,
                line_top,
                line_h,
                p.text_normal,
                label,
            );
            *x += text_w(cx, cx.fonts.regular, 12.0, label) + cx.px(5.0);
        };
        // The branch pills: 0 5px padding on the lane colour mixed 45% into the background.
        let tip = state
            .rebase
            .steps
            .last()
            .map(|row| row.step.sha.clone())
            .unwrap_or_default();
        let pill_color = lane_color(p, state, &tip).mix(p.app_bg0, 0.45);
        let pill = |x: &mut f32, label: &str| {
            let w = text_w(cx, cx.fonts.regular, 12.0, label) + cx.px(10.0);
            draw.add_rect_filled(
                vec2(*x, line_top),
                vec2(*x + w, line_top + cx.px(17.0)),
                pill_color,
                0.0,
            );
            line_text(
                cx,
                draw,
                cx.fonts.regular,
                12.0,
                *x + cx.px(5.0),
                line_top,
                cx.px(17.0),
                p.text_normal,
                label,
            );
            *x += w + cx.px(5.0);
        };
        words(&mut x, "Rebasing");
        pill(&mut x, &state.rebase.head_label);
        words(&mut x, "onto");
        pill(&mut x, &state.rebase.base_label);
        requests.cancel |= button(
            cx,
            draw,
            "##cancel_header",
            "Cancel Rebase",
            hmax.x - cx.px(PADDING),
            origin.y + cx.px(6.5),
            cx.px(22.0),
            5.0,
            Tone::Danger,
        )
        .clicked;
    }

    // The rows, newest first, in a scrolling list with 20px padding.
    let list_top = origin.y + cx.px(HEADER_HEIGHT);
    let list_h = size.y - cx.px(HEADER_HEIGHT) - cx.px(FOOTER_HEIGHT);
    cx.set_cursor(vec2(origin.x, list_top));
    let scrollbar_bg = ui.push_style_color(StyleColor::ScrollbarBg, Color::TRANSPARENT);
    let editor_pos = ui
        .child_window("##rebase_rows")
        .size([size.x, list_h.max(1.0)])
        .build(ui, || draw_rows(cx, ui_state, state))
        .flatten();
    drop(scrollbar_bg);
    // The reword editor floats over the list.
    if let Some(pos) = editor_pos
        && state.rebase.editing.is_some_and(|step| step < count)
    {
        draw_reword_editor(cx, ui_state, state, pos);
    }

    draw_footer(cx, app, ui_state, state, requests, origin, size);
}

/// The rows of the plan, newest first. Returns where the reword editor opens when a row is being reworded.
#[allow(clippy::too_many_lines)]
fn draw_rows(cx: &mut Cx<'_>, ui_state: &mut State, state: &mut RepoState) -> Option<Vec2> {
    let ui = cx.ui;
    let p = cx.palette;
    let rows = cx.window_draw_list();
    let count = state.rebase.steps.len();
    let list_origin = cx.cursor();
    let rows_right = list_origin.x + cx.avail().x - cx.px(PADDING);
    // The menu opens after the rows, at the ID stack level the popup is begun at.
    let mut open_menu = false;
    let mut editor_pos = None;
    for display in 0..count {
        let i = count - 1 - display;
        let y = list_origin.y + cx.px(PADDING) + cx.px(ROW_HEIGHT) * display as f32;
        let x0 = list_origin.x + cx.px(PADDING);
        let top = y + cx.px(4.0);
        let bottom = top + cx.px(24.0);
        let kind = Kind::of(&state.rebase.steps[i].step.action);
        let (drop, squash) = (kind == Kind::Drop, kind == Kind::Squash);
        let _id = ui.push_id(i as i32);

        // The message zone: hover / selected backgrounds, the row's click target.
        let wmin = vec2(x0 + cx.px(MESSAGE_X), top);
        let wmax = vec2((wmin.x + 1.0).max(rows_right), bottom);
        cx.set_cursor(wmin);
        let row_clicked = ui.invisible_button("##row", (wmax - wmin).to_array());
        let row_hovered = ui.is_item_hovered();
        if row_hovered && ui.is_mouse_double_clicked(MouseButton::Left) && kind == Kind::Reword {
            open_reword(ui_state, state, i);
        }
        let selected = state.rebase.selected == Some(i);
        if selected || row_hovered {
            rows.add_rect_filled(
                wmin,
                wmax,
                if selected {
                    p.selected_row
                } else {
                    p.hover_row
                },
                0.0,
            );
        }
        if row_clicked {
            select_step(state, i);
        }

        // The action select: 90 x 24, rgba(0,0,0,.2), a 1px border green (pick), orange (reword / squash) or
        // red (drop), radius 4, 14px text 5px in and the chevron at the end.
        let smin = vec2(x0, top);
        let smax = vec2(x0 + cx.px(SELECT_WIDTH), bottom);
        cx.set_cursor(smin);
        if ui.invisible_button("##action", (smax - smin).to_array()) {
            select_step(state, i);
            ui_state.menu_step = Some(i);
            ui_state.menu_pos = vec2(smin.x, smax.y + cx.px(2.0));
            open_menu = true;
        }
        let border = match kind {
            Kind::Pick => p.green,
            Kind::Drop => p.red,
            Kind::Reword | Kind::Squash => p.orange,
        };
        rows.add_rect_filled(smin, smax, p.input_bg, cx.px(4.0));
        rows.add_rect(smin, smax, border, cx.px(4.0), 1.0);
        line_text(
            cx,
            rows,
            cx.fonts.regular,
            14.0,
            smin.x + cx.px(6.0),
            top,
            cx.px(24.0),
            p.text_normal,
            kind.label(),
        );
        if let Some(path) = svg_path("chevron-down") {
            cx.draw_svg_path(
                rows,
                "chevron-down",
                path,
                20.0,
                20.0,
                vec2(smax.x - cx.px(20.0), top + cx.px(4.0)),
                vec2(smax.x - cx.px(4.0), top + cx.px(20.0)),
                p.text_normal,
            );
        }

        // The node zone: squash adds a level-down indicator before the node, drop shifts it the same 24px and
        // fades it; the streak runs from the node's centre to the message.
        let row = &state.rebase.steps[i];
        let lane = lane_color(p, state, &row.step.sha);
        let zone_x = x0 + cx.px(NODE_X);
        let node_x = if squash || drop {
            zone_x + cx.px(24.0)
        } else {
            zone_x
        };
        let alpha = if drop { 0.5 } else { 1.0 };
        if squash {
            cx.draw_fa_icon_mirrored(
                rows,
                "fas:level-down",
                vec2(zone_x + cx.px(12.0), bottom - cx.px(7.0)),
                cx.px(14.0),
                p.text_secondary,
            );
        }
        let center = vec2(node_x + cx.px(12.0), top + cx.px(12.0));
        // The streak: the lane colour at 10% (50% on the selected row) with a 2px lane-coloured right edge.
        rows.add_rect_filled(
            vec2(center.x, top),
            vec2(wmin.x, bottom),
            lane.with_alpha((if selected { 0.5 } else { 0.1 }) * alpha),
            0.0,
        );
        rows.add_rect_filled(
            vec2(wmin.x - cx.px(2.0), top),
            vec2(wmin.x, bottom),
            lane.with_alpha(alpha),
            0.0,
        );
        rows.add_circle_filled(center, cx.px(12.0), lane.with_alpha(alpha), 0);
        cx.draw_avatar(rows, center, cx.px(20.0), &row.author, None);
        if drop {
            rows.add_circle_filled(center, cx.px(12.0), p.app_bg0.with_alpha(0.5), 0);
        }

        // The message: 14px, 10px in; squashed ones disabled, dropped ones struck through. A reworded commit
        // shows its new summary.
        let summary = match action_message(&row.step.action) {
            Some(message) if kind == Kind::Reword && !message.is_empty() => {
                message.split('\n').next().unwrap_or_default()
            }
            _ => row.summary.as_str(),
        };
        let text = if drop || squash {
            p.text_disabled
        } else if selected {
            p.text_selected
        } else {
            p.text_normal
        };
        let text_x = wmin.x + cx.px(10.0);
        let content = (14.0_f32 * 2189.0 / 2048.0).round() + (14.0_f32 * 600.0 / 2048.0).round();
        let ascent = 14.0 * 2189.0 / 2048.0 * cx.scale;
        let text_y = (top + ((24.0 - content) / 2.0).floor() * cx.scale + ascent.round()).round()
            - ascent.ceil();
        let shown = cx.draw_text_ellipsis(
            rows,
            cx.fonts.regular,
            14.0,
            vec2(text_x, text_y),
            wmax.x - text_x - cx.px(10.0),
            text,
            summary,
        );
        if drop {
            let strike = (top + cx.px(13.0)).round();
            rows.add_rect_filled(
                vec2(text_x, strike),
                vec2(text_x + shown, strike + 1.0),
                text,
                0.0,
            );
        }
        if state.rebase.editing == Some(i) {
            editor_pos = Some(vec2(wmin.x - cx.px(3.0), top + cx.px(28.0)));
        }
    }
    cx.set_cursor(list_origin);
    ui.dummy([1.0, cx.px(PADDING * 2.0) + cx.px(ROW_HEIGHT) * count as f32]);

    draw_action_menu(cx, ui_state, state, open_menu);
    handle_keys(cx, ui_state, state);
    editor_pos
}

/// The action select's menu: panel-bg1 options, 24px each.
fn draw_action_menu(cx: &mut Cx<'_>, ui_state: &mut State, state: &mut RepoState, open: bool) {
    let ui = cx.ui;
    let p = cx.palette;
    raw::set_next_window_pos(ui, ui_state.menu_pos, Vec2::ZERO);
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, cx.px(2.0)]));
    let _rounding = ui.push_style_var(StyleVar::PopupRounding(cx.px(2.0)));
    let _background = ui.push_style_color(StyleColor::PopupBg, p.panel_bg1);
    if open {
        ui.open_popup(MENU_ID);
    }
    let Some(_popup) = ui.begin_popup(MENU_ID) else {
        return;
    };
    for (a, kind) in KINDS.into_iter().enumerate() {
        let omin = cx.cursor();
        let picked = {
            let _id = ui.push_id(a as i32);
            ui.invisible_button("##option", [cx.px(SELECT_WIDTH), cx.px(24.0)])
        };
        let hovered = ui.is_item_hovered();
        let menu = cx.window_draw_list();
        let current = ui_state
            .menu_step
            .and_then(|step| state.rebase.steps.get(step))
            .is_some_and(|row| Kind::of(&row.step.action) == kind);
        if hovered || current {
            menu.add_rect_filled(
                omin,
                omin + vec2(cx.px(SELECT_WIDTH), cx.px(24.0)),
                if hovered { p.hover_row } else { p.selected_row },
                0.0,
            );
        }
        line_text(
            cx,
            menu,
            cx.fonts.regular,
            14.0,
            omin.x + cx.px(6.0),
            omin.y,
            cx.px(24.0),
            p.text_normal,
            kind.label(),
        );
        if picked
            && let Some(step) = ui_state
                .menu_step
                .filter(|&step| step < state.rebase.steps.len())
        {
            set_action(ui_state, state, step, kind);
            ui.close_current_popup();
        }
    }
}

/// Keys on the selected row, while no field has the keyboard.
fn handle_keys(cx: &Cx<'_>, ui_state: &mut State, state: &mut RepoState) {
    let ui = cx.ui;
    let count = state.rebase.steps.len();
    let Some(s) = state.rebase.selected.filter(|&s| s < count) else {
        return;
    };
    if ui.io().want_text_input() || state.rebase.editing.is_some() || ui.is_popup_open(MENU_ID) {
        return;
    }
    let io = ui.io();
    let ctrl = io.key_ctrl();
    // The letters alone: Ctrl+P is the command palette, Ctrl+S / Ctrl+R / Ctrl+D other shortcuts.
    let plain = !io.key_ctrl() && !io.key_alt() && !io.key_super();
    for (key, kind) in [
        (Key::P, Kind::Pick),
        (Key::S, Kind::Squash),
        (Key::R, Kind::Reword),
        (Key::D, Kind::Drop),
    ] {
        if plain && ui.is_key_pressed(key) {
            set_action(ui_state, state, s, kind);
        }
    }
    // Up is newer (a higher step index), down older.
    if ui.is_key_pressed(Key::UpArrow) && s + 1 < count {
        if ctrl {
            state.rebase.swap_steps(s, s + 1);
        }
        select_step(state, s + 1);
    }
    if ui.is_key_pressed(Key::DownArrow) && s > 0 {
        if ctrl {
            state.rebase.swap_steps(s, s - 1);
        }
        select_step(state, s - 1);
    }
}

/// The footer: 10px 20px padding; the shortcuts on the left, the buttons on the right. The buttons come first:
/// in a narrow panel the shortcuts that would run under them are left out.
fn draw_footer(
    cx: &mut Cx<'_>,
    app: &mut Application,
    ui_state: &mut State,
    state: &mut RepoState,
    requests: &mut Requests,
    origin: Vec2,
    size: Vec2,
) {
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let ftop = origin.y + size.y - cx.px(FOOTER_HEIGHT);
    let x0 = origin.x + cx.px(PADDING);

    let btop = ftop + cx.px(13.0);
    let bh = cx.px(31.140_625);
    let mut right = origin.x + size.x - cx.px(PADDING);
    let start = button(
        cx,
        draw,
        "##start_rebase",
        "Start Rebase",
        right,
        btop,
        bh,
        12.0,
        Tone::Success,
    );
    requests.start = start.clicked;
    right -= start.width + cx.px(20.0);
    let cancel = button(
        cx,
        draw,
        "##cancel_rebase",
        "Cancel Rebase",
        right,
        btop,
        bh,
        12.0,
        Tone::Danger,
    );
    requests.cancel |= cancel.clicked;
    right -= cancel.width + cx.px(20.0);
    let reset = button(
        cx,
        draw,
        "##reset_rebase",
        "Reset",
        right,
        btop,
        bh,
        12.0,
        Tone::Primary,
    );
    if reset.clicked {
        state.rebase.reset();
        ui_state.reword = None;
        let workdir = state.repo.workdir();
        ui_state.remembered.retain(|(dir, _), _| dir != workdir);
    }
    let buttons_left = right - reset.width;
    // Toasts stack above the buttons instead of covering them.
    app.toasts.keep_clear(
        vec2(buttons_left, btop),
        vec2(origin.x + size.x - cx.px(PADDING), btop + bh),
    );

    let limit = buttons_left - cx.px(10.0);
    let (secondary, normal) = (p.text_secondary, p.text_normal);
    let line1 = ftop + cx.px(12.0);
    let line2 = ftop + cx.px(30.0);
    let line_h = cx.px(17.0);
    let regular = cx.fonts.regular;
    let semibold = cx.fonts.semibold;
    if x0 + cx.px(18.5) + text_w(cx, regular, 12.0, "shortcuts:") <= limit {
        cx.draw_fa_icon(
            draw,
            "far:keyboard",
            vec2(x0 + cx.px(6.75), line1 + cx.px(8.5)),
            cx.px(12.0),
            secondary,
        );
        line_text(
            cx,
            draw,
            regular,
            12.0,
            x0 + cx.px(18.5),
            line1,
            line_h,
            secondary,
            "shortcuts:",
        );
    }
    let mut x = x0;
    for word in ["Pick", "Squash", "Reword", "Drop"] {
        let (first, rest) = word.split_at(1);
        let first_w = text_w(cx, semibold, 12.0, first);
        let rest_w = text_w(cx, regular, 12.0, rest);
        if x + first_w + rest_w > limit {
            break;
        }
        line_text(cx, draw, semibold, 12.0, x, line2, line_h, normal, first);
        line_text(
            cx,
            draw,
            regular,
            12.0,
            x + first_w,
            line2,
            line_h,
            secondary,
            rest,
        );
        x += first_w + rest_w + cx.px(10.0);
    }
    for (icon, label) in [("fas:arrow-up", "Move Up"), ("fas:arrow-down", "Move Down")] {
        let ctrl = cx.shortcut_label("Ctrl + ");
        let ctrl_w = text_w(cx, semibold, 12.0, &ctrl);
        let label_w = text_w(cx, regular, 12.0, label);
        if x + ctrl_w + cx.px(15.5) + label_w > limit {
            break;
        }
        line_text(cx, draw, semibold, 12.0, x, line2, line_h, normal, &ctrl);
        x += ctrl_w;
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(x + cx.px(5.25), line2 + cx.px(8.5)),
            cx.px(12.0),
            normal,
        );
        x += cx.px(10.5) + cx.px(5.0);
        line_text(cx, draw, regular, 12.0, x, line2, line_h, secondary, label);
        x += label_w + cx.px(10.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_joins_messages() {
        assert_eq!(split_message("one"), ("one".to_owned(), String::new()));
        assert_eq!(
            split_message("one\n\n\ntwo\nthree"),
            ("one".to_owned(), "two\nthree".to_owned())
        );
        assert_eq!(split_message("one\n"), ("one".to_owned(), String::new()));
        assert_eq!(join_message("one", ""), "one");
        assert_eq!(join_message("one", "two"), "one\n\ntwo");
        let (summary, description) = split_message("a\r\n\r\nb");
        assert_eq!((summary.as_str(), description.as_str()), ("a\r", "b"));
    }

    #[test]
    fn actions_carry_their_messages() {
        let reword = Kind::Reword.action(Some("m".to_owned()));
        assert_eq!(action_message(&reword), Some("m"));
        assert_eq!(Kind::of(&reword), Kind::Reword);
        assert_eq!(action_message(&Kind::Squash.action(None)), None);
        assert_eq!(Kind::Pick.action(Some("m".to_owned())), RebaseAction::Pick);
        assert_eq!(Kind::Drop.action(Some("m".to_owned())), RebaseAction::Drop);
        for kind in KINDS {
            assert_eq!(Kind::of(&kind.action(None)), kind);
            assert_eq!(kind.label(), ACTION_LABELS[kind as usize]);
        }
    }
}
