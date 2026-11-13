//! What the menus share: the repository-bound click, the undo entries, the sequencer reports, the commit chain
//! the history rewrites work on, the branch groups and the item builders more than one menu offers.

use std::path::Path;

use imgit_app::repo_state::{AUTO_STASH_BEFORE_UNDO, with_changes_stashed};
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

/// A row whose click changes the repository at `workdir` (found again when it runs: the tab may have been
/// closed or its state be out meanwhile, then nothing happens). While a git command is in flight on the
/// repository the click only toasts "Wait for <title> to finish" ([`Application::with_idle_repo`]): this is where
/// every menu action that mutates it gets its busy guard. Rows that only look (hide, solo, pin, compare, open a
/// dialog or a view...) are [`repo_view_item`]s.
pub fn repo_item(
    label: impl Into<String>,
    workdir: &Path,
    run: impl FnOnce(&mut Cx<'_>, &mut Application, &mut RepoState) + 'static,
) -> MenuItem {
    let workdir = workdir.to_path_buf();
    menu_item(label, move |cx, app| {
        app.with_idle_repo(&workdir, |app, state| run(cx, app, state));
    })
}

/// [`repo_item`] for a row that does not change the repository: it stays available while a git command runs.
pub fn repo_view_item(
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

// ---------------------------------------------------------------------------------------------------------
// Command lines with names the user typed.
//
// A branch, tag or remote name may start with '-' (libgit2 accepts it, and it can come from elsewhere), and git
// would read such a name as an option. Every command line that takes one builds its arguments here, with `--`
// before the positional names where git's syntax allows it (checked against git's documentation: `rebase`,
// `merge`, `branch`, `push`, `fetch`, `worktree add`, `tag`).

/// `git rebase -- <onto>`.
#[must_use]
pub fn rebase_args(onto: &str) -> Vec<String> {
    vec!["rebase".to_owned(), "--".to_owned(), onto.to_owned()]
}

/// `git merge --squash -- <name>`.
#[must_use]
pub fn squash_merge_args(name: &str) -> Vec<String> {
    ["merge", "--squash", "--", name]
        .map(str::to_owned)
        .to_vec()
}

/// `git branch -f -- <name> <target>`: moves the branch `name` to `target`.
#[must_use]
pub fn force_branch_args(name: &str, target: &str) -> Vec<String> {
    ["branch", "-f", "--", name, target]
        .map(str::to_owned)
        .to_vec()
}

/// `git push -- <remote> <refspec>`.
#[must_use]
pub fn push_args(remote: &str, refspec: &str) -> Vec<String> {
    ["push", "--", remote, refspec].map(str::to_owned).to_vec()
}

/// `git push --delete -- <remote> <name>`: deletes the branch `name` on the remote.
#[must_use]
pub fn push_delete_args(remote: &str, name: &str) -> Vec<String> {
    ["push", "--delete", "--", remote, name]
        .map(str::to_owned)
        .to_vec()
}

/// `git fetch -- <remote> <refspec>`.
#[must_use]
pub fn fetch_refspec_args(remote: &str, refspec: &str) -> Vec<String> {
    ["fetch", "--", remote, refspec].map(str::to_owned).to_vec()
}

/// `git fetch -- <remote>`.
#[must_use]
pub fn fetch_args(remote: &str) -> Vec<String> {
    ["fetch", "--", remote].map(str::to_owned).to_vec()
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
        with_changes_stashed(
            repo,
            AUTO_STASH_BEFORE_UNDO,
            mode == ResetMode::Hard,
            |repo| repo.reset(sha, mode),
        )
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
        app.run_git_cli(state, "Squash merge", squash_merge_args(name));
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
            // Oldest first.
            let oldest_first: Vec<Sha> = shas.iter().rev().cloned().collect();
            cherry_pick_all(app, state, &oldest_first);
        },
    ))
}

/// Cherry picks `shas` (oldest first) one after another. When commit N of M fails or stops on conflicts, the N-1
/// before it stay applied, so the undo entry for them is pushed before the failure is reported: undo goes back
/// to the tip the first pick started from.
pub fn cherry_pick_all(app: &mut Application, state: &mut RepoState, shas: &[Sha]) {
    let count = shas.len();
    let mut undo = tip_restore_entry(state, "cherry pick", ResetMode::Hard);
    for (applied, sha) in shas.iter().enumerate() {
        let result = state.repo.cherry_pick(sha);
        if !matches!(result, Ok(Outcome::Done)) {
            if applied > 0 {
                undo.label = format!("cherry pick {applied} commits");
                state.push_undo(undo);
            }
            report_sequencer(app, state, "Cherry pick", &result, "");
            if applied > 0 {
                app.toasts.push(
                    ToastKind::Info,
                    format!(
                        "{applied} of {count} commits were cherry picked; undo takes them back"
                    ),
                );
            }
            return;
        }
    }
    state.push_undo(undo);
    app.toasts
        .push(ToastKind::Success, format!("Cherry picked {count} commits"));
    state.refresh();
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
    repo_view_item("Edit commit message", workdir, move |_cx, app, state| {
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
    repo_view_item("Hide", workdir, move |_cx, _app, state| {
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
    let item = repo_view_item(label, workdir, move |_cx, _app, state| {
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
    repo_view_item(label, workdir, move |_cx, _app, state| {
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
    vec![repo_view_item(
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

    fn git(dir: &Path, args: &[&str]) -> (i32, String) {
        let mut full = vec!["-c", "user.name=Test", "-c", "user.email=test@example.com"];
        full.extend(args);
        let output = cli::run(dir, full, cli::RunOptions::default());
        (output.exit_code, output.text.trim().to_owned())
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let (code, text) = git(dir, args);
        assert_eq!(code, 0, "git {args:?}: {text}");
        text
    }

    fn run_args(dir: &Path, args: &[String]) -> (i32, String) {
        let output = cli::run(
            dir,
            ["-c", "user.name=Test", "-c", "user.email=test@example.com"]
                .into_iter()
                .chain(args.iter().map(String::as_str)),
            cli::RunOptions::default(),
        );
        (output.exit_code, output.text)
    }

    fn commit_file(dir: &Path, name: &str, text: &str, message: &str) {
        std::fs::write(dir.join(name), text).unwrap();
        git_ok(dir, &["add", "-A"]);
        git_ok(dir, &["commit", "-m", message]);
    }

    /// A repository with `base` on `main`, in the folder `repo` of a scratch folder.
    fn repository() -> (tempfile::TempDir, std::path::PathBuf) {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("repo");
        std::fs::create_dir_all(&folder).unwrap();
        git_ok(&folder, &["init", "-b", "main"]);
        git_ok(&folder, &["config", "core.autocrlf", "false"]);
        commit_file(&folder, "a.txt", "base\n", "base");
        (scratch, folder)
    }

    /// Whether git took `output` for a failure to parse the command line, not for what the command did.
    fn is_usage_error(output: &str) -> bool {
        output.contains("usage:")
            || output.contains("unknown switch")
            || output.contains("unknown option")
    }

    #[test]
    fn branch_names_that_start_with_a_dash_are_not_options() {
        let (_scratch, dir) = repository();
        // libgit2 and update-ref accept such a name; git's own commands refuse to make one.
        git_ok(&dir, &["update-ref", "refs/heads/-x", "HEAD"]);
        git_ok(&dir, &["checkout", "-b", "work"]);
        commit_file(&dir, "w.txt", "w\n", "work");
        git_ok(&dir, &["checkout", "main"]);
        commit_file(&dir, "m.txt", "m\n", "main");
        git_ok(&dir, &["checkout", "work"]);

        let (code, text) = run_args(&dir, &rebase_args("-x"));
        assert_eq!(code, 0, "{text}");
        assert!(!is_usage_error(&text), "{text}");
        // The history is what `git rebase -x`'s base gives: work on top of the base commit only.
        let (code, text) = run_args(&dir, &squash_merge_args("-x"));
        assert_eq!(code, 0, "{text}");
        assert!(!is_usage_error(&text), "{text}");
        // `branch -f` refuses the name itself, which proves it was read as one.
        let (code, text) = run_args(&dir, &force_branch_args("-x", "HEAD"));
        assert_ne!(code, 0);
        assert!(!is_usage_error(&text), "{text}");
        assert!(text.contains("not a valid branch name"), "{text}");
        // The remote operations: a remote named like an option is a remote that is not there.
        let (code, text) = run_args(&dir, &push_args("-x", "work:work"));
        assert_ne!(code, 0);
        assert!(!is_usage_error(&text), "{text}");
        let (code, text) = run_args(&dir, &push_delete_args("-x", "-y"));
        assert_ne!(code, 0);
        assert!(!is_usage_error(&text), "{text}");
        let (code, text) = run_args(&dir, &fetch_args("-x"));
        assert_ne!(code, 0);
        assert!(!is_usage_error(&text), "{text}");
        let (code, text) = run_args(&dir, &fetch_refspec_args("-x", "a:b"));
        assert_ne!(code, 0);
        assert!(!is_usage_error(&text), "{text}");
    }

    #[test]
    fn the_command_lines_keep_their_order() {
        assert_eq!(rebase_args("main"), ["rebase", "--", "main"]);
        assert_eq!(
            squash_merge_args("topic"),
            ["merge", "--squash", "--", "topic"]
        );
        assert_eq!(
            force_branch_args("topic", "abc"),
            ["branch", "-f", "--", "topic", "abc"]
        );
        assert_eq!(push_args("origin", "a:b"), ["push", "--", "origin", "a:b"]);
        assert_eq!(
            push_delete_args("origin", "topic"),
            ["push", "--delete", "--", "origin", "topic"]
        );
        assert_eq!(fetch_args("origin"), ["fetch", "--", "origin"]);
        assert_eq!(
            fetch_refspec_args("origin", "a:b"),
            ["fetch", "--", "origin", "a:b"]
        );
    }

    #[test]
    fn the_command_lines_do_what_they_did() {
        // `--` changes nothing for a normal name: the commands still push, delete and rebase.
        let (scratch, dir) = repository();
        let remote = scratch.path().join("remote.git");
        git_ok(
            scratch.path(),
            &["init", "--bare", "-b", "main", "remote.git"],
        );
        git_ok(
            &dir,
            &["remote", "add", "origin", &remote.to_string_lossy()],
        );
        let ok = |args: Vec<String>| {
            let (code, text) = run_args(&dir, &args);
            assert_eq!(code, 0, "{args:?}: {text}");
        };
        ok(push_args("origin", "main:main"));
        ok(push_args("origin", "main:topic"));
        assert!(git_ok(&remote, &["branch", "--list", "topic"]).contains("topic"));
        ok(push_delete_args("origin", "topic"));
        assert_eq!(git_ok(&remote, &["branch", "--list", "topic"]), "");
        ok(fetch_args("origin"));
        ok(fetch_refspec_args("origin", "main:refs/heads/copy"));
        assert_eq!(
            git_ok(&dir, &["rev-parse", "copy"]),
            git_ok(&dir, &["rev-parse", "main"])
        );
        git_ok(&dir, &["checkout", "-b", "work"]);
        commit_file(&dir, "w.txt", "w\n", "work");
        ok(rebase_args("main"));
        ok(force_branch_args("copy", "work"));
        assert_eq!(
            git_ok(&dir, &["rev-parse", "copy"]),
            git_ok(&dir, &["rev-parse", "work"])
        );
        git_ok(&dir, &["checkout", "main"]);
        ok(squash_merge_args("work"));
        assert!(dir.join("w.txt").exists());
    }

    #[test]
    fn a_cherry_pick_that_stops_midway_can_still_be_undone() {
        let (scratch, dir) = repository();
        git_ok(&dir, &["checkout", "-b", "side"]);
        commit_file(&dir, "b.txt", "b\n", "adds b");
        commit_file(&dir, "a.txt", "side\n", "changes a");
        git_ok(&dir, &["checkout", "main"]);
        commit_file(&dir, "a.txt", "main\n", "main changes a");
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.open_repository(&dir.to_string_lossy());
        app.with_active(|app, state| {
            let tip = state.history.head_sha.clone().unwrap();
            let pick = |summary: &str| {
                state
                    .history
                    .commits
                    .iter()
                    .find(|commit| commit.summary == summary)
                    .unwrap()
                    .sha
                    .clone()
            };
            // Commit 2 of 2 conflicts with "main changes a".
            let shas = [pick("adds b"), pick("changes a")];
            cherry_pick_all(app, state, &shas);
            // The first was applied: HEAD moved, and there is something to undo.
            assert_ne!(state.history.head_sha.as_deref(), Some(tip.as_str()));
            assert!(dir.join("b.txt").exists());
            assert_eq!(state.undo_stack.len(), 1);
            let titles: Vec<&str> = app
                .toasts
                .history()
                .iter()
                .map(|toast| toast.title.as_str())
                .collect();
            assert!(
                titles.contains(&"Cherry pick stopped on conflicts"),
                "{titles:?}"
            );
            assert!(
                titles.iter().any(|t| t.starts_with("1 of 2 commits")),
                "{titles:?}"
            );
            app.undo(state);
            assert_eq!(state.history.head_sha.as_deref(), Some(tip.as_str()));
            assert!(!dir.join("b.txt").exists());
            assert_eq!(
                std::fs::read_to_string(dir.join("a.txt")).unwrap(),
                "main\n"
            );
        });
    }

    #[test]
    fn a_cherry_pick_that_fails_at_once_has_nothing_to_undo() {
        let (scratch, dir) = repository();
        git_ok(&dir, &["checkout", "-b", "side"]);
        commit_file(&dir, "a.txt", "side\n", "changes a");
        git_ok(&dir, &["checkout", "main"]);
        commit_file(&dir, "a.txt", "main\n", "main changes a");
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.open_repository(&dir.to_string_lossy());
        app.with_active(|app, state| {
            let sha = state
                .history
                .commits
                .iter()
                .find(|commit| commit.summary == "changes a")
                .unwrap()
                .sha
                .clone();
            cherry_pick_all(app, state, &[sha]);
            assert!(state.undo_stack.is_empty());
        });
    }

    #[test]
    fn menu_clicks_that_change_the_repository_wait_for_a_running_command() {
        use imgit_app::repo_state::AsyncOperation;
        let (scratch, dir) = repository();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.open_repository(&dir.to_string_lossy());
        app.with_active(|_, state| {
            state.async_op = Some(AsyncOperation {
                title: "Rebase".to_owned(),
                job: imgit_app::job::Job::idle(),
                silent: false,
                args: Vec::new(),
                attempt: None,
                queued: Vec::new(),
            });
        });
        // `repo_item` clicks run through `with_idle_repo`, `repo_view_item` ones through `with_repo`. They name the
        // repository by the folder libgit2 reports, which is not `dir` itself where the temporary folder sits
        // behind a symbolic link (macOS's /var -> /private/var).
        let workdir = app.active_workdir().unwrap().to_path_buf();
        let mut changed = false;
        app.with_idle_repo(&workdir, |_, _| changed = true);
        assert!(!changed);
        assert_eq!(app.toasts.history()[0].title, "Wait for Rebase to finish");
        let mut looked = false;
        app.with_repo(&workdir, |_, _| looked = true);
        assert!(looked);
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
