//! The controls of the pages: Chromium's checkbox, Bootstrap's text fields, native and react-select selects and
//! the button families.

use std::rc::Rc;

use dear_imgui_rs::{InputTextFlags, StyleColor, StyleVar, sys};

use super::page::{BoxStyle, Face, LABEL, Page, Style, ascent_of, descent_of};
use crate::app::application::Application;
use crate::theme::Color;
use crate::ui::menu::{MenuItems, menu_item, popup_menu};
use crate::ui::{Cx, vec2};

/// What a select does with the option picked, after its menu closed.
pub(super) type Pick = Rc<dyn Fn(&mut Cx<'_>, &mut Application, &str)>;

/// Whether the text field `id` has the keyboard (`ImGui::GetActiveID() == ImGui::GetID(id)`).
pub(super) fn focused(cx: &Cx<'_>, id: &str) -> bool {
    // SAFETY: `cx.ui` proves a frame is open; the active id is one integer.
    unsafe { sys::igGetActiveID() == cx.id(id) }
}

/// Chromium's dark checkbox tick.
const TICK: Color = Color::new(34.0 / 255.0, 34.0 / 255.0, 34.0 / 255.0, 1.0);

impl Page<'_, '_> {
    /// Chromium's checkbox: a 12px box whose corner pixels are blended (a ~1.5px radius); checked = the accent
    /// with a dark tick, unchecked = a 40% white border around 20% black. `label_width` extends the click area
    /// over a <label> next to it. Returns true when clicked.
    pub(super) fn checkbox(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        checked: bool,
        disabled: bool,
        label_width: f32,
    ) -> bool {
        let draw = self.draw;
        let min = self.at(x, y);
        let one = self.px(1.0);
        let max = vec2(min.x + self.px(12.0), min.y + self.px(12.0));
        let opacity = if disabled { 0.5 } else { 1.0 };
        let accent = self.cx.palette.link;
        let field = self.cx.palette.input_bg;
        let corners = |color: Color, alpha: f32| {
            let c = color.faded(alpha * opacity);
            draw.add_rect_filled(min, vec2(min.x + one, min.y + one), c, 0.0);
            draw.add_rect_filled(vec2(max.x - one, min.y), vec2(max.x, min.y + one), c, 0.0);
            draw.add_rect_filled(vec2(min.x, max.y - one), vec2(min.x + one, max.y), c, 0.0);
            draw.add_rect_filled(vec2(max.x - one, max.y - one), max, c, 0.0);
        };
        if checked {
            let fill = accent.faded(opacity);
            draw.add_rect_filled(
                vec2(min.x + one, min.y),
                vec2(max.x - one, max.y),
                fill,
                0.0,
            );
            draw.add_rect_filled(
                vec2(min.x, min.y + one),
                vec2(min.x + one, max.y - one),
                fill,
                0.0,
            );
            draw.add_rect_filled(
                vec2(max.x - one, min.y + one),
                vec2(max.x, max.y - one),
                fill,
                0.0,
            );
            corners(accent, 0.85);
            let points = [
                vec2(min.x + self.px(2.7), min.y + self.px(5.9)),
                vec2(min.x + self.px(5.6), min.y + self.px(8.7)),
                vec2(min.x + self.px(9.9), min.y + self.px(2.9)),
            ];
            draw.add_polyline(&points, TICK.faded(opacity), false, self.px(1.9));
        } else {
            let border = self.ink(0.4).faded(opacity);
            draw.add_rect_filled(
                vec2(min.x + one, min.y + one),
                vec2(max.x - one, max.y - one),
                field.faded(opacity),
                0.0,
            );
            draw.add_rect_filled(
                vec2(min.x + one, min.y),
                vec2(max.x - one, min.y + one),
                border,
                0.0,
            );
            draw.add_rect_filled(
                vec2(min.x + one, max.y - one),
                vec2(max.x - one, max.y),
                border,
                0.0,
            );
            draw.add_rect_filled(
                vec2(min.x, min.y + one),
                vec2(min.x + one, max.y - one),
                border,
                0.0,
            );
            draw.add_rect_filled(
                vec2(max.x - one, min.y + one),
                vec2(max.x, max.y - one),
                border,
                0.0,
            );
            corners(self.ink(0.4), 0.5);
        }
        self.hit(id, x - 2.0, y - 4.0, 16.0 + label_width, 20.0, !disabled)
    }
}

/// How a text field lays its text out.
#[derive(Debug, Clone, Copy)]
pub(super) struct Field<'a> {
    pub placeholder: &'a str,
    /// Padding + border.
    pub pad_left: f32,
    pub pad_right: f32,
    pub style: Style,
    pub flags: InputTextFlags,
    /// A text input: the <input> sits 1px inside its form-control box.
    pub inset: f32,
}

impl Default for Field<'_> {
    fn default() -> Self {
        Self {
            placeholder: "",
            pad_left: 13.0,
            pad_right: 13.0,
            style: LABEL,
            flags: InputTextFlags::NONE,
            inset: 0.0,
        }
    }
}

impl<'a> Field<'a> {
    pub(super) fn placeholder(self, placeholder: &'a str) -> Self {
        Self {
            placeholder,
            ..self
        }
    }

    pub(super) fn inset(self) -> Self {
        Self { inset: 1.0, ..self }
    }

    pub(super) fn flags(self, flags: InputTextFlags) -> Self {
        Self { flags, ..self }
    }
}

/// The editable context menu of the text fields, as `Cx::input_text` runs it.
fn run_field_menu(cx: &mut Cx<'_>, value: &str) {
    if let Some(menu) = cx.state.text_field_menu {
        menu(cx, value);
    }
}

impl Page<'_, '_> {
    /// The text of an <input> drawn by ImGui where Chromium puts it: its text box centred in the field.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn input_text_at(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        value: &mut String,
        field: &Field<'_>,
    ) -> bool {
        let ui = self.cx.ui;
        let style = field.style;
        let font = self.font(style.face);
        let size = style.size;
        let font_line =
            (size * ascent_of(style.face)).round() + (size * descent_of(style.face)).round();
        let scale = self.cx.scale;
        let ascent = size * ascent_of(style.face) * scale;
        let exact_top = self.origin.y + (y + field.inset) * scale;
        let baseline = (exact_top + (h - font_line) / 2.0 * scale + ascent.round()).round();
        let min = self.at(x, y);
        let text_pos = vec2(min.x + self.px(field.pad_left), baseline - ascent.ceil());
        self.cx.set_cursor(text_pos);
        let text_color = style.color.resolve(self.cx);
        let disabled = self.ink(0.4);
        let _frame = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
        let _text = ui.push_style_color(StyleColor::Text, text_color);
        let _hint = ui.push_style_color(StyleColor::TextDisabled, disabled);
        let _padding = ui.push_style_var(StyleVar::FramePadding([0.0, 0.0]));
        let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
        let _font = self.cx.push_font(font, size);
        let width = min.x + self.px(w) - text_pos.x - self.px(field.pad_right);
        let hint = (!field.placeholder.is_empty()).then_some(field.placeholder);
        self.cx.input_text(id, value, hint, field.flags, width)
    }

    /// A Bootstrap text field (form-control).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn text_field(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        w: f32,
        value: &mut String,
        field: &Field<'_>,
        h: f32,
    ) -> bool {
        let focus = focused(self.cx, id);
        self.field_box(x, y, w, h, focus);
        self.input_text_at(id, x, y, w, h, value, field)
    }

    /// The text input: a form-control box without the inset shadow, ringed by another 1px of the accent while
    /// focused.
    pub(super) fn form_text_field(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        w: f32,
        value: &mut String,
        field: &Field<'_>,
    ) -> bool {
        let focus = focused(self.cx, id);
        let accent = self.cx.palette.accent;
        let fill = self.cx.palette.input_bg;
        let border = if focus { accent } else { self.ink(0.08) };
        self.boxed(x, y, w, 34.0, BoxStyle::new(fill, border, 4.0));
        if focus {
            self.boxed(
                x - 1.0,
                y - 1.0,
                w + 2.0,
                36.0,
                BoxStyle::new(BoxStyle::CLEAR, accent, 5.0),
            );
        }
        self.input_text_at(id, x, y, w, 34.0, value, field)
    }

    /// A <textarea>: ImGui's multiline input at Chromium's first line.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn text_area(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        value: &mut String,
        placeholder: Option<&str>,
        pad: f32,
        line_height: f32,
        style: Style,
    ) -> bool {
        let ui = self.cx.ui;
        let font = self.font(style.face);
        let font_line = (style.size * ascent_of(style.face)).round()
            + (style.size * descent_of(style.face)).round();
        let scale = self.cx.scale;
        let ascent = style.size * ascent_of(style.face) * scale;
        let exact_top = self.origin.y + y * scale;
        let baseline =
            (exact_top + (pad + (line_height - font_line) / 2.0) * scale + ascent.round()).round();
        let min = self.at(x, y);
        let max = vec2(min.x + self.px(w), min.y + self.px(h));
        let text_pos = vec2(min.x + self.px(pad), baseline - ascent.ceil());
        self.cx.set_cursor(text_pos);
        let color = style.color.resolve(self.cx);
        let (changed, active) = {
            let _frame = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
            let _text = ui.push_style_color(StyleColor::Text, color);
            let _padding = ui.push_style_var(StyleVar::FramePadding([0.0, 0.0]));
            let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
            let _font = self.cx.push_font(font, style.size);
            let size = vec2(
                max.x - text_pos.x - self.px(pad),
                max.y - text_pos.y - self.px(6.0),
            );
            let changed = ui.input_text_multiline(id, value, size).build();
            let active = ui.is_item_active();
            run_field_menu(self.cx, value);
            (changed, active)
        };
        if value.is_empty()
            && !active
            && let Some(placeholder) = placeholder
        {
            let hint = self.cx.palette.ink_255(102);
            let mut line_y = text_pos.y;
            for line in placeholder.split('\n') {
                self.cx.draw_text(
                    self.draw,
                    font,
                    style.size,
                    vec2(text_pos.x, line_y),
                    hint,
                    line,
                );
                line_y += self.px(line_height);
            }
        }
        changed
    }

    /// Opens a native menu of `options` at CSS (x, y); `pick` runs with the one chosen.
    pub(super) fn open_options<S: AsRef<str>>(
        &mut self,
        x: f32,
        y: f32,
        options: &[S],
        pick: Pick,
    ) {
        let items: MenuItems = options
            .iter()
            .map(|option| {
                let option = option.as_ref().to_owned();
                let pick = Rc::clone(&pick);
                menu_item(option.clone(), move |cx, app| pick(cx, app, &option))
            })
            .collect();
        let pos = self.at(x, y);
        popup_menu(self.cx, items, Some(pos));
    }

    /// A native <select class="form-control">: the value 17px in, Chromium's bold chevron 9px from the right;
    /// it opens its options as a native menu.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn native_select<S: AsRef<str>>(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        w: f32,
        value: &str,
        options: &[S],
        pick: impl Fn(&mut Cx<'_>, &mut Application, &str) + 'static,
    ) {
        self.field_box(x, y, w, 34.0, false);
        let color = self.ink(1.0);
        let scale = self.cx.scale;
        let ascent = 14.0 * ascent_of(Face::Regular) * scale;
        let exact_top = self.origin.y + y * scale;
        let baseline = (exact_top + (34.0 - 19.0) / 2.0 * scale + ascent.round()).round();
        let min = self.at(x, y);
        let right = min.x + self.px(w);
        let font = self.font(Face::Regular);
        self.cx.draw_text(
            self.draw,
            font,
            14.0,
            vec2(min.x + self.px(17.0), baseline - ascent.ceil()),
            color,
            value,
        );
        let top = exact_top.floor();
        let chevron = [
            vec2(right - self.px(12.5), top + self.px(15.0)),
            vec2(right - self.px(9.0), top + self.px(18.6)),
            vec2(right - self.px(5.4), top + self.px(15.0)),
        ];
        self.draw
            .add_polyline(&chevron, self.ink(1.0), false, self.px(1.8));
        if self.hit(id, x, y, w, 34.0, true) {
            self.open_options(x, y + 34.0, options, Rc::new(pick));
        }
    }

    /// The design system's select: a 31px button with the value, an optional muted suffix and Material's expand
    /// chevron.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ds_select<S: AsRef<str>>(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        value: &str,
        suffix: &str,
        options: &[S],
        pick: impl Fn(&mut Cx<'_>, &mut Application, &str) + 'static,
    ) {
        let style = Style::ink(14.0, 0.8);
        let mut w = 9.0 + self.measure(style, value) + 8.0 + 16.0 + 9.0;
        if !suffix.is_empty() {
            w += 5.0 + self.measure(style, suffix);
        }
        let (fill, border) = (self.cx.palette.input_bg, self.ink(0.2));
        self.boxed(x, y, w, 31.0, BoxStyle::new(fill, border, 2.0));
        let mut cx = x + 9.0 + self.text(x + 9.0, y + 6.0, style, value);
        if !suffix.is_empty() {
            cx += 5.0 + self.text(cx + 5.0, y + 6.0, Style::ink(14.0, 0.4), suffix);
        }
        let chevron = self.ink(0.6);
        self.chevron(cx + 8.0, y + 7.5, 16.0, chevron);
        if self.hit(id, x, y, w, 31.0, true) {
            self.open_options(x, y + 31.0, options, Rc::new(pick));
        }
    }

    /// react-select's control: the value 11px in, an 8% separator and the chevron 25px from the right.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn combo<S: AsRef<str>>(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        value: &str,
        placeholder: bool,
        options: &[S],
        pick: impl Fn(&mut Cx<'_>, &mut Application, &str) + 'static,
        face: Face,
    ) {
        let (fill, border) = (self.cx.palette.input_bg, self.ink(0.08));
        self.boxed(x, y, w, h, BoxStyle::new(fill, border, 4.0));
        if face == Face::Mono {
            self.text(
                x + 11.0,
                y + 8.5,
                Style::ink(14.0, 0.75).with_face(Face::Mono),
                value,
            );
        } else {
            let alpha = if placeholder { 0.4 } else { 0.75 };
            self.text(x + 11.0, y + 7.0, Style::ink(14.0, alpha), value);
        }
        let separator = self.ink(0.08);
        self.boxed(x + w - 30.0, y + 7.0, 1.0, 20.0, BoxStyle::fill(separator));
        let chevron = self.ink(0.75);
        self.chevron(x + w - 25.0, y + 7.0, 20.0, chevron);
        if self.hit(id, x, y, w, h, true) {
            self.open_options(x, y + h, options, Rc::new(pick));
        }
    }
}

/// The colour family of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Primary,
    Success,
    Danger,
    Default,
}

/// The size of a button.
#[derive(Debug, Clone, Copy)]
pub(super) struct ButtonShape {
    pub height: f32,
    pub line: f32,
    /// Padding + border.
    pub pad: f32,
}

/// .btn: 12px with a 17.14px line, 6px padding, 1px border.
pub(super) const BTN: ButtonShape = ButtonShape {
    height: 31.140_625,
    line: 17.140_625,
    pad: 13.0,
};
/// Next to a 34px field.
pub(super) const BTN_34: ButtonShape = ButtonShape {
    height: 34.0,
    line: 17.140_625,
    pad: 13.0,
};

/// The design system's outline buttons.
const OUTLINE: Color = Color::new(119.0 / 255.0, 120.0 / 255.0, 123.0 / 255.0, 1.0);

impl Page<'_, '_> {
    fn tone_color(&self, tone: Tone) -> Color {
        let p = self.cx.palette;
        match tone {
            Tone::Primary => p.accent,
            Tone::Success => p.green,
            Tone::Danger => p.red,
            Tone::Default => p.ink(0.75),
        }
    }

    /// Bootstrap's .btn: the tone at 20% inside its solid border (btn-default: a 75% white border only).
    /// Returns its width (CSS px) with whether it was clicked.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn button_sized(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        label: &str,
        tone: Tone,
        shape: ButtonShape,
        disabled: bool,
    ) -> (bool, f32) {
        let style = Style::ink(12.0, 0.75);
        let w = shape.pad * 2.0 + self.measure(style, label);
        let opacity = if disabled { 0.5 } else { 1.0 };
        let color = self.tone_color(tone);
        let fill = color.with_alpha(if tone == Tone::Default { 0.0 } else { 0.2 });
        self.boxed_faded(
            x,
            y,
            w,
            shape.height,
            BoxStyle::new(fill, color, 2.0),
            opacity,
        );
        self.text_faded(
            x + shape.pad,
            y + (shape.height - shape.line) / 2.0 + Self::leading(style, shape.line),
            style,
            label,
            opacity,
        );
        let clicked = self.hit(id, x, y, w, shape.height, !disabled);
        if self.cx.ui.is_item_hovered() && !disabled {
            let min = self.at(x, y);
            let max = vec2(min.x + self.px(w), min.y + self.px(shape.height));
            let wash = self.cx.palette.ink_255(12);
            self.draw.add_rect_filled(min, max, wash, self.px(2.0));
        }
        (clicked, w)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn button(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        label: &str,
        tone: Tone,
        shape: ButtonShape,
        disabled: bool,
    ) -> bool {
        self.button_sized(id, x, y, label, tone, shape, disabled).0
    }

    /// The design system's button (12px, 70% white): 23px chips with 5px padding, 27px ones with 9px. Returns
    /// whether it was clicked and its width.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ds_button(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        label: &str,
        large: bool,
        disabled: bool,
    ) -> (bool, f32) {
        let style = Style::ink(12.0, 0.7);
        let pad = if large { 9.0 } else { 5.0 };
        let h = if large { 27.140_625 } else { 23.140_625 };
        let w = pad * 2.0 + self.measure(style, label);
        let shape = BoxStyle::new(BoxStyle::CLEAR, OUTLINE, 2.0);
        let opacity = if disabled { 0.5 } else { 1.0 };
        self.boxed_faded(x, y, w, h, shape, opacity);
        self.text_faded(
            x + pad,
            y + if large { 5.0 } else { 3.0 },
            style,
            label,
            opacity,
        );
        let clicked = self.hit(id, x, y, w, h, !disabled);
        if self.cx.ui.is_item_hovered() && !disabled {
            let min = self.at(x, y);
            let max = vec2(min.x + self.px(w), min.y + self.px(h));
            let wash = self.cx.palette.ink_255(12);
            self.draw.add_rect_filled(min, max, wash, self.px(2.0));
        }
        (clicked, w)
    }

    /// An outline button with a 12px icon (the colour rows' reset, Custom Themes): fa-fw icon, a 5px caption
    /// margin. Returns whether it was clicked and its width.
    pub(super) fn icon_button(
        &mut self,
        id: &str,
        x: f32,
        y: f32,
        icon: &str,
        label: &str,
    ) -> (bool, f32) {
        let style = Style::ink(12.0, 0.75);
        let w = 13.0 + 15.0 + 5.0 + self.measure(style, label) + 5.0 + 13.0;
        let (border, ink) = (self.ink(0.75), self.ink(0.75));
        self.boxed(
            x,
            y,
            w,
            31.140_625,
            BoxStyle::new(BoxStyle::CLEAR, border, 2.0),
        );
        self.icon(icon, x + 13.0, y + 9.5, 15.0, 12.0, 12.0, ink);
        self.text(x + 33.0, y + 7.0, style, label);
        let clicked = self.hit(id, x, y, w, 31.140_625, true);
        (clicked, w)
    }
}
