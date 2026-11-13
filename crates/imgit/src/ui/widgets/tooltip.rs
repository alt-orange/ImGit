//! The (Bootstrap) tooltips: Arial 12px white on black, rad4, a 10x5 arrow, 90% opacity, fading in after a
//! short hover.

use dear_imgui_rs::{WindowHoveredFlags, sys};

use crate::theme::Color;
use crate::ui::Cx;
use crate::ui::geom::{Rect, Vec2, vec2};

/// Which tooltip is waiting or showing (the C++ function statics).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TooltipState {
    /// The text and anchor the timer runs for.
    shown_for: sys::ImGuiID,
    /// When it was first asked for.
    since: f64,
    /// The last frame it was asked for in: a gap restarts the timer.
    last_frame: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TooltipSide {
    Bottom,
    Top,
    Left,
}

/// The seconds a tooltip waits before it shows, below or above its anchor.
const WAIT: f32 = 0.35;
/// `placement="left"` uses `delayShow` 250.
const WAIT_LEFT: f32 = 0.25;
/// The fade-in: `.15s`.
const FADE: f32 = 0.15;

impl Cx<'_> {
    fn show_tooltip(&mut self, anchor: Rect, text: &str, side: TooltipSide, wait: f32) {
        // Bootstrap's OverlayTrigger shows it after a short delay and fades it in over .15s.
        let text = self.shortcut_label(text);
        let text = text.as_str();
        let key = self.id(text) ^ (anchor.min.x * 31.0 + anchor.min.y) as sys::ImGuiID;
        let now = self.time();
        let frame = self.frame_count();
        let state = &mut self.state.tooltip;
        if state.shown_for != key || state.last_frame < frame - 1 {
            state.shown_for = key;
            state.since = now;
        }
        state.last_frame = frame;
        let elapsed = (now - state.since) as f32;
        if elapsed < wait {
            return;
        }
        let alpha = 0.9 * ((elapsed - wait) / FADE).clamp(0.0, 1.0);
        let font = self.fonts.arial;
        let size = self.text_size(font, 12.0, text);
        let inner_w = (size.x + self.px(16.0)).round();
        let inner_h = self.px(23.1).round();
        let cx = ((anchor.min.x + anchor.max.x) / 2.0).round();
        let viewport = self.ui.main_viewport();
        let (viewport_x, viewport_w) = (viewport.pos()[0], viewport.size()[0]);
        let mut x = (cx - inner_w / 2.0).round();
        // `std::clamp(x, low, high)` as it behaves with its bounds crossed (a tooltip wider than the window).
        let (low, high) = (
            viewport_x + self.px(2.0),
            viewport_x + viewport_w - inner_w - self.px(2.0),
        );
        x = if x < low {
            low
        } else if high < x {
            high
        } else {
            x
        };
        let draw = self.foreground_draw_list();
        // IM_COL32 truncates the alpha.
        let alpha_8 = (alpha * 255.0) as u8;
        let black = Color::from_rgba8(0, 0, 0, alpha_8);
        let white = Color::from_rgba8(255, 255, 255, alpha_8);
        let arrow = self.px(5.0);
        let top;
        match side {
            TooltipSide::Left => {
                // .tooltip.left: margin-left -3px, padding 0 5px with the 5x10 arrow in it.
                let tip = (anchor.min.x - self.px(3.0)).round();
                x = tip - arrow - inner_w;
                top = ((anchor.min.y + anchor.max.y) / 2.0 - inner_h / 2.0).round();
                let cy = top + inner_h / 2.0;
                draw.add_triangle_filled(
                    vec2(tip - arrow, cy - arrow),
                    vec2(tip, cy),
                    vec2(tip - arrow, cy + arrow),
                    black,
                );
            }
            TooltipSide::Top => {
                let arrow_tip = (anchor.min.y + self.px(3.0)).round();
                top = arrow_tip - arrow - inner_h;
                draw.add_triangle_filled(
                    vec2(cx - arrow, arrow_tip - arrow),
                    vec2(cx + arrow, arrow_tip - arrow),
                    vec2(cx, arrow_tip),
                    black,
                );
            }
            TooltipSide::Bottom => {
                let arrow_tip = (anchor.max.y - self.px(3.0)).round();
                top = arrow_tip + arrow;
                draw.add_triangle_filled(
                    vec2(cx, arrow_tip),
                    vec2(cx + arrow, top),
                    vec2(cx - arrow, top),
                    black,
                );
            }
        }
        draw.add_rect_filled(
            vec2(x, top),
            vec2(x + inner_w, top + inner_h),
            black,
            self.px(4.0),
        );
        // 17.14px line box in 23.1 (3px padding): the 14px Arial text box 1.6px into it.
        let ascent = 12.0 * 1854.0 / 2048.0 * self.scale;
        let baseline =
            (top + self.px(3.0) + self.px((17.1429 - 14.0) / 2.0) + ascent.round()).round();
        // Chromium composites the faded tooltip on its own layer: grey-scale anti-aliasing.
        draw.set_grayscale_text(true);
        self.draw_text(
            draw,
            font,
            12.0,
            vec2(x + self.px(8.0), baseline - ascent.ceil()),
            white,
            text,
        );
        draw.set_grayscale_text(false);
    }

    /// The tooltip for an explicit anchor rectangle, under it or (`above`) over it. Call it every frame the
    /// anchor is hovered.
    pub fn styled_tooltip(&mut self, anchor_min: Vec2, anchor_max: Vec2, text: &str, above: bool) {
        let side = if above {
            TooltipSide::Top
        } else {
            TooltipSide::Bottom
        };
        self.show_tooltip(Rect::new(anchor_min, anchor_max), text, side, WAIT);
    }

    /// `placement="left"` (`delayShow` 250): vertically centred, its arrow 3px left of the anchor.
    pub fn styled_tooltip_left(&mut self, anchor_min: Vec2, anchor_max: Vec2, text: &str) {
        self.show_tooltip(
            Rect::new(anchor_min, anchor_max),
            text,
            TooltipSide::Left,
            WAIT_LEFT,
        );
    }

    /// The tooltip under the last item, while it is hovered.
    pub fn tooltip(&mut self, text: &str) {
        if self.ui.is_item_hovered() {
            let rect = self.item_rect();
            self.styled_tooltip(rect.min, rect.max, text, false);
        }
    }

    /// The tooltip for an area drawn without an item (the mouse inside `[min, max)` of the hovered window).
    pub fn tooltip_on_rect(&mut self, min: Vec2, max: Vec2, text: &str, above: bool) {
        if self
            .ui
            .is_window_hovered_with_flags(WindowHoveredFlags::CHILD_WINDOWS)
            && Rect::new(min, max).contains(self.mouse_pos())
        {
            self.styled_tooltip(min, max, text, above);
        }
    }
}
