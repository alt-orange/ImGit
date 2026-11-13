//! The right panel: the selected commit's details, the working directory's staged / unstaged files, the commit
//! form.

mod commit;
mod controls;
mod files;
mod wip;

use dear_imgui_rs::WindowFlags;
use imgit_app::repo_state::UndoEntry;
use imgit_app::{RepoState, WIP_SHA};
use imgit_git::{Error, FileChange, NodeType};

use crate::app::application::Application;
use crate::ui::{Cx, Vec2, vec2};

/// The commit view's "View all files" list: every file of the commit's tree, read once per commit.
#[derive(Default)]
struct AllFiles {
    /// The repository and commit the `files` were read for, and how many files the commit changed.
    key: String,
    files: Vec<FileChange>,
}

/// What the detail panel remembers between frames (the C++ function-local statics).
#[derive(Default)]
pub struct State {
    /// The commit view's "View all files": every file of the commit's tree instead of its changes only.
    view_all_files: bool,
    /// A caret move behind the summary text asked for and not yet carried out.
    caret_to_end: bool,
    all_files: AllFiles,
}

/// The detail panel, `size` device px.
pub fn draw_detail_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    size: Vec2,
) {
    app.with_panel_state(
        |panels| &mut panels.detail_panel,
        |app, panel| draw(cx, app, state, panel, size),
    );
}

fn draw(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    panel: &mut State,
    size: Vec2,
) {
    let ui = cx.ui;
    let p = cx.palette;
    let origin = cx.cursor();
    ui.child_window("##detail_panel")
        .size(size.into())
        .flags(WindowFlags::NO_SCROLLBAR)
        .build(ui, || {
            // .commit-detail-wrapper: panel-bg0.
            let draw = cx.window_draw_list();
            draw.add_rect_filled(origin, origin + size, p.panel_bg0, 0.0);
            draw.add_line(
                origin,
                vec2(origin.x, origin.y + size.y),
                p.panel_border,
                1.0,
            );
            cx.set_cursor(vec2(origin.x + 1.0, origin.y));
            let inner = vec2(size.x - 1.0, size.y);

            let compared = compared_commit(state);
            if let Some(commit) = compared {
                commit::draw_compare_panel(cx, app, state, &commit, inner);
            } else if state.selected.len() > 1 {
                commit::draw_multi_panel(cx, state, inner);
            } else if let Some(commit) = state
                .single_selected()
                .filter(|commit| commit.node_type != NodeType::WorkDir)
                .cloned()
            {
                commit::draw_commit_panel(cx, app, state, panel, &commit, inner);
            } else {
                wip::draw_wip_panel(cx, app, state, panel, inner);
            }
        });
}

/// The WIP row and one commit selected: that commit, to compare against the working directory (merge and
/// plain commits only).
fn compared_commit(state: &RepoState) -> Option<imgit_git::Commit> {
    let [a, b] = state.selected.as_slice() else {
        return None;
    };
    let other = match (a == WIP_SHA, b == WIP_SHA) {
        (true, _) => b,
        (_, true) => a,
        _ => return None,
    };
    state
        .commit_for(other)
        .filter(|commit| matches!(commit.node_type, NodeType::Commit | NodeType::Merge))
        .cloned()
}

/// Stashes every change with `message` (empty for git's default); `false` when it failed.
pub fn stash_changes(
    _cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    message: &str,
) -> bool {
    if app.refuse_while_busy(state) {
        return false;
    }
    let pushed = state.repo.stash_push(message);
    let Some(stash_sha) = app.run(state, "Stash", pushed, Some("Changes stashed")) else {
        return false;
    };
    // The undo finds the stash by its commit: one pushed since shifts the indexes.
    let message = message.to_owned();
    state.push_undo(
        UndoEntry::new("stash", move |repo| {
            let index = repo
                .stash_index_of(&stash_sha)
                .ok_or_else(|| Error::Message("the stash is gone".to_owned()))?;
            repo.stash_pop(index)
        })
        .with_redo(move |repo| repo.stash_push(&message).map(|_| ())),
    );
    true
}
