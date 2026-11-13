//! The menus of commits and refs: the commit row, a branch pill / left panel row, a tag, a stash, and the menu
//! of a ref dropped on another.

use std::path::Path;

use imgit_app::RepoState;
use imgit_app::repo_state::UndoEntry;
use imgit_git::repository::short_sha;
use imgit_git::{NodeType, RebaseAction, RebaseStep, Ref, RefType, ResetMode, Sha};

use super::shared::{
    MergeState, OperationState, RefGroup, arranged_steps, branch_group_name, branch_groups_at,
    chain_index, checkout_commit_item, cherry_pick_item, cherry_picks_item, compare_items,
    copy_sha_item, create_branch_item, group_for_ref, head_chain, head_restore_entry,
    merge_into_head, merge_state, pick_steps_with_reword, pin_item, remote_branch_name, repo_item,
    reset_items, revert_item, reword_item, rewrite, selected_commits, selected_youngest_first,
    selection_contiguous_in_chain, toggle_hide_item, toggle_solo_item, workdir_of,
    worktree_commit_item, worktree_item,
};
use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::ui::Cx;
use crate::ui::menu::{MenuItem, MenuItems, menu_item, menu_submenu, separated_menu};
use crate::ui::panels::workflows::gitflow_finish_items;

/// `git` arguments from literals and names.
pub fn git_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

// ---------------------------------------------------------------------------------------------------------
// Rebase entries

/// getInteractiveRebaseShortcuts + the interactive rebase entries (the "gs" group).
fn rebase_group(state: &RepoState, sha: &str, multi: bool) -> MenuItems {
    let mut items = MenuItems::new();
    if state.history.head_sha.is_none() || state.repo.state() == OperationState::Rebase {
        return items;
    }
    let workdir = workdir_of(state);
    let chain = head_chain(state);
    let Some(index) = chain_index(&chain, sha) else {
        return items;
    };
    let commit = chain[index];
    let selected = selected_youngest_first(state);
    let sha_selected = selected.iter().any(|selected| selected == sha);
    let contiguous = selection_contiguous_in_chain(&chain, &selected);
    if index > 0 {
        let short: String = sha.chars().take(6).collect();
        let owned = sha.to_owned();
        items.push(repo_item(
            format!("Interactive Rebase {index} children of {short}"),
            &workdir,
            move |_cx, _app, state| {
                state.start_rebase_plan(&owned, short_sha(&owned), None, None);
            },
        ));
    }

    let selection_on_chain = multi && sha_selected && contiguous;
    let movable =
        |i: Option<usize>| i.is_some_and(|i| chain.get(i).is_some_and(|c| c.parents.len() == 1));
    // Squash: a contiguous multi-selection on the chain.
    if selection_on_chain {
        let selected = selected.clone();
        items.push(repo_item(
            format!("Squash {} commits", selected.len()),
            &workdir,
            move |_cx, app, state| {
                let oldest_first: Vec<Sha> = selected.iter().rev().cloned().collect();
                let mut message = String::new();
                for sha in &oldest_first {
                    if let Some(commit) = state.commit_for(sha) {
                        if !message.is_empty() {
                            message.push_str("\n\n");
                        }
                        message.push_str(&commit.summary);
                        if !commit.body.is_empty() {
                            message.push_str("\n\n");
                            message.push_str(&commit.body);
                        }
                    }
                }
                app.dialog.open(DialogKind::SquashCommits);
                app.dialog.shas = oldest_first;
                app.dialog.text_a = message;
            },
        ));
    }
    if !multi && index > 0 && commit.parents.len() == 1 {
        items.push(reword_item(&workdir, sha));
    }

    let n = selected.len();
    if selection_on_chain {
        items.push(rewrite_item(
            format!("Drop {n} commits"),
            &workdir,
            selected.clone(),
            0,
            "drop commits",
            format!("Dropped {n} commits"),
        ));
    } else if !multi && movable(Some(index)) {
        items.push(rewrite_item(
            "Drop commit".to_owned(),
            &workdir,
            vec![sha.to_owned()],
            0,
            "drop commit",
            format!("Dropped {}", short_sha(sha)),
        ));
    }
    let (mut top, mut bottom) = (index, index);
    if selection_on_chain {
        top = selected
            .first()
            .and_then(|s| chain_index(&chain, s))
            .unwrap_or(index);
        bottom = selected
            .last()
            .and_then(|s| chain_index(&chain, s))
            .unwrap_or(index);
    }
    let can_up = top > 0 && movable(Some(top));
    let can_down =
        movable(Some(bottom)) && movable((bottom + 1 < chain.len()).then_some(bottom + 1));
    if selection_on_chain && can_up {
        items.push(rewrite_item(
            format!("Move {n} commits up"),
            &workdir,
            selected.clone(),
            1,
            "move commits",
            format!("Moved {n} commits up"),
        ));
    } else if !multi && can_up {
        items.push(rewrite_item(
            "Move commit up".to_owned(),
            &workdir,
            vec![sha.to_owned()],
            1,
            "move commit",
            "Moved commit up".to_owned(),
        ));
    }
    if selection_on_chain && can_down {
        items.push(rewrite_item(
            format!("Move {n} commits down"),
            &workdir,
            selected,
            -1,
            "move commits",
            format!("Moved {n} commits down"),
        ));
    } else if !multi && can_down {
        items.push(rewrite_item(
            "Move commit down".to_owned(),
            &workdir,
            vec![sha.to_owned()],
            -1,
            "move commit",
            "Moved commit down".to_owned(),
        ));
    }
    items
}

/// Drop / move: rebuilds the chain above the parent of the lowest affected commit.
fn rewrite_item(
    menu_label: String,
    workdir: &Path,
    affected: Vec<Sha>,
    direction: i32,
    label: &'static str,
    success: String,
) -> MenuItem {
    repo_item(menu_label, workdir, move |_cx, app, state| {
        let Some((base, steps)) = rewrite_plan(state, &affected, direction) else {
            return;
        };
        rewrite(app, state, &base, &steps, label, &success);
    })
}

/// The base and the steps (oldest first) for dropping / moving `affected`; `None` when they are not on the
/// chain or there is no commit to rebuild onto.
fn rewrite_plan(
    state: &RepoState,
    affected: &[Sha],
    direction: i32,
) -> Option<(Sha, Vec<RebaseStep>)> {
    let chain = head_chain(state);
    let oldest = affected
        .iter()
        .filter_map(|sha| chain_index(&chain, sha))
        .max()?;
    // Moving down also rewrites the commit below the block.
    let start = oldest + usize::from(direction < 0);
    let start_commit = chain.get(start)?;
    if start_commit.parents.len() != 1 {
        return None;
    }
    let base = start_commit.parents[0].clone();
    let steps: Vec<RebaseStep> = chain[..=start]
        .iter()
        .rev()
        .map(|commit| RebaseStep {
            sha: commit.sha.clone(),
            action: RebaseAction::Pick,
        })
        .collect();
    Some((base, arranged_steps(steps, affected, direction)))
}

fn rebase_onto_commit_items(state: &RepoState, sha: &str) -> MenuItems {
    let Some(head) = state
        .history
        .head_branch
        .as_deref()
        .filter(|head| !head.is_empty())
    else {
        return Vec::new();
    };
    if state.history.detached
        || state.history.head_sha.as_deref() == Some(sha)
        || state.repo.state() == OperationState::Rebase
    {
        return Vec::new();
    }
    let sha = sha.to_owned();
    vec![repo_item(
        format!("Rebase {head} onto this commit"),
        state.repo.workdir(),
        move |_cx, app, state| app.run_git_cli(state, "Rebase", ["rebase", sha.as_str()]),
    )]
}

// ---------------------------------------------------------------------------------------------------------
// Merge / rebase between HEAD and a ref

fn placeholder(label: String, what: &'static str, why: &'static str) -> MenuItem {
    menu_item(label, move |_cx, app| {
        app.toasts.push_with_detail(ToastKind::Info, what, why);
    })
}

fn merge_item(label: String, workdir: &Path, name: String, no_fast_forward: bool) -> MenuItem {
    repo_item(label, workdir, move |_cx, app, state| {
        merge_into_head(app, state, &name, no_fast_forward);
    })
}

/// Fast-forwards their branch to HEAD (it is behind), with an undo entry.
fn fast_forward_item(label: String, workdir: &Path, theirs: &Ref, head_sha: &str) -> MenuItem {
    let (name, sha, head_sha) = (
        theirs.name.clone(),
        theirs.target.clone(),
        head_sha.to_owned(),
    );
    repo_item(label, workdir, move |_cx, app, state| {
        let undo = {
            let (undo_name, undo_sha) = (name.clone(), sha);
            let (redo_name, redo_sha) = (name.clone(), head_sha.clone());
            UndoEntry::new(format!("fast-forward {name}"), move |repo| {
                repo.set_branch_target(&undo_name, &undo_sha)
            })
            .with_redo(move |repo| repo.set_branch_target(&redo_name, &redo_sha))
        };
        let result = state.repo.set_branch_target(&name, &head_sha);
        if app
            .run(
                state,
                "Fast-forward",
                result,
                Some(&format!("Fast-forwarded {name}")),
            )
            .is_some()
        {
            state.push_undo(undo);
        }
    })
}

/// getMergeItems + getInteractiveRebaseSteps between HEAD (ours) and `theirs`.
fn merge_and_rebase_items(state: &RepoState, theirs: &Ref) -> MenuItems {
    let mut items = MenuItems::new();
    let Some(head) = state.head_ref() else {
        return items;
    };
    let workdir = workdir_of(state);
    let ours = head.name.clone();
    let their_short = theirs.name.clone();
    let their_state = merge_state(state, &theirs.target, &head.target);
    let our_state = merge_state(state, &head.target, &theirs.target);
    match their_state {
        MergeState::FastForward => {
            if theirs.ref_type != RefType::RemoteBranch {
                let label = format!("Fast-forward {their_short} to {ours}");
                items.push(if theirs.ref_type == RefType::Tag {
                    placeholder(label, "Fast-forward", "Tags cannot be fast-forwarded.")
                } else {
                    fast_forward_item(label, &workdir, theirs, &head.target)
                });
                let (their_ref, ours_name) = (theirs.clone(), ours.clone());
                items.push(repo_item(
                    format!("Merge {ours} into {their_short}"),
                    &workdir,
                    move |cx, app, state| {
                        if their_ref.ref_type != RefType::LocalBranch {
                            app.toasts.push_with_detail(
                                ToastKind::Info,
                                "Merge",
                                "Only branches can be merged into.",
                            );
                            return;
                        }
                        checkout_ref(cx, app, state, &their_ref);
                        merge_into_head(app, state, &ours_name, false);
                    },
                ));
            }
        }
        MergeState::Normal => items.push(merge_item(
            format!("Merge {their_short} into {ours}"),
            &workdir,
            their_short.clone(),
            false,
        )),
        MergeState::UpToDate => {
            if our_state == MergeState::FastForward {
                items.push(merge_item(
                    format!("Fast-forward {ours} to {their_short}"),
                    &workdir,
                    their_short.clone(),
                    false,
                ));
                items.push(merge_item(
                    format!("Merge {their_short} into {ours}"),
                    &workdir,
                    their_short.clone(),
                    false,
                ));
            }
        }
    }
    let onto = their_short.clone();
    items.push(repo_item(
        format!("Rebase {ours} onto {their_short}"),
        &workdir,
        move |_cx, app, state| app.run_git_cli(state, "Rebase", ["rebase", onto.as_str()]),
    ));
    let above = state.commits_above(Some(&theirs.target), None);
    let linear = above.iter().all(|commit| commit.parents.len() <= 1);
    if state.repo.is_ancestor(&theirs.target, &head.target) && !above.is_empty() && linear {
        let (target, label) = (theirs.target.clone(), their_short.clone());
        items.push(repo_item(
            format!("Interactive Rebase {ours} onto {their_short}"),
            &workdir,
            move |_cx, _app, state| state.start_rebase_plan(&target, &label, None, None),
        ));
    }
    items
}

// ---------------------------------------------------------------------------------------------------------
// Deleting branches

/// Deletes every branch in `refs` after a confirmation: the local ones here (each undoable), the remote ones
/// with one `git push <remote> --delete` per remote branch, run one after another.
fn delete_branches(app: &mut Application, workdir: &Path, refs: Vec<Ref>) {
    app.dialog.open(DialogKind::Confirm);
    app.dialog.workdir = Some(workdir.to_path_buf());
    app.dialog.message = format!(
        "This is a destructive operation, are you sure you want to delete all \"{}\" branches, local and remote?",
        refs[0].name
    );
    app.dialog.confirm_label = "Delete".to_owned();
    app.dialog.danger = true;
    let workdir = workdir.to_path_buf();
    app.dialog.on_confirm = Some(Box::new(move |_cx, app| {
        app.with_repo(&workdir, |app, tab| {
            let mut pushes: Vec<Vec<String>> = Vec::new();
            let mut deleted: Vec<String> = Vec::new();
            for reference in &refs {
                match reference.ref_type {
                    RefType::RemoteBranch => {
                        pushes.push(git_args(&[
                            "push",
                            &reference.remote,
                            "--delete",
                            remote_branch_name(reference),
                        ]));
                    }
                    RefType::LocalBranch => {
                        let (name, sha) = (reference.name.clone(), reference.target.clone());
                        if let Err(error) = tab.repo.delete_branch(&name) {
                            app.toasts.push_with_detail(
                                ToastKind::Error,
                                "Delete branch failed",
                                error.to_string(),
                            );
                            continue;
                        }
                        deleted.push(name.clone());
                        let (redo_name, undo_name) = (name.clone(), name.clone());
                        tab.push_undo(
                            UndoEntry::new(format!("delete branch {name}"), move |repo| {
                                repo.set_branch_target(&undo_name, &sha)
                            })
                            .with_redo(move |repo| repo.delete_branch(&redo_name)),
                        );
                    }
                    RefType::Tag | RefType::Stash => {}
                }
            }
            tab.refresh();
            if !pushes.is_empty() {
                app.run_git_cli_sequence(tab, "Delete remote branches", pushes);
            } else if let Some(first) = deleted.first() {
                app.toasts
                    .push(ToastKind::Success, format!("Deleted {first}"));
            }
        });
    }));
}

fn delete_items(workdir: &Path, refs: &[Ref]) -> MenuItems {
    let mut items = MenuItems::new();
    for reference in refs {
        let reference = reference.clone();
        items.push(menu_item(
            format!("Delete {}", reference.name),
            move |_cx, app| {
                app.dialog.open(DialogKind::DeleteBranch);
                if reference.ref_type == RefType::RemoteBranch {
                    app.dialog.ref_name = remote_branch_name(&reference).to_owned();
                    app.dialog.ref_remote.clone_from(&reference.remote);
                } else {
                    app.dialog.ref_name.clone_from(&reference.name);
                    app.dialog.text_a = reference.upstream.clone().unwrap_or_default();
                    app.dialog.sha.clone_from(&reference.target);
                }
            },
        ));
    }
    if let [local, remote] = refs {
        let (local, remote) = (local.clone(), remote.clone());
        items.push(menu_item(
            format!("Delete {} and {}", local.name, remote.name),
            move |_cx, app| {
                app.dialog.open(DialogKind::DeleteBranch);
                app.dialog.ref_name = local.name;
                app.dialog.text_a = remote.name;
                app.dialog.sha = local.target;
                app.dialog.flag = true;
            },
        ));
    } else if refs.len() > 2 {
        let all = refs.to_vec();
        let workdir = workdir.to_path_buf();
        items.push(menu_item(
            format!("Delete all \"{}\" branches (local && remote)", refs[0].name),
            move |_cx, app| delete_branches(app, &workdir, all),
        ));
    }
    items
}

// ---------------------------------------------------------------------------------------------------------
// Branch groups

/// "Pull", "Push" and "Set Upstream" of a local branch.
fn remote_ops(workdir: &Path, state: &RepoState, branch: &Ref) -> MenuItems {
    let first_remote = state
        .history
        .remotes
        .first()
        .map(|remote| remote.name.clone())
        .unwrap_or_default();
    let pull = branch.clone();
    let push = branch.clone();
    let upstream_of = branch.clone();
    vec![
        repo_item(
            "Pull (fast-forward if possible)",
            workdir,
            move |_cx, app, state| {
                // "origin/main"; an upstream without a remote part tracks a local branch, which counts as
                // none here.
                let split = pull
                    .upstream
                    .as_deref()
                    .and_then(|upstream| upstream.split_once('/'));
                if pull.is_head {
                    app.run_git_cli(state, "Pull", ["pull", "--ff"]);
                } else if let Some((remote, name)) = split {
                    let refspec = format!("{name}:{}", pull.name);
                    app.run_git_cli(state, "Pull", ["fetch", remote, refspec.as_str()]);
                } else {
                    app.toasts.push_with_detail(
                        ToastKind::Error,
                        "Pull failed",
                        format!("{} has no upstream.", pull.name),
                    );
                }
            },
        ),
        repo_item("Push", workdir, move |_cx, app, state| {
            let split = push
                .upstream
                .as_deref()
                .and_then(|upstream| upstream.split_once('/'));
            let Some((remote, name)) = split else {
                app.dialog.open(DialogKind::PushNewBranch);
                app.dialog.ref_name.clone_from(&push.name);
                app.dialog.text_a = first_remote;
                return;
            };
            let refspec = format!("{}:{name}", push.name);
            app.run_git_cli(state, "Push", ["push", remote, refspec.as_str()]);
        }),
        menu_item("Set Upstream", move |_cx, app| {
            app.dialog.open(DialogKind::SetUpstream);
            app.dialog.ref_name.clone_from(&upstream_of.name);
            app.dialog.text_a = upstream_of.upstream.unwrap_or_default();
        }),
    ]
}

/// The "Checkout" entries: the group's refs but HEAD's, plus "this commit".
fn checkout_entries(
    workdir: &Path,
    state: &RepoState,
    refs: &[Ref],
    sha: &str,
    extra: usize,
) -> MenuItems {
    let head_name = state.head_ref().map(|head| head.full_name.clone());
    let mut sub: MenuItems = refs
        .iter()
        .filter(|reference| head_name.as_deref() != Some(reference.full_name.as_str()))
        .map(|reference| {
            let reference = reference.clone();
            repo_item(reference.name.clone(), workdir, move |cx, app, state| {
                checkout_ref(cx, app, state, &reference);
            })
        })
        .collect();
    if sub.len() + extra > 1 {
        if extra != 0 {
            sub.push(checkout_commit_item(workdir, sha, true));
        }
        return vec![menu_submenu("Checkout", sub)];
    }
    let mut checkout = MenuItems::new();
    for mut entry in sub {
        entry.label = format!("Checkout {}", entry.label);
        checkout.push(entry);
    }
    if extra != 0 {
        checkout.push(checkout_commit_item(workdir, sha, false));
    }
    checkout
}

/// "Create worktree from": refs without a worktree (the checked-out branch has the main one).
fn worktree_entries(refs: &[Ref], sha: &str, extra: usize) -> MenuItems {
    let mut sub: MenuItems = refs
        .iter()
        .filter(|reference| !reference.is_head)
        .map(|reference| worktree_item(reference.name.clone(), Some(reference), sha))
        .collect();
    if extra != 0 {
        sub.push(worktree_commit_item(sha, true));
    }
    if sub.len() > 1 {
        return vec![menu_submenu("Create worktree from", sub)];
    }
    sub.into_iter()
        .map(|mut entry| {
            entry.label = if entry.label == "this commit" {
                "Create worktree from this commit".to_owned()
            } else {
                format!("Create worktree from {}", entry.label)
            };
            entry
        })
        .collect()
}

/// getRefGroupMenuItems: a single branch group, from a pill, a left panel row or a commit with one branch.
fn ref_group_items(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    group: &RefGroup,
    sha: &str,
    in_left_panel: bool,
    is_submenu: bool,
) -> MenuItems {
    // Gitflow: Finish feature / release / hotfix <name> on a gitflow branch.
    let gitflow_ops = match group.local() {
        Some(local) => {
            let name = local.name.clone();
            gitflow_finish_items(cx, app, state, &name)
        }
        None => MenuItems::new(),
    };
    let state: &RepoState = state;
    let workdir = workdir_of(state);
    let head = state.head_ref();
    let local = group.local();
    let refs = &group.refs;
    let multi = selected_commits(state).len() > 1;
    let head_at_group = head.is_some_and(|head| {
        refs.first()
            .is_some_and(|first| head.target == first.target)
    });
    let head_sha = state.history.head_sha.as_deref();

    let remote_ops = match local {
        Some(local) if !state.history.remotes.is_empty() => remote_ops(&workdir, state, local),
        _ => MenuItems::new(),
    };

    let merge_items = match (head, refs.first()) {
        (Some(head), Some(first)) if !head_at_group && first.full_name != head.full_name => {
            merge_and_rebase_items(state, first)
        }
        _ => MenuItems::new(),
    };

    let extra = usize::from(!(is_submenu || in_left_panel));
    let checkout = checkout_entries(&workdir, state, refs, sha, extra);
    let worktree = worktree_entries(refs, sha, extra);

    let mut commit_ops = MenuItems::new();
    let mut rebase_ops = MenuItems::new();
    if !is_submenu {
        commit_ops.push(create_branch_item(sha));
        if !head_at_group && !multi {
            commit_ops.push(cherry_pick_item(&workdir, sha));
        }
        if multi && state.is_selected(sha) && state.repo.state() != OperationState::Rebase {
            commit_ops.extend(cherry_picks_item(state));
        }
        commit_ops.extend(reset_items(state, sha));
        if head_sha == Some(sha) {
            commit_ops.push(reword_item(&workdir, sha));
        }
        commit_ops.push(revert_item(&workdir, sha));
        rebase_ops = rebase_group(state, sha, multi);
    }

    let mut branch_ops = MenuItems::new();
    if !is_submenu && let Some(local) = local {
        if head_sha == Some(sha) {
            branch_ops.push(repo_item("Apply patch", &workdir, move |cx, app, state| {
                if let Some(file) =
                    imgit_platform::shell::open_file_dialog(Some(cx.shell.window), Some("patch"))
                {
                    app.run_git_cli(
                        state,
                        "Apply patch",
                        ["apply", file.to_string_lossy().as_ref()],
                    );
                }
            }));
        }
        let name = local.name.clone();
        branch_ops.push(menu_item(format!("Rename {name}"), move |_cx, app| {
            app.dialog.open(DialogKind::RenameBranch);
            app.dialog.ref_name.clone_from(&name);
            app.dialog.text_a = name;
        }));
    }
    branch_ops.extend(delete_items(&workdir, refs));

    let mut copy_ops = MenuItems::new();
    let copy_name = refs
        .first()
        .map_or_else(|| group.name.clone(), branch_group_name);
    copy_ops.push(menu_item("Copy branch name", move |cx, app| {
        app.copy_to_clipboard(cx, &copy_name);
    }));
    copy_ops.push(copy_sha_item(sha));
    if !in_left_panel && (!multi || state.is_selected(sha)) {
        copy_ops.extend(patch_items(&workdir, state, sha, multi));
    }

    let mut view_ops = MenuItems::new();
    if let [only] = refs.as_slice() {
        if in_left_panel {
            view_ops.push(toggle_hide_item(
                &workdir,
                state,
                &only.full_name,
                state.solo_ref.is_none(),
            ));
        }
        view_ops.push(pin_item(&workdir, state, &only.full_name, false));
        view_ops.push(toggle_solo_item(&workdir, state, &only.full_name, "Solo"));
    } else if !in_left_panel && !refs.is_empty() {
        let pinned = refs
            .iter()
            .find(|reference| state.pinned_branch.as_deref() == Some(reference.full_name.as_str()));
        if let Some(pinned) = pinned {
            view_ops.push(pin_item(&workdir, state, &pinned.full_name, false));
        } else {
            let pins = refs
                .iter()
                .map(|reference| pin_item(&workdir, state, &reference.full_name, true))
                .collect();
            view_ops.push(menu_submenu("Pin to Left", pins));
        }
        let solos = refs
            .iter()
            .map(|reference| {
                toggle_solo_item(&workdir, state, &reference.full_name, &reference.name)
            })
            .collect();
        view_ops.push(menu_submenu("Solo", solos));
    }

    separated_menu(vec![
        remote_ops,
        merge_items,
        gitflow_ops,
        checkout,
        worktree,
        commit_ops,
        rebase_ops,
        branch_ops,
        copy_ops,
        view_ops,
        compare_items(&workdir, state, sha),
    ])
}

/// "Create patch from commit(s)".
fn patch_items(workdir: &Path, state: &RepoState, sha: &str, multi: bool) -> MenuItems {
    let shas = if multi {
        selected_youngest_first(state)
    } else {
        vec![sha.to_owned()]
    };
    let label = if shas.len() > 1 {
        "Create patch from commits"
    } else {
        "Create patch from commit"
    };
    vec![repo_item(label, workdir, move |cx, app, state| {
        let (Some(newest), Some(oldest)) = (shas.first(), shas.last()) else {
            return;
        };
        let mut args = git_args(&["format-patch", "--stdout"]);
        if shas.len() == 1 {
            args.push("-1".to_owned());
            args.push(newest.clone());
        } else {
            args.push(format!("{oldest}^..{newest}"));
        }
        let workdir = state.repo.workdir().to_path_buf();
        super::shared::save_git_output(
            cx,
            app,
            &workdir,
            &format!("{}.patch", short_sha(newest)),
            &args,
        );
    })]
}

fn create_tag_items(sha: &str) -> MenuItems {
    let (plain, annotated) = (sha.to_owned(), sha.to_owned());
    vec![
        menu_item("Create tag here", move |_cx, app| {
            app.dialog.open(DialogKind::CreateTag);
            app.dialog.sha = plain;
        }),
        menu_item("Create annotated tag here", move |_cx, app| {
            app.dialog.open(DialogKind::CreateTag);
            app.dialog.sha = annotated;
            app.dialog.flag = true;
        }),
    ]
}

/// The menu for a commit row or a branch pill.
fn commit_or_branch_menu(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    sha: &str,
    clicked: Option<RefGroup>,
    in_left_panel: bool,
) -> MenuItems {
    let mut hidden = 0;
    let mut groups = match clicked {
        Some(group) => vec![group],
        None => {
            let mut groups = branch_groups_at(state, sha);
            if groups.len() > 20 {
                hidden = groups.len() - 20;
                groups.truncate(20);
            }
            groups
        }
    };
    let multi = selected_commits(state).len() > 1;
    if groups.len() == 1 {
        let group = groups.remove(0);
        let ref_items = ref_group_items(cx, app, state, &group, sha, in_left_panel, false);
        return separated_menu(vec![ref_items, create_tag_items(sha)]);
    }
    // Several (or no) branches here: one submenu per branch group.
    let mut group_menus = MenuItems::new();
    if groups.len() > 1 {
        for group in &groups {
            let submenu = ref_group_items(cx, app, state, group, sha, in_left_panel, true);
            group_menus.push(menu_submenu(group.name.clone(), submenu));
        }
        if hidden > 0 {
            group_menus.push(MenuItem::disabled(format!(
                "{hidden} more refs not displayed"
            )));
        }
    }
    let state: &RepoState = state;
    let workdir = workdir_of(state);
    let head_sha = state.history.head_sha.as_deref();
    let mut checkout = MenuItems::new();
    let mut worktree = MenuItems::new();
    let mut compare = MenuItems::new();
    if !in_left_panel {
        checkout.push(checkout_commit_item(&workdir, sha, false));
        worktree.push(worktree_commit_item(sha, false));
        compare = compare_items(&workdir, state, sha);
    }
    let mut commit_ops = vec![create_branch_item(sha)];
    if head_sha != Some(sha) && !multi {
        commit_ops.push(cherry_pick_item(&workdir, sha));
    }
    if multi && state.is_selected(sha) && state.repo.state() != OperationState::Rebase {
        commit_ops.extend(cherry_picks_item(state));
    }
    if groups.is_empty() {
        commit_ops.extend(rebase_onto_commit_items(state, sha));
    }
    commit_ops.extend(reset_items(state, sha));
    commit_ops.push(revert_item(&workdir, sha));
    if head_sha == Some(sha) {
        commit_ops.push(reword_item(&workdir, sha));
    }
    let rebase_ops = rebase_group(state, sha, multi);

    let mut copy_ops = MenuItems::new();
    if groups.len() <= 1 {
        copy_ops.push(copy_sha_item(sha));
        if !multi || state.is_selected(sha) {
            copy_ops.extend(patch_items(&workdir, state, sha, multi));
        }
    }
    separated_menu(vec![
        checkout,
        worktree,
        commit_ops,
        rebase_ops,
        group_menus,
        copy_ops,
        compare,
        create_tag_items(sha),
    ])
}

fn stash_menu(state: &RepoState, stash: &Ref) -> MenuItems {
    let workdir = workdir_of(state);
    let index = stash.stash_index.unwrap_or(0);
    let (sha, name, message) = (
        stash.target.clone(),
        stash.name.clone(),
        stash.message.clone(),
    );
    let (edit_sha, edit_message) = (sha.clone(), message.clone());
    separated_menu(vec![
        vec![
            repo_item("Apply Stash", &workdir, move |_cx, app, state| {
                let result = state.repo.stash_apply(index);
                app.run(state, "Apply stash", result, Some("Stash applied"));
            }),
            repo_item("Pop Stash", &workdir, move |_cx, app, state| {
                let result = state.repo.stash_pop(index);
                app.run(state, "Pop stash", result, Some("Stash popped"));
            }),
            menu_item("Delete Stash", move |_cx, app| {
                app.dialog.open(DialogKind::DeleteStash);
                app.dialog.stash_index = Some(index);
                // Found again at confirm: a stash pushed since shifts the indexes.
                app.dialog.sha = sha;
                app.dialog.ref_name = name;
                app.dialog.text_b = imgit_git::repository::stash_summary(&message).to_owned();
            }),
        ],
        vec![menu_item("Edit stash message", move |_cx, app| {
            app.dialog.open(DialogKind::EditStashMessage);
            app.dialog.sha = edit_sha;
            app.dialog.message = "Stash message:".to_owned();
            app.dialog.text_a = imgit_git::repository::stash_summary(&edit_message).to_owned();
        })],
        vec![toggle_hide_item(&workdir, state, &stash.full_name, true)],
    ])
}

fn tag_menu(state: &RepoState, tag: &Ref, in_left_panel: bool) -> MenuItems {
    let workdir = workdir_of(state);
    let sha = tag.target.clone();
    let name = tag.name.clone();
    let multi = selected_commits(state).len() > 1;
    let push_tag = |remote: String, name: String| {
        move |_cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState| {
            let target = format!("refs/tags/{name}");
            app.run_git_cli(
                state,
                "Push tag",
                ["push", remote.as_str(), target.as_str()],
            );
        }
    };
    let mut push = MenuItems::new();
    match state.history.remotes.as_slice() {
        [] => {}
        [only] => push.push(repo_item(
            format!("Push {name} to {}", only.name),
            &workdir,
            push_tag(only.name.clone(), name.clone()),
        )),
        remotes => {
            let mut entries: MenuItems = remotes
                .iter()
                .map(|remote| {
                    repo_item(
                        format!("Push {name} to {}", remote.name),
                        &workdir,
                        push_tag(remote.name.clone(), name.clone()),
                    )
                })
                .collect();
            let all_name = name.clone();
            let all = repo_item(
                format!("Push {name} to all remotes"),
                &workdir,
                move |_cx, app, state| {
                    let runs: Vec<Vec<String>> = state
                        .history
                        .remotes
                        .iter()
                        .map(|remote| {
                            vec![
                                "push".to_owned(),
                                remote.name.clone(),
                                format!("refs/tags/{all_name}"),
                            ]
                        })
                        .collect();
                    app.run_git_cli_sequence(state, "Push tag", runs);
                },
            );
            entries = separated_menu(vec![entries, vec![all]]);
            push.push(menu_submenu("Push tag to remote", entries));
        }
    }
    let merge_items = merge_and_rebase_items(state, tag);
    let mut checkout_tag = MenuItems::new();
    let mut checkout = MenuItems::new();
    let mut worktree = MenuItems::new();
    if in_left_panel {
        let tag_ref = tag.clone();
        checkout_tag.push(repo_item(
            format!("Checkout the commit at {name}"),
            &workdir,
            move |cx, app, state| {
                checkout_ref(cx, app, state, &tag_ref);
            },
        ));
    } else {
        checkout.push(checkout_commit_item(&workdir, &sha, false));
        worktree.push(worktree_commit_item(&sha, false));
    }
    let mut commit_ops = vec![create_branch_item(&sha)];
    if state.history.head_sha.as_deref() != Some(sha.as_str()) && !multi {
        commit_ops.push(cherry_pick_item(&workdir, &sha));
    }
    commit_ops.extend(reset_items(state, &sha));
    commit_ops.push(revert_item(&workdir, &sha));
    let mut view = MenuItems::new();
    if in_left_panel {
        view.push(toggle_hide_item(
            &workdir,
            state,
            &tag.full_name,
            state.solo_ref.is_none(),
        ));
    }
    view.push(toggle_solo_item(&workdir, state, &tag.full_name, "Solo"));
    let (delete_name, delete_sha) = (name.clone(), sha.clone());
    let (copy_name, annotate_name, annotate_sha) = (name.clone(), name.clone(), sha);
    separated_menu(vec![
        push,
        merge_items,
        checkout_tag,
        checkout,
        worktree,
        commit_ops,
        vec![menu_item(
            format!("Delete {name} locally"),
            move |_cx, app| {
                app.dialog.open(DialogKind::DeleteTag);
                app.dialog.ref_name = delete_name;
                app.dialog.sha = delete_sha;
            },
        )],
        vec![menu_item("Copy tag name", move |cx, app| {
            app.copy_to_clipboard(cx, &copy_name);
        })],
        view,
        vec![menu_item(format!("Annotate {name}"), move |_cx, app| {
            app.dialog.open(DialogKind::CreateTag);
            app.dialog.sha = annotate_sha;
            app.dialog.text_a = annotate_name;
            app.dialog.flag = true;
        })],
    ])
}

// ---------------------------------------------------------------------------------------------------------
// Public menus

#[must_use]
pub fn commit_menu(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    sha: &str,
) -> MenuItems {
    let Some(commit) = state.commit_for(sha) else {
        return MenuItems::new();
    };
    match commit.node_type {
        NodeType::WorkDir => MenuItems::new(),
        NodeType::Stash => {
            let stash = state
                .refs_at(sha)
                .into_iter()
                .find(|reference| reference.ref_type == RefType::Stash)
                .cloned();
            stash.map_or_else(MenuItems::new, |stash| stash_menu(state, &stash))
        }
        NodeType::Commit | NodeType::Merge => {
            commit_or_branch_menu(cx, app, state, sha, None, false)
        }
    }
}

#[must_use]
pub fn ref_menu(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    reference: &Ref,
    in_left_panel: bool,
) -> MenuItems {
    match reference.ref_type {
        RefType::Tag => tag_menu(state, reference, in_left_panel),
        RefType::Stash => stash_menu(state, reference),
        RefType::LocalBranch | RefType::RemoteBranch => {
            let group = if in_left_panel {
                RefGroup {
                    name: branch_group_name(reference),
                    refs: vec![reference.clone()],
                }
            } else {
                group_for_ref(state, reference)
            };
            commit_or_branch_menu(
                cx,
                app,
                state,
                &reference.target,
                Some(group),
                in_left_panel,
            )
        }
    }
}

/// Whether a drop is allowed: another commit, with a local branch on either side (or a hosting service both
/// sides are connected to, which ImGit has none of).
#[must_use]
pub fn can_drop_ref(state: &RepoState, source: &Ref, target: &Ref) -> bool {
    if source.target == target.target
        || source.ref_type == RefType::Stash
        || target.ref_type == RefType::Stash
    {
        return false;
    }
    group_for_ref(state, source).local().is_some() || group_for_ref(state, target).local().is_some()
}

/// The drop menu: the dragged group D dropped on the target group T.
#[must_use]
pub fn ref_drop_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    source: &Ref,
    target: &Ref,
) -> MenuItems {
    if source.target == target.target {
        return MenuItems::new();
    }
    let state: &RepoState = state;
    let workdir = workdir_of(state);
    let dragged = group_for_ref(state, source);
    let dropped = group_for_ref(state, target);
    let dragged_local = dragged.local();
    let target_local = dropped.local();
    // A group's primary ref: its local branch, else its first ref (a remote branch or the tag).
    let (Some(dragged_primary), Some(target_primary)) = (
        dragged_local.or(dragged.refs.first()),
        target_local.or(dropped.refs.first()),
    ) else {
        return MenuItems::new();
    };
    let target_remotes: Vec<&Ref> = dropped
        .refs
        .iter()
        .filter(|reference| reference.ref_type == RefType::RemoteBranch)
        .collect();

    // Merges check the receiving branch out first; a fast-forward of another branch only moves it.
    let merge = |label: String, into: &Ref, from: &Ref| {
        let (into, from) = (into.clone(), from.clone());
        repo_item(label, &workdir, move |cx, app, state| {
            if !into.is_head && !checkout_ref(cx, app, state, &into) {
                return;
            }
            merge_into_head(app, state, &from.name, true);
        })
    };
    let fast_forward = |label: String, branch: &Ref, to: &Ref| {
        let (branch, to) = (branch.clone(), to.clone());
        repo_item(label, &workdir, move |_cx, app, state| {
            if branch.is_head {
                merge_into_head(app, state, &to.name, false);
            } else {
                app.run_git_cli(
                    state,
                    "Fast-forward",
                    ["branch", "-f", branch.name.as_str(), to.target.as_str()],
                );
            }
        })
    };

    let mut merges = MenuItems::new();
    if let Some(ours) = target_local {
        // getMergeItems: ours = T's local branch, theirs = D's primary ref.
        let their_state = merge_state(state, &dragged_primary.target, &ours.target);
        let our_state = merge_state(state, &ours.target, &dragged_primary.target);
        match their_state {
            MergeState::FastForward => {
                if let Some(dragged_local) = dragged_local {
                    merges.push(fast_forward(
                        format!("Fast-forward {} to {}", dragged_local.name, ours.name),
                        dragged_local,
                        ours,
                    ));
                    merges.push(merge(
                        format!("Merge {} into {}", ours.name, dragged_local.name),
                        dragged_local,
                        ours,
                    ));
                }
            }
            MergeState::Normal => merges.push(merge(
                format!("Merge {} into {}", dragged_primary.name, ours.name),
                ours,
                dragged_primary,
            )),
            MergeState::UpToDate => {
                if our_state == MergeState::FastForward {
                    merges.push(fast_forward(
                        format!("Fast-forward {} to {}", ours.name, dragged_primary.name),
                        ours,
                        dragged_primary,
                    ));
                    merges.push(merge(
                        format!("Merge {} into {}", dragged_primary.name, ours.name),
                        ours,
                        dragged_primary,
                    ));
                }
            }
        }
    }
    // getInteractiveRebaseSteps: D's local branch onto T, when T has a local or remote branch.
    if let Some(ours) = dragged_local
        && (target_local.is_some() || !target_remotes.is_empty())
    {
        let onto = target_primary;
        let (rebase_ours, rebase_onto) = (ours.clone(), onto.clone());
        merges.push(repo_item(
            format!("Rebase {} onto {}", ours.name, onto.name),
            &workdir,
            move |cx, app, state| {
                if !rebase_ours.is_head && !checkout_ref(cx, app, state, &rebase_ours) {
                    return;
                }
                app.run_git_cli(state, "Rebase", ["rebase", rebase_onto.name.as_str()]);
            },
        ));
        // Offered when D has commits T lacks.
        if !state.repo.is_ancestor(&ours.target, &onto.target) {
            let (ours_ref, onto_ref) = (ours.clone(), onto.clone());
            merges.push(repo_item(
                format!("Interactive Rebase {} onto {}", ours.name, onto.name),
                &workdir,
                move |cx, app, state| {
                    let fork_point = state.repo.merge_base(&ours_ref.target, &onto_ref.target);
                    if state
                        .commits_above(fork_point.as_deref(), Some(&ours_ref.target))
                        .is_empty()
                    {
                        app.toasts.push_with_detail(
                            ToastKind::Error,
                            "Interactive Rebase",
                            "The commits to rebase include a merge commit.",
                        );
                        return;
                    }
                    if !ours_ref.is_head && !checkout_ref(cx, app, state, &ours_ref) {
                        return;
                    }
                    state.start_rebase_plan(
                        &onto_ref.target,
                        &onto_ref.name,
                        fork_point.as_deref(),
                        Some(&ours_ref.target),
                    );
                },
            ));
        }
    }
    let mut push_reset = MenuItems::new();
    if let Some(local) = dragged_local {
        let push_to = |remote: &Ref| {
            let (local, remote) = (local.clone(), remote.clone());
            move |_cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState| {
                let refspec = format!("{}:{}", local.name, remote_branch_name(&remote));
                app.run_git_cli(
                    state,
                    "Push",
                    ["push", remote.remote.as_str(), refspec.as_str()],
                );
            }
        };
        match target_remotes.as_slice() {
            [] => {}
            [only] => push_reset.push(repo_item(
                format!("Push {} to {}", local.name, only.name),
                &workdir,
                push_to(only),
            )),
            several => {
                let sub = several
                    .iter()
                    .map(|remote| repo_item(remote.name.clone(), &workdir, push_to(remote)))
                    .collect();
                push_reset.push(menu_submenu(format!("Push {} to...", local.name), sub));
            }
        }
        if local.is_head {
            push_reset.extend(reset_items(state, &target.target));
        }
    }
    // Pull request items need a connected hosting service; ImGit leaves them out without one.
    separated_menu(vec![merges, push_reset])
}

/// Whether the commit can be reworded from the detail panel: HEAD, or a commit of HEAD's first-parent chain
/// above its first merge commit.
#[must_use]
pub fn can_reword(state: &RepoState, sha: &str) -> bool {
    let Some(head) = state.history.head_sha.as_deref() else {
        return false;
    };
    if sha.is_empty() || state.rebase.active || state.repo.state() == OperationState::Rebase {
        return false;
    }
    if sha == head {
        return true;
    }
    // An older commit: on HEAD's first-parent chain above the first merge, with a parent to rebuild onto (as
    // the commit menu's Edit commit message).
    let chain = head_chain(state);
    chain_index(&chain, sha).is_some_and(|index| index > 0 && chain[index].parents.len() == 1)
}

/// Rewords a commit's message (rebuilt with `Repository::rewrite_history`, with an undo entry). The reworded
/// commit stays selected; `false`, with a toast, when it failed.
pub fn reword_commit(
    _cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    sha: &str,
    message: &str,
) -> bool {
    reword(app, state, sha, message)
}

fn reword(app: &mut Application, state: &mut RepoState, sha: &str, message: &str) -> bool {
    if !can_reword(state, sha) {
        app.toasts.push_with_detail(
            ToastKind::Error,
            "Edit commit message failed",
            "Only commits of the current branch above its last merge commit can be reworded.",
        );
        return false;
    }
    if state.history.head_sha.as_deref() == Some(sha) {
        let undo = super::shared::tip_restore_entry(state, "edit commit message", ResetMode::Hard);
        let result = state.repo.reword_head(message);
        if app.run(state, "Update message", result, None).is_none() {
            return false;
        }
        state.push_undo(undo);
        if let Some(head) = state.history.head_sha.clone() {
            state.select_only(&head);
        }
        return true;
    }
    let chain = head_chain(state);
    let Some(index) = chain_index(&chain, sha) else {
        return false;
    };
    let Some(base) = chain[index].parents.first().cloned() else {
        return false;
    };
    let steps = pick_steps_with_reword(&state.commits_above(Some(&base), None), sha, message);
    if !rewrite(
        app,
        state,
        &base,
        &steps,
        "Edit commit message",
        "Commit message updated",
    ) {
        return false;
    }
    // The reworded commit has a new sha at the same place under HEAD: keep it selected.
    let rewritten = head_chain(state);
    if let Some(commit) = rewritten.get(index) {
        let sha = commit.sha.clone();
        state.select_only(&sha);
    }
    true
}

/// Selects the commit and scrolls the graph to it, closing the open diff.
pub fn jump_to_commit(state: &mut RepoState, sha: &str) {
    let Some(&row) = state.history.row_by_sha.get(sha) else {
        return;
    };
    state.select_only(sha);
    state.scroll_to_row = Some(row);
    state.close_diff();
}

/// Checks `reference` out; a remote branch the first time gets its local branch made.
fn check_out(
    repo: &imgit_git::Repository,
    reference: &Ref,
    first_time: bool,
) -> imgit_git::Result<()> {
    match reference.ref_type {
        RefType::LocalBranch => repo.checkout_branch(&reference.name),
        RefType::RemoteBranch if first_time => {
            repo.checkout_remote_branch(&reference.remote, remote_branch_name(reference))
        }
        RefType::RemoteBranch => repo.checkout_branch(remote_branch_name(reference)),
        RefType::Tag | RefType::Stash => repo.checkout_commit(&reference.target),
    }
}

/// Checks the ref out; `false` when the checkout failed (`true` when it is HEAD already, or a stash).
pub fn checkout_ref(
    _cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    reference: &Ref,
) -> bool {
    check_out_ref(app, state, reference)
}

fn check_out_ref(app: &mut Application, state: &mut RepoState, reference: &Ref) -> bool {
    let (label, message) = match reference.ref_type {
        RefType::LocalBranch if !reference.is_head => (
            reference.name.clone(),
            format!("Checked out {}", reference.name),
        ),
        RefType::LocalBranch | RefType::Stash => return true,
        RefType::RemoteBranch => {
            let local = remote_branch_name(reference).to_owned();
            let message = format!("Checked out {local}");
            (local, message)
        }
        RefType::Tag => (
            reference.name.clone(),
            format!("Checked out {} (detached HEAD)", reference.name),
        ),
    };
    // Undo goes back to where HEAD was; redo checks the same thing out again.
    let target = reference.clone();
    let undo = head_restore_entry(state, &format!("checkout {label}"), move |repo| {
        check_out(repo, &target, false)
    });
    let result = check_out(&state.repo, reference, true);
    let ok = app.run(state, "Checkout", result, Some(&message)).is_some();
    if ok {
        state.push_undo(undo);
    }
    ok
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use imgit_git::cli;

    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let mut full = vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ];
        full.extend(args);
        let output = cli::run(dir, full, cli::RunOptions::default());
        assert_eq!(output.exit_code, 0, "git {args:?}: {}", output.text);
        output.text
    }

    fn commit_file(dir: &Path, name: &str, message: &str) {
        std::fs::write(dir.join(name), format!("{message}\n")).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-m", message]);
    }

    /// A repository with three linear commits `one`, `two`, `three` on `main`, opened in an application.
    fn fixture() -> (tempfile::TempDir, Application) {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("repo");
        std::fs::create_dir_all(&folder).unwrap();
        git(&folder, &["init", "-b", "main"]);
        git(&folder, &["config", "user.name", "Test"]);
        git(&folder, &["config", "user.email", "test@example.com"]);
        for name in ["one", "two", "three"] {
            commit_file(&folder, &format!("{name}.txt"), name);
        }
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.open_repository(&folder.to_string_lossy());
        (scratch, app)
    }

    fn log(state: &RepoState) -> Vec<String> {
        let output = git(state.repo.workdir(), &["log", "--format=%s", "main"]);
        output.lines().map(str::to_owned).collect()
    }

    fn sha_of(state: &RepoState, summary: &str) -> Sha {
        state
            .history
            .commits
            .iter()
            .find(|commit| commit.summary == summary)
            .unwrap()
            .sha
            .clone()
    }

    #[test]
    fn only_head_and_the_chain_above_the_first_merge_can_be_reworded() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let (one, two, three) = (
                sha_of(state, "one"),
                sha_of(state, "two"),
                sha_of(state, "three"),
            );
            assert!(can_reword(state, &three));
            assert!(can_reword(state, &two));
            // The root has no parent to rebuild onto.
            assert!(!can_reword(state, &one));
            assert!(!can_reword(state, ""));
            assert!(!can_reword(state, "nonsense"));
            // A merge on top: nothing below it is on the chain.
            let dir = state.repo.workdir().to_path_buf();
            git(&dir, &["checkout", "-b", "side", &one]);
            commit_file(&dir, "side.txt", "side");
            git(&dir, &["checkout", "main"]);
            git(&dir, &["merge", "--no-ff", "side", "-m", "merged"]);
            state.refresh();
            let merged = sha_of(state, "merged");
            assert!(can_reword(state, &merged));
            assert!(!can_reword(state, &three));
            assert!(!can_reword(state, &two));
        });
    }

    #[test]
    fn a_rebase_in_progress_blocks_rewording() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let head = state.history.head_sha.clone().unwrap();
            assert!(can_reword(state, &head));
            state.rebase.active = true;
            assert!(!can_reword(state, &head));
        });
    }

    #[test]
    fn rewording_head_and_an_older_commit_keeps_them_selected_and_is_undoable() {
        let (_scratch, mut app) = fixture();
        app.with_active(|app, state| {
            let original = log(state);
            assert_eq!(original, ["three", "two", "one"]);
            let two = sha_of(state, "two");
            assert!(reword(app, state, &two, "second"));
            assert_eq!(log(state), ["three", "second", "one"]);
            assert_eq!(
                state
                    .single_selected()
                    .map(|commit| commit.summary.as_str()),
                Some("second")
            );
            let head = state.history.head_sha.clone().unwrap();
            assert!(reword(app, state, &head, "third"));
            assert_eq!(log(state), ["third", "second", "one"]);
            assert_eq!(state.undo_stack.len(), 2);
            app.undo(state);
            app.undo(state);
            assert_eq!(log(state), original);
            // A commit that cannot be reworded says so.
            let one = sha_of(state, "one");
            assert!(!reword(app, state, &one, "uno"));
            assert_eq!(app.toasts.history()[0].kind, ToastKind::Error);
            assert_eq!(app.toasts.history()[0].title, "Edit commit message failed");
        });
    }

    #[test]
    fn dropping_and_moving_rewrite_the_chain_with_undo() {
        let (_scratch, mut app) = fixture();
        app.with_active(|app, state| {
            let two = sha_of(state, "two");
            // Drop "two".
            let (base, steps) = rewrite_plan(state, std::slice::from_ref(&two), 0).unwrap();
            assert!(rewrite(app, state, &base, &steps, "drop commit", "Dropped"));
            assert_eq!(log(state), ["three", "one"]);
            app.undo(state);
            assert_eq!(log(state), ["three", "two", "one"]);
            // Move "two" up (younger): "three" and "two" swap.
            let two = sha_of(state, "two");
            let (base, steps) = rewrite_plan(state, std::slice::from_ref(&two), 1).unwrap();
            assert!(rewrite(app, state, &base, &steps, "move commit", "Moved"));
            assert_eq!(log(state), ["two", "three", "one"]);
            // The root cannot be dropped, nor an unknown commit.
            let one = sha_of(state, "one");
            assert!(rewrite_plan(state, std::slice::from_ref(&one), 0).is_none());
            assert!(rewrite_plan(state, &["zzz".to_owned()], 0).is_none());
        });
    }

    #[test]
    fn checking_out_branches_and_tags_is_undoable() {
        let (_scratch, mut app) = fixture();
        app.with_active(|app, state| {
            let dir = state.repo.workdir().to_path_buf();
            git(&dir, &["branch", "topic", "HEAD~1"]);
            git(&dir, &["tag", "v1", "HEAD~2"]);
            state.refresh();
            let topic = state.find_ref("refs/heads/topic").unwrap().clone();
            assert!(check_out_ref(app, state, &topic));
            assert_eq!(state.history.head_branch.as_deref(), Some("topic"));
            // HEAD's own branch is a no-op that needs no undo.
            let topic = state.find_ref("refs/heads/topic").unwrap().clone();
            let undo_count = state.undo_stack.len();
            assert!(check_out_ref(app, state, &topic));
            assert_eq!(state.undo_stack.len(), undo_count);
            let tag = state.find_ref("refs/tags/v1").unwrap().clone();
            assert!(check_out_ref(app, state, &tag));
            assert!(state.history.detached);
            assert_eq!(state.history.head_sha.as_deref(), Some(tag.target.as_str()));
            app.undo(state);
            assert_eq!(state.history.head_branch.as_deref(), Some("topic"));
            app.undo(state);
            assert_eq!(state.history.head_branch.as_deref(), Some("main"));
            // A stash is not checked out and counts as done.
            let stash = Ref {
                ref_type: RefType::Stash,
                ..Ref::default()
            };
            assert!(check_out_ref(app, state, &stash));
            // A failing checkout reports false.
            let ghost = Ref {
                name: "ghost".to_owned(),
                full_name: "refs/heads/ghost".to_owned(),
                ..Ref::default()
            };
            assert!(!check_out_ref(app, state, &ghost));
        });
    }

    #[test]
    fn merging_reports_and_can_be_undone() {
        let (_scratch, mut app) = fixture();
        app.with_active(|app, state| {
            let dir = state.repo.workdir().to_path_buf();
            let one = sha_of(state, "one");
            git(&dir, &["checkout", "-b", "side", &one]);
            commit_file(&dir, "side.txt", "side");
            git(&dir, &["checkout", "main"]);
            state.refresh();
            let before = state.history.head_sha.clone();
            merge_into_head(app, state, "side", true);
            assert_ne!(state.history.head_sha, before);
            assert_eq!(app.toasts.history()[0].title, "Merged side into main");
            app.undo(state);
            assert_eq!(state.history.head_sha, before);
        });
    }

    #[test]
    fn drops_need_a_local_branch_on_one_side_and_two_commits() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let dir = state.repo.workdir().to_path_buf();
            git(&dir, &["tag", "v1", "HEAD~1"]);
            git(&dir, &["tag", "v2", "HEAD~2"]);
            state.refresh();
            let main = state.find_ref("refs/heads/main").unwrap().clone();
            let v1 = state.find_ref("refs/tags/v1").unwrap().clone();
            let v2 = state.find_ref("refs/tags/v2").unwrap().clone();
            assert!(can_drop_ref(state, &main, &v1));
            assert!(can_drop_ref(state, &v1, &main));
            // Two tags: no local branch on either side.
            assert!(!can_drop_ref(state, &v1, &v2));
            // Same commit.
            let same = Ref {
                target: main.target.clone(),
                ..v1.clone()
            };
            assert!(!can_drop_ref(state, &main, &same));
            // A stash on either side.
            let stash = Ref {
                ref_type: RefType::Stash,
                target: v2.target.clone(),
                ..Ref::default()
            };
            assert!(!can_drop_ref(state, &main, &stash));
            assert!(!can_drop_ref(state, &stash, &main));
        });
    }

    #[test]
    fn jumping_to_a_commit_selects_it_and_scrolls() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let one = sha_of(state, "one");
            let row = state.history.row_by_sha[&one];
            jump_to_commit(state, &one);
            assert_eq!(state.selected, [one]);
            assert_eq!(state.scroll_to_row, Some(row));
            state.scroll_to_row = None;
            jump_to_commit(state, "missing");
            assert_eq!(state.scroll_to_row, None);
        });
    }

    fn labels(items: &[MenuItem]) -> Vec<String> {
        items.iter().map(|item| item.label.clone()).collect()
    }

    #[test]
    fn the_chain_commits_offer_the_history_rewrites_they_allow() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let (one, two, three) = (
                sha_of(state, "one"),
                sha_of(state, "two"),
                sha_of(state, "three"),
            );
            assert_eq!(
                labels(&rebase_group(state, &three, false)),
                ["Drop commit", "Move commit down"]
            );
            assert_eq!(
                labels(&rebase_group(state, &two, false)),
                [
                    format!("Interactive Rebase 1 children of {}", &two[..6]),
                    "Edit commit message".to_owned(),
                    "Drop commit".to_owned(),
                    "Move commit up".to_owned(),
                ]
            );
            // The root can be neither reworded, dropped nor moved.
            assert_eq!(
                labels(&rebase_group(state, &one, false)),
                [format!("Interactive Rebase 2 children of {}", &one[..6])]
            );
            // A contiguous selection on the chain is squashed, dropped or moved as one.
            state.select(&three, false, false);
            state.select(&two, true, false);
            assert_eq!(
                labels(&rebase_group(state, &two, true)),
                [
                    format!("Interactive Rebase 1 children of {}", &two[..6]),
                    "Squash 2 commits".to_owned(),
                    "Drop 2 commits".to_owned(),
                ]
            );
            // An unrelated commit has none.
            assert!(rebase_group(state, "nonsense", false).is_empty());
        });
    }

    #[test]
    fn merging_menus_follow_how_the_branches_relate() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let dir = state.repo.workdir().to_path_buf();
            git(&dir, &["branch", "behind", "HEAD~1"]);
            let one = sha_of(state, "one");
            git(&dir, &["checkout", "-q", "-b", "side", &one]);
            commit_file(&dir, "side.txt", "side");
            git(&dir, &["checkout", "-q", "main"]);
            state.refresh();
            let behind = state.find_ref("refs/heads/behind").unwrap().clone();
            assert_eq!(
                labels(&merge_and_rebase_items(state, &behind)),
                [
                    "Fast-forward behind to main",
                    "Merge main into behind",
                    "Rebase main onto behind",
                    "Interactive Rebase main onto behind",
                ]
            );
            let side = state.find_ref("refs/heads/side").unwrap().clone();
            assert_eq!(
                labels(&merge_and_rebase_items(state, &side)),
                ["Merge side into main", "Rebase main onto side"]
            );
        });
    }

    #[test]
    fn deleting_offers_each_branch_and_the_pair() {
        let named = |name: &str, remote: &str| Ref {
            ref_type: if remote.is_empty() {
                RefType::LocalBranch
            } else {
                RefType::RemoteBranch
            },
            name: name.to_owned(),
            remote: remote.to_owned(),
            ..Ref::default()
        };
        let local = named("topic", "");
        let origin = named("origin/topic", "origin");
        let fork = named("fork/topic", "fork");
        let path = Path::new(".");
        assert_eq!(
            labels(&delete_items(path, std::slice::from_ref(&local))),
            ["Delete topic"]
        );
        assert_eq!(
            labels(&delete_items(path, &[local.clone(), origin.clone()])),
            [
                "Delete topic",
                "Delete origin/topic",
                "Delete topic and origin/topic"
            ]
        );
        assert_eq!(
            labels(&delete_items(path, &[local, origin, fork]))
                .last()
                .map(String::as_str),
            Some("Delete all \"topic\" branches (local && remote)")
        );
    }

    #[test]
    fn checkout_and_worktree_entries_fold_into_submenus() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let dir = state.repo.workdir().to_path_buf();
            git(&dir, &["branch", "alt"]);
            state.refresh();
            let main = state.find_ref("refs/heads/main").unwrap().clone();
            let alt = state.find_ref("refs/heads/alt").unwrap().clone();
            let sha = main.target.clone();
            // HEAD's branch is not offered: "alt" and "this commit" fold into one submenu.
            let items = checkout_entries(&dir, state, &[main.clone(), alt.clone()], &sha, 1);
            assert_eq!(labels(&items), ["Checkout"]);
            assert_eq!(labels(&items[0].submenu), ["alt", "this commit"]);
            // Alone, the one entry gets a verb; in the left panel there is no "this commit".
            let items = checkout_entries(&dir, state, &[main.clone(), alt.clone()], &sha, 0);
            assert_eq!(labels(&items), ["Checkout alt"]);
            let items = worktree_entries(&[main.clone(), alt], &sha, 1);
            assert_eq!(labels(&items), ["Create worktree from"]);
            assert_eq!(labels(&items[0].submenu), ["alt", "this commit"]);
            let items = worktree_entries(&[main], &sha, 1);
            assert_eq!(labels(&items), ["Create worktree from this commit"]);
        });
    }

    #[test]
    fn stashes_and_tags_have_their_own_menus() {
        let (_scratch, mut app) = fixture();
        app.with_active(|_app, state| {
            let dir = state.repo.workdir().to_path_buf();
            std::fs::write(dir.join("one.txt"), "changed\n").unwrap();
            git(&dir, &["stash"]);
            git(&dir, &["tag", "v1", "HEAD~1"]);
            git(&dir, &["remote", "add", "origin", "../elsewhere.git"]);
            state.refresh();
            let stash = state.find_ref("refs/stash").unwrap().clone();
            assert_eq!(
                labels(
                    &stash_menu(state, &stash)
                        .into_iter()
                        .filter(|i| i.kind != crate::ui::menu::MenuItemKind::Separator)
                        .collect::<Vec<_>>()
                ),
                [
                    "Apply Stash",
                    "Pop Stash",
                    "Delete Stash",
                    "Edit stash message",
                    "Hide"
                ]
            );
            let tag = state.find_ref("refs/tags/v1").unwrap().clone();
            let in_graph = labels(&tag_menu(state, &tag, false));
            assert_eq!(in_graph[0], "Push v1 to origin");
            assert!(in_graph.contains(&"Checkout this commit".to_owned()));
            assert!(in_graph.contains(&"Delete v1 locally".to_owned()));
            assert!(in_graph.contains(&"Annotate v1".to_owned()));
            let in_panel = labels(&tag_menu(state, &tag, true));
            assert!(in_panel.contains(&"Checkout the commit at v1".to_owned()));
            assert!(in_panel.contains(&"Hide".to_owned()));
        });
    }
}
