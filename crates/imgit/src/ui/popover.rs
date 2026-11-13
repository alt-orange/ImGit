//! The HTML dropdowns (bootstrap `.dropdown-menu`): a toolbar-bg2 box with a `0 6px 12px rgba(0,0,0,.176)`
//! shadow, an optional focused search box, and 27px rows of 12px text that tint on hover (hover-row) or mark the
//! current entry in green.
//!
//! A popover is built inside [`Cx::popover`]'s closure, its rows laid out by the `popover_*` methods:
//!
//! ```ignore
//! cx.open_popover("##branches");           // when the button is clicked
//! cx.popover("##branches", anchor, 300.0, |cx| {
//!     cx.popover_padding(5.0);
//!     if cx.popover_row("main", Some("far:code-branch"), true) { /* ... */ cx.close_popover(); }
//! });
//! ```

use std::collections::HashMap;

use dear_imgui_rs::{InputTextFlags, MouseCursor, StyleColor, StyleVar, WindowFlags, sys};

use crate::theme::{Color, Ink};
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};
use crate::ui::{Cx, raw};

/// 5 + 17.1429 (12px / line-height 1.4286) + 5.
const ROW_HEIGHT: f32 = 27.1429;
const HEADER_HEIGHT: f32 = 25.1429;
const SHADOW: Color = Color::new(0.0, 0.0, 0.0, 0.176);
const TEXT: Ink = Ink(0.9);
const SECONDARY: Ink = Ink(0.6);

/// What one popover remembers between frames.
#[derive(Debug, Clone, Copy)]
struct PopoverState {
    just_opened: bool,
    /// The layout cursor (logical px from the popover's top), fractional like Chromium's.
    y: f32,
    width: f32,
    origin: Vec2,
    row: i32,
    /// The row the keyboard selected, -1 for none.
    keyboard: i32,
    enter: bool,
}

impl Default for PopoverState {
    fn default() -> Self {
        Self {
            just_opened: false,
            y: 0.0,
            width: 0.0,
            origin: Vec2::ZERO,
            row: 0,
            keyboard: -1,
            enter: false,
        }
    }
}

/// The popovers' state, by their ids. Lives in `UiState::popovers`.
#[derive(Debug, Default)]
pub struct Popovers {
    states: HashMap<sys::ImGuiID, PopoverState>,
    /// The popover being built.
    current: Option<sys::ImGuiID>,
}

impl Popovers {
    fn state(&mut self) -> &mut PopoverState {
        let id = self
            .current
            .expect("popover rows are built inside Cx::popover");
        self.states.entry(id).or_default()
    }
}

/// The error function's complement (Numerical Recipes' `erfcc`, fractional error below 1.2e-7).
#[allow(clippy::excessive_precision)] // the published coefficients
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let polynomial = -z * z - 1.265_512_2
        + t * (1.000_023_7
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let answer = t * polynomial.exp();
    if x >= 0.0 { answer } else { 2.0 - answer }
}

/// Fraction of a Gaussian-blurred edge past distance `d` (0.5 * erfc(d / (sigma * sqrt 2))).
fn gauss_tail(d: f32, sigma: f32) -> f32 {
    0.5 * erfc(d / (sigma * std::f32::consts::SQRT_2))
}

fn snap(p: Vec2) -> Vec2 {
    p.round()
}

/// CSS box-shadow (outer): a Gaussian of `blur` px around `[min, max]` moved by `offset`.
#[allow(clippy::too_many_arguments)] // the CSS box-shadow's own parameters
pub fn draw_box_shadow(
    draw: DrawList<'_>,
    min: Vec2,
    max: Vec2,
    offset: Vec2,
    blur: f32,
    spread: f32,
    color: Color,
    rounding: f32,
) {
    // Concentric 1px rings: each pixel ring at signed distance d from the shadow box gets the Gaussian tail.
    let sigma = (blur / 2.0).max(0.5);
    let shadow_min = vec2(min.x + offset.x - spread, min.y + offset.y - spread);
    let shadow_max = vec2(max.x + offset.x + spread, max.y + offset.y + spread);
    draw.push_clip_rect_full_screen();
    let reach = (blur * 1.5).ceil() as i32;
    for i in -reach..reach {
        let d = i as f32 + 0.5;
        let alpha = color.a * gauss_tail(d, sigma);
        if alpha < 0.002 {
            continue;
        }
        let grow = d;
        if shadow_max.x - shadow_min.x + 2.0 * grow <= 0.0
            || shadow_max.y - shadow_min.y + 2.0 * grow <= 0.0
        {
            continue;
        }
        draw.add_rect(
            vec2(shadow_min.x - grow, shadow_min.y - grow),
            vec2(shadow_max.x + grow, shadow_max.y + grow),
            color.with_alpha(alpha),
            (rounding + grow).max(0.0),
            1.0,
        );
    }
    draw.pop_clip_rect();
}

impl Cx<'_> {
    /// Rows react to the mouse and to the keyboard selection: `(clicked, hovered)`.
    fn popover_row_interaction(&mut self, min: Vec2, max: Vec2) -> (bool, bool) {
        let ui = self.ui;
        let state = self.state.popovers.state();
        let index = state.row;
        state.row += 1;
        let keyboard = state.keyboard;
        let enter = state.enter;
        self.set_cursor(min);
        let clicked = {
            let _id = ui.push_id(index);
            ui.invisible_button("##row", max - min)
        };
        let item_hovered = ui.is_item_hovered();
        let hovered = item_hovered || keyboard == index;
        if item_hovered && ui.io().mouse_delta()[1] != 0.0 {
            self.state.popovers.state().keyboard = -1;
        }
        if hovered {
            ui.set_mouse_cursor(Some(MouseCursor::Hand));
        }
        (clicked || (enter && keyboard == index), hovered)
    }

    /// Opens the popover `id` (the next [`Self::popover`] call with it shows it).
    pub fn open_popover(&mut self, id: &str) {
        raw::open_popup(self.ui, id);
        let key = self.id(id);
        self.state
            .popovers
            .states
            .entry(key)
            .or_default()
            .just_opened = true;
    }

    /// Closes the popover being built.
    pub fn close_popover(&self) {
        self.ui.close_current_popup();
    }

    /// Builds popover `id` at `pos` (screen px), `width` logical px wide, when it is open: `content` lays out its
    /// rows with the `popover_*` methods. The content height is measured on the previous frame. Closes on a
    /// click outside or Escape. `None` while it is closed.
    pub fn popover<R>(
        &mut self,
        id: &str,
        pos: Vec2,
        width: f32,
        content: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        let ui = self.ui;
        if !ui.is_popup_open(id) {
            return None;
        }
        let key = self.id(id);
        raw::set_next_window_pos(ui, snap(pos), Vec2::ZERO);
        raw::set_next_window_size(ui, vec2(self.px(width), 0.0));
        // Held for the whole popup: the safe API pops style in the scope it pushed it.
        let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
        let _border = ui.push_style_var(StyleVar::PopupBorderSize(0.0));
        let _rounding = ui.push_style_var(StyleVar::PopupRounding(0.0));
        let _spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));
        let _background = ui.push_style_color(StyleColor::PopupBg, Color::TRANSPARENT);
        let popup = ui.begin_popup_with_flags(
            id,
            WindowFlags::NO_MOVE | WindowFlags::ALWAYS_AUTO_RESIZE | WindowFlags::NO_SCROLLBAR,
        )?;
        let origin: Vec2 = ui.window_pos().into();
        let enter = ui.is_key_pressed(dear_imgui_rs::Key::Enter)
            || ui.is_key_pressed(dear_imgui_rs::Key::KeypadEnter);
        {
            let state = self.state.popovers.states.entry(key).or_default();
            state.y = 0.0;
            state.width = width;
            state.origin = origin;
            state.row = 0;
            state.enter = enter;
        }
        self.state.popovers.current = Some(key);
        let draw = self.window_draw_list();
        draw.channels_split(2);
        draw.channels_set_current(1);
        if ui.is_key_pressed(dear_imgui_rs::Key::Escape) {
            ui.close_current_popup();
        }
        let result = content(self);

        let state = self.state.popovers.states.entry(key).or_default();
        if ui.is_key_pressed(dear_imgui_rs::Key::DownArrow) {
            state.keyboard = (state.keyboard + 1).min(state.row - 1);
        }
        if ui.is_key_pressed(dear_imgui_rs::Key::UpArrow) {
            state.keyboard = (state.keyboard - 1).max(0);
        }
        let state = *state;
        // Reserve the laid-out height so the window sizes to it.
        self.set_cursor(state.origin);
        ui.dummy(vec2(self.px(state.width), self.px(state.y)));
        draw.channels_set_current(0);
        let min = state.origin;
        let max = vec2(
            min.x + self.px(state.width),
            min.y + self.px(state.y).round(),
        );
        draw_box_shadow(
            draw,
            min,
            max,
            vec2(0.0, self.px(6.0)),
            self.px(12.0),
            0.0,
            SHADOW,
            0.0,
        );
        draw.add_rect_filled(min, max, self.palette.toolbar_bg2, 0.0);
        draw.channels_merge();
        self.state
            .popovers
            .states
            .entry(key)
            .or_default()
            .just_opened = false;
        self.state.popovers.current = None;
        drop(popup);
        Some(result)
    }

    /// Forgets the keyboard's selected row (the content changed under it).
    pub fn popover_reset_keyboard(&mut self) {
        if self.state.popovers.current.is_some() {
            self.state.popovers.state().keyboard = -1;
        }
    }

    /// The search box: a 34px field with the magnifier, focused when the popover opens. `true` when the text
    /// changed.
    pub fn popover_search(&mut self, text: &mut String, placeholder: &str) -> bool {
        // .pl1.pr1.pt1 (5px) around a 34px .form-control with 40px left padding; focused: #4D88FF border and a
        // 1px #4D88FF ring.
        let ui = self.ui;
        let s = *self.state.popovers.state();
        let p = self.palette;
        let draw = self.window_draw_list();
        let min = snap(vec2(
            s.origin.x + self.px(5.0),
            s.origin.y + self.px(s.y + 5.0),
        ));
        let max = vec2(min.x + self.px(s.width - 10.0), min.y + self.px(34.0));
        draw.add_rect_filled(
            vec2(min.x - 1.0, min.y - 1.0),
            vec2(max.x + 1.0, max.y + 1.0),
            p.accent,
            self.px(5.0),
        );
        draw.add_rect_filled(min, max, p.toolbar_bg2, self.px(4.0));
        draw.add_rect_filled(
            vec2(min.x + 1.0, min.y + 1.0),
            vec2(max.x - 1.0, max.y - 1.0),
            Color::new(0.0, 0.0, 0.0, 0.2),
            self.px(3.0),
        );
        // The border is drawn over the inner fill's edge pixels: 1px #4D88FF.
        draw.add_rect(
            vec2(min.x + 0.5, min.y + 0.5),
            vec2(max.x - 0.5, max.y - 0.5),
            p.accent,
            self.px(4.0),
            1.0,
        );
        self.draw_fa_icon(
            draw,
            "far:search",
            vec2(min.x + self.px(17.0), min.y + self.px(17.0)),
            self.px(14.0),
            SECONDARY.of(p),
        );

        let font = self.fonts.regular;
        let ascent = 14.0 * 2189.0 / 2048.0 * self.scale;
        let baseline = (min.y + self.px((34.0 - 19.0) / 2.0) + ascent.round()).round();
        self.set_cursor(vec2(min.x + self.px(40.0), baseline - ascent.ceil()));
        if s.just_opened {
            ui.set_keyboard_focus_here();
        }
        let changed = {
            let _background = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
            let _text = ui.push_style_color(StyleColor::Text, TEXT.of(p));
            let _disabled = ui.push_style_color(StyleColor::TextDisabled, p.ink(0.4));
            let _padding = ui.push_style_var(StyleVar::FramePadding([0.0, 0.0]));
            let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
            let _font = self.push_font(font, 14.0);
            self.input_text(
                "##popover_search",
                text,
                Some(placeholder),
                InputTextFlags::empty(),
                max.x - min.x - self.px(52.0),
            )
        };
        let state = self.state.popovers.state();
        if changed {
            state.keyboard = 0;
        }
        state.y += 44.0;
        changed
    }

    /// `li.dropdown-header`: 12px secondary text after a 12px icon (`far:clock`).
    pub fn popover_header(&mut self, text: &str, icon: Option<&str>) {
        // padding 5px 20px 3px; the icon (12px) then 5px; 12px text-secondary.
        let s = *self.state.popovers.state();
        let draw = self.window_draw_list();
        let secondary = SECONDARY.of(self.palette);
        let top = s.origin.y + self.px(s.y + 5.0);
        let mut x = s.origin.x + self.px(20.0);
        if let Some(icon) = icon {
            self.draw_fa_icon(
                draw,
                icon,
                vec2(x + self.px(6.0), top.round() + self.px(8.5)),
                self.px(12.0),
                secondary,
            );
            x += self.px(17.0);
        }
        self.draw_text(
            draw,
            self.fonts.regular,
            12.0,
            vec2(x, top.round()),
            secondary,
            text,
        );
        self.state.popovers.state().y += HEADER_HEIGHT;
    }

    /// The row's rectangle at the layout cursor.
    fn popover_row_rect(&self, s: &PopoverState, height: f32) -> (Vec2, Vec2) {
        (
            vec2(s.origin.x, (s.origin.y + self.px(s.y)).round()),
            vec2(
                s.origin.x + self.px(s.width),
                (s.origin.y + self.px(s.y + height)).round(),
            ),
        )
    }

    /// `li > a`: `true` when clicked. `current` marks the checked-out branch / open repository.
    pub fn popover_row(&mut self, text: &str, icon: Option<&str>, current: bool) -> bool {
        let s = *self.state.popovers.state();
        let p = self.palette;
        let draw = self.window_draw_list();
        let (min, max) = self.popover_row_rect(&s, ROW_HEIGHT);
        let (clicked, hovered) = self.popover_row_interaction(min, max);
        if current {
            draw.add_rect_filled(min, max, p.checked_out, 0.0);
        } else if hovered {
            draw.add_rect_filled(min, max, p.hover_row, 0.0);
        }
        let color = if current || hovered {
            p.ink(1.0)
        } else {
            TEXT.of(p)
        };
        let top = (s.origin.y + self.px(s.y + 5.0)).round();
        let mut x = s.origin.x + self.px(20.0);
        if let Some(icon) = icon {
            self.draw_fa_icon(
                draw,
                icon,
                vec2(x + self.px(4.5), top + self.px(8.5)),
                self.px(12.0),
                color,
            );
            x += self.px(14.0);
        }
        self.draw_text_ellipsis(
            draw,
            self.fonts.regular,
            12.0,
            vec2(x, top),
            s.origin.x + self.px(s.width - 20.0) - x,
            color,
            text,
        );
        self.state.popovers.state().y += ROW_HEIGHT;
        clicked
    }

    /// A wrapped 12px secondary paragraph with 10px padding (the pull dropdown's explanation).
    pub fn popover_note(&mut self, text: &str) {
        let s = *self.state.popovers.state();
        let draw = self.window_draw_list();
        let wrap = self.px(s.width - 20.0);
        let font = self.fonts.regular;
        let top = (s.origin.y + self.px(s.y + 10.0)).round();
        let lines = self.wrap_text(font, 12.0, text, wrap);
        let lines: Vec<&str> = if lines.is_empty() { vec![""] } else { lines };
        let secondary = SECONDARY.of(self.palette);
        for (i, line) in lines.iter().enumerate() {
            // line-height 17.14px for 12px text: the 17px text box sits at the top of each line.
            self.draw_text(
                draw,
                font,
                12.0,
                vec2(
                    s.origin.x + self.px(10.0),
                    top + self.px(17.1429 * i as f32).round(),
                ),
                secondary,
                line.trim_end(),
            );
        }
        self.state.popovers.state().y += 20.0 + 17.1429 * lines.len() as f32;
    }

    /// The pull dropdown's rows: 14px circle / dot-circle, 12px text, 5px 10px padding; `selected` in green.
    pub fn popover_radio_row(&mut self, text: &str, selected: bool) -> bool {
        let s = *self.state.popovers.state();
        let p = self.palette;
        let draw = self.window_draw_list();
        let (min, max) = self.popover_row_rect(&s, ROW_HEIGHT);
        let (clicked, hovered) = self.popover_row_interaction(min, max);
        if selected {
            draw.add_rect_filled(min, max, p.checked_out, 0.0);
        } else if hovered {
            draw.add_rect_filled(min, max, p.hover_row, 0.0);
        }
        let color = if selected { p.ink(1.0) } else { TEXT.of(p) };
        let top = (s.origin.y + self.px(s.y + 5.0)).round();
        self.draw_fa_icon(
            draw,
            if selected {
                "far:dot-circle"
            } else {
                "far:circle"
            },
            vec2(
                s.origin.x + self.px(17.0),
                (s.origin.y + self.px(s.y + 6.5)).round() + self.px(7.0),
            ),
            self.px(14.0),
            if selected { color } else { SECONDARY.of(p) },
        );
        self.draw_text(
            draw,
            self.fonts.regular,
            12.0,
            vec2(s.origin.x + self.px(34.0), top),
            color,
            text,
        );
        self.state.popovers.state().y += ROW_HEIGHT;
        clicked
    }

    /// Empty space in the layout (the `.dropdown-menu`'s 5px top / bottom padding), in logical px.
    pub fn popover_padding(&mut self, height: f32) {
        self.state.popovers.state().y += height;
    }

    /// `li.lfs-option > a`: 12px text with 6px 12px padding (29.14px rows), tinted on hover.
    pub fn popover_menu_item(&mut self, text: &str) -> bool {
        const HEIGHT: f32 = 6.0 + 17.1429 + 6.0;
        let s = *self.state.popovers.state();
        let p = self.palette;
        let draw = self.window_draw_list();
        let (min, max) = self.popover_row_rect(&s, HEIGHT);
        let (clicked, hovered) = self.popover_row_interaction(min, max);
        if hovered {
            draw.add_rect_filled(min, max, p.hover_row, 0.0);
        }
        self.draw_text(
            draw,
            self.fonts.regular,
            12.0,
            vec2(
                s.origin.x + self.px(12.0),
                (s.origin.y + self.px(s.y + 6.0)).round(),
            ),
            if hovered { p.ink(1.0) } else { TEXT.of(p) },
            text,
        );
        self.state.popovers.state().y += HEIGHT;
        clicked
    }

    /// A row whose label is a btn-link ("View all repositories").
    pub fn popover_link_row(&mut self, text: &str) -> bool {
        let s = *self.state.popovers.state();
        let p = self.palette;
        let draw = self.window_draw_list();
        let (min, max) = self.popover_row_rect(&s, ROW_HEIGHT);
        let (clicked, hovered) = self.popover_row_interaction(min, max);
        if hovered {
            draw.add_rect_filled(min, max, p.hover_row, 0.0);
        }
        let top = (s.origin.y + self.px(s.y + 5.0)).round();
        let x = s.origin.x + self.px(23.0);
        self.draw_text(draw, self.fonts.regular, 12.0, vec2(x, top), p.link, text);
        if hovered {
            let w = self.text_size(self.fonts.regular, 12.0, text).x;
            draw.add_rect_filled(
                vec2(x, top + self.px(14.0)),
                vec2(x + w, top + self.px(15.0)),
                p.link,
                0.0,
            );
        }
        self.state.popovers.state().y += ROW_HEIGHT;
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_function_matches_known_values() {
        for (x, expected) in [
            (0.0_f32, 1.0_f32),
            (0.5, 0.479_500_1),
            (1.0, 0.157_299_2),
            (2.0, 0.004_677_7),
            (-1.0, 1.842_700_8),
        ] {
            assert!((erfc(x) - expected).abs() < 1e-5, "erfc({x}) = {}", erfc(x));
        }
    }

    #[test]
    fn a_gaussian_edge_is_half_at_the_edge_and_fades_out() {
        assert!((gauss_tail(0.0, 6.0) - 0.5).abs() < 1e-6);
        assert!(gauss_tail(6.0, 6.0) < 0.2);
        assert!(gauss_tail(-6.0, 6.0) > 0.8);
        assert!(gauss_tail(30.0, 6.0) < 1e-6);
    }
}
