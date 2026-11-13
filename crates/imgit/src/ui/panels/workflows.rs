//! The workflows: branch cleanup and Gitflow.
//!
//! Branch cleanup deletes the local branches merged into the default branch or whose upstream is gone, after a
//! review; Gitflow starts and finishes features, releases and hotfixes. Both run git through the application
//! (libgit2 for the branch deletions, the git CLI for the gitflow merges, so a conflict leaves the repository in
//! git's usual merge state). The pure logic (which branches qualify, which commands run) is in
//! [`imgit_app::workflows`].

use std::time::{Duration, Instant};

use dear_imgui_rs::{ChildFlags, Key, StyleColor, StyleVar};
use imgit_app::RepoState;
use imgit_app::repo_state::UndoEntry;
use imgit_app::workflows::cleanup::{self, BranchFacts};
use imgit_app::workflows::gitflow::{self, Branch, Kind};
use imgit_git::{FileStatus, Ref, RefType, Repository, Sha};

use crate::app::application::Application;
use crate::app::dialog::{Check, DialogKind};
use crate::app::toasts::ToastKind;
use crate::ui::Cx;
use crate::ui::geom::vec2;
use crate::ui::menu::{MenuItems, menu_item};
use crate::ui::panels::tabs_bar::open_text_prompt;

#[derive(Default)]
pub struct State {}

/// How long the repository's `gitflow.*` configuration is trusted before it is read again.
const CONFIG_MAX_AGE: Duration = Duration::from_secs(1);

/// The repository's `gitflow.*` configuration, read again at most once a second (the left panel and the menus
/// ask for it every frame).
fn gitflow_config(state: &RepoState) -> gitflow::Config {
    let mut cache = state.gitflow_config_cache.borrow_mut();
    if let Some((read_at, config)) = cache.as_ref()
        && read_at.elapsed() <= CONFIG_MAX_AGE
    {
        return config.clone();
    }
    let value = |key: &str, fallback: &str| {
        state
            .repo
            .config_string(key)
            .unwrap_or_else(|| fallback.to_owned())
    };
    let config = gitflow::Config {
        master: state
            .repo
            .config_string("gitflow.branch.master")
            .unwrap_or_default(),
        develop: state
            .repo
            .config_string("gitflow.branch.develop")
            .unwrap_or_default(),
        feature_prefix: value("gitflow.prefix.feature", "feature/"),
        release_prefix: value("gitflow.prefix.release", "release/"),
        hotfix_prefix: value("gitflow.prefix.hotfix", "hotfix/"),
        version_tag: state
            .repo
            .config_string("gitflow.prefix.versiontag")
            .unwrap_or_default(),
    };
    *cache = Some((Instant::now(), config.clone()));
    config
}

fn has_branch(state: &RepoState, name: &str) -> bool {
    state.find_ref(&format!("refs/heads/{name}")).is_some()
}

/// Gitflow merges and checkouts need a working directory without changes (untracked files aside); a toast says
/// so when there are.
fn working_directory_clean(app: &mut Application, state: &RepoState, what: &str) -> bool {
    let status = &state.status;
    let dirty = !status.staged.is_empty()
        || !status.conflicted.is_empty()
        || status
            .unstaged
            .iter()
            .any(|change| change.status != FileStatus::Untracked);
    if dirty {
        app.toasts.push_with_detail(
            ToastKind::Error,
            format!("{what} failed"),
            "The working directory has changes. Commit or stash them first.",
        );
    }
    !dirty
}

/// Asks for the name of the new branch of `kind`, then starts it.
fn start(app: &mut Application, state: &mut RepoState, kind: Kind) {
    let config = gitflow_config(state);
    let title = format!("Start {kind}");
    let base = config.base_branch(kind);
    if !has_branch(state, base) {
        app.toasts.push_with_detail(
            ToastKind::Error,
            format!("{title} failed"),
            format!("There is no local branch '{base}'."),
        );
        return;
    }
    let workdir = state.repo.workdir().to_path_buf();
    open_text_prompt(
        app,
        format!("Name of the new {kind}:"),
        "",
        false,
        move |_cx, app, text| {
            let name = text.trim_matches(' ');
            if name.is_empty() {
                return;
            }
            app.with_repo(&workdir, |app, tab| {
                let branch = config.branch_name(kind, name);
                if has_branch(tab, &branch) {
                    app.toasts.push_with_detail(
                        ToastKind::Error,
                        format!("{title} failed"),
                        format!("The branch '{branch}' exists already."),
                    );
                    return;
                }
                if !working_directory_clean(app, tab, &title) {
                    return;
                }
                app.run_git_cli_sequence(
                    tab,
                    &format!("{title} {name}"),
                    config.start_runs(kind, name),
                );
            });
        },
    );
}

/// The question the Finish dialog asks.
fn finish_message(
    config: &gitflow::Config,
    branch: &Branch,
    full: &str,
    tag: &str,
    tag_exists: bool,
) -> String {
    let mut message = if branch.kind == Kind::Feature {
        format!("Merge '{full}' into '{}' and delete it?", config.develop)
    } else {
        format!(
            "Merge '{full}' into '{}', {}merge it into '{}' and delete it?",
            config.master,
            if tag_exists {
                String::new()
            } else {
                format!("tag it '{tag}', ")
            },
            config.develop
        )
    };
    message.push_str(
        " A merge conflict stops the finish there; resolve and commit it, then finish again.",
    );
    message
}

/// Asks for confirmation, then merges the gitflow `branch` back and deletes it.
fn finish(app: &mut Application, state: &mut RepoState, branch: &Branch) {
    let config = gitflow_config(state);
    let kind = branch.kind;
    let full = config.branch_name(kind, &branch.name);
    let title = format!("Finish {kind} {}", branch.name);
    for target in [&config.master, &config.develop] {
        if !has_branch(state, target) {
            app.toasts.push_with_detail(
                ToastKind::Error,
                format!("{title} failed"),
                format!("There is no local branch '{target}'."),
            );
            return;
        }
    }
    if !working_directory_clean(app, state, &title) {
        return;
    }
    let tag = config.tag_name(&branch.name);
    let tag_exists = state.find_ref(&format!("refs/tags/{tag}")).is_some();
    let message = finish_message(&config, branch, &full, &tag, tag_exists);
    let workdir = state.repo.workdir().to_path_buf();
    app.dialog.open(DialogKind::Confirm);
    app.dialog.workdir = Some(workdir.clone());
    app.dialog.title = format!("Finish {kind}");
    app.dialog.message = message;
    app.dialog.confirm_label = format!("Finish {}", kind.title());
    let branch = branch.clone();
    app.dialog.on_confirm = Some(Box::new(move |_cx, app| {
        app.with_repo(&workdir, |app, tab| {
            app.run_git_cli_sequence(
                tab,
                &title,
                config.finish_runs(branch.kind, &branch.name, tag_exists),
            );
        });
    }));
}

// ---------------------------------------------------------------------------------------------------------
// Gitflow

/// Whether Initialize Gitflow has set the repository up (`gitflow.branch.master` is set).
#[must_use]
pub fn gitflow_initialized(state: &RepoState) -> bool {
    gitflow_config(state).initialized()
}

/// Start Feature / Release / Hotfix, each asking for the name in the prompt bar.
#[must_use]
pub fn gitflow_start_items(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
) -> MenuItems {
    if !gitflow_initialized(state) {
        return MenuItems::new();
    }
    let workdir = state.repo.workdir().to_path_buf();
    Kind::ALL
        .into_iter()
        .map(|kind| {
            let workdir = workdir.clone();
            menu_item(format!("Start {}", kind.title()), move |_cx, app| {
                app.with_repo(&workdir, |app, state| start(app, state, kind));
            })
        })
        .collect()
}

/// "Finish feature <name>" when `branch` is a gitflow branch (empty otherwise).
#[must_use]
pub fn gitflow_finish_items(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    branch: &str,
) -> MenuItems {
    let Some(flow) = gitflow_config(state).classify(branch) else {
        return MenuItems::new();
    };
    let workdir = state.repo.workdir().to_path_buf();
    vec![menu_item(
        format!("Finish {} {}", flow.kind, flow.name),
        move |_cx, app| {
            app.with_repo(&workdir, |app, state| finish(app, state, &flow));
        },
    )]
}

/// The local gitflow branches (feature/..., release/..., hotfix/...), sorted.
#[must_use]
pub fn gitflow_branches(state: &RepoState) -> Vec<&Ref> {
    let config = gitflow_config(state);
    let mut branches: Vec<&Ref> = state
        .history
        .refs
        .iter()
        .filter(|r| r.ref_type == RefType::LocalBranch && config.classify(&r.name).is_some())
        .collect();
    branches.sort_by(|a, b| a.name.cmp(&b.name));
    branches
}

// ---------------------------------------------------------------------------------------------------------
// Branch cleanup

/// Branch cleanup: the review list of the local branches merged into the default branch or whose upstream is
/// gone (never the checked-out, default, or gitflow main / develop branch); the ticked ones are deleted.
/// Opens `app.dialog` as `DialogKind::BranchCleanup`.
pub fn open_branch_cleanup(_cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let remote = state
        .history
        .remotes
        .first()
        .map_or("origin", |remote| remote.name.as_str());
    let default_branch = state.repo.default_branch(remote).unwrap_or_default();
    let default_ref = (!default_branch.is_empty())
        .then(|| state.find_ref(&format!("refs/heads/{default_branch}")))
        .flatten();
    let branches: Vec<BranchFacts> = state
        .history
        .refs
        .iter()
        .filter(|r| r.ref_type == RefType::LocalBranch)
        .map(|r| BranchFacts {
            name: r.name.clone(),
            is_head: r.is_head || state.repo.branch_checked_out(&r.name),
            merged: default_ref.is_some_and(|default| {
                r.target == default.target || state.repo.is_ancestor(&r.target, &default.target)
            }),
            gone: state.repo.upstream_gone(&r.name),
        })
        .collect();
    let mut protected_branches = vec![default_branch.clone()];
    let config = gitflow_config(state);
    if config.initialized() {
        protected_branches.push(config.master);
        protected_branches.push(config.develop);
    }
    let found = cleanup::candidates(&branches, &protected_branches);
    if found.is_empty() {
        app.toasts.push_with_detail(
            ToastKind::Info,
            "No branches to clean up",
            if default_branch.is_empty() {
                "No local branch has lost its upstream.".to_owned()
            } else {
                format!("No local branch is merged into {default_branch} or has lost its upstream.")
            },
        );
        return;
    }
    app.dialog.open(DialogKind::BranchCleanup);
    app.dialog.workdir = Some(state.repo.workdir().to_path_buf());
    app.dialog.text_a = default_branch;
    app.dialog.checks = found
        .iter()
        .map(|branch| Check::new(branch.name.as_str(), branch.reason()))
        .collect();
}

/// "Delete 1 Branch" / "Delete 3 Branches": the confirm button's label.
fn delete_label(ticked: usize) -> String {
    if ticked == 1 {
        "Delete 1 Branch".to_owned()
    } else {
        format!("Delete {ticked} Branches")
    }
}

/// The toast after a successful cleanup: "Deleted 1 branch" / "Deleted 3 branches".
fn deleted_message(count: usize) -> String {
    if count == 1 {
        "Deleted 1 branch".to_owned()
    } else {
        format!("Deleted {count} branches")
    }
}

/// The intro line above the review list.
fn intro(default_branch: &str) -> String {
    let reason = if default_branch.is_empty() {
        "These local branches have lost their upstream.".to_owned()
    } else {
        format!(
            "These local branches are merged into {default_branch} or have lost their upstream."
        )
    };
    format!("{reason} Untick the ones to keep.")
}

/// Puts the deleted branches back at their tips (undo), or deletes them again (redo).
fn restore_branches(repo: &Repository, deleted: &[(String, Sha)]) -> imgit_git::Result<()> {
    deleted
        .iter()
        .try_for_each(|(name, sha)| repo.set_branch_target(name, sha))
}

fn delete_branches(repo: &Repository, deleted: &[(String, Sha)]) -> imgit_git::Result<()> {
    deleted
        .iter()
        .try_for_each(|(name, _)| repo.delete_branch(name))
}

/// The `BranchCleanup` dialog's contents (`app.dialog`), inside its modal. The C++ also took the dialog; here
/// it is `app.dialog`.
pub fn draw_branch_cleanup(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    let p = cx.palette;
    {
        let _wrap = ui.push_text_wrap_pos(0.0);
        ui.text(intro(&app.dialog.text_a));
    }
    ui.dummy([0.0, cx.px(6.0)]);
    if cx.link_text("Select all") {
        app.dialog.checks.iter_mut().for_each(|c| c.checked = true);
    }
    ui.same_line_with_spacing(0.0, cx.px(16.0));
    if cx.link_text("Select none") {
        app.dialog.checks.iter_mut().for_each(|c| c.checked = false);
    }
    ui.dummy([0.0, cx.px(4.0)]);
    let row_h = ui.frame_height_with_spacing();
    let list_h = cx
        .px(280.0)
        .min(row_h * app.dialog.checks.len() as f32 + cx.px(12.0));
    {
        let _background = ui.push_style_color(StyleColor::ChildBg, p.input_bg);
        ui.child_window("##cleanup_list")
            .size([-1.0, list_h])
            .child_flags(ChildFlags::BORDERS)
            .build(ui, || {
                for (index, check) in app.dialog.checks.iter_mut().enumerate() {
                    let _id = ui.push_id(index);
                    cx.checkbox(&check.label, &mut check.checked);
                    ui.same_line_with_spacing(0.0, cx.px(10.0));
                    // On the label's baseline: checkbox() frames its box with 2px padding.
                    {
                        let _padding =
                            ui.push_style_var(StyleVar::FramePadding([cx.px(2.0), cx.px(2.0)]));
                        ui.align_text_to_frame_padding();
                    }
                    ui.text_colored(p.text_secondary.to_array(), &check.note);
                }
            });
    }

    let ticked = app.dialog.checks.iter().filter(|c| c.checked).count();
    let label = delete_label(ticked);
    ui.dummy([0.0, cx.px(8.0)]);
    let confirm_w = cx.px(150.0);
    let cancel_w = cx.px(110.0);
    // GetContentRegionMax().x: the right edge of the content in window coordinates.
    let content_max_x = ui.cursor_pos_x() + cx.avail().x;
    // SAFETY: `ui` proves a frame is open, so the style exists; one float is read.
    let spacing_x = unsafe { (*dear_imgui_rs::sys::igGetStyle()).ItemSpacing.x };
    ui.set_cursor_pos_x(content_max_x - confirm_w - cancel_w - spacing_x);
    let cancel = cx.default_button("Cancel", vec2(cancel_w, cx.px(32.0)), true);
    ui.same_line();
    let confirm = cx.danger_button(&label, vec2(confirm_w, cx.px(32.0)), ticked > 0);
    if cancel || ui.is_key_pressed(Key::Escape) {
        ui.close_current_popup();
        return;
    }
    if !confirm {
        return;
    }

    // Each branch deleted with its tip remembered: one undo entry puts them all back.
    let mut deleted: Vec<(String, Sha)> = Vec::new();
    let mut error = None;
    for check in app.dialog.checks.iter().filter(|c| c.checked) {
        let Some(target) = state
            .find_ref(&format!("refs/heads/{}", check.label))
            .map(|r| r.target.clone())
        else {
            continue;
        };
        if let Err(e) = state.repo.delete_branch(&check.label) {
            error = Some(format!("{}: {e}", check.label));
            break;
        }
        deleted.push((check.label.clone(), target));
    }
    let count = deleted.len();
    if !deleted.is_empty() {
        let label = match deleted.as_slice() {
            [(name, _)] => format!("delete branch {name}"),
            all => format!("delete {} branches", all.len()),
        };
        let for_redo = deleted.clone();
        state.push_undo(
            UndoEntry::new(label, move |repo| restore_branches(repo, &deleted))
                .with_redo(move |repo| delete_branches(repo, &for_redo)),
        );
    }
    match error {
        Some(error) => {
            app.toasts
                .push_with_detail(ToastKind::Error, "Branch cleanup failed", error);
        }
        None => app.toasts.push(ToastKind::Success, deleted_message(count)),
    }
    state.refresh();
    ui.close_current_popup();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_count_the_branches() {
        assert_eq!(delete_label(0), "Delete 0 Branches");
        assert_eq!(delete_label(1), "Delete 1 Branch");
        assert_eq!(delete_label(4), "Delete 4 Branches");
        assert_eq!(deleted_message(1), "Deleted 1 branch");
        assert_eq!(deleted_message(2), "Deleted 2 branches");
    }

    #[test]
    fn the_intro_names_the_default_branch() {
        assert_eq!(
            intro("main"),
            "These local branches are merged into main or have lost their upstream. Untick the ones to keep."
        );
        assert_eq!(
            intro(""),
            "These local branches have lost their upstream. Untick the ones to keep."
        );
    }

    fn config() -> gitflow::Config {
        gitflow::Config {
            master: "main".to_owned(),
            develop: "develop".to_owned(),
            version_tag: "v".to_owned(),
            ..gitflow::Config::default()
        }
    }

    #[test]
    fn the_finish_question_depends_on_the_kind() {
        let feature = Branch {
            kind: Kind::Feature,
            name: "x".to_owned(),
        };
        let message = finish_message(&config(), &feature, "feature/x", "vx", false);
        assert!(message.starts_with("Merge 'feature/x' into 'develop' and delete it?"));
        let release = Branch {
            kind: Kind::Release,
            name: "1.0".to_owned(),
        };
        let message = finish_message(&config(), &release, "release/1.0", "v1.0", false);
        assert!(message.starts_with(
            "Merge 'release/1.0' into 'main', tag it 'v1.0', merge it into 'develop' and delete it?"
        ));
        let message = finish_message(&config(), &release, "release/1.0", "v1.0", true);
        assert!(message.starts_with(
            "Merge 'release/1.0' into 'main', merge it into 'develop' and delete it?"
        ));
        assert!(message.ends_with("then finish again."));
    }
}
