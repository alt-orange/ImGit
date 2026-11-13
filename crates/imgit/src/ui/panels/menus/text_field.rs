//! The editable context menu of text fields (Cut / Copy / Paste / Select All).

use std::ffi::CString;

use dear_imgui_rs::{Key, MouseButton, sys};

use crate::ui::menu::{MenuItem, menu_item, popup_menu};
use crate::ui::{Cx, raw};

/// A menu command waiting for its field to have the keyboard again, then sent as a key or as typed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTextEdit {
    id: sys::ImGuiID,
    key: Option<Key>,
    ctrl: bool,
    text: String,
}

/// The id of the item just drawn (`ImGui::GetItemID()`).
fn item_id(_ui: &dear_imgui_rs::Ui) -> sys::ImGuiID {
    // SAFETY: a frame is open (`_ui`).
    unsafe { sys::igGetItemID() }
}

/// Gives a text field the keyboard back, keeping its cursor and selection (a plain activation selects all).
fn refocus_field(_ui: &dear_imgui_rs::Ui, id: sys::ImGuiID) {
    // SAFETY: a frame is open (`_ui`), so the context exists; the activation is requested for the next frame
    // and its flags are plain data of the context.
    unsafe {
        sys::igActivateItemByID(id);
        let context = sys::igGetCurrentContext();
        if !context.is_null() {
            (*context).NavNextActivateFlags = (sys::ImGuiActivateFlags_PreferInput
                | sys::ImGuiActivateFlags_TryToPreserveState)
                as sys::ImGuiActivateFlags;
        }
    }
}

/// The text selected in the text field `id` while it is being edited.
fn selected_text(_ui: &dear_imgui_rs::Ui, id: sys::ImGuiID) -> Option<String> {
    // SAFETY: a frame is open (`_ui`); the state pointer is null or valid for this frame, its buffer holds
    // `TextA.Size` bytes, and the selection indices (clamped) are read before anything else can change it.
    unsafe {
        let state = sys::igGetInputTextState(id);
        if state.is_null() {
            return None;
        }
        let start = sys::ImGuiInputTextState_GetSelectionStart(state);
        let end = sys::ImGuiInputTextState_GetSelectionEnd(state);
        let (from, to) = (start.min(end), start.max(end));
        let buffer = &(*state).TextA;
        if from >= to || buffer.Data.is_null() {
            return None;
        }
        let length = buffer.Size.max(0);
        let (from, to) = (from.clamp(0, length), to.clamp(0, length));
        let bytes = std::slice::from_raw_parts(
            buffer.Data.cast::<u8>().add(from as usize),
            (to - from) as usize,
        );
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Queues UTF-8 text as typed input (`io.AddInputCharactersUTF8()`).
fn add_input_text(_ui: &dear_imgui_rs::Ui, text: &str) {
    let text = CString::new(text.replace('\0', "?")).unwrap_or_default();
    // SAFETY: a frame is open (`_ui`), so the IO exists; the call copies the characters out of the string.
    unsafe { sys::ImGuiIO_AddInputCharactersUTF8(sys::igGetIO_Nil(), text.as_ptr()) }
}

/// `ImGui::SetWindowFocus()`: the current window takes the focus.
fn focus_current_window(_ui: &dear_imgui_rs::Ui) {
    // SAFETY: a frame is open (`_ui`).
    unsafe { sys::igSetWindowFocus_Nil() }
}

/// Sends a pending command to its field once that field is active again.
fn send_pending(cx: &mut Cx<'_>, id: sys::ImGuiID) {
    let ui = cx.ui;
    let Some(pending) = cx
        .state
        .text_edit
        .take_if(|pending| pending.id == id && ui.is_item_active())
    else {
        return;
    };
    if !pending.text.is_empty() {
        add_input_text(ui, &pending.text);
        return;
    }
    let Some(key) = pending.key else {
        return;
    };
    let modifier = cx.shortcut_modifier_event();
    if pending.ctrl {
        // Ctrl chords only reach a field whose window has focus (the menu took it).
        focus_current_window(ui);
        raw::add_key_event(ui, modifier, true);
    }
    raw::add_key_event(ui, key, true);
    raw::add_key_event(ui, key, false);
    if pending.ctrl {
        raw::add_key_event(ui, modifier, false);
    }
}

/// A command that gives the field the keyboard back and then sends `key` (with the shortcut modifier).
fn key_command(id: sys::ImGuiID, key: Key, ctrl: bool) -> impl FnOnce(&mut Cx<'_>) + 'static {
    move |cx| {
        refocus_field(cx.ui, id);
        cx.state.text_edit = Some(PendingTextEdit {
            id,
            key: Some(key),
            ctrl,
            text: String::new(),
        });
    }
}

/// Call right after a text field: its right-click menu (Cut / Copy / Paste / Select All). This is
/// `UiState::text_field_menu`, installed by the application's frame; `Cx::input_text` calls it.
///
/// Electron's editable context menu: Cut / Copy / Paste / Select All, enabled by the field's state. A
/// right-click focuses the field like Chromium does.
pub fn text_field_menu(cx: &mut Cx<'_>, value: &str) {
    let ui = cx.ui;
    let id = item_id(ui);
    send_pending(cx, id);
    if !ui.is_item_hovered() || !ui.is_mouse_released(MouseButton::Right) {
        return;
    }
    let selected = if ui.is_item_active() {
        selected_text(ui, id).unwrap_or_default()
    } else {
        String::new()
    };
    if !ui.is_item_active() {
        refocus_field(ui, id);
    }
    let clipboard = raw::clipboard_text(ui).unwrap_or_default();
    let has_selection = !selected.is_empty();
    // The menu takes the mouse, so each command gives the field the keyboard back first.
    let with_accelerator =
        |item: MenuItem, accelerator: &str| item.accelerator(cx.shortcut_label(accelerator));
    let cut_text = selected.clone();
    let cut_keys = key_command(id, Key::Delete, false);
    let cut = with_accelerator(
        menu_item("Cut", move |cx, _app| {
            raw::set_clipboard_text(cx.ui, &cut_text);
            cut_keys(cx);
        })
        .enabled(has_selection),
        "Ctrl+X",
    );
    let copy = with_accelerator(
        menu_item("Copy", move |cx, _app| {
            raw::set_clipboard_text(cx.ui, &selected);
            refocus_field(cx.ui, id);
        })
        .enabled(has_selection),
        "Ctrl+C",
    );
    // Paste types the clipboard into the field (replacing the selection), like Ctrl+V.
    let paste = with_accelerator(
        menu_item("Paste", move |cx, _app| {
            refocus_field(cx.ui, id);
            cx.state.text_edit = Some(PendingTextEdit {
                id,
                key: None,
                ctrl: false,
                text: raw::clipboard_text(cx.ui).unwrap_or_default(),
            });
        })
        .enabled(!clipboard.is_empty()),
        "Ctrl+V",
    );
    let select_all_keys = key_command(id, Key::A, true);
    let select_all = with_accelerator(
        menu_item("Select All", move |cx, _app| select_all_keys(cx)).enabled(!value.is_empty()),
        "Ctrl+A",
    );
    popup_menu(cx, vec![cut, copy, paste, select_all], None);
}
