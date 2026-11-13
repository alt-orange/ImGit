//! Text fields and the checkbox.

use dear_imgui_rs::{
    InputTextCallback, InputTextCallbackHandler, InputTextFlags, StyleColor, StyleVar,
    TextCallbackData, sys,
};

use crate::ui::Cx;
use crate::ui::geom::{Vec2, vec2};

/// Moves the caret behind the text of the field that asked for it, once that field is active.
struct CaretToEnd<'s> {
    field: sys::ImGuiID,
    pending: &'s mut Option<sys::ImGuiID>,
}

impl InputTextCallbackHandler for CaretToEnd<'_> {
    fn on_always(&mut self, mut data: TextCallbackData<'_>) {
        if *self.pending == Some(self.field) {
            let end = data.str().len();
            data.set_cursor_pos(end);
            data.set_selection_start(end);
            data.set_selection_end(end);
            *self.pending = None;
        }
    }
}

impl Cx<'_> {
    /// Runs the editable context menu the menus registered for the text field just drawn.
    fn text_field_menu(&mut self, value: &str) {
        if let Some(menu) = self.state.text_field_menu {
            menu(self, value);
        }
    }

    /// A single-line text field `width` device px wide (negative: up to that far from the right edge, -1
    /// filling the row, as `ImGui::SetNextItemWidth`), with the editable context menu. `true` when the text
    /// changed.
    pub fn input_text(
        &mut self,
        id: &str,
        value: &mut String,
        hint: Option<&str>,
        flags: InputTextFlags,
        width: f32,
    ) -> bool {
        let ui = self.ui;
        ui.set_next_item_width(width);
        let field = ui.input_text(id, value).flags(flags);
        let changed = match hint {
            Some(hint) => field.hint(hint).build(),
            None => field.build(),
        };
        self.text_field_menu(value);
        changed
    }

    /// A multi-line text field of `size` device px, showing `hint` while it is empty and not being edited.
    /// `caret_to_end` moves the caret behind the text this frame (pass it with the frame that focuses the
    /// field).
    pub fn input_multiline(
        &mut self,
        id: &str,
        value: &mut String,
        hint: Option<&str>,
        size: Vec2,
        caret_to_end: bool,
    ) -> bool {
        let ui = self.ui;
        let pos = self.cursor();
        // The field becomes active a frame after the focus request, so the request is kept until the callback
        // runs.
        let field = self.id(id);
        if caret_to_end {
            self.state.pending_caret = Some(field);
        }
        let changed = ui
            .input_text_multiline(id, value, size)
            .callback(
                InputTextCallback::ALWAYS,
                CaretToEnd {
                    field,
                    pending: &mut self.state.pending_caret,
                },
            )
            .build();
        self.text_field_menu(value);
        if let Some(hint) = hint
            && value.is_empty()
            && !ui.is_item_active()
        {
            // SAFETY: a frame is open (`ui`), so the style exists; one vector is read.
            let padding: Vec2 = unsafe { (*sys::igGetStyle()).FramePadding }.into();
            self.window_draw_list().add_text_current_font(
                vec2(pos.x + padding.x, pos.y + padding.y),
                self.palette.text_disabled,
                hint,
            );
        }
        changed
    }

    /// A checkbox: the input background, filled with the link colour when checked.
    pub fn checkbox(&self, label: &str, value: &mut bool) -> bool {
        let ui = self.ui;
        let p = self.palette;
        let _padding = ui.push_style_var(StyleVar::FramePadding([self.px(2.0), self.px(2.0)]));
        let _frame = ui.push_style_color(StyleColor::FrameBg, p.input_bg);
        let _mark = ui.push_style_color(StyleColor::CheckMark, p.text_inverse);
        let _hovered = ui.push_style_color(StyleColor::FrameBgHovered, p.link.with_alpha(0.3));
        let _active = ui.push_style_color(StyleColor::FrameBgActive, p.link);
        // Dear ImGui 1.93 fills a checked box with its own colour, where 1.92 used FrameBg.
        let _checked = value.then(|| ui.push_style_color(StyleColor::CheckboxSelectedBg, p.link));
        ui.checkbox(label, value)
    }
}
