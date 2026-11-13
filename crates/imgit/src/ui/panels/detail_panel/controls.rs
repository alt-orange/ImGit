//! The small controls the detail panel's views share: CSS-placed text, the coloured tone buttons, the 12px
//! checkboxes, the Path | Tree toggle, the Sort button and the summary / description message box.

use dear_imgui_rs::{
    DrawFlags, InputTextCallback, InputTextCallbackHandler, MouseCursor, StyleColor, StyleVar,
    TextCallbackData,
};

use crate::app::settings::FileSort;
use crate::fonts::Font;
use crate::theme::{Color, FONT_SIZE_ROW};
use crate::ui::menu::{menu_checkbox, popup_menu};
use crate::ui::panels::menus::text_field_menu;
use crate::ui::{Cx, DrawList, Vec2, vec2};

/// Where ImGui must put the top of `size`px Open Sans text whose CSS box starts at `box_top`: Chromium places
/// the text on the pixel nearest box top + the rounded ascent; ImGui puts the baseline `ceil(ascent)` under its
/// y.
pub fn css_text_y(cx: &Cx<'_>, size: f32, box_top: f32) -> f32 {
    let ascent = size * 2189.0 / 2048.0 * cx.scale;
    (box_top + ascent.round()).round() - ascent.ceil()
}

/// `text` in the regular font with its CSS box at `box_pos`.
pub fn css_text(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    size: f32,
    box_pos: Vec2,
    color: Color,
    text: &str,
) {
    css_text_in(cx, draw, cx.fonts.regular, size, box_pos, color, text);
}

/// [`css_text`] in `font`.
#[allow(clippy::too_many_arguments)]
pub fn css_text_in(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    font: Font,
    size: f32,
    box_pos: Vec2,
    color: Color,
    text: &str,
) {
    cx.draw_text(
        draw,
        font,
        size,
        vec2(box_pos.x, css_text_y(cx, size, box_pos.y)),
        color,
        text,
    );
}

/// The width of a [`tone_button`].
pub fn tone_button_width(cx: &Cx<'_>, label: &str) -> f32 {
    cx.text_size(cx.fonts.semibold, FONT_SIZE_ROW, label).x + cx.px(12.0)
}

/// The coloured buttons (.btn-success / .btn-danger / .btn-default): the tone at 20% (30% hovered) inside a 1px
/// border of the tone, radius 2, 12px semibold text 5px in. `height` 22 is .btn-xs, 24.1 .btn-small. Returns
/// true when clicked.
pub fn tone_button(
    cx: &mut Cx<'_>,
    id: &str,
    label: &str,
    pos: Vec2,
    height: f32,
    tone: Color,
    enabled: bool,
) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    let w = tone_button_width(cx, label);
    cx.set_cursor(pos);
    let clicked = ui.invisible_button(id, vec2(w, height)) && enabled;
    let hovered = ui.is_item_hovered() && enabled;
    let alpha = if enabled { 1.0 } else { 0.5 };
    let draw = cx.window_draw_list();
    let max = vec2(pos.x + w, pos.y + height);
    draw.add_rect_filled(
        pos,
        max,
        tone.with_alpha(if hovered { 0.3 } else { 0.2 } * alpha),
        cx.px(2.0),
    );
    draw.add_rect(pos, max, tone.with_alpha(alpha), cx.px(2.0), 1.0);
    css_text_in(
        cx,
        draw,
        cx.fonts.semibold,
        FONT_SIZE_ROW,
        vec2(pos.x + cx.px(6.0), pos.y + (height - cx.px(17.0)) / 2.0),
        p.text_normal.with_alpha(p.text_normal.a * alpha),
        label,
    );
    clicked
}

/// A 12px checkbox (rgba(0,0,0,.2), 1px 40% white border, radius 1) with its label; true when toggled.
pub fn small_checkbox(
    cx: &mut Cx<'_>,
    id: &str,
    label: &str,
    value: &mut bool,
    box_pos: Vec2,
    label_pos: Vec2,
    size: f32,
) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let label_w = cx.text_size(cx.fonts.regular, size, label).x;
    cx.set_cursor(box_pos);
    let clicked = ui.invisible_button(
        id,
        vec2(
            label_pos.x + label_w - box_pos.x,
            cx.px(12.0).max(cx.px(20.0)),
        ),
    );
    if clicked {
        *value = !*value;
    }
    let bmax = vec2(box_pos.x + cx.px(12.0), box_pos.y + cx.px(12.0));
    if *value {
        draw.add_rect_filled(box_pos, bmax, p.accent, cx.px(1.0));
        cx.draw_fa_icon(
            draw,
            "fas:check",
            vec2(box_pos.x + cx.px(6.0), box_pos.y + cx.px(6.0)),
            cx.px(9.0),
            p.white,
        );
    } else {
        draw.add_rect_filled(box_pos, bmax, p.input_bg, cx.px(1.0));
        draw.add_rect(box_pos, bmax, p.text_disabled, cx.px(1.0), 1.0);
    }
    css_text(cx, draw, size, label_pos, p.text_normal, label);
    clicked
}

/// .btn-success / .btn-danger (12px, 6px 12px padding, 31.14px tall): a 1px border around 20% of the colour, 30%
/// while hovered.
pub fn tone_action_button(
    cx: &mut Cx<'_>,
    id: &str,
    label: &str,
    min: Vec2,
    max: Vec2,
    tone: Color,
) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    cx.set_cursor(min);
    let clicked = ui.invisible_button(id, max - min);
    let hovered = ui.is_item_hovered();
    draw.add_rect_filled(
        min,
        max,
        tone.with_alpha(if hovered { 0.3 } else { 0.2 }),
        cx.px(2.0),
    );
    draw.add_rect(min, max, tone, cx.px(2.0), 1.0);
    let text_w = cx.text_size(cx.fonts.regular, 12.0, label).x;
    css_text(
        cx,
        draw,
        12.0,
        vec2(((min.x + max.x - text_w) / 2.0).round(), min.y + cx.px(7.0)),
        p.text_normal,
        label,
    );
    clicked
}

/// The Sort button (sort-alpha-up, 20px) at `pos`: a menu of the file lists' orders. `current` is the order in
/// use (ticked in the menu).
pub fn sort_button(cx: &mut Cx<'_>, current: FileSort, pos: Vec2, row_h: f32) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    cx.set_cursor(pos);
    let clicked = ui.invisible_button("##sort", vec2(cx.px(20.0), row_h));
    let hovered = ui.is_item_hovered();
    cx.draw_fa_icon(
        draw,
        "far:sort-alpha-up",
        vec2(pos.x + cx.px(10.0), pos.y + cx.px(12.1)),
        cx.px(16.0),
        if hovered {
            p.text_selected
        } else {
            p.text_normal
        },
    );
    cx.tooltip("Sort");
    if !clicked {
        return;
    }
    let choice = |label: &str, sort: FileSort| {
        menu_checkbox(label, current == sort, move |_cx, app| {
            app.settings.file_sort = sort;
            app.settings.save(&app.config_path("settings.txt"));
        })
    };
    let items = vec![
        choice("Path (A to Z)", FileSort::Path),
        choice("Path (Z to A)", FileSort::PathDescending),
        choice("File name", FileSort::Name),
        choice("Status", FileSort::Status),
    ];
    popup_menu(cx, items, Some(vec2(pos.x, pos.y + row_h)));
}

/// What [`view_toggle_row`] changes.
pub struct ViewToggle<'a> {
    pub file_sort: FileSort,
    pub tree_view: &'a mut bool,
    /// Where the Path | Tree pair is centred: the panel's left edge and width.
    pub origin_x: f32,
    pub width: f32,
    pub top: f32,
    /// The Sort button's left edge.
    pub sort_x: f32,
    pub semibold: bool,
}

/// The sort icon and the centred Path | Tree toggle (the toggle button pair: 1px 20% white borders, the checked
/// one rgba(77,136,255,.2) with white text).
pub fn view_toggle_row(cx: &mut Cx<'_>, toggle: ViewToggle<'_>) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let row_h = cx.px(24.1);
    let top = toggle.top;
    sort_button(cx, toggle.file_sort, vec2(toggle.sort_x, top), row_h);
    let font = if toggle.semibold {
        cx.fonts.semibold
    } else {
        cx.fonts.regular
    };
    let icons = ["far:bars", "fas:folder-tree"];
    let labels = ["Path", "Tree"];
    let widths = [0, 1].map(|i| {
        cx.px(11.0)
            + cx.fa_icon_width(icons[i], cx.px(12.0))
            + cx.px(5.0)
            + cx.text_size(font, FONT_SIZE_ROW, labels[i]).x
            + cx.px(11.0)
    });
    let mut bx = toggle.origin_x + (toggle.width - widths[0] - widths[1]) / 2.0;
    for i in 0..2 {
        let checked = *toggle.tree_view == (i == 1);
        let bmin = vec2(bx, top);
        let bmax = vec2(bx + widths[i], top + row_h);
        cx.set_cursor(bmin);
        let hovered = {
            let _id = ui.push_id(i as i32);
            if ui.invisible_button("##view_mode", vec2(widths[i], row_h)) {
                *toggle.tree_view = i == 1;
            }
            ui.is_item_hovered()
        };
        let accent = p.accent.with_alpha(0.2);
        if checked {
            draw.add_rect_filled(bmin, bmax, accent, 0.0);
        } else if hovered {
            draw.add_rect_filled(bmin, bmax, p.hover_row, 0.0);
        }
        draw.add_rect(
            bmin,
            bmax,
            if checked { accent } else { p.ink(0.2) },
            0.0,
            1.0,
        );
        let color = if checked || hovered {
            p.text_selected
        } else {
            p.text_normal
        };
        let icon_w = cx.fa_icon_width(icons[i], cx.px(12.0));
        cx.draw_fa_icon(
            draw,
            icons[i],
            vec2(bmin.x + cx.px(11.0) + icon_w / 2.0, top + cx.px(12.05)),
            cx.px(12.0),
            color,
        );
        css_text_in(
            cx,
            draw,
            font,
            FONT_SIZE_ROW,
            vec2(bmin.x + cx.px(16.0) + icon_w, top + cx.px(3.57)),
            color,
            labels[i],
        );
        bx += widths[i];
    }
}

/// Which way the summary field takes the keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SummaryFocus {
    #[default]
    None,
    /// Keyboard focus selects the text.
    SelectAll,
    /// An autofocused input (the amend editor) puts the caret at the end.
    CaretEnd,
}

/// Moves the summary field's caret behind its text once it is active.
struct CaretToEnd<'a>(&'a mut bool);

impl InputTextCallbackHandler for CaretToEnd<'_> {
    fn on_always(&mut self, mut data: TextCallbackData<'_>) {
        if *self.0 {
            let end = data.str().len();
            data.set_cursor_pos(end);
            data.set_selection_start(end);
            data.set_selection_end(end);
            *self.0 = false;
        }
    }
}

/// .commit-message-input: a 1px subtle-border box, radius 4, holding the summary row (padding-top 10px: the
/// 72-character counter 10px from the right edge, the 16px input filling the rest) over the 14px description
/// textarea (5px 10px padding), which fills the box below the 38px summary row.
pub struct MessageBox<'a> {
    pub summary: &'a mut String,
    pub description: &'a mut String,
    pub min: Vec2,
    pub max: Vec2,
    pub summary_id: &'a str,
    pub description_id: &'a str,
    pub focus: SummaryFocus,
    pub summary_hint: &'a str,
    pub counter_shown: bool,
    /// A caret move behind the summary asked for and not yet carried out (the input becomes active a frame
    /// after the focus request).
    pub caret_to_end: &'a mut bool,
}

impl MessageBox<'_> {
    pub fn draw(self, cx: &mut Cx<'_>) {
        let ui = cx.ui;
        let p = cx.palette;
        let draw = cx.window_draw_list();
        let (bmin, bmax) = (self.min, self.max);
        let summary = self.summary;
        let description = self.description;
        draw.add_rect_filled_corners(
            vec2(bmin.x + 1.0, bmin.y + 1.0),
            vec2(bmax.x - 1.0, bmin.y + cx.px(38.0)),
            p.input_bg,
            cx.px(4.0),
            DrawFlags::ROUND_TOP,
        );
        draw.add_rect_filled_corners(
            vec2(bmin.x + 1.0, bmin.y + cx.px(38.0)),
            vec2(bmax.x - 1.0, bmax.y - 1.0),
            p.input_bg,
            cx.px(4.0),
            DrawFlags::ROUND_BOTTOM,
        );
        draw.add_rect(bmin, bmax, p.subtle_border, cx.px(4.0), 1.0);

        let _id = ui.push_id(self.summary_id);
        let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
        let clear = Color::TRANSPARENT;
        let _bg = ui.push_style_color(StyleColor::FrameBg, clear);
        let _bg_hovered = ui.push_style_color(StyleColor::FrameBgHovered, clear);
        let _bg_active = ui.push_style_color(StyleColor::FrameBgActive, clear);
        let _text = ui.push_style_color(StyleColor::Text, p.text_normal);
        let remaining = 72 - i32::try_from(summary.len()).unwrap_or(i32::MAX);
        let counter = remaining.to_string();
        let counter_w = if self.counter_shown {
            cx.text_size(cx.fonts.regular, 16.0, &counter).x
        } else {
            0.0
        };
        let counter_x = bmax.x - cx.px(10.0) - counter_w;

        // Summary: an input with 10px padding inside a 1px transparent border.
        let smin = vec2(bmin.x + 1.0, bmin.y + cx.px(12.1));
        let text_box = smin.y + cx.px(1.43);
        cx.set_cursor(smin);
        {
            let _padding = ui.push_style_var(StyleVar::FramePadding([
                cx.px(11.0),
                css_text_y(cx, 16.0, text_box) - smin.y,
            ]));
            let _font = cx.push_font(cx.fonts.regular, 16.0);
            ui.set_next_item_width(counter_x - smin.x);
            if self.focus != SummaryFocus::None {
                ui.set_keyboard_focus_here();
                *self.caret_to_end = self.focus == SummaryFocus::CaretEnd;
            }
            ui.input_text(self.summary_id, summary)
                .callback(CaretToEnd(self.caret_to_end))
                .callback_flags(InputTextCallback::ALWAYS)
                .build();
            text_field_menu(cx, summary);
        }
        if summary.is_empty() {
            css_text(
                cx,
                draw,
                16.0,
                vec2(smin.x + cx.px(11.0), text_box),
                p.text_secondary,
                self.summary_hint,
            );
        }
        if self.counter_shown {
            css_text(
                cx,
                draw,
                16.0,
                vec2(counter_x, bmin.y + cx.px(13.08)),
                if remaining < 0 {
                    p.orange
                } else {
                    p.text_disabled
                },
                &counter,
            );
        }
        // Description: a textarea with 5px 10px padding.
        let dmin = vec2(bmin.x + 1.0, bmin.y + cx.px(38.0));
        cx.set_cursor(dmin);
        {
            let _padding = ui.push_style_var(StyleVar::FramePadding([
                cx.px(10.0),
                css_text_y(cx, 14.0, dmin.y + cx.px(5.5)) - dmin.y,
            ]));
            let _font = cx.push_font(cx.fonts.regular, 14.0);
            ui.input_text_multiline(
                self.description_id,
                description,
                [bmax.x - bmin.x - 2.0, (bmax.y - 1.0 - dmin.y).max(1.0)],
            )
            .build();
            text_field_menu(cx, description);
        }
        if description.is_empty() {
            css_text(
                cx,
                draw,
                14.0,
                vec2(dmin.x + cx.px(10.0), dmin.y + cx.px(5.5)),
                p.text_secondary,
                "Description",
            );
        }
    }
}

/// The 'not allowed' cursor over a disabled button.
pub fn not_allowed_cursor(cx: &Cx<'_>) {
    cx.ui.set_mouse_cursor(Some(MouseCursor::NotAllowed));
}
