//! The few Dear ImGui calls the safe `dear_imgui_rs` API does not offer (next-window placement for popups, the
//! viewport side bar, queuing input events from inside a frame), each in a small wrapper whose `&Ui` argument
//! proves that a frame of the current context is open on this thread.

use std::ffi::CString;

use dear_imgui_rs::{Key, MouseButton, Ui, WindowFlags, sys};

use crate::ui::geom::Vec2;

/// `ImGui::SetNextWindowPos(pos, ImGuiCond_Always, pivot)` for the next window or popup.
pub fn set_next_window_pos(_ui: &Ui, pos: Vec2, pivot: Vec2) {
    // SAFETY: a frame is open (`_ui`); the call stores the values for the next `Begin`.
    unsafe {
        sys::igSetNextWindowPos(
            pos.into(),
            sys::ImGuiCond_Always as sys::ImGuiCond,
            pivot.into(),
        );
    }
}

/// `ImGui::SetNextWindowFocus()`.
pub fn set_next_window_focus(_ui: &Ui) {
    // SAFETY: a frame is open (`_ui`).
    unsafe { sys::igSetNextWindowFocus() }
}

/// `ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow())`: the window being built draws over every
/// other one, popups included.
pub fn bring_current_window_to_display_front(_ui: &Ui) {
    // SAFETY: a window is current while its contents are built, which the callers only do inside `build()`.
    unsafe {
        let window = sys::igGetCurrentWindow();
        if !window.is_null() {
            sys::igBringWindowToDisplayFront(window);
        }
    }
}

/// `ImGui::OpenPopup(id)`.
pub fn open_popup(_ui: &Ui, id: &str) {
    let id = CString::new(id).unwrap_or_default();
    // SAFETY: a frame is open (`_ui`); the string is copied (hashed) by the call.
    unsafe {
        sys::igOpenPopup_Str(id.as_ptr(), 0);
    }
}

/// `ImGui::BeginViewportSideBar` for the main viewport, run around `content`, which gets whether the window is
/// visible. `End()` is always called, as ImGui requires. The window is begun with no padding, no border and no
/// minimum size (the style is pushed and popped around the `Begin` alone, as the C++ did: the safe API cannot pop
/// a style inside the window it was pushed outside of).
pub fn viewport_side_bar_up<R>(
    _ui: &Ui,
    name: &str,
    height: f32,
    flags: WindowFlags,
    content: impl FnOnce(bool) -> R,
) -> R {
    let name = CString::new(name).unwrap_or_default();
    // SAFETY: a frame is open (`_ui`); the viewport is the main one the context owns; `End` pairs the `Begin`
    // the call made whatever it returned.
    unsafe {
        let zero = sys::ImVec2 { x: 0.0, y: 0.0 };
        sys::igPushStyleVar_Vec2(sys::ImGuiStyleVar_WindowPadding as sys::ImGuiStyleVar, zero);
        sys::igPushStyleVar_Float(
            sys::ImGuiStyleVar_WindowBorderSize as sys::ImGuiStyleVar,
            0.0,
        );
        sys::igPushStyleVar_Vec2(sys::ImGuiStyleVar_WindowMinSize as sys::ImGuiStyleVar, zero);
        let open = sys::igBeginViewportSideBar(
            name.as_ptr(),
            sys::igGetMainViewport(),
            sys::ImGuiDir_Up as sys::ImGuiDir,
            height,
            flags.bits(),
        );
        sys::igPopStyleVar(3);
        let result = content(open);
        sys::igEnd();
        result
    }
}

/// Queues a key press or release (`io.AddKeyEvent()`), processed at the start of the next frame.
pub fn add_key_event(_ui: &Ui, key: Key, down: bool) {
    // SAFETY: a frame is open (`_ui`), so the IO of the current context exists; the event is copied.
    unsafe { sys::ImGuiIO_AddKeyEvent(sys::igGetIO_Nil(), key.into(), down) }
}

/// Queues a mouse button press or release (`io.AddMouseButtonEvent()`).
pub fn add_mouse_button_event(_ui: &Ui, button: MouseButton, down: bool) {
    // SAFETY: as for `add_key_event`.
    unsafe { sys::ImGuiIO_AddMouseButtonEvent(sys::igGetIO_Nil(), button.into(), down) }
}

/// Zeroes `io.MouseWheel`: the wheel of this frame is taken (a menu swallows it).
pub fn consume_mouse_wheel(_ui: &Ui) {
    // SAFETY: as for `add_key_event`; one float is written, with no reference to the IO held.
    unsafe { (*sys::igGetIO_Nil()).MouseWheel = 0.0 }
}

/// The main viewport's position and size (`ImGuiViewport::Pos`, `Size`).
#[must_use]
pub fn viewport(ui: &Ui) -> (Vec2, Vec2) {
    let viewport = ui.main_viewport();
    (viewport.pos().into(), viewport.size().into())
}

/// The main viewport's work area (what is left under the side bars): position and size.
#[must_use]
pub fn work_area(ui: &Ui) -> (Vec2, Vec2) {
    let viewport = ui.main_viewport();
    (viewport.work_pos().into(), viewport.work_size().into())
}

/// `ImGui::SetNextWindowSize(size, ImGuiCond_Always)`; a zero axis fits the content.
pub fn set_next_window_size(_ui: &Ui, size: Vec2) {
    // SAFETY: a frame is open (`_ui`); the call stores the value for the next `Begin`.
    unsafe { sys::igSetNextWindowSize(size.into(), sys::ImGuiCond_Always as sys::ImGuiCond) }
}

/// `ImGui::SetClipboardText(text)` (interior NUL bytes become `?`).
pub fn set_clipboard_text(_ui: &Ui, text: &str) {
    let text = CString::new(text.replace('\0', "?")).unwrap_or_default();
    // SAFETY: a frame is open (`_ui`); the call hands the platform backend a copy of the string.
    unsafe { sys::igSetClipboardText(text.as_ptr()) }
}

/// `ImGui::GetClipboardText()`; `None` when the clipboard holds no text.
#[must_use]
pub fn clipboard_text(_ui: &Ui) -> Option<String> {
    // SAFETY: a frame is open (`_ui`); ImGui returns null or a NUL-terminated string that lives until the next
    // clipboard call, which is copied here.
    unsafe {
        let text = sys::igGetClipboardText();
        (!text.is_null()).then(|| {
            std::ffi::CStr::from_ptr(text)
                .to_string_lossy()
                .into_owned()
        })
    }
}

/// `ImGui::SetNextWindowPos(pos, ImGuiCond_Appearing, pivot)`: positions the next window only when it appears.
pub fn set_next_window_pos_appearing(_ui: &Ui, pos: Vec2, pivot: Vec2) {
    // SAFETY: a frame is open (`_ui`); the call stores the values for the next `Begin`.
    unsafe {
        sys::igSetNextWindowPos(
            pos.into(),
            sys::ImGuiCond_Appearing as sys::ImGuiCond,
            pivot.into(),
        );
    }
}
