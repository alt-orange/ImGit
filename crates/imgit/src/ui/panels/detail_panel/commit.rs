//! The views of committed history: one commit (message with the reword editor, author, parents, changed
//! files), a commit compared against the working directory, and several commits selected.

use dear_imgui_rs::{MouseCursor, StyleColor, WindowFlags};
use imgit_app::{RepoState, WIP_SHA};
use imgit_git::repository::short_sha;
use imgit_git::{Author, Commit, FileChange, FileStatus, NodeType};

use super::State;
use super::controls::{
    MessageBox, SummaryFocus, ViewToggle, css_text, css_text_in, css_text_y, tone_action_button,
    view_toggle_row,
};
use super::files::{FileList, ListKind, apply_events, build_tree, with_unchanged_files};
use crate::app::application::Application;
use crate::theme::{FONT_SIZE_NORMAL, FONT_SIZE_ROW};
use crate::ui::panels::file_editor::navigate_from_editor;
use crate::ui::panels::menus::{can_reword, jump_to_commit, reword_commit};
use crate::ui::widgets::ResizeAxis;
use crate::ui::{Cx, DrawList, Vec2, vec2};

/// The first six characters of a sha, as the header and the parents show it.
fn sha6(sha: &str) -> &str {
    sha.get(..6).unwrap_or(short_sha(sha))
}

/// "Co-Authored-By: Name <email>" trailers of a commit message.
pub fn co_authors(body: &str) -> Vec<Author> {
    const KEY: &str = "co-authored-by:";
    body.split('\n')
        .filter_map(|line| {
            let prefix = line.get(..KEY.len())?;
            if line.len() <= KEY.len() || !prefix.eq_ignore_ascii_case(KEY) {
                return None;
            }
            let open = line.find('<');
            let close = open.and_then(|open| line[open..].find('>').map(|close| open + close));
            let name = line[KEY.len()..open.unwrap_or(line.len())].trim_matches(' ');
            let email = match (open, close) {
                (Some(open), Some(close)) => line[open + 1..close].to_owned(),
                _ => String::new(),
            };
            Some(Author {
                name: name.to_owned(),
                email,
            })
        })
        .collect()
}

/// Text laid out top-down with CSS line boxes (line-height 1.42857, Bootstrap's body default).
fn css_line_height(cx: &Cx<'_>, font_size: f32) -> f32 {
    cx.px(font_size) * 1.42857
}

/// Draws `text` in the regular font wrapped to `width` with the given CSS font size; returns the height used.
fn draw_wrapped(
    cx: &Cx<'_>,
    draw: DrawList<'_>,
    size: f32,
    pos: Vec2,
    width: f32,
    color: crate::theme::Color,
    text: &str,
) -> f32 {
    let font = cx.fonts.regular;
    let line_h = css_line_height(cx, size);
    let glyph_h = cx.font_px(font, size);
    let mut y = pos.y;
    for line in cx.wrap_text(font, size, text, width) {
        if !line.is_empty() {
            draw.add_text(
                font,
                glyph_h,
                vec2(pos.x, y + (line_h - glyph_h) / 2.0),
                color,
                line,
            );
        }
        y += line_h;
    }
    y - pos.y
}

/// The commit view: an optional 36px "file changes in working directory" banner, the 36px header, then the
/// message view and the file list sharing the rest equally.
pub fn draw_commit_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    panel: &mut State,
    commit: &Commit,
    size: Vec2,
) {
    state.load_commit_files(&commit.sha);
    let ui = cx.ui;
    let update_message = ui
        .child_window("##commit_detail")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR)
        .build(ui, || commit_body(cx, app, state, panel, commit, size))
        .unwrap_or(false);
    // Update Message rewrites the history `commit` points into (HEAD alone, or the commits above the reworded
    // one) and reloads it: done once the panel is drawn.
    if update_message {
        let mut message = state.amend_summary.clone();
        if !state.amend_description.is_empty() {
            message.push_str("\n\n");
            message.push_str(&state.amend_description);
        }
        // The reworded commit stays selected.
        if reword_commit(cx, app, state, &commit.sha, &message) {
            state.amending_message = false;
        }
    }
}

/// The banner, header, message and author block, and the file list. Returns whether Update Message was pressed.
fn commit_body(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    panel: &mut State,
    commit: &Commit,
    size: Vec2,
) -> bool {
    let ui = cx.ui;
    let p = cx.palette;
    let draw = cx.window_draw_list();
    let origin = cx.cursor();
    let right = origin.x + size.x - cx.px(10.0);
    let left = origin.x + cx.px(10.0);
    let is_stash = commit.node_type == NodeType::Stash;
    let width = size.x;

    // The banner.
    let wip = state
        .history
        .commits
        .iter()
        .find(|row| row.node_type == NodeType::WorkDir)
        .map(|row| row.sha.clone());
    let wip_files = if wip.is_some() {
        state.status.count()
    } else {
        0
    };
    let banner_h = if wip_files > 0 { cx.px(36.0) } else { 0.0 };
    if let (true, Some(wip_sha)) = (banner_h > 0.0, &wip) {
        draw.add_rect_filled(
            origin,
            vec2(origin.x + width, origin.y + banner_h),
            p.accent.with_alpha(0.4),
            0.0,
        );
        let text = format!(
            "{wip_files} {} in working directory",
            if wip_files == 1 {
                "file change"
            } else {
                "file changes"
            }
        );
        let button = "View Changes";
        let text_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, &text).x;
        let button_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, button).x + cx.px(12.0);
        let x = (origin.x + (width - (text_w + cx.px(10.0) + button_w)) / 2.0).round();
        css_text(
            cx,
            draw,
            FONT_SIZE_ROW,
            vec2(x, origin.y + cx.px(9.4)),
            p.text_selected,
            &text,
        );
        let bmin = vec2(x + text_w + cx.px(10.0), origin.y + cx.px(7.0));
        let bmax = vec2(bmin.x + button_w, bmin.y + cx.px(22.0));
        cx.set_cursor(bmin);
        if ui.invisible_button("##view_changes", bmax - bmin) {
            let wip_sha = wip_sha.clone();
            navigate_from_editor(app, state, move |_app, state| {
                state.select_only(&wip_sha);
                let _ = state.close_diff();
            });
        }
        if ui.is_item_hovered() {
            draw.add_rect_filled(bmin, bmax, p.ink(0.1), cx.px(2.0));
        }
        draw.add_rect(bmin, bmax, p.text_normal, cx.px(2.0), 1.0);
        css_text(
            cx,
            draw,
            FONT_SIZE_ROW,
            vec2(bmin.x + cx.px(6.0), bmin.y + cx.px(2.5)),
            p.text_normal,
            button,
        );
    }

    // Header: "commit: <sha6>", centred.
    let header_top = origin.y + banner_h;
    {
        let label = if is_stash { "stash:" } else { "commit:" };
        let sha = sha6(&commit.sha);
        let label_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label).x;
        let sha_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, sha).x;
        let total = label_w + cx.px(5.0) + sha_w;
        let x = origin.x + cx.px(10.0) + (width - cx.px(20.0) - total) / 2.0;
        let y = header_top + cx.px(9.5);
        css_text(cx, draw, FONT_SIZE_ROW, vec2(x, y), p.text_normal, label);
        cx.set_cursor(vec2(x + label_w + cx.px(5.0), y));
        ui.invisible_button("##sha", vec2(sha_w, cx.px(17.0)));
        css_text(
            cx,
            draw,
            FONT_SIZE_ROW,
            vec2(x + label_w + cx.px(5.0), y),
            p.text_selected,
            sha,
        );
        cx.tooltip("Copy commit sha");
        if ui.is_item_clicked_with_button(dear_imgui_rs::MouseButton::Left) {
            app.copy_to_clipboard(cx, &commit.sha);
        }
        draw.add_rect_filled(
            vec2(origin.x, header_top + cx.px(35.0)),
            vec2(origin.x + width, header_top + cx.px(36.0)),
            p.section_border,
            0.0,
        );
    }

    // The message: the re-resizable box, layout.CommitMessage.rightPanelHeight tall (150 by default) within
    // [120, panelHeight - 213] (52 more while editing), panelHeight being what the header leaves. The view
    // fills it but its 17px bottom margin; the editor fills it but 50px. Its 100 x 4 handle, 9px above its
    // bottom edge, drags it with the row-resize cursor. The message of HEAD, or of an older commit of its
    // first-parent chain above the last merge, is editable: hovering the view turns its border blue, clicking
    // it opens the editor with Update Message / Cancel Amend under it.
    let box_top = header_top + cx.px(46.0);
    let amendable = !is_stash && can_reword(state, &commit.sha);
    if !amendable || state.amend_sha.as_deref() != Some(commit.sha.as_str()) {
        state.amending_message = false;
    }
    let editing = state.amending_message;
    let mut update_message = false;
    let panel_h = origin.y + size.y - header_top - cx.px(36.0);
    let max_h = panel_h - cx.px(143.0 + 50.0 + 20.0) - if editing { cx.px(52.0) } else { 0.0 };
    let min_h = cx.px(120.0);
    // A short window may leave less than the minimum: the box keeps the minimum rather than going negative.
    let max_h = min_h.max(max_h);
    let mut message_h = cx
        .px(app.settings.commit_message_height)
        .clamp(min_h, max_h);
    {
        let cx_mid = (origin.x + width / 2.0).round();
        let hmin = vec2(cx_mid - cx.px(50.0), box_top + message_h - cx.px(9.0));
        let hmax = vec2(cx_mid + cx.px(50.0), hmin.y + cx.px(4.0));
        let before = message_h;
        if cx.resize_drag(
            "##message_resize",
            hmin,
            hmax,
            ResizeAxis::Rows,
            1.0,
            &mut message_h,
            min_h,
            max_h,
        ) {
            app.settings.save(&app.config_path("settings.txt"));
        }
        if message_h != before {
            app.settings.commit_message_height = message_h / cx.scale;
        }
        draw.add_rect_filled(
            vec2(cx_mid - cx.px(50.0), box_top + message_h - cx.px(9.0)),
            vec2(cx_mid + cx.px(50.0), box_top + message_h - cx.px(5.0)),
            p.section_border,
            cx.px(4.0),
        );
    }
    if editing {
        let focus = if state.amend_focus_requested {
            SummaryFocus::CaretEnd
        } else {
            SummaryFocus::None
        };
        MessageBox {
            summary: &mut state.amend_summary,
            description: &mut state.amend_description,
            min: vec2(left, box_top),
            max: vec2(right, box_top + message_h - cx.px(50.0)),
            summary_id: "##amend_summary",
            description_id: "##amend_description",
            focus,
            summary_hint: "Commit summary",
            counter_shown: true,
            caret_to_end: &mut panel.caret_to_end,
        }
        .draw(cx);
        state.amend_focus_requested = false;
        // The button row (10px padding): Update Message, 5px, Cancel Amend, sharing the width.
        let row_top = box_top + message_h + cx.px(10.0);
        let button_w = (right - left - cx.px(5.0)) / 2.0;
        let umin = vec2(left, row_top);
        let umax = vec2((left + button_w).round(), row_top + cx.px(31.14));
        let cmin = vec2(umax.x + cx.px(5.0), row_top);
        let cmax = vec2(right, umax.y);
        update_message = tone_action_button(
            cx,
            "##update_message",
            "Update Message",
            umin,
            umax,
            p.green,
            // A message needs a summary.
            !state.amend_summary.trim().is_empty(),
        );
        if tone_action_button(
            cx,
            "##cancel_amend",
            "Cancel Amend",
            cmin,
            cmax,
            p.red,
            true,
        ) {
            state.amending_message = false;
        }
    } else {
        let bmin = vec2(left, box_top);
        let bmax = vec2(right, (bmin.y + 1.0).max(box_top + message_h - cx.px(17.0)));
        let mut hovered = false;
        if amendable {
            cx.set_cursor(bmin);
            if ui.invisible_button("##editable_message", bmax - bmin) {
                state.amending_message = true;
                state.amend_sha = Some(commit.sha.clone());
                state.amend_focus_requested = true;
                state.amend_summary = commit.summary.clone();
                state.amend_description = commit.body.trim_end_matches(['\n', '\r']).to_owned();
            }
            hovered = ui.is_item_hovered();
            if hovered {
                ui.set_mouse_cursor(Some(MouseCursor::Hand));
                let is_head = state.history.head_sha.as_deref() == Some(commit.sha.as_str());
                cx.styled_tooltip_left(
                    bmin,
                    bmax,
                    if is_head {
                        "Click to amend your commit message"
                    } else {
                        "Click to edit this commit's message"
                    },
                );
            }
        }
        draw.add_rect_filled(bmin, bmax, p.input_bg, cx.px(4.0));
        draw.add_rect(
            bmin,
            bmax,
            if hovered { p.accent } else { p.subtle_border },
            cx.px(4.0),
            1.0,
        );
        draw.with_clip_rect(bmin, bmax, true, || {
            let text_x = bmin.x + cx.px(11.0);
            let text_w = bmax.x - cx.px(11.0) - text_x;
            let mut y = bmin.y + cx.px(11.0);
            y += draw_wrapped(
                cx,
                draw,
                16.0,
                vec2(text_x, y),
                text_w,
                p.text_normal,
                &commit.summary,
            );
            if !commit.body.is_empty() {
                y += cx.px(10.0);
                draw_wrapped(
                    cx,
                    draw,
                    FONT_SIZE_ROW,
                    vec2(text_x, y),
                    text_w,
                    p.text_normal,
                    &commit.body,
                );
            }
        });
    }

    // Author: 40px avatar, name (14px), "authored <date>", "parent: <sha6>" on the right.
    let author_top = box_top + message_h + if editing { cx.px(61.14) } else { 0.0 };
    {
        let avatar = cx.px(40.0);
        cx.draw_avatar(
            draw,
            vec2(left + avatar / 2.0, author_top + avatar / 2.0),
            avatar,
            &commit.author,
            Some(cx.px(2.0)),
        );
        let name_x = left + cx.px(48.0);
        cx.draw_text_ellipsis(
            draw,
            cx.fonts.regular,
            14.0,
            vec2(name_x, css_text_y(cx, 14.0, author_top + cx.px(0.7))),
            right - name_x - cx.px(100.0),
            p.text_normal,
            &commit.author.name,
        );
        let verb = "authored";
        let verb_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, verb).x;
        css_text(
            cx,
            draw,
            FONT_SIZE_ROW,
            vec2(name_x, author_top + cx.px(22.1)),
            p.text_secondary,
            verb,
        );
        let date = cx.format_date_time(commit.author_time);
        let date_pos = vec2(name_x + verb_w + cx.px(5.0), author_top + cx.px(22.1));
        css_text(cx, draw, FONT_SIZE_ROW, date_pos, p.text_normal, &date);
        // The date alone in the Date word Format, and the commit's.
        let mut dates = format!("Authored {}", cx.format_date_word(commit.author_time));
        if commit.time != commit.author_time {
            dates.push_str(&format!("\nCommitted {}", cx.format_date_long(commit.time)));
        }
        let date_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, &date).x;
        cx.tooltip_on_rect(
            date_pos,
            vec2(date_pos.x + date_w, date_pos.y + cx.px(20.0)),
            &dates,
            false,
        );
        cx.tooltip_on_rect(
            vec2(left, author_top),
            vec2(left + avatar, author_top + avatar),
            &format!("{} <{}>", commit.author.name, commit.author.email),
            false,
        );

        let mut px_right = right;
        for (i, parent) in commit.parents.iter().enumerate().rev() {
            let short = sha6(parent);
            let w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, short).x;
            px_right -= w;
            cx.set_cursor(vec2(px_right, author_top));
            let hovered = {
                let _id = ui.push_id(i as i32);
                ui.invisible_button("##parent", vec2(w, cx.px(17.0)));
                if ui.is_item_clicked_with_button(dear_imgui_rs::MouseButton::Left) {
                    jump_to_commit(state, parent);
                }
                let hovered = ui.is_item_hovered();
                cx.tooltip("Jump to parent");
                hovered
            };
            css_text(
                cx,
                draw,
                FONT_SIZE_ROW,
                vec2(px_right, author_top),
                if hovered {
                    p.text_selected
                } else {
                    p.text_normal
                },
                short,
            );
            px_right -= cx.px(5.0);
        }
        if !commit.parents.is_empty() {
            let label = if commit.parents.len() == 1 {
                "parent:"
            } else {
                "parents:"
            };
            let w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, label).x;
            css_text(
                cx,
                draw,
                FONT_SIZE_ROW,
                vec2(px_right - w, author_top),
                p.text_secondary,
                label,
            );
        }
    }

    // Co-authors from the message trailers.
    let co = co_authors(&commit.body);
    if !co.is_empty() {
        let co_y = author_top + cx.px(44.0);
        css_text(
            cx,
            draw,
            FONT_SIZE_ROW,
            vec2(left, co_y + cx.px(2.0)),
            p.text_secondary,
            "Co-authors:",
        );
        let mut x = left
            + cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, "Co-authors:")
                .x
            + cx.px(5.0);
        for author in &co {
            let avatar = cx.px(20.0);
            cx.draw_avatar(
                draw,
                vec2(x + avatar / 2.0, co_y + avatar / 2.0),
                avatar,
                author,
                Some(cx.px(2.0)),
            );
            cx.tooltip_on_rect(
                vec2(x, co_y),
                vec2(x + avatar, co_y + avatar),
                &format!("{} <{}>", author.name, author.email),
                false,
            );
            x += avatar + cx.px(4.0);
        }
    }

    if let Some(error) = &state.commit_files_error {
        cx.set_cursor(vec2(left, author_top + cx.px(70.0)));
        let _color = ui.push_style_color(StyleColor::Text, p.red);
        ui.text_wrapped(error);
        return update_message;
    }

    // .tiny-files-readout: "✎ 1 modified" etc., a 10px solid icon (fa-fw), 5px, the text, 8px.
    {
        let (mut added, mut modified, mut deleted, mut renamed) = (0, 0, 0, 0);
        for change in &state.commit_files {
            match change.status {
                FileStatus::Added | FileStatus::Untracked => added += 1,
                FileStatus::Deleted => deleted += 1,
                FileStatus::Renamed => renamed += 1,
                _ => modified += 1,
            }
        }
        let line_top = author_top + cx.px(70.4);
        let mut x = left;
        let mut item = |cx: &mut Cx<'_>, count: usize, icon: &str, color, word: &str| {
            if count == 0 {
                return;
            }
            cx.draw_fa_icon(
                draw,
                icon,
                vec2(x + cx.px(6.25), line_top + cx.px(9.3)),
                cx.px(10.0),
                color,
            );
            let text = format!("{count} {word}");
            css_text(
                cx,
                draw,
                FONT_SIZE_ROW,
                vec2(x + cx.px(17.5), line_top),
                p.text_normal,
                &text,
            );
            x += cx.px(17.5) + cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, &text).x + cx.px(8.0);
        };
        item(cx, modified, "fas:pencil", p.orange, "modified");
        item(cx, added, "fas:plus", p.green, "added");
        item(cx, deleted, "fas:minus", p.red, "deleted");
        item(cx, renamed, "fas:pencil", p.orange, "renamed");
    }

    // Sort, Path | Tree, View all files
    let row_top = author_top + cx.px(98.9);
    let row_h = cx.px(24.1);
    view_toggle_row(
        cx,
        ViewToggle {
            file_sort: app.settings.file_sort,
            tree_view: &mut state.tree_view,
            origin_x: origin.x,
            width,
            top: row_top,
            sort_x: left + cx.px(10.0),
            semibold: false,
        },
    );
    {
        let all = "View all files";
        let all_w = cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, all).x;
        let label_x = origin.x + width - cx.px(15.0) - all_w;
        let box_x = label_x - cx.px(20.0);
        cx.set_cursor(vec2(box_x, row_top));
        if ui.invisible_button("##view_all", vec2(label_x + all_w - box_x, row_h)) {
            panel.view_all_files = !panel.view_all_files;
        }
        let cmin = vec2(box_x, row_top + cx.px(6.1));
        let cmax = vec2(cmin.x + cx.px(12.0), cmin.y + cx.px(12.0));
        if panel.view_all_files {
            draw.add_rect_filled(cmin, cmax, p.accent, cx.px(1.0));
            cx.draw_fa_icon(
                draw,
                "fas:check",
                vec2(cmin.x + cx.px(6.0), cmin.y + cx.px(6.0)),
                cx.px(9.0),
                p.white,
            );
        } else {
            draw.add_rect_filled(cmin, cmax, p.input_bg, cx.px(1.0));
            draw.add_rect(cmin, cmax, p.text_disabled, cx.px(1.0), 1.0);
        }
        css_text(
            cx,
            draw,
            FONT_SIZE_ROW,
            vec2(label_x, row_top + cx.px(3.5)),
            p.text_normal,
            all,
        );
    }
    let mut y = author_top + cx.px(129.0);
    draw.add_rect_filled(vec2(left, y), vec2(right, y + 1.0), p.section_border, 0.0);
    y += 1.0;

    // Files
    cx.set_cursor(vec2(left, y));
    if panel.view_all_files {
        panel.load_all_files(state, &commit.sha);
    }
    let kind = ListKind::Commit(commit.sha.clone());
    let list_size = vec2(right - left, (origin.y + size.y - cx.px(10.0) - y).max(1.0));
    let tree_view = state.tree_view;
    let events = ui
        .child_window("##commit_files")
        .size(list_size.into())
        .build(ui, || {
            let files: &[FileChange] = if panel.view_all_files {
                &panel.all_files.files
            } else {
                &state.commit_files
            };
            let root = build_tree(files, tree_view);
            let mut list = FileList {
                width: right - left,
                kind: kind.clone(),
                action: None,
                file_sort: app.settings.file_sort,
                diff_target: state.diff_target.as_ref(),
                expanded: &mut state.expanded_folders,
                events: Vec::new(),
            };
            let _spacing = ui.push_style_var_y(dear_imgui_rs::StyleVarVec2::ItemSpacing, 0.0);
            list.draw(cx, &root, 1, "commit:");
            list.events
        })
        .unwrap_or_default();
    apply_events(cx, app, state, &kind, events);
    update_message
}

impl State {
    /// The commit's changes plus the files of its tree it left as they were, read once per commit.
    pub(super) fn load_all_files(&mut self, state: &RepoState, sha: &str) {
        let key = format!(
            "{}\n{sha}\n{}",
            state.repo.workdir().display(),
            state.commit_files.len()
        );
        if self.all_files.key == key {
            return;
        }
        let paths = state.repo.commit_tree_paths(sha).unwrap_or_default();
        self.all_files = super::AllFiles {
            files: with_unchanged_files(&state.commit_files, paths),
            key,
        };
    }
}

/// The WIP row and one commit selected (Compare commit against working directory): the files that differ
/// between that commit and the working directory; clicking one opens its diff.
pub fn draw_compare_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    commit: &Commit,
    size: Vec2,
) {
    let ui = cx.ui;
    let p = cx.palette;
    if state.compare_sha.as_deref() != Some(commit.sha.as_str()) {
        state.load_compare_files(&commit.sha);
    }
    let kind = ListKind::Compare(commit.sha.clone());
    let events = ui
        .child_window("##compare_detail")
        .size(size.into())
        .build(ui, || {
            ui.dummy([0.0, cx.px(8.0)]);
            ui.indent_by(cx.px(12.0));
            {
                let _font = cx.push_font(cx.fonts.semibold, FONT_SIZE_NORMAL + 1.0);
                ui.text("Comparing against the working directory");
            }
            let _font = cx.push_font(cx.fonts.regular, FONT_SIZE_ROW);
            {
                let _color = ui.push_style_color(StyleColor::Text, p.text_secondary);
                let _mono = cx.push_font(cx.fonts.mono, FONT_SIZE_ROW);
                ui.text(short_sha(&commit.sha));
            }
            ui.same_line();
            ui.text(&commit.summary);
            {
                let _color = ui.push_style_color(StyleColor::Text, p.text_secondary);
                if let Some(error) = &state.compare_error {
                    ui.text_wrapped(error);
                } else {
                    let count = state.compare_files.len();
                    ui.text(format!(
                        "{count} {} differ",
                        if count == 1 { "file" } else { "files" }
                    ));
                }
            }
            drop(_font);
            ui.unindent_by(cx.px(12.0));
            ui.dummy([0.0, cx.px(6.0)]);
            let width = cx.avail().x;
            let root = build_tree(&state.compare_files, state.tree_view);
            let mut list = FileList {
                width,
                kind: kind.clone(),
                action: None,
                file_sort: app.settings.file_sort,
                diff_target: state.diff_target.as_ref(),
                expanded: &mut state.expanded_folders,
                events: Vec::new(),
            };
            let _spacing = ui.push_style_var_y(dear_imgui_rs::StyleVarVec2::ItemSpacing, 0.0);
            list.draw(cx, &root, 1, "compare:");
            list.events
        })
        .unwrap_or_default();
    apply_events(cx, app, state, &kind, events);
}

/// Several commits selected: a summary and the list of them.
pub fn draw_multi_panel(cx: &mut Cx<'_>, state: &RepoState, size: Vec2) {
    let ui = cx.ui;
    let p = cx.palette;
    ui.child_window("##multi_detail")
        .size(size.into())
        .build(ui, || {
            ui.dummy([0.0, cx.px(8.0)]);
            ui.indent_by(cx.px(12.0));
            {
                let _font = cx.push_font(cx.fonts.semibold, FONT_SIZE_NORMAL + 1.0);
                ui.text(format!("{} commits selected", state.selected.len()));
            }
            {
                let _color = ui.push_style_color(StyleColor::Text, p.text_secondary);
                ui.text_wrapped(
                    "Right-click the graph to cherry pick, squash or drop them together. Select one \
                     commit to see its changes.",
                );
            }
            ui.unindent_by(cx.px(12.0));
            ui.dummy([0.0, cx.px(8.0)]);
            let _font = cx.push_font(cx.fonts.regular, FONT_SIZE_ROW);
            for sha in &state.selected {
                let Some(commit) = state.commit_for(sha) else {
                    continue;
                };
                ui.indent_by(cx.px(12.0));
                {
                    let _color = ui.push_style_color(StyleColor::Text, p.text_secondary);
                    let _mono = cx.push_font(cx.fonts.mono, FONT_SIZE_ROW);
                    ui.text(if sha == WIP_SHA {
                        "WIP    "
                    } else {
                        short_sha(sha)
                    });
                }
                ui.same_line();
                ui.text(&commit.summary);
                ui.unindent_by(cx.px(12.0));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_co_author_trailers() {
        let body = "Some text\n\nCo-authored-by: Ann Lee <ann@example.com>\n\
                    co-authored-by:Bob\nCo-Authored-By:   Cy  <cy@x.org>  \nnot a trailer: x";
        let authors = co_authors(body);
        assert_eq!(
            authors,
            [
                Author {
                    name: "Ann Lee".into(),
                    email: "ann@example.com".into()
                },
                Author {
                    name: "Bob".into(),
                    email: String::new()
                },
                Author {
                    name: "Cy".into(),
                    email: "cy@x.org".into()
                },
            ]
        );
        assert!(co_authors("").is_empty());
        // Nothing after the key: no trailer.
        assert!(co_authors("Co-authored-by:").is_empty());
    }

    #[test]
    fn shas_are_six_characters() {
        assert_eq!(sha6("0123456789abcdef"), "012345");
        assert_eq!(sha6("abc"), "abc");
    }
}
