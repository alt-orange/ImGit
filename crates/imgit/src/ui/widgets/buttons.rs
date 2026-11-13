//! Pills, the button families (border + translucent fill), links and the segmented control.

use dear_imgui_rs::{MouseButton, MouseCursor, StyleColor, StyleVar};

use crate::theme::{Color, FONT_SIZE_SMALL};
use crate::ui::Cx;
use crate::ui::geom::{Vec2, vec2};

/// The usual height of a segmented control, in logical pixels.
pub const SEGMENTED_HEIGHT: f32 = 24.0;

/// What [`Cx::pill`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pill {
    /// Clicked with the left button this frame.
    pub clicked: bool,
    /// The size it was drawn at, in device pixels.
    pub size: Vec2,
}

/// The colours of one button family.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonColors {
    border: Color,
    bg: Color,
    hover: Color,
    text: Color,
    text_hover: Color,
}

impl Cx<'_> {
    fn styled_button(&self, label: &str, size: Vec2, enabled: bool, colors: ButtonColors) -> bool {
        let ui = self.ui;
        let hovered_before = self.hovered_id() == self.id(label);
        let _border_size = ui.push_style_var(StyleVar::FrameBorderSize(1.0));
        let _rounding = ui.push_style_var(StyleVar::FrameRounding(self.px(3.0)));
        let _border = ui.push_style_color(StyleColor::Border, colors.border);
        let _button = ui.push_style_color(StyleColor::Button, colors.bg);
        let _hovered = ui.push_style_color(StyleColor::ButtonHovered, colors.hover);
        let _active = ui.push_style_color(StyleColor::ButtonActive, colors.hover);
        let _text = ui.push_style_color(
            StyleColor::Text,
            if hovered_before {
                colors.text_hover
            } else {
                colors.text
            },
        );
        let _disabled = (!enabled).then(|| ui.begin_disabled());
        ui.button_with_size(label, size)
    }

    /// The family of a semantic colour: its border, a 20% fill, 60% when hovered.
    fn tinted_button(&self, label: &str, size: Vec2, enabled: bool, tint: Color) -> bool {
        let p = self.palette;
        self.styled_button(
            label,
            size,
            enabled,
            ButtonColors {
                border: tint,
                bg: tint.with_alpha(0.2),
                hover: tint.with_alpha(0.6),
                text: p.text_normal,
                text_hover: p.text_selected,
            },
        )
    }

    /// The accent button. `size` in device pixels, a zero component fitting the label (`ImGui::Button`).
    pub fn primary_button(&self, label: &str, size: Vec2, enabled: bool) -> bool {
        self.tinted_button(label, size, enabled, self.palette.accent)
    }

    /// The neutral button: a border in the text colour, no fill.
    pub fn default_button(&self, label: &str, size: Vec2, enabled: bool) -> bool {
        let p = self.palette;
        self.styled_button(
            label,
            size,
            enabled,
            ButtonColors {
                border: p.text_normal,
                bg: Color::TRANSPARENT,
                hover: Color::TRANSPARENT,
                text: p.text_normal,
                text_hover: p.text_selected,
            },
        )
    }

    /// The red button of destructive actions.
    pub fn danger_button(&self, label: &str, size: Vec2, enabled: bool) -> bool {
        self.tinted_button(label, size, enabled, self.palette.red)
    }

    /// The green button.
    pub fn success_button(&self, label: &str, size: Vec2, enabled: bool) -> bool {
        self.tinted_button(label, size, enabled, self.palette.green)
    }

    /// A transparent button that only highlights on hover.
    pub fn text_button(&self, label: &str, size: Vec2, enabled: bool) -> bool {
        let p = self.palette;
        self.styled_button(
            label,
            size,
            enabled,
            ButtonColors {
                border: Color::TRANSPARENT,
                bg: Color::TRANSPARENT,
                hover: p.hover_row,
                text: p.text_normal,
                text_hover: p.text_selected,
            },
        )
    }

    /// Text in the link colour, underlined while hovered. `true` when clicked.
    pub fn link_text(&self, label: &str) -> bool {
        let ui = self.ui;
        let link = self.palette.link;
        {
            let _color = ui.push_style_color(StyleColor::Text, link);
            ui.text(label);
        }
        let hovered = ui.is_item_hovered();
        if hovered {
            ui.set_mouse_cursor(Some(MouseCursor::Hand));
            let rect = self.item_rect();
            self.window_draw_list().add_line(
                vec2(rect.min.x, rect.max.y),
                vec2(rect.max.x, rect.max.y),
                link,
                1.0,
            );
        }
        hovered && ui.is_mouse_clicked(MouseButton::Left)
    }

    /// The width of a [`Self::pill`] of `height` logical px holding `text`, plus `extra` logical px.
    #[must_use]
    pub fn pill_width(&self, text: &str, bold: bool, height: f32, extra: f32) -> f32 {
        let font = if bold {
            self.fonts.semibold
        } else {
            self.fonts.regular
        };
        self.text_size(font, height * 0.62, text).x + self.px(12.0) + self.px(extra)
    }

    /// A rounded label; `height` is logical, `max_width` in device pixels (the text gets an ellipsis). Hover
    /// brightens `bg` to `bg_hover`.
    #[allow(clippy::too_many_arguments)]
    pub fn pill(
        &self,
        id: &str,
        text: &str,
        bg: Color,
        bg_hover: Color,
        fg: Color,
        bold: bool,
        height: f32,
        max_width: Option<f32>,
    ) -> Pill {
        let ui = self.ui;
        let h = self.px(height);
        let font_size = height * 0.62;
        let font = if bold {
            self.fonts.semibold
        } else {
            self.fonts.regular
        };
        let text_size = self.text_size(font, font_size, text);
        let mut width = text_size.x + self.px(12.0);
        if let Some(max_width) = max_width.filter(|max_width| *max_width > 0.0) {
            width = width.min(max_width);
        }
        let pos = self.cursor();
        ui.invisible_button(id, vec2(width.max(1.0), h));
        let hovered = ui.is_item_hovered();
        let draw = self.window_draw_list();
        draw.add_rect_filled(
            pos,
            pos + vec2(width, h),
            if hovered { bg_hover } else { bg },
            self.px(2.0),
        );
        self.draw_text_ellipsis(
            draw,
            font,
            font_size,
            vec2(pos.x + self.px(6.0), pos.y + (h - text_size.y) / 2.0),
            width - self.px(12.0),
            fg,
            text,
        );
        Pill {
            clicked: ui.is_item_clicked_with_button(MouseButton::Left),
            size: vec2(width, h),
        }
    }

    /// Two or three options in one rounded frame, like "List | Agents" and "Path | Tree"; `height` is logical
    /// ([`SEGMENTED_HEIGHT`] usually). `true` when the selection changed.
    pub fn segmented(&self, id: &str, labels: &[&str], selected: &mut usize, height: f32) -> bool {
        let ui = self.ui;
        let p = self.palette;
        let font = self.fonts.regular;
        let h = self.px(height);
        let draw = self.window_draw_list();
        let origin = self.cursor();
        let sizes: Vec<Vec2> = labels
            .iter()
            .map(|label| self.text_size(font, FONT_SIZE_SMALL, label))
            .collect();
        let widths: Vec<f32> = sizes.iter().map(|size| size.x + self.px(20.0)).collect();
        let total: f32 = widths.iter().sum();
        let mut changed = false;
        {
            let _id = ui.push_id(id);
            draw.add_rect(
                origin,
                origin + vec2(total, h),
                p.toggle_border,
                self.px(3.0),
                1.0,
            );
            let mut x = origin.x;
            for (index, label) in labels.iter().enumerate() {
                let width = widths[index];
                self.set_cursor(vec2(x, origin.y));
                let hovered = {
                    let _id = ui.push_id(index);
                    ui.invisible_button("##seg", vec2(width, h));
                    if ui.is_item_clicked_with_button(MouseButton::Left) && *selected != index {
                        *selected = index;
                        changed = true;
                    }
                    ui.is_item_hovered()
                };
                let active = *selected == index;
                if active {
                    draw.add_rect_filled(
                        vec2(x, origin.y),
                        vec2(x + width, origin.y + h),
                        p.selected_row,
                        self.px(3.0),
                    );
                }
                self.draw_text(
                    draw,
                    font,
                    FONT_SIZE_SMALL,
                    vec2(
                        x + (width - sizes[index].x) / 2.0,
                        origin.y + (h - sizes[index].y) / 2.0,
                    ),
                    if active || hovered {
                        p.text_selected
                    } else {
                        p.text_normal
                    },
                    label,
                );
                x += width;
                if index + 1 < labels.len() {
                    draw.add_line(
                        vec2(x, origin.y + self.px(4.0)),
                        vec2(x, origin.y + h - self.px(4.0)),
                        p.toggle_border,
                        1.0,
                    );
                }
            }
        }
        self.set_cursor(origin);
        ui.dummy(vec2(total, h));
        changed
    }
}
