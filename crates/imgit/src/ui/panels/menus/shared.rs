//! What the menus share: the repository-bound click, the undo entries, the sequencer reports, the commit chain
//! the history rewrites work on, the branch groups and the item builders more than one menu offers.

use std::path::Path;

use imgit_app::{RepoState, WIP_SHA};
use imgit_git::repository::{Outcome, short_sha};
use imgit_git::{
    Commit, Error as GitError, RebaseAction, RebaseStep, Ref, RefType, Repository, ResetMode, Sha,
    cli,
};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::ui::Cx;
use crate::ui::menu::{MenuItem, menu_item};

pub use imgit_git::RepoState as OperationState;

/// A row whose click runs `run` on the repository at `workdir` (found again when it runs: the tab may have
/// been closed or its state be out meanwhile, then nothing happens).
pub fn repo_item(
    label: impl Into<String>,
    workdir: &Path,
    run: impl FnOnce(&mut Cx<'_>, &mut Application, &mut RepoState) + 'static,
) -> MenuItem {
    let workdir = workdir.to_path_buf();
    menu_item(label, move |cx, app| {
        app.with_repo(&workdir, |app, state| run(cx, app, state));
    })
}

/// The working directory of the repository the menu is for.
pub fn workdir_of(state: &RepoState) -> std::path::PathBuf {
    state.repo.workdir().to_path_buf()
}

/// Undo for anything that moved HEAD: go back to the branch (or detached commit) we were on.
pub fn head_restore_entry(
    state: &RepoState,
    label: &str,
    redo: impl FnMut(&Repository) -> imgit_git::Result<()> + 'static,
) -> imgit_app::repo_state::UndoEntry {
    let branch = if state.history.detached {
        None
    } else {
        state.history.head_branch.clone()
    };
    let sha = state.history.head_sha.clone();
    imgit_app::repo_state::UndoEntry::new(label, move |repo: &Repository| {
        if let Some(branch) = branch.as_deref().filter(|branch| !branch.is_empty()) {
            return repo.checkout_branch(branch);
        }
        if let Some(sha) = sha.as_deref().filter(|sha| !sha.is_empty()) {
            return repo.checkout_commit(sha);
        }
        Ok(())
    })
    .with_redo(redo)
}

/// Undo for history rewrites on the current branch: a hard reset to the previous tip. Uncommitted changes
/// present at the time are stashed around the reset so they survive.
pub fn tip_restore_entry(
    state: &RepoState,
    label: &str,
    mode: ResetMode,
) -> imgit_app::repo_state::UndoEntry {
    let sha = state.history.head_sha.clone();
    imgit_app::repo_state::UndoEntry::new(label, move |repo: &Repository| {
        let Some(sha) = sha.as_deref().filter(|sha| !sha.is_empty()) else {
            return Err(GitError::Message("nothing to restore".to_owned()));
        };
        let dirty = repo.load_status().map(|status| status.count()).unwrap_or(0) > 0;
        let stashed = dirty && repo.stash_push("ImGit: auto stash before undo").is_ok();
        let result = repo.reset(sha, mode);
        if stashed {
            let _ = repo.stash_pop(0);
        }
        result
    })
    .with_redo_mode(mode)
}

/// The toast of a cherry pick / revert / merge, and the reload.
pub fn report_sequencer(
    app: &mut Application,
    state: &mut RepoState,
    what: &str,
    result: &imgit_git::Result<Outcome>,
    success: &str,
) {
    match result {
        Err(error) => app.toasts.push_with_detail(
            ToastKind::Error,
            format!("{what} failed"),
            error.to_string(),
        ),
        Ok(Outcome::Conflicts) => app.toasts.push_with_detail(
            ToastKind::Error,
            format!("{what} stopped on conflicts"),
            "Resolve them in the working directory, then commit.",
        ),
        Ok(Outcome::Done) => {
            if !success.is_empty() {
                app.toasts.push(ToastKind::Success, success);
            }
        }
    }
    state.refresh();
}

/// Merges `name` into HEAD, with an undo entry.
pub fn merge_into_head(
    app: &mut Application,
    state: &mut RepoState,
    name: &str,
    no_fast_forward: bool,
) {
    // Preferences > Commit > Squash: the branch's changes are staged without a merge commit.
    if state.repo.config_bool("imgit.squashMerges", false) {
        app.run_git_cli(state, "Squash merge", ["merge", "--squash", name]);
        return;
    }
    let head = state.history.head_branch.clone().unwrap_or_default();
    let undo = tip_restore_entry(state, &format!("merge {name}"), ResetMode::Hard);
    let result = state.repo.merge(name, no_fast_forward);
    if matches!(result, Ok(Outcome::Done)) {
        state.push_undo(undo);
    }
    report_sequencer(
        app,
        state,
        "Merge",
        &result,
        &format!("Merged {name} into {head}"),
    );
}

/// Rebuilds the current branch on `base` from `steps` (oldest first), with an undo entry; `false` when it
/// failed (a toast says why).
pub fn rewrite(
    app: &mut Application,
    state: &mut RepoState,
    base: &str,
    steps: &[RebaseStep],
    label: &str,
    success: &str,
) -> bool {
    let undo = tip_restore_entry(state, label, ResetMode::Hard);
    let result = state.repo.rewrite_history(base, steps);
    let ok = result.is_ok();
    match result {
        Err(error) => app.toasts.push_with_detail(
            ToastKind::Error,
            format!("{label} failed"),
            error.to_string(),
        ),
        Ok(()) => {
            state.push_undo(undo);
            app.toasts.push(ToastKind::Success, success);
        }
    }
    state.refresh();
    ok
}

/// Opens the generic confirmation dialog.
pub fn confirm(
    app: &mut Application,
    title: &str,
    message: impl Into<String>,
    confirm_label: &str,
    danger: bool,
    action: impl FnOnce(&mut Cx<'_>, &mut Application) + 'static,
) {
    app.dialog.open(DialogKind::Confirm);
    app.dialog.title = title.to_owned();
    app.dialog.message = message.into();
    app.dialog.confirm_label = confirm_label.to_owned();
    app.dialog.danger = danger;
    app.dialog.on_confirm = Some(Box::new(action));
}

/// The absolute path of `path` (relative to the working directory), as the C++ wrote it: the working
/// directory, a separator, the path.
#[must_use]
pub fn full_path(workdir: &Path, path: &str) -> String {
    let mut full = workdir.to_string_lossy().into_owned();
    if !full.is_empty() && !full.ends_with(['/', '\\']) {
        full.push('/');
    }
    full.push_str(path);
    full
}

/// `git <args>` written to a file the user picks.
pub fn save_git_output(
    cx: &Cx<'_>,
    app: &mut Application,
    workdir: &Path,
    default_name: &str,
    args: &[String],
) {
    let Some(destination) = imgit_platform::shell::save_file_dialog(
        Some(cx.shell.window),
        Some(default_name),
        Some("patch"),
    ) else {
        return;
    };
    let output = cli::run(workdir, args, cli::RunOptions::default());
    if output.exit_code != 0 {
        app.toasts
            .push_with_detail(ToastKind::Error, "Could not create patch", output.text);
        return;
    }
    match std::fs::write(&destination, output.text) {
        Ok(()) => app.toasts.push_with_detail(
            ToastKind::Success,
            "Patch saved",
            destination.to_string_lossy().into_owned(),
        ),
        Err(error) => app.toasts.push_with_detail(
            ToastKind::Error,
            "Could not save the patch",
            error.to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Commit facts the selectors provide.

/// HEAD's first-parent chain down to (excluding) the first merge commit; the root commit is included.
/// Youngest first.
#[must_use]
pub fn head_chain(state: &RepoState) -> Vec<&Commit> {
    let mut chain = Vec::new();
    let mut commit = state
        .history
        .head_sha
        .as_deref()
        .and_then(|sha| state.commit_for(sha));
    while let Some(current) = commit.filter(|commit| commit.parents.len() <= 1) {
        chain.push(current);
        commit = current
            .parents
            .first()
            .and_then(|parent| state.commit_for(parent));
    }
    chain
}

/// Where `sha` is in the chain.
#[must_use]
pub fn chain_index(chain: &[&Commit], sha: &str) -> Option<usize> {
    chain.iter().position(|commit| commit.sha == sha)
}

/// The selected commits, without the working-directory row.
#[must_use]
pub fn selected_commits(state: &RepoState) -> Vec<Sha> {
    state
        .selected
        .iter()
        .filter(|sha| sha.as_str() != WIP_SHA)
        .cloned()
        .collect()
}

/// Selected shas sorted youngest (top row) to oldest.
#[must_use]
pub fn selected_youngest_first(state: &RepoState) -> Vec<Sha> {
    let mut shas = selected_commits(state);
    shas.sort_by_key(|sha| {
        state
            .history
            .row_by_sha
            .get(sha)
            .copied()
            .unwrap_or(usize::MAX)
    });
    shas
}

/// Whether every selected commit is in the chain (with exactly one parent) and they are adjacent in it.
#[must_use]
pub fn selection_contiguous_in_chain(chain: &[&Commit], shas: &[Sha]) -> bool {
    if shas.is_empty() {
        return false;
    }
    let mut indices = Vec::with_capacity(shas.len());
    for sha in shas {
        match chain_index(chain, sha) {
            Some(index) if chain[index].parents.len() == 1 => indices.push(index),
            _ => return false,
        }
    }
    let (Some(&lowest), Some(&highest)) = (indices.iter().min(), indices.iter().max()) else {
        return false;
    };
    highest - lowest + 1 == indices.len()
}

/// `getMergeState(a, b)`: how `b` relates to `a`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeState {
    /// `a` can fast-forward to `b`.
    FastForward,
    /// They diverged.
    Normal,
    /// `a` has everything `b` has.
    UpToDate,
}

#[must_use]
pub fn merge_state(state: &RepoState, a: &str, b: &str) -> MergeState {
    if a == b || state.repo.is_ancestor(b, a) {
        MergeState::UpToDate
    } else if state.repo.is_ancestor(a, b) {
        MergeState::FastForward
    } else {
        MergeState::Normal
    }
}

/// A branch's name without its remote: "main" for "origin/main".
#[must_use]
pub fn branch_group_name(reference: &Ref) -> String {
    if reference.ref_type == RefType::RemoteBranch && reference.name.len() > reference.remote.len()
    {
        return reference.name[reference.remote.len() + 1..].to_owned();
    }
    reference.name.clone()
}

/// The branch's name on its remote ("main" for "origin/main").
#[must_use]
pub fn remote_branch_name(reference: &Ref) -> &str {
    reference
        .name
        .get(reference.remote.len() + 1..)
        .unwrap_or(&reference.name)
}

/// A clicked pill / row: the branches (local first, then remote) sharing one name at one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefGroup {
    pub name: String,
    pub refs: Vec<Ref>,
}

impl RefGroup {
    #[must_use]
    pub fn local(&self) -> Option<&Ref> {
        self.refs
            .iter()
            .find(|reference| reference.ref_type == RefType::LocalBranch)
    }
}

/// The branch groups at `sha`, in the order their first ref is shown.
#[must_use]
pub fn branch_groups_at(state: &RepoState, sha: &str) -> Vec<RefGroup> {
    let mut groups: Vec<RefGroup> = Vec::new();
    for reference in state.refs_at(sha) {
        if !matches!(
            reference.ref_type,
            RefType::LocalBranch | RefType::RemoteBranch
        ) {
            continue;
        }
        let name = branch_group_name(reference);
        let index = groups
            .iter()
            .position(|group| group.name == name)
            .unwrap_or_else(|| {
                groups.push(RefGroup {
                    name,
                    refs: Vec::new(),
                });
                groups.len() - 1
            });
        let group = &mut groups[index];
        if reference.ref_type == RefType::LocalBranch {
            group.refs.insert(0, reference.clone());
        } else {
            group.refs.push(reference.clone());
        }
    }
    groups
}

/// The group `reference` belongs to at its commit.
#[must_use]
pub fn group_for_ref(state: &RepoState, reference: &Ref) -> RefGroup {
    let name = branch_group_name(reference);
    branch_groups_at(state, &reference.target)
        .into_iter()
        .find(|group| group.name == name)
        .unwrap_or_else(|| RefGroup {
            name,
            refs: vec![reference.clone()],
        })
}

// ---------------------------------------------------------------------------------------------------------
// Item builders.

/// "Checkout this commit" (in a submenu: "this commit").
pub fn checkout_commit_item(workdir: &Path, sha: &str, in_submenu: bool) -> MenuItem {
    let sha = sha.to_owned();
    repo_item(
        if in_submenu {
            "this commit"
        } else {
            "Checkout this commit"
        },
        workdir,
        move |_cx, app, state| {
            let undo = {
                let redo_sha = sha.clone();
                head_restore_entry(
                    state,
                    &format!("checkout {}", short_sha(&sha)),
                    move |repo| repo.checkout_commit(&redo_sha),
                )
            };
            let result = state.repo.checkout_commit(&sha);
            let message = format!("Checked out {} (detached HEAD)", short_sha(&sha));
            if app.run(state, "Checkout", result, Some(&message)).is_some() {
                state.push_undo(undo);
            }
        },
    )
}

/// The Create Worktree dialog for a branch (`None`: the commit `sha`, detached).
pub fn worktree_item(label: impl Into<String>, reference: Option<&Ref>, sha: &str) -> MenuItem {
    let mut branch = String::new();
    let mut remote = String::new();
    if let Some(reference) = reference {
        branch.clone_from(&reference.name);
        if reference.ref_type == RefType::RemoteBranch {
            remote.clone_from(&reference.remote);
            branch = remote_branch_name(reference).to_owned();
        }
    }
    let sha = sha.to_owned();
    menu_item(label, move |_cx, app| {
        app.dialog.open(DialogKind::CreateWorktree);
        app.dialog.ref_name = branch;
        app.dialog.ref_remote = remote;
        app.dialog.sha = sha;
    })
}

pub fn worktree_commit_item(sha: &str, in_submenu: bool) -> MenuItem {
    worktree_item(
        if in_submenu {
            "this commit"
        } else {
            "Create worktree from this commit"
        },
        None,
        sha,
    )
}

pub fn create_branch_item(sha: &str) -> MenuItem {
    let sha = sha.to_owned();
    menu_item("Create branch here", move |_cx, app| {
        app.dialog.open(DialogKind::CreateBranch);
        app.dialog.sha = sha;
        app.dialog.flag = true;
    })
}

pub fn cherry_pick_item(workdir: &Path, sha: &str) -> MenuItem {
    let sha = sha.to_owned();
    repo_item("Cherry pick commit", workdir, move |_cx, app, state| {
        let undo = tip_restore_entry(
            state,
            &format!("cherry pick {}", short_sha(&sha)),
            ResetMode::Hard,
        );
        let result = state.repo.cherry_pick(&sha);
        if matches!(result, Ok(Outcome::Done)) {
            state.push_undo(undo);
        }
        report_sequencer(
            app,
            state,
            "Cherry pick",
            &result,
            &format!("Cherry picked {}", short_sha(&sha)),
        );
    })
}

/// "Cherry pick N commits" for the selection, `None` when a selected commit is a merge or a stash.
pub fn cherry_picks_item(state: &RepoState) -> Option<MenuItem> {
    let shas = selected_youngest_first(state);
    for sha in &shas {
        let commit = state.commit_for(sha)?;
        if commit.parents.len() > 1 || commit.node_type == imgit_git::NodeType::Stash {
            return None;
        }
    }
    let count = shas.len();
    Some(repo_item(
        format!("Cherry pick {count} commits"),
        state.repo.workdir(),
        move |_cx, app, state| {
            let undo = tip_restore_entry(state, "cherry pick", ResetMode::Hard);
            // Oldest first.
            for sha in shas.iter().rev() {
                let result = state.repo.cherry_pick(sha);
                if !matches!(result, Ok(Outcome::Done)) {
                    report_sequencer(app, state, "Cherry pick", &result, "");
                    return;
                }
            }
            state.push_undo(undo);
            app.toasts
                .push(ToastKind::Success, format!("Cherry picked {count} commits"));
            state.refresh();
        },
    ))
}

/// "Reset <branch> to this commit" with its three modes, which open the Reset dialog.
pub fn reset_items(state: &RepoState, sha: &str) -> Vec<MenuItem> {
    if state.history.head_sha.is_none() {
        return Vec::new();
    }
    let head = if state.history.detached {
        "HEAD".to_owned()
    } else {
        state.history.head_branch.clone().unwrap_or_default()
    };
    let reset = |label: &str, mode: ResetMode| {
        let (sha, head) = (sha.to_owned(), head.clone());
        menu_item(label, move |_cx, app| {
            app.dialog.open(DialogKind::Reset);
            app.dialog.sha = sha;
            app.dialog.ref_name = head;
            app.dialog.reset_mode = mode;
        })
    };
    vec![crate::ui::menu::menu_submenu(
        format!("Reset {head} to this commit"),
        vec![
            reset("Soft - keep all changes", ResetMode::Soft),
            reset(
                "Mixed - keep working copy but reset index",
                ResetMode::Mixed,
            ),
            reset("Hard - discard all changes", ResetMode::Hard),
        ],
    )]
}

pub fn revert_item(workdir: &Path, sha: &str) -> MenuItem {
    let sha = sha.to_owned();
    repo_item("Revert commit", workdir, move |_cx, app, state| {
        let undo = tip_restore_entry(
            state,
            &format!("revert {}", short_sha(&sha)),
            ResetMode::Hard,
        );
        let result = state.repo.revert(&sha);
        if matches!(result, Ok(Outcome::Done)) {
            state.push_undo(undo);
        }
        report_sequencer(
            app,
            state,
            "Revert",
            &result,
            &format!("Reverted {}", short_sha(&sha)),
        );
    })
}

/// "Edit commit message": opens the Reword dialog with the commit's message.
pub fn reword_item(workdir: &Path, sha: &str) -> MenuItem {
    let sha = sha.to_owned();
    repo_item("Edit commit message", workdir, move |_cx, app, state| {
        let Some(commit) = state.commit_for(&sha) else {
            return;
        };
        let message = if commit.body.is_empty() {
            commit.summary.clone()
        } else {
            format!("{}\n\n{}", commit.summary, commit.body)
        };
        app.dialog.open(DialogKind::RewordCommit);
        app.dialog.sha = sha;
        app.dialog.text_a = message;
    })
}

pub fn copy_sha_item(sha: &str) -> MenuItem {
    let sha = sha.to_owned();
    menu_item("Copy commit sha", move |cx, app| {
        app.copy_to_clipboard(cx, &sha);
    })
}

/// "Hide" checkbox of a ref.
pub fn toggle_hide_item(
    workdir: &Path,
    state: &RepoState,
    full_name: &str,
    enabled: bool,
) -> MenuItem {
    let checked = state.hidden_refs.contains(full_name);
    let full_name = full_name.to_owned();
    repo_item("Hide", workdir, move |_cx, _app, state| {
        if !state.hidden_refs.remove(&full_name) {
            state.hidden_refs.insert(full_name);
        }
        state.solo_ref = None;
        state.refresh();
    })
    .checkbox(checked)
    .enabled(enabled)
}

/// "Solo" checkbox of a ref (`label` replaces the text).
pub fn toggle_solo_item(
    workdir: &Path,
    state: &RepoState,
    full_name: &str,
    label: &str,
) -> MenuItem {
    let checked = state.solo_ref.as_deref() == Some(full_name);
    let full_name = full_name.to_owned();
    let item = repo_item(label, workdir, move |_cx, _app, state| {
        state.solo_ref = if state.solo_ref.as_deref() == Some(full_name.as_str()) {
            None
        } else {
            Some(full_name)
        };
        state.refresh();
    });
    item.checkbox(checked)
}

/// "Pin to Left" / "Unpin from Left", or with `short_label` the ref's own name (in a submenu).
pub fn pin_item(workdir: &Path, state: &RepoState, full_name: &str, short_label: bool) -> MenuItem {
    let pinned = state.pinned_branch.as_deref() == Some(full_name);
    let label = if short_label {
        full_name
            .get(5..)
            .and_then(|rest| rest.find('/').map(|slash| &full_name[5 + slash + 1..]))
            .unwrap_or(full_name)
            .to_owned()
    } else if pinned {
        "Unpin from Left".to_owned()
    } else {
        "Pin to Left".to_owned()
    };
    let full_name = full_name.to_owned();
    repo_item(label, workdir, move |_cx, _app, state| {
        state.pinned_branch = if pinned { None } else { Some(full_name) };
        state.refresh();
    })
}

/// "Compare commit against working directory", offered while there are changes.
pub fn compare_items(workdir: &Path, state: &RepoState, sha: &str) -> Vec<MenuItem> {
    if state.status.is_empty() {
        return Vec::new();
    }
    let sha = sha.to_owned();
    vec![repo_item(
        "Compare commit against working directory",
        workdir,
        move |_cx, _app, state| {
            // The detail panel shows the comparison while exactly these two rows are selected.
            state.select_only(WIP_SHA);
            state.select(&sha, true, false);
            state.compare_sha = None;
        },
    )]
}

/// A pick of the steps for an "Edit commit message" (as `RebaseAction::Reword`) of one commit.
#[must_use]
pub fn pick_steps_with_reword(commits: &[&Commit], sha: &str, message: &str) -> Vec<RebaseStep> {
    commits
        .iter()
        .map(|commit| RebaseStep {
            sha: commit.sha.clone(),
            action: if commit.sha == sha {
                RebaseAction::Reword(Some(message.to_owned()))
            } else {
                RebaseAction::Pick
            },
        })
        .collect()
}

/// The steps (oldest first) that drop `affected` from `steps`, or move that block one place younger
/// (`direction` > 0) or older (< 0).
#[must_use]
pub fn arranged_steps(
    mut steps: Vec<RebaseStep>,
    affected: &[Sha],
    direction: i32,
) -> Vec<RebaseStep> {
    let is_affected = |step: &RebaseStep| affected.contains(&step.sha);
    if direction == 0 {
        for step in &mut steps {
            if is_affected(step) {
                step.action = RebaseAction::Drop;
            }
        }
        return steps;
    }
    // Steps are oldest first: "up" moves the block one step later (younger), "down" earlier.
    let mut block = Vec::new();
    let mut rest = Vec::new();
    let mut first = None;
    for step in steps {
        if is_affected(&step) {
            first.get_or_insert(rest.len());
            block.push(step);
        } else {
            rest.push(step);
        }
    }
    let first = first.unwrap_or(0);
    let insert = if direction > 0 {
        (first + 1).min(rest.len())
    } else {
        first.saturating_sub(1)
    };
    rest.splice(insert..insert, block);
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, parents: &[&str]) -> Commit {
        Commit {
            sha: sha.to_owned(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            ..Commit::default()
        }
    }

    fn pick(sha: &str) -> RebaseStep {
        RebaseStep {
            sha: sha.to_owned(),
            action: RebaseAction::Pick,
        }
    }

    fn names(steps: &[RebaseStep]) -> Vec<(&str, bool)> {
        steps
            .iter()
            .map(|step| (step.sha.as_str(), step.action == RebaseAction::Drop))
            .collect()
    }

    #[test]
    fn a_selection_is_contiguous_when_adjacent_in_the_chain() {
        let (a, b, c, d) = (
            commit("a", &["b"]),
            commit("b", &["c"]),
            commit("c", &["d"]),
            commit("d", &["e"]),
        );
        let chain = vec![&a, &b, &c, &d];
        let shas = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        assert!(selection_contiguous_in_chain(&chain, &shas(&["b", "c"])));
        assert!(selection_contiguous_in_chain(
            &chain,
            &shas(&["c", "a", "b"])
        ));
        assert!(!selection_contiguous_in_chain(&chain, &shas(&["a", "c"])));
        assert!(!selection_contiguous_in_chain(&chain, &shas(&["a", "zzz"])));
        assert!(!selection_contiguous_in_chain(&chain, &[]));
        // A root commit (no parent) cannot be moved or dropped.
        let root = commit("r", &[]);
        assert!(!selection_contiguous_in_chain(&[&root], &shas(&["r"])));
    }

    #[test]
    fn dropping_marks_the_affected_steps() {
        let steps = vec![pick("c"), pick("b"), pick("a")];
        let dropped = arranged_steps(steps, &["b".to_owned()], 0);
        assert_eq!(names(&dropped), [("c", false), ("b", true), ("a", false)]);
    }

    #[test]
    fn a_block_moves_one_place_and_stops_at_the_ends() {
        let ids = |steps: &[RebaseStep]| steps.iter().map(|s| s.sha.clone()).collect::<Vec<_>>();
        let steps = || vec![pick("d"), pick("c"), pick("b"), pick("a")];
        let block = ["c".to_owned(), "b".to_owned()];
        // Younger: after the commit that follows the block.
        assert_eq!(
            ids(&arranged_steps(steps(), &block, 1)),
            ["d", "a", "c", "b"]
        );
        // Older: before the commit that precedes it.
        assert_eq!(
            ids(&arranged_steps(steps(), &block, -1)),
            ["c", "b", "d", "a"]
        );
        // Already at the oldest end.
        let oldest = ["d".to_owned()];
        assert_eq!(
            ids(&arranged_steps(steps(), &oldest, -1)),
            ["d", "c", "b", "a"]
        );
        let youngest = ["a".to_owned()];
        assert_eq!(
            ids(&arranged_steps(steps(), &youngest, 1)),
            ["d", "c", "b", "a"]
        );
    }

    #[test]
    fn pin_labels_drop_the_ref_namespace() {
        let scratch = tempfile::tempdir().unwrap();
        let state = RepoState::new(Repository::init(scratch.path()).unwrap());
        let label = |full: &str| {
            let item = pin_item(Path::new("."), &state, full, true);
            item.label
        };
        assert_eq!(label("refs/heads/main"), "main");
        assert_eq!(label("refs/heads/feature/x"), "feature/x");
        assert_eq!(label("refs/remotes/origin/main"), "origin/main");
    }

    #[test]
    fn full_paths_get_one_separator() {
        assert_eq!(full_path(Path::new("C:/repo/"), "a/b"), "C:/repo/a/b");
        assert_eq!(full_path(Path::new("/repo"), "a"), "/repo/a");
    }

    #[test]
    fn remote_group_names_lose_their_remote() {
        let remote = Ref {
            ref_type: RefType::RemoteBranch,
            name: "origin/feature/x".to_owned(),
            remote: "origin".to_owned(),
            ..Ref::default()
        };
        assert_eq!(branch_group_name(&remote), "feature/x");
        assert_eq!(remote_branch_name(&remote), "feature/x");
        let local = Ref {
            name: "main".to_owned(),
            ..Ref::default()
        };
        assert_eq!(branch_group_name(&local), "main");
    }
}
