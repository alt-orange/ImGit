//! What shows a profile outside the Profiles page: its icon and the react-select menu that switches profiles
//! (the tabs bar's profile menu and the Preferences sidebar).

use dear_imgui_rs::{Key, MouseButton, StyleColor, StyleVar, WindowFlags};
use imgit_app::profiles::Profile;
use imgit_git::Author;

use crate::app::application::Application;
use crate::theme::{Color, Ink, Palette};
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};
use crate::ui::popover::draw_box_shadow;
use crate::ui::{Cx, raw};

/// 8px padding, the 28px icon, 8px, a 1px bottom border.
const OPTION_HEIGHT: f32 = 45.0;
const MAX_LIST_HEIGHT: f32 = 270.0;

/// The colours a profile picker's menu is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileMenuStyle {
    /// The tabs bar menu's picker (toolbar colours).
    Popover,
    /// The Preferences sidebar's picker (panel-bg0).
    Sidebar,
}

/// The tabs bar menu's picker.
pub const POPOVER_PROFILE_MENU: ProfileMenuStyle = ProfileMenuStyle::Popover;
/// The Preferences sidebar's picker.
pub const SIDEBAR_PROFILE_MENU: ProfileMenuStyle = ProfileMenuStyle::Sidebar;

impl ProfileMenuStyle {
    /// `.react-select__menu` and its options.
    #[must_use]
    pub fn background(self, palette: &Palette) -> Color {
        match self {
            Self::Popover => palette.toolbar_bg0,
            Self::Sidebar => palette.app_bg0,
        }
    }

    /// The menu list's and each option's 1px border.
    #[must_use]
    pub const fn border(self) -> Ink {
        match self {
            Self::Popover => Ink(0.2),
            Self::Sidebar => Ink(0.08),
        }
    }

    #[must_use]
    pub const fn text(self) -> Ink {
        match self {
            Self::Popover => Ink(0.9),
            Self::Sidebar => Ink(0.75),
        }
    }
}

/// The keyboard / mouse position of the open profile picker. Lives in `UiState::profile_menu`.
#[derive(Debug, Default)]
pub struct ProfileMenuState {
    focused: usize,
    scroll: f32,
}

/// 14px text in a 20px line box whose top is `line_top` (device px), placed like Chromium.
fn line_text(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    x: f32,
    line_top: f32,
    color: Color,
    text: &str,
    max_width: f32,
) {
    let content = (14.0_f32 * 2189.0 / 2048.0).round() + (14.0_f32 * 600.0 / 2048.0).round();
    let top = line_top + ((20.0 - content) / 2.0).floor() * cx.scale;
    let ascent = 14.0 * 2189.0 / 2048.0 * cx.scale;
    let baseline = (top + ascent.round()).round();
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        14.0,
        vec2(x, baseline - ascent.ceil()),
        max_width,
        color,
        text,
    );
}

/// The author a profile signs with.
fn author_of(profile: &Profile) -> Author {
    Author {
        name: profile.user_name.clone(),
        email: profile.user_email.clone(),
    }
}

/// A profile's picture, `size` device px square at `min`: its author's Gravatar with 2px corners, or the
/// author's initials when there is none (or while it loads).
pub fn draw_profile_icon(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    min: Vec2,
    size: f32,
    profile: &Profile,
) {
    let rounding = cx.px(2.0);
    cx.draw_avatar(
        draw,
        vec2(min.x + size / 2.0, min.y + size / 2.0),
        size,
        &author_of(profile),
        Some(rounding),
    );
}

/// react-select's focused control: a 1px #2684FF border ring around `[min, max]`.
pub fn draw_select_focus(cx: &Cx<'_>, draw: DrawList<'_>, min: Vec2, max: Vec2, rounding: f32) {
    draw.add_rect(
        vec2(min.x - cx.px(0.5), min.y - cx.px(0.5)),
        vec2(max.x + cx.px(0.5), max.y + cx.px(0.5)),
        cx.palette.accent,
        rounding + cx.px(0.5),
        cx.px(1.0),
    );
}

/// The profile picker's react-select menu for popup `id` (opened by the caller's control at `[control_min,
/// control_max]`): 8px below the control, a 45px option per profile with its 28px icon. Returns the guid picked,
/// `None` when none was.
pub fn profile_select_menu(
    cx: &mut Cx<'_>,
    app: &Application,
    id: &str,
    control_min: Vec2,
    control_max: Vec2,
    style: ProfileMenuStyle,
) -> Option<String> {
    let ui = cx.ui;
    if !ui.is_popup_open(id) {
        return None;
    }
    let profiles = app.profiles.all();
    let content = OPTION_HEIGHT * profiles.len() as f32;
    let list_height = MAX_LIST_HEIGHT.min(4.0 + content + 4.0 + 2.0);
    let min = vec2(control_min.x, control_max.y + cx.px(8.0));
    let max = vec2(control_max.x, min.y + cx.px(list_height));
    raw::set_next_window_pos(ui, min, Vec2::ZERO);
    raw::set_next_window_size(ui, max - min);
    // Held for the whole popup: the safe API pops style in the scope it pushed it.
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::PopupBorderSize(0.0));
    let _spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));
    let _background = ui.push_style_color(StyleColor::PopupBg, Color::TRANSPARENT);
    let popup = ui.begin_popup_with_flags(
        id,
        WindowFlags::NO_MOVE | WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE,
    )?;
    // The option the keyboard / mouse is on starts at the current profile.
    if ui.is_window_appearing() {
        let menu = &mut cx.state.profile_menu;
        menu.scroll = 0.0;
        menu.focused = profiles
            .iter()
            .position(|profile| profile.guid == app.profiles.current().guid)
            .unwrap_or(0);
    }
    let (mut focused, mut scroll) = (cx.state.profile_menu.focused, cx.state.profile_menu.scroll);
    let p = cx.palette;
    let draw = cx.window_draw_list();
    draw.push_clip_rect_full_screen();
    // 0 0 0 1px and 0 4px 11px rgba(0, 0, 0, .1)
    let shadow = Color::new(0.0, 0.0, 0.0, 0.1);
    draw_box_shadow(
        draw,
        min,
        max,
        Vec2::ZERO,
        0.0,
        cx.px(1.0),
        shadow,
        cx.px(4.0),
    );
    draw_box_shadow(
        draw,
        min,
        max,
        vec2(0.0, cx.px(4.0)),
        cx.px(11.0),
        0.0,
        shadow,
        cx.px(4.0),
    );
    let border = style.border().of(p);
    draw.add_rect_filled(min, max, style.background(p), cx.px(4.0));
    draw.add_rect(min, max, border, cx.px(4.0), cx.px(1.0));
    draw.pop_clip_rect();

    let list_min = vec2(min.x + cx.px(1.0), min.y + cx.px(5.0));
    let list_max = vec2(max.x - cx.px(1.0), max.y - cx.px(5.0));
    let max_scroll = (cx.px(content) - (list_max.y - list_min.y)).max(0.0);
    if ui.is_window_hovered() {
        scroll = (scroll - ui.io().mouse_wheel() * cx.px(OPTION_HEIGHT)).clamp(0.0, max_scroll);
    }
    let last = profiles.len().saturating_sub(1);
    if ui.is_key_pressed(Key::DownArrow) {
        focused = (focused + 1).min(last);
    }
    if ui.is_key_pressed(Key::UpArrow) {
        focused = focused.saturating_sub(1);
    }
    let mut picked = None;
    if ui.is_key_pressed(Key::Enter) && focused < profiles.len() {
        picked = Some(profiles[focused].guid.clone());
    }
    if ui.is_key_pressed(Key::Escape) {
        ui.close_current_popup();
    }
    draw.push_clip_rect(list_min, list_max, true);
    for (i, profile) in profiles.iter().enumerate() {
        let row_min = vec2(
            list_min.x,
            list_min.y + cx.px(OPTION_HEIGHT) * i as f32 - scroll,
        );
        let row_max = vec2(list_max.x, row_min.y + cx.px(OPTION_HEIGHT));
        if row_max.y <= list_min.y || row_min.y >= list_max.y {
            continue;
        }
        cx.set_cursor(row_min);
        let clicked = {
            let _id = ui.push_id(i as i32);
            ui.invisible_button("##option", row_max - row_min)
        };
        if clicked {
            picked = Some(profile.guid.clone());
        }
        if ui.is_item_hovered() && ui.is_mouse_hovering_rect(list_min.into(), list_max.into()) {
            focused = i;
        }
        if focused == i {
            draw.add_rect_filled(
                row_min,
                vec2(row_max.x, row_max.y - cx.px(1.0)),
                p.hover_row,
                0.0,
            );
        }
        draw.add_rect_filled(
            vec2(row_min.x, row_max.y - cx.px(1.0)),
            row_max,
            border,
            0.0,
        );
        let icon = vec2(row_min.x + cx.px(12.0), row_min.y + cx.px(8.0));
        draw_profile_icon(cx, draw, icon, cx.px(28.0), profile);
        let text_x = icon.x + cx.px(38.0);
        line_text(
            cx,
            draw,
            text_x,
            row_min.y + cx.px(12.0),
            style.text().of(p),
            &profile.name,
            row_max.x - cx.px(12.0) - text_x,
        );
    }
    draw.pop_clip_rect();
    if picked.is_some() {
        ui.close_current_popup();
    }
    cx.state.profile_menu.focused = focused;
    cx.state.profile_menu.scroll = scroll;
    drop(popup);
    let _ = MouseButton::Left;
    picked
}
