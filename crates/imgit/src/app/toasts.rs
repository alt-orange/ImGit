//! Notifications in a corner of the window. Every toast is also kept, newest first, for the notifications
//! popover (read / unread).

use std::time::{SystemTime, UNIX_EPOCH};

use dear_imgui_rs::{Condition, DrawFlags, StyleVar, WindowFlags};

use crate::ui::geom::{Rect, Vec2, vec2};
use crate::ui::icons::svg_path;
use crate::ui::widgets::ease_in_out;
use crate::ui::{Cx, raw};

/// How long a toast stays: `TOAST_DURATION_DEFAULT`.
const LIFETIME: f64 = 10.0;
/// How many toasts the history keeps.
const MAX_HISTORY: usize = 50;
/// How long a card takes to slide in or out.
const SLIDE_SECONDS: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

/// The window corner the toast cards stack in (Preferences > UI Customization > Notification Location).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastCorner {
    TopLeft,
    TopRight,
    #[default]
    BottomLeft,
    BottomRight,
}

/// "Top Right" -> `TopRight`; `BottomLeft` for anything else.
#[must_use]
pub fn toast_corner(name: &str) -> ToastCorner {
    match name {
        "Top Left" => ToastCorner::TopLeft,
        "Top Right" => ToastCorner::TopRight,
        "Bottom Right" => ToastCorner::BottomRight,
        _ => ToastCorner::BottomLeft,
    }
}

/// A toast as the notifications popover lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: ToastKind,
    pub title: String,
    pub detail: String,
    /// Unix seconds.
    pub time: i64,
    pub read: bool,
}

/// A card on screen.
#[derive(Debug, Clone)]
struct Toast {
    kind: ToastKind,
    title: String,
    detail: String,
    /// The slide in starts here (the toasts' clock).
    shown_at: f64,
    expires_at: f64,
    /// The slide out started (expired or dismissed).
    hidden_at: Option<f64>,
}

/// The toast cards and their history.
#[derive(Debug, Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
    /// This frame's rectangles, cleared by [`Self::draw`].
    keep_clear: Vec<Rect>,
    /// Newest first.
    history: Vec<Entry>,
    /// The clock the slides run on (ImGui's time), set once per frame by [`Self::set_time`]: a toast pushed
    /// during a frame starts with it.
    now: f64,
}

impl Toasts {
    /// The clock of this frame (`ImGui::GetTime()`); call at the start of every frame.
    pub fn set_time(&mut self, now: f64) {
        self.now = now;
    }

    /// A toast with a title only.
    pub fn push(&mut self, kind: ToastKind, title: impl Into<String>) {
        self.push_with_detail(kind, title, "");
    }

    /// A toast with a title and a second line of secondary text.
    pub fn push_with_detail(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) {
        let title = title.into();
        let detail = detail.into();
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
            .unwrap_or_default();
        self.push_entry(Entry {
            kind,
            title: title.clone(),
            detail: detail.clone(),
            time,
            read: false,
        });
        self.toasts.push(Toast {
            kind,
            title,
            detail,
            shown_at: self.now,
            expires_at: self.now + LIFETIME,
            hidden_at: None,
        });
    }

    fn push_entry(&mut self, entry: Entry) {
        self.history.insert(0, entry);
        self.history.truncate(MAX_HISTORY);
    }

    /// Keeps this frame's toasts off a screen rectangle they would otherwise cover (buttons near the bottom or
    /// the top of the window): the stack starts past it.
    pub fn keep_clear(&mut self, min: Vec2, max: Vec2) {
        self.keep_clear.push(Rect::new(min, max));
    }

    /// Every toast shown so far (at most 50), newest first.
    #[must_use]
    pub fn history(&self) -> &[Entry] {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut [Entry] {
        &mut self.history
    }

    /// Removes history entry `index` (the notifications popover's trash); nothing when there is
    /// no such entry.
    pub fn remove_history(&mut self, index: usize) {
        if index < self.history.len() {
            self.history.remove(index);
        }
    }

    /// How many of them are not read yet.
    #[must_use]
    pub fn unread(&self) -> usize {
        self.history.iter().filter(|entry| !entry.read).count()
    }

    pub fn mark_all_read(&mut self) {
        for entry in &mut self.history {
            entry.read = true;
        }
    }

    /// Expires and removes the cards whose time is up. `true` when any remain.
    fn tick(&mut self, now: f64) -> bool {
        for toast in &mut self.toasts {
            if toast.hidden_at.is_none() && toast.expires_at < now {
                toast.hidden_at = Some(now);
            }
        }
        self.toasts.retain(|toast| {
            toast
                .hidden_at
                .is_none_or(|hidden| hidden + SLIDE_SECONDS > now)
        });
        !self.toasts.is_empty()
    }

    /// How far in a card is: 0 fully off its edge, 1 in place; whether it is still moving.
    fn slide(toast: &Toast, now: f64) -> (f32, bool) {
        let slide_in = (now - toast.shown_at) / SLIDE_SECONDS;
        let mut visible = ease_in_out(slide_in as f32);
        if let Some(hidden) = toast.hidden_at {
            let out = (now - hidden) / SLIDE_SECONDS;
            visible = visible.min(1.0 - ease_in_out(out as f32));
        }
        (visible, slide_in < 1.0 || toast.hidden_at.is_some())
    }

    /// The toast cards: in `corner`, 16px in from the side and 32px above the window bottom (below the tabs
    /// bar and toolbar at the top), 352px wide, at least 80px tall, newest nearest the edge, 8px apart. A 36px
    /// status column with the white icon, the 16px title and 14px secondary text with 16px padding, and the
    /// x in a 31px column. Clicking a card dismisses it. A card slides in from its side's edge over 0.3 s and
    /// back out when it expires or is dismissed; its slot in the stack grows and shrinks with it, so the cards
    /// above settle instead of jumping. Call once per frame, after the main window.
    pub fn draw(&mut self, cx: &mut Cx<'_>, corner: ToastCorner) {
        let ui = cx.ui;
        let now = cx.time();
        self.now = now;
        let any = self.tick(now);
        let keep_clear = std::mem::take(&mut self.keep_clear);
        if !any {
            return;
        }
        let p = cx.palette;
        let (viewport_pos, viewport_size) = raw::viewport(ui);
        let (work_pos, _) = raw::work_area(ui);
        let width = cx.px(352.0);
        let text_w = cx.px(252.7);
        let right = matches!(corner, ToastCorner::TopRight | ToastCorner::BottomRight);
        let top = matches!(corner, ToastCorner::TopLeft | ToastCorner::TopRight);
        let x = if right {
            viewport_pos.x + viewport_size.x - cx.px(16.0) - width
        } else {
            viewport_pos.x + cx.px(16.0)
        };
        // The edge the next card stacks from: the bottom one's bottom, or the top one's top (under the tabs bar
        // and the toolbar); moved past any rectangle asked to be kept clear that the first card would cover.
        let mut edge = if top {
            work_pos.y + cx.px(96.0)
        } else {
            viewport_pos.y + viewport_size.y - cx.px(32.0)
        };
        for clear in &keep_clear {
            if clear.min.x >= x + width || clear.max.x <= x {
                continue;
            }
            if !top && clear.max.y > edge - cx.px(80.0) {
                edge = edge.min(clear.min.y - cx.px(8.0));
            } else if top && clear.min.y < edge + cx.px(80.0) {
                edge = edge.max(clear.max.y + cx.px(8.0));
            }
        }
        let lines_of = |cx: &Cx<'_>, text: &str, size: f32| -> Vec<String> {
            // The C++ skips the spaces and line breaks a wrap leaves at a line's start, so blank lines vanish.
            cx.wrap_text(cx.fonts.regular, size, text, text_w)
                .into_iter()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        };

        let mut dismiss = None;
        for index in (0..self.toasts.len()).rev() {
            let toast = &self.toasts[index];
            let title_lines = lines_of(cx, &toast.title, 16.0);
            let body_lines = lines_of(cx, &toast.detail, 14.0);
            let title_h = title_lines.len() as f32 * cx.px(20.8);
            let body_h = body_lines.len() as f32 * cx.px(18.2);
            let height = cx
                .px(80.0)
                .max(cx.px(16.0) + title_h + cx.px(10.0) + body_h + cx.px(5.0) + cx.px(16.0));
            let (visible, moving) = Self::slide(toast, now);
            if moving {
                cx.keep_animating();
            }
            let card_x =
                x + if right { 1.0 } else { -1.0 } * (1.0 - visible) * (width + cx.px(16.0));
            let min = vec2(card_x.round(), if top { edge } else { edge - height });
            let max = vec2(min.x + width, min.y + height);
            // The slot grows and shrinks with the slide.
            let slot = visible * (height + cx.px(8.0));
            edge += if top { slot } else { -slot };
            let hidden = toast.hidden_at.is_some();
            let (accent, icon, key) = match toast.kind {
                ToastKind::Info => (p.accent, "circle-info", "toast-info"),
                ToastKind::Success => (p.green, "circle-check", "toast-success"),
                ToastKind::Error => (p.red, "circle-xmark", "toast-error"),
            };

            let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
            let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
            let clicked_card = ui
                .window(format!("##toast{index}"))
                .position(min.into(), Condition::Always)
                .size(vec2(width, height).into(), Condition::Always)
                .flags(
                    WindowFlags::NO_DECORATION
                        | WindowFlags::NO_MOVE
                        | WindowFlags::NO_SAVED_SETTINGS
                        | WindowFlags::NO_FOCUS_ON_APPEARING
                        | WindowFlags::NO_NAV
                        | WindowFlags::NO_BACKGROUND,
                )
                .build(|| {
                    let draw = cx.window_draw_list();
                    draw.add_rect_filled(min, max, p.panel_bg1, cx.px(4.0));
                    draw.add_rect_filled_corners(
                        min,
                        vec2(min.x + cx.px(36.0), max.y),
                        accent,
                        cx.px(4.0),
                        DrawFlags::ROUND_LEFT,
                    );
                    let icon_y = ((min.y + max.y) / 2.0 - cx.px(10.0)).round();
                    if let Some(path) = svg_path(icon) {
                        cx.draw_svg_path(
                            draw,
                            key,
                            path,
                            512.0,
                            512.0,
                            vec2(min.x + cx.px(8.0), icon_y),
                            vec2(min.x + cx.px(28.0), icon_y + cx.px(20.0)),
                            p.white,
                        );
                    }
                    wrapped(
                        cx,
                        &title_lines,
                        16.0,
                        20.8,
                        vec2(min.x + cx.px(52.0), min.y + cx.px(16.0)),
                        p.text_selected,
                    );
                    wrapped(
                        cx,
                        &body_lines,
                        14.0,
                        18.2,
                        vec2(
                            min.x + cx.px(52.0),
                            min.y + cx.px(16.0) + title_h + cx.px(10.0),
                        ),
                        p.text_secondary,
                    );
                    // The x: far:times at 18px in the dismiss column.
                    let close_min = vec2(max.x - cx.px(31.3), min.y + cx.px(8.0));
                    cx.set_cursor(close_min);
                    let close = ui.invisible_button("##dismiss", vec2(cx.px(21.3), cx.px(28.7)));
                    let close_hovered = ui.is_item_hovered();
                    cx.draw_fa_icon(
                        draw,
                        "far:times",
                        vec2(close_min.x + cx.px(10.6), min.y + cx.px(22.0)),
                        cx.px(18.0),
                        if close_hovered {
                            p.text_selected
                        } else {
                            p.text_secondary
                        },
                    );
                    cx.set_cursor(min);
                    let clicked = ui.invisible_button("##card", vec2(width, height));
                    close || clicked
                })
                .unwrap_or(false);
            if clicked_card && !hidden {
                dismiss = Some(index);
            }
        }
        if let Some(index) = dismiss {
            self.toasts[index].hidden_at = Some(now);
        }
    }
}

/// Draws already wrapped `lines` of regular text at the CSS `size`, each in a `line_height` (logical px) box,
/// placed like Chromium.
fn wrapped(
    cx: &mut Cx<'_>,
    lines: &[String],
    size: f32,
    line_height: f32,
    pos: Vec2,
    color: crate::theme::Color,
) {
    let draw = cx.window_draw_list();
    let font = cx.fonts.regular;
    let ascent = size * 2189.0 / 2048.0 * cx.scale;
    let content = ascent.round() + (size * 600.0 / 2048.0 * cx.scale).round();
    let mut y = pos.y;
    for line in lines {
        let box_top = y + (cx.px(line_height) - content) / 2.0;
        cx.draw_text(
            draw,
            font,
            size,
            vec2(pos.x, (box_top + ascent.round()).round() - ascent.ceil()),
            color,
            line.trim_end(),
        );
        y += cx.px(line_height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pushed(toasts: &mut Toasts, title: &str) {
        toasts.push_with_detail(ToastKind::Info, title, "detail");
    }

    #[test]
    fn the_history_is_newest_first_and_capped() {
        let mut toasts = Toasts::default();
        for i in 0..60 {
            pushed(&mut toasts, &format!("toast {i}"));
        }
        assert_eq!(toasts.history().len(), 50);
        assert_eq!(toasts.history()[0].title, "toast 59");
        assert_eq!(toasts.history()[49].title, "toast 10");
        assert_eq!(toasts.history()[0].detail, "detail");
    }

    #[test]
    fn read_and_unread_are_counted() {
        let mut toasts = Toasts::default();
        assert_eq!(toasts.unread(), 0);
        pushed(&mut toasts, "a");
        toasts.push(ToastKind::Error, "b");
        assert_eq!(toasts.unread(), 2);
        toasts.history_mut()[0].read = true;
        assert_eq!(toasts.unread(), 1);
        toasts.mark_all_read();
        assert_eq!(toasts.unread(), 0);
        // New ones are unread again, and the old ones stay read.
        pushed(&mut toasts, "c");
        assert_eq!(toasts.unread(), 1);
        assert!(toasts.history()[1].read);
    }

    #[test]
    fn a_toast_expires_after_ten_seconds_and_slides_out() {
        let mut toasts = Toasts::default();
        toasts.set_time(100.0);
        pushed(&mut toasts, "a");
        assert!(toasts.tick(105.0));
        assert!(toasts.toasts[0].hidden_at.is_none());
        assert!(toasts.tick(110.5));
        assert_eq!(toasts.toasts[0].hidden_at, Some(110.5));
        // Still on screen while it slides out, gone after.
        assert!(toasts.tick(110.7));
        assert!(!toasts.tick(110.9));
        // The history keeps it.
        assert_eq!(toasts.history().len(), 1);
    }

    #[test]
    fn a_card_slides_in_and_out() {
        let mut toasts = Toasts::default();
        toasts.set_time(10.0);
        pushed(&mut toasts, "a");
        let toast = toasts.toasts[0].clone();
        assert_eq!(Toasts::slide(&toast, 10.0), (0.0, true));
        let (halfway, moving) = Toasts::slide(&toast, 10.15);
        assert!((halfway - 0.5).abs() < 1e-5 && moving);
        assert_eq!(Toasts::slide(&toast, 11.0), (1.0, false));
        let leaving = Toast {
            hidden_at: Some(12.0),
            ..toast
        };
        assert_eq!(Toasts::slide(&leaving, 12.0), (1.0, true));
        assert_eq!(Toasts::slide(&leaving, 12.3).0, 0.0);
    }

    #[test]
    fn corners_are_named_by_the_preference() {
        assert_eq!(toast_corner("Top Left"), ToastCorner::TopLeft);
        assert_eq!(toast_corner("Top Right"), ToastCorner::TopRight);
        assert_eq!(toast_corner("Bottom Right"), ToastCorner::BottomRight);
        assert_eq!(toast_corner("Bottom Left"), ToastCorner::BottomLeft);
        assert_eq!(toast_corner("anything"), ToastCorner::BottomLeft);
    }
}
