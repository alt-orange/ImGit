//! The file view's editor (Edit This File): Save / Cancel and the text.
//
// The working directory's copy of a file as plain text in the code font, with Save and Cancel in place of the
// toolbar. Line endings are edited as "\n" and written back as the file had them (a file without any gets the
// End of Line preference's).

use std::fs;
use std::path::PathBuf;

use dear_imgui_rs::{Key, KeyChord, KeyMods, StyleColor, StyleVar};
use imgit_app::repo_state::{DiffSource, DiffTarget, FileEdit};
use imgit_app::{RepoState, pref, preferences};
use imgit_git::FileStatus;

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::theme::{Color, FONT_SIZE_ROW};
use crate::ui::panels::diff_view::{css_text_y, set_tab_size};
use crate::ui::{Cx, Vec2, vec2};

/// Larger files open in an external editor instead.
const MAX_EDIT_SIZE: u64 = 8 << 20;

#[derive(Default)]
pub struct State {}

fn workdir_file(state: &RepoState, path: &str) -> PathBuf {
    state.repo.workdir().join(path)
}

/// A `.btn-xs` (20px) in `tone` with an optional 12px icon before the label, at `pos`. Returns whether it was
/// clicked (never while disabled) and its width.
fn editor_button(
    cx: &mut Cx<'_>,
    id: &str,
    label: &str,
    icon: Option<&str>,
    pos: Vec2,
    tone: Color,
    enabled: bool,
) -> (bool, f32) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let icon_w = if icon.is_some() { cx.px(17.0) } else { 0.0 };
    let w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label).x + icon_w + cx.px(12.0);
    let max = pos + vec2(w, cx.px(20.0));
    cx.set_cursor(pos);
    let clicked = ui.invisible_button(id, [w, cx.px(20.0)]) && enabled;
    let hovered = ui.is_item_hovered() && enabled;
    let alpha = if enabled { 1.0 } else { 0.5 };
    draw.add_rect_filled(
        pos,
        max,
        tone.with_alpha(if hovered { 0.3 } else { 0.2 } * alpha),
        cx.px(2.0),
    );
    draw.add_rect(pos, max, tone.with_alpha(alpha), cx.px(2.0), 1.0);
    let color = p.text_normal.with_alpha(p.text_normal.a * alpha);
    if let Some(icon) = icon {
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(pos.x + cx.px(12.0), pos.y + cx.px(10.0)),
            cx.px(12.0),
            color,
        );
    }
    cx.draw_text(
        draw,
        cx.fonts.regular,
        FONT_SIZE_ROW,
        vec2(
            pos.x + cx.px(6.0) + icon_w,
            css_text_y(cx, 12.0, pos.y + cx.px(1.0), false),
        ),
        color,
        label,
    );
    (clicked, w)
}

/// The file's line endings: CRLF when most of its line breaks are. Returns the text with "\n" breaks and
/// whether it was CRLF.
fn read_line_endings(bytes: &str) -> (String, bool) {
    let breaks = bytes.matches('\n').count();
    let crlf_breaks = bytes.matches("\r\n").count();
    let crlf = crlf_breaks * 2 > breaks;
    let text = if crlf {
        bytes.replace("\r\n", "\n")
    } else {
        bytes.to_owned()
    };
    (text, crlf)
}

/// `text` as it goes to the disk: its "\n" breaks as "\r\n" when `crlf`.
fn write_line_endings(text: &str, crlf: bool) -> String {
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text.to_owned()
    }
}

/// Writes the editor's text; `true` when it reached the disk.
fn save(app: &mut Application, state: &mut RepoState) -> bool {
    let Some(edit) = &state.file_edit else {
        return false;
    };
    // A file keeps its own line endings; one that had no line break yet (or was empty) gets the End of Line
    // preference's.
    let crlf = if edit.saved.contains('\n') {
        edit.crlf
    } else {
        preferences().get(pref::EDITOR_EOL) == "CRLF"
    };
    let bytes = write_line_endings(&edit.text, crlf);
    let path = edit.path.clone();
    if fs::write(workdir_file(state, &path), bytes).is_err() {
        app.toasts.push_with_detail(
            ToastKind::Error,
            format!("Could not save {path}"),
            "The file could not be written.",
        );
        return false;
    }
    if let Some(edit) = &mut state.file_edit {
        edit.saved.clone_from(&edit.text);
    }
    true
}

/// The status the unstaged changes list gives `path`.
fn unstaged_status(state: &RepoState, path: &str, default: FileStatus) -> FileStatus {
    state
        .status
        .unstaged
        .iter()
        .rfind(|change| change.path == path)
        .map_or(default, |change| change.status)
}

/// Back to the file's diff in the working directory, which now holds the saved text.
fn close_editor(state: &mut RepoState) {
    let Some(edit) = state.file_edit.take() else {
        return;
    };
    state.refresh_status();
    let status = unstaged_status(state, &edit.path, FileStatus::Modified);
    // The edit is gone, so nothing can refuse this.
    let _ = state.open_diff(DiffTarget {
        source: DiffSource::Unstaged,
        path: edit.path,
        status,
    });
}

/// Asks whether to discard the editor's unsaved changes (the Discard Changes confirmation); `then` runs, with
/// the repository found again, when the user confirms. Cancelling changes nothing.
fn ask_discard(
    app: &mut Application,
    state: &RepoState,
    then: impl FnOnce(&mut Application, &mut RepoState) + 'static,
) {
    let workdir = state.repo.workdir().to_path_buf();
    let path = state
        .file_edit
        .as_ref()
        .map(|edit| edit.path.clone())
        .unwrap_or_default();
    app.dialog.open(DialogKind::Confirm);
    app.dialog.title = "Discard Changes".to_owned();
    app.dialog.message = format!("Discard your unsaved changes to {path}?");
    app.dialog.confirm_label = "Discard".to_owned();
    app.dialog.danger = true;
    app.dialog.on_confirm = Some(Box::new(move |_cx, app| {
        app.with_repo(&workdir, then);
    }));
}

/// Runs `navigate`, a move away from the open file editor (another file's diff, closing the file view, the
/// merge tool, another commit...). While the editor holds unsaved text it asks first (Discard Changes): the
/// confirmation drops the text and runs `navigate`, cancelling leaves everything as it is. Every such
/// navigation goes through here (`RepoState::open_diff` and `close_diff` refuse on their own otherwise).
///
/// `navigate` runs with the edit gone when it was deferred, so the `RepoState` calls in it are not refused.
pub fn navigate_from_editor(
    app: &mut Application,
    state: &mut RepoState,
    navigate: impl FnOnce(&mut Application, &mut RepoState) + 'static,
) {
    if state.has_unsaved_edit() {
        ask_discard(app, state, move |app, state| {
            state.discard_file_edit();
            navigate(app, state);
        });
    } else {
        navigate(app, state);
    }
}

/// Opens `path` of the working directory in the editor (and in the file view); `false`, with a toast, when it
/// is missing, binary or too large, and when unsaved text in the editor first has to be discarded (asked).
pub fn open_file_editor(
    _cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    path: &str,
) -> bool {
    if state.has_unsaved_edit() {
        let path = path.to_owned();
        ask_discard(app, state, move |app, state| {
            state.discard_file_edit();
            // The confirmation runs outside a frame's drawing, with no `Cx` of its own to lend.
            open_file_editor_now(app, state, &path);
        });
        return false;
    }
    open_file_editor_now(app, state, path)
}

fn open_file_editor_now(app: &mut Application, state: &mut RepoState, path: &str) -> bool {
    let file = workdir_file(state, path);
    let Ok(metadata) = fs::metadata(&file).and_then(|metadata| {
        if metadata.is_file() {
            Ok(metadata)
        } else {
            Err(std::io::ErrorKind::NotFound.into())
        }
    }) else {
        app.toasts.push_with_detail(
            ToastKind::Error,
            format!("Could not edit {path}"),
            "The file is not in the working directory.",
        );
        return false;
    };
    if metadata.len() > MAX_EDIT_SIZE {
        app.toasts.push_with_detail(
            ToastKind::Info,
            "Edit file",
            format!("{path} is too large for the built-in editor."),
        );
        return false;
    }
    let Ok(bytes) = fs::read(&file) else {
        app.toasts.push_with_detail(
            ToastKind::Error,
            format!("Could not edit {path}"),
            "The file could not be read.",
        );
        return false;
    };
    if bytes.contains(&0) {
        app.toasts.push_with_detail(
            ToastKind::Info,
            "Edit file",
            format!("{path} is a binary file."),
        );
        return false;
    }
    let Ok(bytes) = String::from_utf8(bytes) else {
        app.toasts.push_with_detail(
            ToastKind::Info,
            "Edit file",
            format!("{path} is not UTF-8 text."),
        );
        return false;
    };
    let (text, crlf) = read_line_endings(&bytes);
    let edit = FileEdit {
        path: path.to_owned(),
        saved: text.clone(),
        text,
        crlf,
        ..FileEdit::default()
    };
    // The file view shows the file while it is edited: its working directory diff, unless one is open.
    if state
        .diff_target
        .as_ref()
        .is_none_or(|target| target.path != path)
    {
        let status = unstaged_status(state, path, FileStatus::Unknown);
        if !state.open_diff(DiffTarget {
            source: DiffSource::Unstaged,
            path: path.to_owned(),
            status,
        }) {
            return false;
        }
    }
    state.file_edit = Some(edit);
    true
}

/// `state.file_edit`: Save / Cancel at `toolbar_pos`, the text over `[min, max)`.
pub fn draw_file_editor(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    toolbar_pos: Vec2,
    min: Vec2,
    max: Vec2,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let Some(edit) = state.file_edit.as_mut() else {
        return;
    };
    let dirty = edit.text != edit.saved;

    // Toolbar: Save, Cancel, then the file's path and whether it has unsaved changes.
    let save_shortcut = ui.is_key_chord_pressed(KeyChord::new(Key::S).with_mods(KeyMods::CTRL));
    let (save_clicked, w) = editor_button(
        cx,
        "##editor_save",
        "Save",
        Some("far:save"),
        toolbar_pos,
        p.green,
        dirty,
    );
    let mut x = toolbar_pos.x + w + cx.px(10.0);
    let (cancel_clicked, w) = editor_button(
        cx,
        "##editor_cancel",
        if dirty { "Cancel" } else { "Close" },
        None,
        vec2(x, toolbar_pos.y),
        p.accent,
        true,
    );
    x += w + cx.px(15.0);
    let label = format!(
        "Editing {}{}",
        edit.path,
        if dirty {
            "  \u{2022} unsaved changes"
        } else {
            ""
        }
    );
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        FONT_SIZE_ROW,
        vec2(x, css_text_y(cx, 12.0, toolbar_pos.y + cx.px(1.0), false)),
        max.x - x - cx.px(10.0),
        if dirty { p.orange } else { p.text_secondary },
        &label,
    );

    // The text, in the code font, 10px in.
    let escape;
    cx.set_cursor(min);
    {
        let _frame = ui.push_style_color(StyleColor::FrameBg, p.app_bg0);
        let _padding = ui.push_style_var(StyleVar::FramePadding([cx.px(10.0), cx.px(4.0)]));
        let _border = ui.push_style_var(StyleVar::FrameBorderSize(0.0));
        let prefs = preferences();
        let font = cx.fonts.editor;
        let size = prefs.number(pref::EDITOR_FONT_SIZE, 6..=40) as f32;
        let _font = cx.push_font(font, size);
        set_tab_size(
            font,
            cx.font_px(font, size),
            prefs.number(pref::EDITOR_TAB_SIZE, 1..=16),
        );
        if edit.focus {
            ui.set_keyboard_focus_here();
            edit.focus = false;
        }
        // Escape in the text is typing for the application's shortcuts, so it cancels the edit from here; the
        // field is not drawn then, as ImGui would revert its text on the key.
        escape = ui.is_any_item_active() && ui.is_key_pressed(Key::Escape);
        if escape {
            ui.dummy(max - min);
        } else {
            cx.input_multiline("##file_editor_text", &mut edit.text, None, max - min, false);
        }
    }

    let path = edit.path.clone();
    if (save_clicked || save_shortcut) && dirty {
        if save(app, state) {
            app.toasts.push(ToastKind::Success, format!("Saved {path}"));
            close_editor(state);
        }
        return;
    }
    if cancel_clicked || escape {
        if !dirty {
            close_editor(state);
            return;
        }
        ask_discard(app, state, |_app, state| {
            if state.file_edit.is_some() {
                close_editor(state);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_is_kept_when_most_breaks_are() {
        assert_eq!(
            read_line_endings("a\r\nb\r\nc\n"),
            ("a\nb\nc\n".to_owned(), true)
        );
        // A lone "\r\n" among "\n" is left as it is.
        assert_eq!(
            read_line_endings("a\nb\nc\r\n"),
            ("a\nb\nc\r\n".to_owned(), false)
        );
        assert_eq!(read_line_endings(""), (String::new(), false));
        assert_eq!(
            read_line_endings("no breaks"),
            ("no breaks".to_owned(), false)
        );
    }

    #[test]
    fn text_goes_back_with_the_files_line_endings() {
        assert_eq!(write_line_endings("a\nb\n", true), "a\r\nb\r\n");
        assert_eq!(write_line_endings("a\nb\n", false), "a\nb\n");
        let (text, crlf) = read_line_endings("x\r\ny\r\n");
        assert_eq!(write_line_endings(&text, crlf), "x\r\ny\r\n");
    }
}
