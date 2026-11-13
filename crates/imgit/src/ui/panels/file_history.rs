//! The File History view.

use dear_imgui_rs::{Key, ListClipper, MouseButton, StyleVar, WindowFlags};
use imgit_app::RepoState;
use imgit_app::repo_state::DiffMode;
use imgit_git::repository::short_sha;
use imgit_git::{FileHistoryEntry, FileStatus, Sha};

use crate::app::application::Application;
use crate::theme::{Color, Palette};
use crate::ui::menu::popup_menu;
use crate::ui::panels::diff_view::{
    DiffEditorOptions, HEADER_H, TOOLBAR_H, close_cross, css_text_y, draw_diff_editor,
    encoding_button, sha_color_index, style_item_spacing_x, view_toggle_button, view_toggle_width,
};
use crate::ui::panels::menus::{file_history_commit_menu, jump_to_commit};
use crate::ui::widgets::format_relative;
use crate::ui::{Cx, Vec2, vec2};

// The File History view: the file view's 36px header, the commit list down the left, and the file view's
// toolbar row over the editor on the right.
const LIST_W: f32 = 432.0;
const ROW_H: f32 = 44.0;
/// The avatar's centre.
const NODE_X: f32 = 30.0;
const NODE_DIAMETER: f32 = 24.0;
const ROW_TEXT_X: f32 = 54.0;
const LIST_TOP_PADDING: f32 = 4.0;

#[derive(Default)]
pub struct State {}

/// What the end of the commit list says the oldest entry did to the file.
fn status_word(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "ADDED",
        FileStatus::Deleted => "DELETED",
        FileStatus::Renamed => "RENAMED",
        _ => "MODIFIED",
    }
}

/// The lane colour of a commit: its graph lane's when it is loaded in the graph, else one from its sha.
fn commit_color(palette: &Palette, state: &RepoState, sha: &str) -> Color {
    let column = state
        .history
        .row_by_sha
        .get(sha)
        .and_then(|&row| state.layout.rows.get(row))
        .map_or_else(|| sha_color_index(sha), |row| row.column);
    palette.graph_color(column as i32)
}

/// What a row of the list asked for.
enum ListAction {
    /// A click: select it.
    Select(usize),
    /// A right click: select it and open its menu.
    Menu(usize),
    /// A double click: show the commit in the graph.
    Open(usize),
}

/// The text for the list when there are no entries to show.
fn list_message(state: &RepoState) -> Option<(&str, bool)> {
    let view = state.file_history.as_ref()?;
    if view.history_loading {
        Some(("Loading...", false))
    } else if let Some(error) = &view.history_error {
        Some((error, true))
    } else if view.history.entries.is_empty() {
        Some(("No history for this file", false))
    } else {
        None
    }
}

/// The commit list: one 44px row per entry (the author's avatar, the summary, the short sha; "<age> by
/// <author>" under it), then a line in the colour of what the oldest entry did to the file with its label, and
/// "End of History".
fn draw_list(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, min: Vec2, max: Vec2) {
    let ui = cx.ui;
    cx.set_cursor(min);
    ui.child_window("##file_history_list")
        .size((max - min).into())
        .build(ui, || draw_list_contents(cx, app, state, max.x - min.x));
}

fn draw_list_contents(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState, width: f32) {
    let ui = cx.ui;
    let p = cx.palette;
    // The rows are laid out by the clipper at exactly their height: no item spacing.
    let _spacing = ui.push_style_var(StyleVar::ItemSpacing([style_item_spacing_x(), 0.0]));
    let draw = cx.window_draw_list();
    let row_h = cx.px(ROW_H);
    let top = cx.cursor();
    if let Some((message, is_error)) = list_message(state) {
        let w = cx.text_size(cx.fonts.regular, 12.0, message).x;
        cx.draw_text(
            draw,
            cx.fonts.regular,
            12.0,
            vec2(
                top.x + ((width - w) / 2.0).round(),
                css_text_y(cx, 12.0, top.y + cx.px(20.0), false),
            ),
            if is_error { p.red } else { p.text_secondary },
            message,
        );
        return;
    }
    let Some(view) = state.file_history.as_mut() else {
        return;
    };
    let count = view.history.entries.len();
    if let Some(scroll_to) = view.scroll_to.take() {
        let y = cx.px(LIST_TOP_PADDING) + scroll_to as f32 * row_h;
        let view_h = ui.window_height();
        if y < ui.scroll_y() || y + row_h > ui.scroll_y() + view_h {
            ui.set_scroll_y((y - (view_h - row_h) / 2.0).max(0.0));
        }
    }
    // Keyboard: the arrows move the selection while the list has the focus.
    if ui.is_window_focused()
        && let Some(selected) = view.selected
    {
        let moved = if ui.is_key_pressed(Key::DownArrow) && selected + 1 < count {
            Some(selected + 1)
        } else if ui.is_key_pressed(Key::UpArrow) && selected > 0 {
            Some(selected - 1)
        } else {
            None
        };
        if let Some(moved) = moved {
            state.select_file_history_entry(Some(moved));
            if let Some(view) = state.file_history.as_mut() {
                view.scroll_to = view.selected;
            }
        }
    }
    let Some(view) = state.file_history.as_ref() else {
        return;
    };
    let entries = &view.history.entries;
    ui.dummy([width, cx.px(LIST_TOP_PADDING)]);
    let node_x = (top.x + cx.px(NODE_X)).round();
    let mut action = None;
    let mut clipper = ListClipper::new(entries.len())
        .items_height(row_h)
        .begin(ui);
    while clipper.step() {
        for i in clipper.display_range() {
            let entry = &entries[i];
            let pos = cx.cursor();
            let _id = ui.push_id(i);
            ui.invisible_button("##row", [width, row_h]);
            let hovered = ui.is_item_hovered();
            let selected = view.selected == Some(i);
            if selected || hovered {
                draw.add_rect_filled(
                    pos,
                    pos + vec2(width, row_h),
                    if selected {
                        p.selected_row
                    } else {
                        p.hover_row
                    },
                    0.0,
                );
            }
            if ui.is_item_clicked_with_button(MouseButton::Left) && !selected {
                action = Some(ListAction::Select(i));
            }
            if ui.is_item_clicked_with_button(MouseButton::Right) {
                action = Some(ListAction::Menu(i));
            }
            if hovered && ui.is_mouse_double_clicked(MouseButton::Left) {
                action = Some(ListAction::Open(i));
            }
            drop(_id);
            cx.draw_avatar(
                draw,
                vec2(node_x, pos.y + row_h / 2.0),
                cx.px(NODE_DIAMETER),
                &entry.commit.author,
                None,
            );
            draw_row_text(cx, entry, draw, pos, width, hovered);
        }
    }
    drop(clipper);
    match action {
        Some(ListAction::Select(i)) => state.select_file_history_entry(Some(i)),
        Some(ListAction::Menu(i)) => {
            state.select_file_history_entry(Some(i));
            let target = state
                .file_history
                .as_ref()
                .and_then(|view| view.history.entries.get(i))
                .map(|entry| (entry.commit.sha.clone(), entry.path.clone()));
            if let Some((sha, path)) = target {
                let items = file_history_commit_menu(cx, app, state, &sha, &path);
                popup_menu(cx, items, None);
            }
        }
        Some(ListAction::Open(i)) => {
            // The commit in the graph.
            let sha = state
                .file_history
                .as_ref()
                .and_then(|view| view.history.entries.get(i))
                .map(|entry| entry.commit.sha.clone());
            if let Some(sha) = sha {
                state.close_file_history();
                jump_to_commit(state, &sha);
            }
            return;
        }
        None => {}
    }
    let Some(view) = state.file_history.as_ref() else {
        return;
    };
    // Under the oldest entry: a line in the colour of what it did to the file, "ADDED <path>" under it, then
    // the end of the history.
    let Some(oldest) = view.history.entries.last() else {
        return;
    };
    let pos = cx.cursor();
    let tone = cx.status_color(oldest.status);
    draw.add_rect_filled(
        vec2(pos.x, pos.y + cx.px(4.0)),
        vec2(pos.x + width, pos.y + cx.px(5.0)),
        tone,
        0.0,
    );
    let word = status_word(oldest.status);
    cx.draw_text(
        draw,
        cx.fonts.semibold,
        11.0,
        vec2(
            pos.x + cx.px(10.0),
            css_text_y(cx, 11.0, pos.y + cx.px(8.0), false),
        ),
        tone,
        word,
    );
    let word_w = cx.text_size(cx.fonts.semibold, 11.0, word).x;
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        11.0,
        vec2(
            pos.x + cx.px(10.0) + word_w + cx.px(8.0),
            css_text_y(cx, 11.0, pos.y + cx.px(8.0), false),
        ),
        width - cx.px(36.0) - word_w,
        p.text_normal,
        &oldest.path,
    );
    let end = if view.history.has_more {
        "History cut short at the walk limit"
    } else {
        "End of History"
    };
    let end_w = cx.text_size(cx.fonts.regular, 12.0, end).x;
    cx.draw_text(
        draw,
        cx.fonts.regular,
        12.0,
        vec2(
            pos.x + ((width - end_w) / 2.0).round(),
            css_text_y(cx, 12.0, pos.y + cx.px(26.0), false),
        ),
        p.text_secondary,
        end,
    );
    ui.dummy([width, cx.px(56.0)]);
}

/// The texts of a list row: the summary, the short sha, "<age> by <author>", and their tooltips.
fn draw_row_text(
    cx: &mut Cx<'_>,
    entry: &FileHistoryEntry,
    draw: crate::ui::DrawList<'_>,
    pos: Vec2,
    width: f32,
    hovered: bool,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let row_h = cx.px(ROW_H);
    // The short sha, right-aligned; the summary fills up to it.
    let sha = short_sha(&entry.commit.sha);
    let sha = sha.get(..6).unwrap_or(sha);
    let sha_w = cx.text_size(cx.fonts.regular, 12.0, sha).x;
    let sha_x = pos.x + width - cx.px(16.0) - sha_w;
    let summary_y = css_text_y(cx, 12.0, pos.y + cx.px(7.0), false);
    cx.draw_text(
        draw,
        cx.fonts.regular,
        12.0,
        vec2(sha_x, summary_y),
        p.text_normal,
        sha,
    );
    let text_x = pos.x + cx.px(ROW_TEXT_X);
    let summary_w = sha_x - cx.px(10.0) - text_x;
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.semibold,
        12.0,
        vec2(text_x, summary_y),
        summary_w,
        p.text_selected,
        &entry.commit.summary,
    );
    let by = format!(
        "{} by {}",
        format_relative(entry.commit.time),
        entry.commit.author.name
    );
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        11.0,
        vec2(text_x, css_text_y(cx, 11.0, pos.y + cx.px(24.0), false)),
        pos.x + width - cx.px(16.0) - text_x,
        p.text_secondary,
        &by,
    );
    // The by-line shows the date in the Date verbose Format; a clipped summary shows whole.
    let by_min = vec2(text_x, pos.y + cx.px(22.0));
    let by_max = vec2(pos.x + width - cx.px(16.0), pos.y + row_h);
    let on_by = hovered && ui.is_mouse_hovering_rect(by_min.into(), by_max.into());
    if on_by {
        let date = cx.format_date_long(entry.commit.time);
        cx.styled_tooltip(by_min, by_max, &date, false);
    } else if hovered
        && cx
            .text_size(cx.fonts.semibold, 12.0, &entry.commit.summary)
            .x
            > summary_w
    {
        cx.styled_tooltip(
            pos,
            vec2(pos.x + width, pos.y + row_h),
            &entry.commit.summary,
            false,
        );
    }
}

/// The File History view for `state.file_history`, over the whole tab, `size` device px.
pub fn draw_file_history(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    size: Vec2,
) {
    state.poll_file_history();
    let Some(view) = &state.file_history else {
        return;
    };
    if view.history_loading || view.blame_loading {
        cx.keep_animating(); // to collect the background jobs
    }
    let ui = cx.ui;
    let origin = cx.cursor();
    ui.child_window("##file_history")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR | WindowFlags::NO_SCROLL_WITH_MOUSE)
        .build(ui, || draw_contents(cx, app, state, origin, size));
}

fn draw_contents(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    origin: Vec2,
    size: Vec2,
) {
    let p = cx.palette;
    let draw = cx.window_draw_list();
    draw.add_rect_filled(origin, origin + size, p.app_bg0, 0.0);

    // Header: "File History: <path>", the encoding and the close cross.
    let h = cx.px(HEADER_H);
    draw.add_rect_filled(
        origin,
        vec2(origin.x + size.x, origin.y + h),
        p.panel_bg0,
        0.0,
    );
    draw.add_rect_filled(
        vec2(origin.x, origin.y + h - 1.0),
        origin + vec2(size.x, h),
        p.section_border,
        0.0,
    );
    let path = state
        .file_history
        .as_ref()
        .map(|view| view.path.as_str())
        .unwrap_or_default();
    let title = format!("File History: {path}");
    cx.draw_text_ellipsis(
        draw,
        cx.fonts.regular,
        14.0,
        vec2(
            origin.x + cx.px(10.0),
            css_text_y(cx, 14.0, origin.y + cx.px(7.5), false),
        ),
        size.x * 0.6,
        p.text_selected,
        &title,
    );
    let cross = vec2(origin.x + size.x - cx.px(30.0), origin.y + cx.px(7.5));
    if close_cross(cx, draw, "##close_file_history", cross) {
        state.close_file_history();
        return;
    }
    encoding_button(cx, app, draw, origin.y, cross.x - cx.px(10.0), false);

    // The commit list down the left, a border after it.
    let body_top = origin.y + h;
    let list_w = cx.px(LIST_W).min((size.x * 0.45).round());
    draw_list(
        cx,
        app,
        state,
        vec2(origin.x, body_top),
        vec2(origin.x + list_w, origin.y + size.y),
    );
    if state.file_history.is_none() {
        return;
    }
    draw.add_rect_filled(
        vec2(origin.x + list_w, body_top),
        vec2(origin.x + list_w + 1.0, origin.y + size.y),
        p.section_border,
        0.0,
    );
    let pane_x = origin.x + list_w + 1.0;
    let pane_w = origin.x + size.x - pane_x;

    let bar_y = body_top + cx.px(10.0);
    draw_toolbar(cx, state, pane_x, pane_w, bar_y);

    // The editor: the file as the selected commit left it (its blame in the gutter), or its diff.
    let editor_min = vec2(pane_x, origin.y + cx.px(HEADER_H + TOOLBAR_H));
    let editor_max = vec2(origin.x + size.x - 1.0, origin.y + size.y);
    let Some(view) = state.file_history.as_ref() else {
        return;
    };
    let entry = view
        .selected
        .and_then(|selected| view.history.entries.get(selected));
    let note = |cx: &Cx<'_>, text: &str| {
        cx.draw_text(
            draw,
            cx.fonts.regular,
            14.0,
            vec2(editor_min.x, css_text_y(cx, 14.0, editor_min.y, false)),
            cx.palette.text_normal,
            text,
        );
    };
    let Some(entry) = entry else {
        if !view.history_loading && view.history_error.is_none() {
            note(cx, "No Changes");
        }
        return;
    };
    if view.file_view && entry.status == FileStatus::Deleted {
        note(cx, "The file was deleted in this commit.");
        return;
    }
    let clicked_sha: Option<Sha> = {
        let state = &*state;
        let mut options = DiffEditorOptions {
            mode: state.diff_mode,
            file_view: view.file_view,
            blame_gutter: true,
            blame: view.blame_data.as_ref(),
            blame_columns: view.blame,
            syntax_highlighting: app.settings.syntax_highlighting,
            word_wrap: app.settings.word_wrap,
            show_whitespace: app.settings.diff_whitespace,
            id: "##file_history_lines",
            ..DiffEditorOptions::default()
        };
        let palette = cx.palette;
        options.blame_color = Some(Box::new(move |sha: &Sha| commit_color(palette, state, sha)));
        draw_diff_editor(
            cx,
            view.diff.as_ref(),
            view.diff_error.as_deref().unwrap_or_default(),
            editor_min,
            editor_max,
            &mut options,
        )
        .map(|hunk| hunk.sha.clone())
    };
    if let Some(sha) = clicked_sha {
        // The blamed commit in the list (selecting it replaces the blame the click pointed into).
        let index = view
            .history
            .entries
            .iter()
            .position(|entry| entry.commit.sha == sha);
        if let Some(index) = index {
            if view.selected != Some(index) {
                state.select_file_history_entry(Some(index));
            }
            if let Some(view) = state.file_history.as_mut() {
                view.scroll_to = Some(index);
            }
        }
    }
    if let Some(view) = &state.file_history
        && let Some(error) = &view.blame_error
        && view.file_view
    {
        // The blame failed (a file libgit2 cannot blame): say so under the toolbar, on the right.
        let w = cx.text_size(cx.fonts.regular, 11.0, error).x;
        cx.draw_text(
            draw,
            cx.fonts.regular,
            11.0,
            vec2(
                editor_max.x - cx.px(40.0) - w,
                css_text_y(cx, 11.0, bar_y + cx.px(29.0), false),
            ),
            p.red,
            error,
        );
    }
}

/// Toolbar: File View | Diff View centred; the diff modes and Blame on the right.
fn draw_toolbar(cx: &mut Cx<'_>, state: &mut RepoState, pane_x: f32, pane_w: f32, bar_y: f32) {
    let Some(view) = state.file_history.as_ref() else {
        return;
    };
    let (file_view, blame) = (view.file_view, view.blame);
    let fw = view_toggle_width(cx, Some("File View"), None);
    let dw = view_toggle_width(cx, Some("Diff View"), None);
    let mut x = (pane_x + (pane_w - fw - dw) / 2.0).round();
    if view_toggle_button(
        cx,
        "##history_file_view",
        Some("File View"),
        None,
        vec2(x, bar_y),
        file_view,
        None,
    ) && !file_view
    {
        if let Some(view) = state.file_history.as_mut() {
            view.file_view = true;
        }
        state.reload_file_history_diff();
    }
    if view_toggle_button(
        cx,
        "##history_diff_view",
        Some("Diff View"),
        None,
        vec2(x + fw, bar_y),
        !file_view,
        None,
    ) && file_view
    {
        if let Some(view) = state.file_history.as_mut() {
            view.file_view = false;
            view.blame = false;
        }
        state.reload_file_history_diff();
    }
    const MODES: [(&str, &str, &str, DiffMode); 3] = [
        (
            "##history_hunk",
            "far:list-alt",
            "Hunk View",
            DiffMode::Hunk,
        ),
        (
            "##history_inline",
            "far:list",
            "Inline View",
            DiffMode::Inline,
        ),
        (
            "##history_split",
            "far:columns",
            "Split View",
            DiffMode::Split,
        ),
    ];
    let bw = view_toggle_width(cx, Some("Blame"), None);
    let mut total = bw;
    if !file_view {
        total += cx.px(10.0);
        total += MODES
            .iter()
            .map(|(_, icon, _, _)| view_toggle_width(cx, None, Some(icon)))
            .sum::<f32>();
    }
    x = pane_x + pane_w - cx.px(10.0) - total;
    if !file_view {
        for (id, icon, tip, mode) in MODES {
            if view_toggle_button(
                cx,
                id,
                None,
                Some(icon),
                vec2(x, bar_y),
                state.diff_mode == mode,
                Some(tip),
            ) && state.diff_mode != mode
            {
                state.diff_mode = mode;
                state.reload_file_history_diff();
            }
            x += view_toggle_width(cx, None, Some(icon));
        }
        x += cx.px(10.0);
    }
    if view_toggle_button(
        cx,
        "##history_blame",
        Some("Blame"),
        None,
        vec2(x, bar_y),
        blame,
        Some("Who last changed each line"),
    ) {
        let now_on = !blame;
        if let Some(view) = state.file_history.as_mut() {
            view.blame = now_on;
        }
        if now_on && !file_view {
            if let Some(view) = state.file_history.as_mut() {
                view.file_view = true;
            }
            state.reload_file_history_diff();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_entry_is_labelled_by_what_it_did() {
        assert_eq!(status_word(FileStatus::Added), "ADDED");
        assert_eq!(status_word(FileStatus::Deleted), "DELETED");
        assert_eq!(status_word(FileStatus::Renamed), "RENAMED");
        assert_eq!(status_word(FileStatus::Modified), "MODIFIED");
        assert_eq!(status_word(FileStatus::TypeChange), "MODIFIED");
    }
}
