//! Linux (X11 / Wayland) and macOS: what winit offers on its own. The frame belongs to the window manager;
//! the cursors are the system theme's.

use winit::window::{CursorIcon, Window};

use super::CursorShape;

pub(super) struct Chrome;

impl Chrome {
    pub(super) fn new(_window: &Window) -> Self {
        // TODO: the window icon on Linux (assets "images/app_icon.png" through `gfx::image` and
        // `Window::set_window_icon`); on macOS it comes from the bundle.
        Self
    }

    pub(super) fn apply_frame(&mut self, _dark: bool) {}

    pub(super) fn frame_changed(&mut self) {}

    pub(super) fn set_cursor(&mut self, window: &Window, shape: CursorShape) {
        window.set_cursor(match shape {
            CursorShape::Move => CursorIcon::Move,
            CursorShape::NoDrop => CursorIcon::NotAllowed,
            CursorShape::ColResize => CursorIcon::ColResize,
            CursorShape::RowResize => CursorIcon::RowResize,
            CursorShape::Grab => CursorIcon::Grab,
        });
    }

    pub(super) fn reset_cursor(&mut self, window: &Window) {
        // The platform backend sets ImGui's cursor again on its next frame.
        window.set_cursor(CursorIcon::Default);
    }
}
