//! The tabs bar's popovers: the tabs list, the profile menu and the notifications.
//
// The tabs bar's popovers (toolbar-bg2 with a 0 6px 12px shadow, 5px under their button): the tabs list (a
// react-select), the notifications and the profile menu.

use dear_imgui_rs::{InputTextFlags, Key, MouseCursor, StyleColor, StyleVar, WindowFlags};
use imgit_platform::system::{LocalTime, local_time};

use crate::app::application::Application;
use crate::app::toasts::{Entry, ToastKind};
use crate::fonts::Font;
use crate::theme::{Color, Ink};
use crate::ui::draw::DrawList;
use crate::ui::icons::svg_path;
use crate::ui::popover::draw_box_shadow;
use crate::ui::profile_widgets::{
    POPOVER_PROFILE_MENU, draw_profile_icon, draw_select_focus, profile_select_menu,
};
use crate::ui::widgets::contains_ci;
use crate::ui::{Cx, Vec2, raw, vec2};

const TEXT: Ink = Ink(0.9);
const SECONDARY: Ink = Ink(0.6);
const DISABLED: Ink = Ink(0.4);
const DIVIDER: Ink = Ink(0.2);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Read,
    Unread,
}

impl Filter {
    const ALL: [Self; 3] = [Self::All, Self::Read, Self::Unread];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Read => "Read",
            Self::Unread => "Unread",
        }
    }

    /// Whether a notification with this read state is listed.
    fn shows(self, read: bool) -> bool {
        match self {
            Self::All => true,
            Self::Read => read,
            Self::Unread => !read,
        }
    }
}

/// The popovers' state between frames (the C++ function-local statics).
#[derive(Default)]
pub struct State {
    /// The tabs list's search box.
    search: String,
    /// The tabs list's keyboard / mouse row.
    focused: Option<usize>,
    /// The notifications' "All" filter. Notifications are read once the popover closes (the eye marks one
    /// read, or unread again, before that).
    filter: Filter,
    filter_open: bool,
    notifications_were_open: bool,
}

/// Where a line of text sits in a popover: its CSS line box starts at `top` (logical, from the popover's
/// origin) and is `height` tall; `x` is logical too.
#[derive(Clone, Copy)]
struct Line {
    font: Font,
    size: f32,
    x: f32,
    top: f32,
    height: f32,
    /// The width (logical) the text is cut at with an ellipsis.
    max_width: Option<f32>,
}

impl Line {
    fn new(font: Font, size: f32, x: f32, top: f32, height: f32) -> Self {
        Self {
            font,
            size,
            x,
            top,
            height,
            max_width: None,
        }
    }

    fn clipped(self, max_width: f32) -> Self {
        Self {
            max_width: Some(max_width),
            ..self
        }
    }
}

/// Text whose CSS line box starts at `line.top` (logical, from `origin`), Chromium-style baseline.
fn text_at(cx: &Cx<'_>, draw: DrawList<'_>, origin: Vec2, line: Line, color: Color, text: &str) {
    let scale = cx.scale;
    let size = line.size;
    let content = (size * 2189.0 / 2048.0).round() + (size * 600.0 / 2048.0).round();
    let ascent = size * 2189.0 / 2048.0 * scale;
    let top = origin.y + (line.top + (line.height - content) / 2.0) * scale;
    let baseline = (top + ascent.round()).round();
    let pos = vec2(origin.x + line.x * scale, baseline - ascent.ceil());
    match line.max_width {
        Some(max_width) => {
            cx.draw_text_ellipsis(draw, line.font, size, pos, max_width * scale, color, text);
        }
        None => cx.draw_text(draw, line.font, size, pos, color, text),
    }
}

/// A popover window `size` (logical) at `pos`; `content` draws in screen space on its draw list, from the
/// window's origin. `None` while it is closed. Escape closes it.
fn with_tab_popover<R>(
    cx: &mut Cx<'_>,
    id: &str,
    pos: Vec2,
    size: Vec2,
    content: impl FnOnce(&mut Cx<'_>, Vec2) -> R,
) -> Option<R> {
    let ui = cx.ui;
    if !ui.is_popup_open(id) {
        return None;
    }
    raw::set_next_window_pos(ui, pos.round(), Vec2::ZERO);
    raw::set_next_window_size(ui, vec2(cx.px(size.x), cx.px(size.y)));
    // Held for the whole popup: the safe API pops style in the scope it pushed it.
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::PopupBorderSize(0.0));
    let _rounding = ui.push_style_var(StyleVar::PopupRounding(0.0));
    let _spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));
    let _background = ui.push_style_color(StyleColor::PopupBg, Color::TRANSPARENT);
    let popup = ui.begin_popup_with_flags(id, WindowFlags::NO_MOVE | WindowFlags::NO_SCROLLBAR)?;
    if ui.is_key_pressed(Key::Escape) {
        ui.close_current_popup();
    }
    let origin: Vec2 = ui.window_pos().into();
    let draw = cx.window_draw_list();
    draw.push_clip_rect_full_screen();
    let result = content(cx, origin);
    draw.pop_clip_rect();
    drop(popup);
    Some(result)
}

fn popover_frame(cx: &Cx<'_>, draw: DrawList<'_>, min: Vec2, size: Vec2) {
    let max = min + vec2(cx.px(size.x), cx.px(size.y));
    draw_box_shadow(
        draw,
        min,
        max,
        vec2(0.0, cx.px(6.0)),
        cx.px(12.0),
        0.0,
        Color::new(0.0, 0.0, 0.0, 0.176),
        cx.px(2.0),
    );
    draw.add_rect_filled(min, max, cx.palette.toolbar_bg2, cx.px(2.0));
}

/// A 14px row with a 20x16 icon in text-disabled and hover-text label (the profile menu's actions).
fn icon_row(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    origin: Vec2,
    y: f32,
    width: f32,
    icon: &str,
    label: &str,
) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    let min = vec2(origin.x, origin.y + cx.px(y));
    let max = vec2(origin.x + cx.px(width), min.y + cx.px(40.0));
    cx.set_cursor(min);
    let clicked = {
        let _id = ui.push_id(label);
        ui.invisible_button("##row", max - min)
    };
    let hovered = ui.is_item_hovered();
    if hovered {
        ui.set_mouse_cursor(Some(MouseCursor::Hand));
    }
    cx.draw_fa_icon(
        draw,
        icon,
        vec2(origin.x + cx.px(30.0), origin.y + cx.px(y + 20.0)),
        cx.px(16.0),
        DISABLED.of(p),
    );
    text_at(
        cx,
        draw,
        origin,
        Line::new(cx.fonts.regular, 14.0, 50.0, y + 10.0, 20.0),
        if hovered { Ink(1.0).of(p) } else { TEXT.of(p) },
        label,
    );
    clicked
}

fn bold_header(cx: &Cx<'_>, draw: DrawList<'_>, origin: Vec2, y: f32, width: f32, text: &str) {
    text_at(
        cx,
        draw,
        origin,
        Line::new(cx.fonts.semibold, 14.0, 20.0, y + 10.0, 20.0),
        TEXT.of(cx.palette),
        text,
    );
    draw.add_rect_filled(
        vec2(origin.x, origin.y + cx.px(y + 40.0)),
        vec2(origin.x + cx.px(width), origin.y + cx.px(y + 41.0)),
        DIVIDER.of(cx.palette),
        0.0,
    );
}

/// "October 12th @ 09:30am".
fn format_notification_time(unix_seconds: i64) -> String {
    local_time(unix_seconds)
        .map(|time| format_time(&time))
        .unwrap_or_default()
}

fn format_time(time: &LocalTime) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let day = u32::from(time.day);
    let suffix = match day {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };
    let hour = if time.hour.is_multiple_of(12) {
        12
    } else {
        time.hour % 12
    };
    let month = MONTHS[usize::from(time.month.clamp(1, 12)) - 1];
    format!(
        "{month} {day}{suffix} @ {hour:02}:{:02}{}",
        time.minute,
        if time.hour < 12 { "am" } else { "pm" }
    )
}

/// The row after `focused` in a list of `len` (the Down arrow).
fn focus_down(focused: Option<usize>, len: usize) -> Option<usize> {
    (len > 0).then(|| focused.map_or(0, |row| row + 1).min(len - 1))
}

/// The row before `focused` (the Up arrow); the first one from none.
fn focus_up(focused: Option<usize>, len: usize) -> Option<usize> {
    (len > 0).then(|| focused.map_or(0, |row| row.saturating_sub(1)))
}

/// What a row of the tabs list stands for.
#[derive(Clone, PartialEq)]
struct ListItem {
    /// `None`: the New Tab page.
    tab: Option<usize>,
    label: String,
}

/// The rows of the tabs list: every repository tab, then New Tab, those whose label has `search` in it.
fn tabs_list_items(labels: impl IntoIterator<Item = String>, search: &str) -> Vec<ListItem> {
    let mut items: Vec<ListItem> = labels
        .into_iter()
        .enumerate()
        .map(|(index, label)| ListItem {
            tab: Some(index),
            label,
        })
        .collect();
    items.push(ListItem {
        tab: None,
        label: "New Tab".to_owned(),
    });
    items.retain(|item| contains_ci(&item.label, search));
    items
}

/// What the tabs list asks for once its window is closed.
#[derive(Default)]
struct TabsListOutcome {
    /// Show this: `Some(None)` is the New Tab page.
    activate: Option<Option<usize>>,
    close: Option<usize>,
}

/// The tabs list popover (`app.tabs_list_requested` opens it), anchored at its button's bottom-right corner.
pub fn draw_tabs_list_popover(cx: &mut Cx<'_>, app: &mut Application, anchor_right_bottom: Vec2) {
    app.with_panel_state(
        |panels| &mut panels.tab_popovers,
        |app, state| tabs_list(cx, app, state, anchor_right_bottom),
    );
}

fn tabs_list(cx: &mut Cx<'_>, app: &mut Application, state: &mut State, anchor_right_bottom: Vec2) {
    let ui = cx.ui;
    let items = tabs_list_items((0..app.tabs.len()).map(|i| app.tab_label(i)), &state.search);
    let menu_h = 4.0 + 8.0 + 15.0 + 2.625 + 47.0 * items.len() as f32;
    let pos = vec2(
        anchor_right_bottom.x - cx.px(330.0),
        anchor_right_bottom.y + cx.px(5.0),
    );
    let outcome = with_tab_popover(
        cx,
        "##tabs_list",
        pos,
        vec2(330.0, 36.0 + menu_h + 8.0),
        |cx, origin| {
            let p = cx.palette;
            let draw = cx.window_draw_list();
            popover_frame(cx, draw, origin, vec2(330.0, 34.0));

            // react-select control: 20% black, 20% white border, 4px radius, the focus ring rgb(38,132,255).
            let control_max = origin + vec2(cx.px(330.0), cx.px(34.0));
            draw.add_rect_filled(
                origin - vec2(1.0, 1.0),
                control_max + vec2(1.0, 1.0),
                p.accent,
                cx.px(5.0),
            );
            draw.add_rect_filled(origin, control_max, p.toolbar_bg2, cx.px(4.0));
            draw.add_rect_filled(
                origin,
                control_max,
                Color::new(0.0, 0.0, 0.0, 0.2),
                cx.px(4.0),
            );
            draw.add_rect(
                origin + vec2(0.5, 0.5),
                control_max - vec2(0.5, 0.5),
                DIVIDER.of(p),
                cx.px(4.0),
                1.0,
            );
            draw.add_rect_filled(
                origin + vec2(cx.px(300.0), cx.px(7.0)),
                origin + vec2(cx.px(301.0), cx.px(27.0)),
                DIVIDER.of(p),
                0.0,
            );
            if let Some(path) = svg_path("chevron-down") {
                cx.draw_svg_path(
                    draw,
                    "chevron-down",
                    path,
                    20.0,
                    20.0,
                    origin + vec2(cx.px(305.0), cx.px(7.0)),
                    origin + vec2(cx.px(325.0), cx.px(27.0)),
                    TEXT.of(p),
                );
            }
            // The input (14px), placeholder "Search Tabs".
            let ascent = 14.0 * 2189.0 / 2048.0 * cx.scale;
            let baseline = (origin.y + cx.px(7.5) + ascent.round()).round();
            cx.set_cursor(vec2(origin.x + cx.px(11.0), baseline - ascent.ceil()));
            if ui.is_window_appearing() {
                ui.set_keyboard_focus_here();
            }
            let changed = {
                let _background = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
                let _text = ui.push_style_color(StyleColor::Text, TEXT.of(p));
                let _disabled = ui.push_style_color(StyleColor::TextDisabled, DISABLED.of(p));
                let _padding = ui.push_style_var(StyleVar::FramePadding([0.0, 0.0]));
                let _font = cx.push_font(cx.fonts.regular, 14.0);
                cx.input_text(
                    "##tabs_search",
                    &mut state.search,
                    Some("Search Tabs"),
                    InputTextFlags::empty(),
                    cx.px(280.0),
                )
            };
            if changed {
                state.focused = Some(0);
            }

            // The menu: the theme's darkest surface (#1C1E23 in ImGit's, toolbar-bg0 as the prompt bar's
            // react-select menu), 20% white border, 4px radius, 0 6px 12px rgba(0,0,0,.4).
            let menu_min = origin + vec2(0.0, cx.px(36.0));
            let menu_max = vec2(origin.x + cx.px(330.0), menu_min.y + cx.px(menu_h));
            draw_box_shadow(
                draw,
                menu_min,
                menu_max,
                vec2(0.0, cx.px(6.0)),
                cx.px(12.0),
                0.0,
                Color::new(0.0, 0.0, 0.0, 0.4),
                cx.px(4.0),
            );
            draw.add_rect_filled(menu_min, menu_max, DIVIDER.of(p), cx.px(4.0));
            draw.add_rect_filled(
                menu_min + vec2(1.0, 1.0),
                menu_max - vec2(1.0, 1.0),
                p.toolbar_bg0,
                cx.px(3.0),
            );
            // "OPEN TABS": 10.5px, weight 500, uppercase, #999.
            text_at(
                cx,
                draw,
                menu_min,
                Line::new(cx.fonts.regular, 10.5, 13.0, 13.0, 15.0),
                Color::from_rgba8(153, 153, 153, 255),
                "OPEN TABS",
            );

            if ui.is_key_pressed(Key::DownArrow) {
                state.focused = focus_down(state.focused, items.len());
            }
            if ui.is_key_pressed(Key::UpArrow) {
                state.focused = focus_up(state.focused, items.len());
            }
            let enter = ui.is_key_pressed(Key::Enter) || ui.is_key_pressed(Key::KeypadEnter);
            let mut outcome = TabsListOutcome::default();
            for (i, item) in items.iter().enumerate() {
                let top = 30.625 + 47.0 * i as f32;
                let row_min = vec2(menu_min.x + 1.0, menu_min.y + cx.px(top));
                let row_max = vec2(menu_max.x - 1.0, row_min.y + cx.px(46.0));
                cx.set_cursor(row_min);
                let _id = ui.push_id(i);
                ui.set_next_item_allow_overlap();
                let clicked = ui.invisible_button("##tab_row", row_max - row_min);
                if ui.is_item_hovered() && ui.io().mouse_delta()[1] != 0.0 {
                    state.focused = Some(i);
                }
                if state.focused == Some(i) {
                    draw.add_rect_filled(row_min, row_max, p.hover_row, 0.0);
                }
                draw.add_rect_filled(
                    vec2(row_min.x, row_max.y),
                    vec2(row_max.x, row_max.y + cx.px(1.0)),
                    DIVIDER.of(p),
                    0.0,
                );
                let mut text_x = 13.0;
                if item.tab.is_some() {
                    if let Some(path) = svg_path("code-branch") {
                        cx.draw_svg_path(
                            draw,
                            "code-branch",
                            path,
                            448.0,
                            512.0,
                            vec2(menu_min.x + cx.px(18.0), row_min.y + cx.px(16.0)),
                            vec2(menu_min.x + cx.px(32.0), row_min.y + cx.px(30.0)),
                            DISABLED.of(p),
                        );
                    }
                    text_x = 42.0;
                }
                text_at(
                    cx,
                    draw,
                    vec2(menu_min.x, row_min.y),
                    Line::new(cx.fonts.regular, 14.0, text_x, 13.0, 20.0).clipped(250.0),
                    TEXT.of(p),
                    &item.label,
                );
                // The close "x" (far:times 17.5x14, text-secondary).
                let x_center = vec2(menu_min.x + cx.px(308.25), row_min.y + cx.px(23.0));
                cx.set_cursor(x_center - vec2(cx.px(10.0), cx.px(10.0)));
                let close_clicked = ui.invisible_button("##close", vec2(cx.px(20.0), cx.px(20.0)));
                cx.draw_fa_icon(
                    draw,
                    "far:times",
                    x_center,
                    cx.px(14.0),
                    if ui.is_item_hovered() {
                        Ink(1.0).of(p)
                    } else {
                        SECONDARY.of(p)
                    },
                );
                if close_clicked {
                    outcome.close = item.tab;
                } else if clicked || (enter && state.focused == Some(i)) {
                    outcome.activate = Some(item.tab);
                }
            }
            if items.is_empty() {
                text_at(
                    cx,
                    draw,
                    menu_min,
                    Line::new(cx.fonts.regular, 14.0, 13.0, 37.0, 20.0),
                    DISABLED.of(p),
                    "No options",
                );
            }
            if let Some(tab) = outcome.activate {
                app.active_tab = tab;
                if tab.is_none() {
                    app.new_tab_open = true; // the New Tab page needs its tab in the bar
                }
                ui.close_current_popup();
            }
            outcome
        },
    );
    match outcome {
        None => {
            state.search.clear();
            state.focused = None;
        }
        Some(outcome) => {
            if let Some(index) = outcome.close {
                app.close_tab(index);
            }
        }
    }
}

/// The profile popover.
pub fn draw_profile_popover(cx: &mut Cx<'_>, app: &mut Application, anchor_right_bottom: Vec2) {
    let ui = cx.ui;
    let pos = vec2(
        anchor_right_bottom.x - cx.px(330.0),
        anchor_right_bottom.y + cx.px(5.0),
    );
    with_tab_popover(cx, "##profile_menu", pos, vec2(330.0, 233.0), |cx, o| {
        let p = cx.palette;
        let draw = cx.window_draw_list();
        popover_frame(cx, draw, o, vec2(330.0, 233.0));
        bold_header(cx, draw, o, 0.0, 330.0, "Current profile");

        // The profile picker (react-select, 42px): its menu switches profiles.
        let profile = app.profiles.current().clone();
        let picker_min = o + vec2(cx.px(20.0), cx.px(61.0));
        let picker_max = picker_min + vec2(cx.px(290.0), cx.px(42.0));
        cx.set_cursor(picker_min);
        if ui.invisible_button("##profile_picker", picker_max - picker_min) {
            cx.open_popover("##profile_select");
        }
        draw.add_rect_filled(
            picker_min,
            picker_max,
            Color::new(0.0, 0.0, 0.0, 0.2),
            cx.px(4.0),
        );
        if ui.is_popup_open("##profile_select") {
            draw_select_focus(cx, draw, picker_min, picker_max, cx.px(4.0));
        }
        draw_profile_icon(
            cx,
            draw,
            o + vec2(cx.px(30.0), cx.px(68.0)),
            cx.px(28.0),
            &profile,
        );
        text_at(
            cx,
            draw,
            o,
            Line::new(cx.fonts.regular, 14.0, 68.0, 72.0, 20.0).clipped(210.0),
            TEXT.of(p),
            &profile.name,
        );
        if let Some(path) = svg_path("chevron-down") {
            cx.draw_svg_path(
                draw,
                "chevron-down",
                path,
                20.0,
                20.0,
                o + vec2(cx.px(286.0), cx.px(72.0)),
                o + vec2(cx.px(306.0), cx.px(92.0)),
                TEXT.of(p),
            );
        }

        // The profile's commit signature.
        let regular = cx.fonts.regular;
        text_at(
            cx,
            draw,
            o,
            Line::new(regular, 12.0, 20.0, 113.0, 17.1429),
            SECONDARY.of(p),
            "Commit signature",
        );
        text_at(
            cx,
            draw,
            o,
            Line::new(regular, 14.0, 30.0, 135.1, 21.0).clipped(280.0),
            TEXT.of(p),
            &profile.user_name,
        );
        text_at(
            cx,
            draw,
            o,
            Line::new(regular, 12.0, 30.0, 156.1, 21.0).clipped(280.0),
            TEXT.of(p),
            &profile.user_email,
        );
        if icon_row(
            cx,
            draw,
            o,
            184.3,
            330.0,
            "far:users-cog",
            "Manage profiles",
        ) {
            app.preferences_open = true;
            app.preferences_page = "Profiles".to_owned();
            ui.close_current_popup();
        }
        if let Some(picked) = profile_select_menu(
            cx,
            app,
            "##profile_select",
            picker_min,
            picker_max,
            POPOVER_PROFILE_MENU,
        ) {
            app.switch_profile(&picked);
            ui.close_current_popup();
        }
    });
}

/// The text of a notification: its title, then its detail.
fn notification_body(entry: &Entry) -> String {
    if entry.detail.is_empty() {
        entry.title.clone()
    } else {
        format!("{} {}", entry.title, entry.detail)
    }
}

/// The history entries `filter` lists, as indices.
fn shown_entries(history: &[Entry], filter: Filter) -> Vec<usize> {
    history
        .iter()
        .enumerate()
        .filter(|(_, entry)| filter.shows(entry.read))
        .map(|(index, _)| index)
        .collect()
}

/// Each notification: 10px padding, an 18px header line, 5px, the wrapped body, 10px, a 1px divider.
fn notification_height(lines: usize) -> f32 {
    10.0 + 18.0 + 5.0 + 17.1429 * lines.max(1) as f32 + 10.0 + 1.0
}

/// The notifications popover (`app.notifications_requested` opens it).
pub fn draw_notifications_popover(
    cx: &mut Cx<'_>,
    app: &mut Application,
    anchor_right_bottom: Vec2,
) {
    app.with_panel_state(
        |panels| &mut panels.tab_popovers,
        |app, state| notifications(cx, app, state, anchor_right_bottom),
    );
}

/// How tall the list may grow before it scrolls off (logical px).
const MAX_LIST_HEIGHT: f32 = 816.0;
const BODY_WIDTH: f32 = 362.0;

fn notifications(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut State,
    anchor_right_bottom: Vec2,
) {
    let ui = cx.ui;
    let font = cx.fonts.regular;
    let shown = shown_entries(app.toasts.history(), state.filter);
    let mut heights = Vec::with_capacity(shown.len());
    let mut content = 0.0;
    for &i in &shown {
        let body = notification_body(&app.toasts.history()[i]);
        let lines = cx.wrap_text(font, 12.0, &body, cx.px(BODY_WIDTH)).len();
        let height = notification_height(lines);
        heights.push(height);
        content += height;
    }
    let empty_h = if shown.is_empty() { 60.0 } else { 0.0 };
    // The filter menu needs the room under the header when the list is short.
    let menu_h = 8.0 + 37.0 * 3.0;
    let list_h = (if state.filter_open {
        menu_h + 8.0
    } else {
        0.0_f32
    })
    .max(MAX_LIST_HEIGHT.min(content + empty_h));
    let height = 52.1 + list_h;
    let pos = vec2(
        anchor_right_bottom.x - cx.px(450.0),
        anchor_right_bottom.y + cx.px(5.0),
    );
    let removed = with_tab_popover(cx, "##notifications", pos, vec2(450.0, height), |cx, o| {
        state.notifications_were_open = true;
        let p = cx.palette;
        let draw = cx.window_draw_list();
        // The filter menu draws over the list, so it gets its own channel; its rows are submitted first
        // and win the hover over the list's buttons under them.
        draw.channels_split(2);
        draw.channels_set_current(0);
        popover_frame(cx, draw, o, vec2(450.0, height));
        text_at(
            cx,
            draw,
            o,
            Line::new(cx.fonts.semibold, 14.0, 20.0, 15.6, 20.0),
            TEXT.of(p),
            "Notifications",
        );
        // The "All ▾" filter and the "⋮" menu (hover-text buttons).
        let filter_label = state.filter.label();
        let label_w = cx.text_size(font, 12.0, filter_label).x / cx.scale;
        let filter_min = o + vec2(cx.px(133.6), cx.px(14.0));
        let filter_max = o + vec2(cx.px(139.6 + label_w + 22.0), cx.px(36.0));
        cx.set_cursor(filter_min);
        if ui.invisible_button("##filter", filter_max - filter_min) {
            state.filter_open = !state.filter_open;
        }
        let filter_hovered = ui.is_item_hovered();
        if filter_hovered {
            ui.set_mouse_cursor(Some(MouseCursor::Hand));
        }
        let filter_color = if filter_hovered || state.filter_open {
            Ink(1.0).of(p)
        } else {
            TEXT.of(p)
        };
        text_at(
            cx,
            draw,
            o,
            Line::new(font, 12.0, 139.6, 16.9, 17.1429),
            filter_color,
            filter_label,
        );
        let tri_x = 139.6 + label_w + 4.8;
        draw.add_triangle_filled(
            o + vec2(cx.px(tri_x), cx.px(24.8)),
            o + vec2(cx.px(tri_x + 8.0), cx.px(24.8)),
            o + vec2(cx.px(tri_x + 4.0), cx.px(28.8)),
            filter_color,
        );
        cx.draw_fa_icon(
            draw,
            "far:ellipsis-v",
            o + vec2(cx.px(417.0), cx.px(25.6)),
            cx.px(16.0),
            TEXT.of(p),
        );
        draw.add_rect_filled(
            o + vec2(0.0, cx.px(51.1)),
            o + vec2(cx.px(450.0), cx.px(52.1)),
            DIVIDER.of(p),
            0.0,
        );

        // The filter's menu: under the control, 150px wide, 37px rows, the current filter highlighted.
        let mut menu_min = Vec2::ZERO;
        let mut menu_max = Vec2::ZERO;
        if state.filter_open {
            draw.channels_set_current(1);
            menu_min = o + vec2(cx.px(133.6), cx.px(40.0));
            menu_max = menu_min + vec2(cx.px(150.0), cx.px(menu_h));
            draw_box_shadow(
                draw,
                menu_min,
                menu_max,
                vec2(0.0, cx.px(6.0)),
                cx.px(12.0),
                0.0,
                Color::new(0.0, 0.0, 0.0, 0.4),
                cx.px(4.0),
            );
            draw.add_rect_filled(menu_min, menu_max, DIVIDER.of(p), cx.px(4.0));
            draw.add_rect_filled(
                menu_min + vec2(1.0, 1.0),
                menu_max - vec2(1.0, 1.0),
                p.toolbar_bg0,
                cx.px(3.0),
            );
            for (i, option) in Filter::ALL.into_iter().enumerate() {
                let row_min = vec2(menu_min.x + 1.0, menu_min.y + cx.px(4.0 + 37.0 * i as f32));
                let row_max = vec2(menu_max.x - 1.0, row_min.y + cx.px(37.0));
                cx.set_cursor(row_min);
                let picked = {
                    let _id = ui.push_id(i);
                    ui.invisible_button("##filter_option", row_max - row_min)
                };
                let hovered = ui.is_item_hovered();
                if hovered {
                    ui.set_mouse_cursor(Some(MouseCursor::Hand));
                }
                if hovered || state.filter == option {
                    draw.add_rect_filled(row_min, row_max, p.hover_row, 0.0);
                }
                text_at(
                    cx,
                    draw,
                    vec2(menu_min.x, row_min.y),
                    Line::new(font, 14.0, 13.0, 8.5, 20.0),
                    TEXT.of(p),
                    option.label(),
                );
                if picked {
                    state.filter = option;
                    state.filter_open = false;
                }
            }
            draw.channels_set_current(0);
        }

        if shown.is_empty() {
            text_at(
                cx,
                draw,
                o,
                Line::new(font, 12.0, 20.0, 72.1, 17.1429),
                SECONDARY.of(p),
                if state.filter == Filter::Read {
                    "No read notifications."
                } else {
                    "You're all caught up."
                },
            );
        }
        let mut y = 52.1;
        let mut remove = None;
        let history = app.toasts.history_mut();
        for (n, &i) in shown.iter().enumerate() {
            if y >= 52.1 + MAX_LIST_HEIGHT {
                break;
            }
            let entry = &mut history[i];
            let accent = match entry.kind {
                ToastKind::Error => p.red,
                ToastKind::Success => p.green,
                ToastKind::Info => p.accent,
            };
            draw.add_rect_filled(
                o + vec2(cx.px(20.0), cx.px(y + 10.8)),
                o + vec2(cx.px(38.0), cx.px(y + 28.8)),
                accent,
                cx.px(2.0),
            );
            text_at(
                cx,
                draw,
                o,
                Line::new(font, 12.0, 48.0, y + 10.5, 17.1429),
                TEXT.of(p),
                "ImGit",
            );
            let sender_w = cx.text_size(font, 12.0, "ImGit").x / cx.scale;
            text_at(
                cx,
                draw,
                o,
                Line::new(font, 12.0, 48.0 + sender_w + 10.0, y + 10.5, 17.1429),
                SECONDARY.of(p),
                &format_notification_time(entry.time),
            );
            let scope = ui.push_id(i);
            // The eye marks the notification read; the eye-slash, unread again.
            let eye = o + vec2(cx.px(383.0), cx.px(y + 19.0));
            cx.set_cursor(eye - vec2(cx.px(9.0), cx.px(9.0)));
            if ui.invisible_button("##read", vec2(cx.px(18.0), cx.px(18.0))) {
                entry.read = !entry.read;
            }
            cx.draw_fa_icon(
                draw,
                if entry.read {
                    "far:eye-slash"
                } else {
                    "far:eye"
                },
                eye,
                cx.px(12.0),
                if ui.is_item_hovered() {
                    Ink(1.0).of(p)
                } else {
                    DISABLED.of(p)
                },
            );
            let trash = o + vec2(cx.px(421.0), cx.px(y + 19.0));
            cx.set_cursor(trash - vec2(cx.px(9.0), cx.px(9.0)));
            if ui.invisible_button("##delete", vec2(cx.px(18.0), cx.px(18.0))) {
                remove = Some(i);
            }
            cx.draw_fa_icon(
                draw,
                "far:trash",
                trash,
                cx.px(12.0),
                if ui.is_item_hovered() {
                    Ink(1.0).of(p)
                } else {
                    DISABLED.of(p)
                },
            );
            drop(scope);
            let body = notification_body(entry);
            let mut line_y = y + 33.0;
            for piece in cx.wrap_text(font, 12.0, &body, cx.px(BODY_WIDTH)) {
                text_at(
                    cx,
                    draw,
                    o,
                    Line::new(font, 12.0, 48.0, line_y, 17.1429),
                    TEXT.of(p),
                    piece,
                );
                line_y += 17.1429;
            }
            y += heights[n];
            draw.add_rect_filled(
                o + vec2(0.0, cx.px(y - 1.0)),
                o + vec2(cx.px(450.0), cx.px(y)),
                DIVIDER.of(p),
                0.0,
            );
        }
        // A click outside the open menu closes it.
        if state.filter_open
            && ui.is_mouse_clicked(dear_imgui_rs::MouseButton::Left)
            && !ui.is_mouse_hovering_rect(menu_min.into(), menu_max.into())
            && !ui.is_mouse_hovering_rect(filter_min.into(), filter_max.into())
        {
            state.filter_open = false;
        }
        draw.channels_merge();
        remove
    });
    match removed {
        None => {
            if state.notifications_were_open {
                app.toasts.mark_all_read();
                state.notifications_were_open = false;
                state.filter_open = false;
            }
        }
        Some(Some(index)) => app.toasts.remove_history(index),
        Some(None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icons::Icons;

    fn entry(read: bool, title: &str, detail: &str) -> Entry {
        Entry {
            kind: ToastKind::Info,
            title: title.to_owned(),
            detail: detail.to_owned(),
            time: 0,
            read,
        }
    }

    fn time(month: u8, day: u8, hour: u8, minute: u8) -> LocalTime {
        LocalTime {
            year: 2026,
            month,
            day,
            hour,
            minute,
            second: 0,
            weekday: 0,
            day_of_year: 0,
            is_dst: false,
        }
    }

    #[test]
    fn notification_times_read_like_the_original() {
        assert_eq!(format_time(&time(10, 12, 9, 30)), "October 12th @ 09:30am");
        assert_eq!(format_time(&time(1, 1, 0, 5)), "January 1st @ 12:05am");
        assert_eq!(format_time(&time(3, 2, 12, 0)), "March 2nd @ 12:00pm");
        assert_eq!(
            format_time(&time(12, 23, 23, 59)),
            "December 23rd @ 11:59pm"
        );
        // 11th, 12th and 13th are not "st", "nd" and "rd".
        for (day, expected) in [
            (11, "11th"),
            (12, "12th"),
            (13, "13th"),
            (21, "21st"),
            (22, "22nd"),
            (31, "31st"),
        ] {
            assert!(format_time(&time(5, day, 1, 1)).contains(expected), "{day}");
        }
    }

    #[test]
    fn the_filter_lists_read_and_unread() {
        let history = [
            entry(true, "a", ""),
            entry(false, "b", ""),
            entry(true, "c", ""),
        ];
        assert_eq!(shown_entries(&history, Filter::All), [0, 1, 2]);
        assert_eq!(shown_entries(&history, Filter::Read), [0, 2]);
        assert_eq!(shown_entries(&history, Filter::Unread), [1]);
    }

    #[test]
    fn the_body_is_the_title_and_the_detail() {
        assert_eq!(notification_body(&entry(false, "Pushed", "")), "Pushed");
        assert_eq!(
            notification_body(&entry(false, "Failed", "no net")),
            "Failed no net"
        );
    }

    #[test]
    fn a_notification_is_as_tall_as_its_lines() {
        assert!((notification_height(1) - 61.1429).abs() < 1e-3);
        assert!((notification_height(3) - 95.4287).abs() < 1e-3);
        // Never shorter than one line.
        assert_eq!(notification_height(0), notification_height(1));
    }

    #[test]
    fn the_keyboard_walks_the_rows() {
        assert_eq!(focus_down(None, 3), Some(0));
        assert_eq!(focus_down(Some(0), 3), Some(1));
        assert_eq!(focus_down(Some(2), 3), Some(2));
        assert_eq!(focus_down(None, 0), None);
        assert_eq!(focus_up(Some(2), 3), Some(1));
        assert_eq!(focus_up(Some(0), 3), Some(0));
        assert_eq!(focus_up(None, 3), Some(0));
        assert_eq!(focus_up(Some(1), 0), None);
    }

    #[test]
    fn the_tabs_list_ends_with_new_tab_and_follows_the_search() {
        let labels = || ["alpha".to_owned(), "Beta".to_owned()];
        let all = tabs_list_items(labels(), "");
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].tab, None);
        assert_eq!(all[2].label, "New Tab");
        let found = tabs_list_items(labels(), "BET");
        assert_eq!(
            found.iter().map(|item| item.tab).collect::<Vec<_>>(),
            [Some(1)]
        );
        let new_tab = tabs_list_items(labels(), "new");
        assert_eq!(new_tab.len(), 1);
        assert!(tabs_list_items(labels(), "zzz").is_empty());
    }

    #[test]
    fn the_icons_of_the_popovers_exist() {
        let icons = Icons::new();
        for name in [
            "far:times",
            "far:users-cog",
            "far:ellipsis-v",
            "far:eye",
            "far:eye-slash",
            "far:trash",
        ] {
            assert!(icons.find(name).is_some(), "{name}");
        }
        assert!(svg_path("chevron-down").is_some());
        assert!(svg_path("code-branch").is_some());
    }

    #[test]
    fn the_notification_time_of_a_real_instant_is_not_empty() {
        assert!(!format_notification_time(1_700_000_000).is_empty());
    }
}
