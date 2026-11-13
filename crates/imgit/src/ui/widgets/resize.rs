//! Resize handles: the splitters between panels and columns.

use dear_imgui_rs::{Condition, StyleVar, WindowFlags};

use crate::ui::Cx;
use crate::ui::geom::{Rect, Vec2};
use crate::window::CursorShape;

/// What a handle resizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResizeAxis {
    /// Widths: a vertical splitter, dragged left and right (Chromium's col-resize cursor).
    Columns,
    /// Heights: a horizontal splitter, dragged up and down (row-resize).
    Rows,
}

/// What [`Cx::resize_handle`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResizeHandle {
    pub held: bool,
    /// Pressed this frame.
    pub activated: bool,
}

impl Cx<'_> {
    /// A resize handle: an invisible hit box over `[min, max)` that takes no layout space, showing Chromium's
    /// col-resize or row-resize cursor while hovered or held.
    pub fn resize_handle(
        &mut self,
        id: &str,
        min: Vec2,
        max: Vec2,
        axis: ResizeAxis,
    ) -> ResizeHandle {
        let item = self.id(id);
        let rect = Rect::new(min, max);
        if !self.item_add(rect, item) {
            return ResizeHandle::default();
        }
        let button = self.button_behavior(rect, item);
        let handle = ResizeHandle {
            held: button.held,
            activated: self.ui.is_item_activated(),
        };
        if button.hovered || button.held {
            self.set_drag_cursor(match axis {
                ResizeAxis::Columns => CursorShape::ColResize,
                ResizeAxis::Rows => CursorShape::RowResize,
            });
        }
        handle
    }

    /// A handle that resizes `size` like re-resizable: the size when it was pressed plus the mouse's travel
    /// since (times `direction`, -1 for a handle on the region's left or top edge), clamped to
    /// `[min_size, max_size]`. Returns `true` on the frame it is released after changing the size.
    #[allow(clippy::too_many_arguments)]
    pub fn resize_drag(
        &mut self,
        id: &str,
        min: Vec2,
        max: Vec2,
        axis: ResizeAxis,
        direction: f32,
        size: &mut f32,
        min_size: f32,
        max_size: f32,
    ) -> bool {
        let start_size = self.id(&format!("{id}#size"));
        let start_mouse = self.id(&format!("{id}#mouse"));
        let was_held = self.id(&format!("{id}#held"));
        let handle = self.resize_handle(id, min, max, axis);
        let mouse = self.mouse_pos();
        let position = match axis {
            ResizeAxis::Columns => mouse.x,
            ResizeAxis::Rows => mouse.y,
        };
        if handle.activated {
            self.set_storage_float(start_size, *size);
            self.set_storage_float(start_mouse, position);
        }
        if handle.held {
            let travel = (position - self.storage_float(start_mouse, 0.0)) * direction;
            *size = min_size.max((self.storage_float(start_size, 0.0) + travel).min(max_size));
            self.set_storage_bool(was_held, true);
            return false;
        }
        if self.storage_bool(was_held, false) {
            self.set_storage_bool(was_held, false);
            return *size != self.storage_float(start_size, 0.0);
        }
        false
    }

    /// [`Self::resize_drag`] for a handle over other windows (the panels' child windows): it gets a
    /// transparent window of its own, above them.
    #[allow(clippy::too_many_arguments)]
    pub fn overlay_resize_drag(
        &mut self,
        id: &str,
        min: Vec2,
        max: Vec2,
        axis: ResizeAxis,
        direction: f32,
        size: &mut f32,
        min_size: f32,
        max_size: f32,
    ) -> bool {
        let ui = self.ui;
        // A top-level window created after the root's children is hit-tested before them (child windows
        // follow their root in ImGui's window order); NoBringToFrontOnFocus would put it behind the root
        // instead.
        let _min_size = ui.push_style_var(StyleVar::WindowMinSize([1.0, 1.0]));
        let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
        let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
        ui.window(id)
            .position(min.into(), Condition::Always)
            .size((max - min).into(), Condition::Always)
            .flags(
                WindowFlags::NO_DECORATION
                    | WindowFlags::NO_BACKGROUND
                    | WindowFlags::NO_SAVED_SETTINGS
                    | WindowFlags::NO_FOCUS_ON_APPEARING
                    | WindowFlags::NO_NAV
                    | WindowFlags::NO_MOVE
                    | WindowFlags::NO_SCROLL_WITH_MOUSE,
            )
            .build(|| self.resize_drag(id, min, max, axis, direction, size, min_size, max_size))
            .unwrap_or(false)
    }
}
