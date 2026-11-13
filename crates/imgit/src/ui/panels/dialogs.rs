//! The modal dialogs (`app.dialog`), the credentials prompt and the slide-out panels.
//!
//! * the **prompt bar** (`prompt_bar`): confirmations, names, the upstream of a pushed branch, credentials;
//! * the **modals** (`modal`): clone, init, create branch / tag / worktree, remotes, reword, squash, upstream,
//!   keyboard shortcuts, about, the branch clean-up;
//! * the **slide panel** (`slide_panel`): Add / Edit Submodule.

mod modal;
mod prompt_bar;
mod slide_panel;

use std::path::Path;

use dear_imgui_rs::{StyleColor, StyleVar, WindowFlags};
use imgit_app::RepoState;
use imgit_app::repo_state::{InlineRef, UndoEntry};
use imgit_git::cli::Credential;
use imgit_git::repository::short_sha;
use imgit_git::{Error, ResetMode, Sha};

use crate::app::application::Application;
use crate::app::dialog::{Dialog, DialogKind};
use crate::app::toasts::ToastKind;
use crate::ui::{Cx, Vec2, raw, vec2};
use prompt_bar::Answer;

/// What the dialogs remember between frames (the C++ function-local statics).
pub struct State {
    /// The set-upstream prompt's remote select has the keyboard focus (react-select's ring and caret); it has
    /// it when the prompt opens.
    push_select_focused: bool,
    /// The name prompt puts the caret behind the name when it opens.
    rename_caret_to_end: bool,
    slide: slide_panel::SlideState,
}

impl Default for State {
    fn default() -> Self {
        Self {
            push_select_focused: true,
            rename_caret_to_end: false,
            slide: slide_panel::SlideState::default(),
        }
    }
}

/// The dialog that is open (`app.dialog`), the profile dialog and the credentials prompt, drawn after the
/// main window.
pub fn draw_dialogs(cx: &mut Cx<'_>, app: &mut Application) {
    draw_credentials(cx, app);
    let Some(kind) = app.dialog.kind else {
        return;
    };
    // The dialog acts on the tab it was opened for (the active one on its first frame), whichever tab is
    // active when it is answered, and closes when that tab is gone.
    if app.dialog.workdir.is_none() {
        app.dialog.workdir = app.active_workdir().map(Path::to_path_buf);
    }
    let needs_repo = !matches!(
        kind,
        DialogKind::CloneRepo
            | DialogKind::InitRepo
            | DialogKind::Preferences
            | DialogKind::KeyboardShortcuts
            | DialogKind::About
            | DialogKind::Confirm
            | DialogKind::TextPrompt
    );
    // A confirmation's action may be repository-free, but one opened from a tab must not outlive it (its action
    // may refer to that tab).
    let repository_gone = !dialog_repo_open(app);
    if repository_gone
        && (needs_repo || (kind == DialogKind::Confirm && app.dialog.workdir.is_some()))
    {
        app.dialog.close();
        return;
    }
    match kind {
        // Adding or editing a submodule is the panel sliding out of the left panel (`draw_slide_panel`).
        DialogKind::AddSubmodule | DialogKind::EditSubmodule => {}
        DialogKind::PushNewBranch => push_prompt(cx, app),
        DialogKind::TextPrompt => text_prompt(cx, app),
        DialogKind::CreateFile => create_file_prompt(cx, app),
        DialogKind::RenameBranch => rename_branch_prompt(cx, app),
        DialogKind::EditStashMessage => stash_message_prompt(cx, app),
        DialogKind::Confirm
        | DialogKind::DiscardAll
        | DialogKind::DeleteBranch
        | DialogKind::DeleteTag
        | DialogKind::DeleteStash => confirm_prompt(cx, app),
        DialogKind::Reset => reset_now(cx, app),
        _ => {
            if !start_inline_ref(app) {
                draw_modal(cx, app, kind);
            }
        }
    }
}

/// The Add / Edit Submodule panel sliding out of the left panel at `origin`, over the graph (`max_width` wide)
/// (nothing while closed).
pub fn draw_slide_panel(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    origin: Vec2,
    height: f32,
    max_width: f32,
) {
    // The panel's state is taken out of `app` while it draws, since it takes the application too.
    let mut slide = std::mem::take(&mut app.panels.dialogs.slide);
    slide_panel::draw(cx, app, state, &mut slide, origin, height, max_width);
    app.panels.dialogs.slide = slide;
}

/// Creates a branch at `sha` (checking it out when `checkout`) with an undo entry.
pub fn create_branch_at(
    _cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    name: &str,
    sha: &str,
    checkout: bool,
) {
    let previous_branch = if state.history.detached {
        None
    } else {
        state.history.head_branch.clone()
    };
    let previous_sha = state.history.head_sha.clone();
    let result = state.repo.create_branch(name, sha, checkout);
    if app
        .run(
            state,
            "Create branch",
            result,
            Some(&format!("Created branch {name}")),
        )
        .is_none()
    {
        return;
    }
    let (undo_name, redo_name, sha) = (name.to_owned(), name.to_owned(), sha.to_owned());
    state.push_undo(
        UndoEntry::new(format!("create branch {name}"), move |repo| {
            if checkout {
                match (&previous_branch, &previous_sha) {
                    (Some(branch), _) => repo.checkout_branch(branch)?,
                    (None, Some(sha)) => repo.checkout_commit(sha)?,
                    (None, None) => return Err(Error::Message("nothing to restore".to_owned())),
                }
            }
            repo.delete_branch(&undo_name)
        })
        .with_redo(move |repo| repo.create_branch(&redo_name, &sha, checkout)),
    );
}

/// Creates a lightweight (empty `message`) or annotated tag at `sha` with an undo entry.
pub fn create_tag_at(
    _cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    name: &str,
    sha: &str,
    message: &str,
) {
    // Sign Tags by Default (tag.gpgSign): git signs it, an annotated tag (the message, else its name).
    if state.repo.config_bool("tag.gpgSign", false) {
        let message = if message.is_empty() { name } else { message };
        app.run_git_cli(
            state,
            "Create signed tag",
            ["tag", "-s", name, sha, "-m", message],
        );
        return;
    }
    let note = (!message.is_empty()).then_some(message);
    let result = state.repo.create_tag(name, sha, note);
    if app
        .run(
            state,
            "Create tag",
            result,
            Some(&format!("Created tag {name}")),
        )
        .is_none()
    {
        return;
    }
    let (undo_name, redo_name, sha, message) = (
        name.to_owned(),
        name.to_owned(),
        sha.to_owned(),
        message.to_owned(),
    );
    state.push_undo(
        UndoEntry::new(format!("create tag {name}"), move |repo| {
            repo.delete_tag(&undo_name)
        })
        .with_redo(move |repo| {
            repo.create_tag(
                &redo_name,
                &sha,
                (!message.is_empty()).then_some(message.as_str()),
            )
        }),
    );
}

/// Undo for a history rewrite: a hard reset to the previous tip, with the uncommitted changes present at undo
/// time stashed around it so they survive (as the menus' tip restore does). Built before the rewrite, while the
/// history still holds the tip.
fn tip_restore_entry(state: &RepoState, label: &str) -> UndoEntry {
    let tip: Option<Sha> = state.history.head_sha.clone();
    UndoEntry::new(label, move |repo| {
        let Some(sha) = &tip else {
            return Err(Error::Message("nothing to restore".to_owned()));
        };
        let changes = repo.load_status().map_or(0, |status| status.count());
        let stashed = changes > 0 && repo.stash_push("ImGit: auto stash before undo").is_ok();
        let result = repo.reset(sha, ResetMode::Hard);
        if stashed {
            let _ = repo.stash_pop(0);
        }
        result
    })
    .with_redo_mode(ResetMode::Hard)
}

/// Whether the repository the dialog was opened for is open (the active tab's while it is not recorded yet).
fn dialog_repo_open(app: &Application) -> bool {
    match &app.dialog.workdir {
        None => app.active().is_some(),
        Some(workdir) => app.find_repo_state(workdir).is_some(),
    }
}

/// The state of the repository the dialog was opened for.
fn dialog_state(app: &Application) -> Option<&RepoState> {
    match &app.dialog.workdir {
        None => app.active(),
        Some(workdir) => app.find_repo_state(workdir),
    }
}

/// Runs `f` with the repository at `workdir` (the active tab's when `None`) taken out of its tab.
fn with_dialog_repo(
    app: &mut Application,
    workdir: Option<&Path>,
    f: impl FnOnce(&mut Application, &mut RepoState),
) {
    match workdir {
        Some(workdir) => {
            app.with_repo(workdir, f);
        }
        None => {
            app.with_active(f);
        }
    }
}

fn draw_credentials(cx: &mut Cx<'_>, app: &mut Application) {
    let Some(prompt) = app.credential_prompt.as_mut() else {
        return;
    };
    let answer = prompt_bar::draw_credentials_prompt(cx, prompt);
    if answer == Answer::Pending {
        return;
    }
    let Some(mut prompt) = app.credential_prompt.take() else {
        return;
    };
    if answer == Answer::Yes
        && let Some(retry) = prompt.retry.take()
    {
        retry(
            app,
            Credential {
                url: prompt.url,
                username: prompt.username,
                password: prompt.password,
                remember: prompt.remember,
            },
        );
    }
}

/// Pushing a branch without an upstream asks for the remote branch in the prompt bar.
fn push_prompt(cx: &mut Cx<'_>, app: &mut Application) {
    let remotes = dialog_state(app)
        .map(|state| state.history.remotes.clone())
        .unwrap_or_default();
    let panel = &mut app.panels.dialogs;
    let answer = prompt_bar::draw_push_prompt(
        cx,
        &mut app.dialog,
        &mut panel.push_select_focused,
        &remotes,
    );
    if answer == Answer::Pending {
        return;
    }
    let request = std::mem::take(&mut app.dialog);
    if answer == Answer::Yes && !request.text_a.is_empty() {
        let target = if request.text_b.is_empty() {
            &request.ref_name
        } else {
            &request.text_b
        };
        let refspec = format!("{}:{target}", request.ref_name);
        with_dialog_repo(app, request.workdir.as_deref(), |app, state| {
            app.run_git_cli(
                state,
                "Push",
                ["push", "--set-upstream", &request.text_a, &refspec],
            );
        });
    }
}

/// Draws the name prompt for the open dialog and, once it is answered, takes the dialog (closing it).
fn answered_name_prompt(cx: &mut Cx<'_>, app: &mut Application) -> Option<(Answer, Dialog)> {
    let panel = &mut app.panels.dialogs;
    let answer =
        prompt_bar::draw_rename_prompt(cx, &mut app.dialog, &mut panel.rename_caret_to_end);
    (answer != Answer::Pending).then(|| (answer, std::mem::take(&mut app.dialog)))
}

/// Tab, tab group and repository names are asked for in the same prompt bar.
fn text_prompt(cx: &mut Cx<'_>, app: &mut Application) {
    let Some((answer, request)) = answered_name_prompt(cx, app) else {
        return;
    };
    if answer == Answer::Yes
        && let Some(on_text) = request.on_text
    {
        on_text(cx, app, &request.text_a);
    }
}

/// The path of a file to create, as the components below the working directory: `None` when it would lie
/// outside of it (absolute, on another drive, above it) or name no file (empty, ends with a separator).
pub fn new_file_components(folder: &str, name: &str) -> Option<Vec<String>> {
    let path = if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    };
    let path = path.replace('\\', "/");
    if path.starts_with('/') || path.ends_with('/') {
        return None;
    }
    // "C:..." names a drive.
    let mut chars = path.chars();
    if let (Some(first), Some(':')) = (chars.next(), chars.next())
        && first.is_ascii_alphabetic()
    {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part.to_owned()),
        }
    }
    (!parts.is_empty()
        && path
            .rsplit('/')
            .next()
            .is_some_and(|last| last != "." && last != ".."))
    .then_some(parts)
}

/// Creating a file asks for its name in the prompt bar, then opens it in the file view's editor.
fn create_file_prompt(cx: &mut Cx<'_>, app: &mut Application) {
    let Some((answer, request)) = answered_name_prompt(cx, app) else {
        return;
    };
    if answer == Answer::No || request.text_a.is_empty() {
        return;
    }
    let shown = if request.text_b.is_empty() {
        request.text_a.clone()
    } else {
        format!("{}/{}", request.text_b, request.text_a)
    };
    let title = format!("Could not create {}", shown.replace('\\', "/"));
    let Some(parts) = new_file_components(&request.text_b, &request.text_a) else {
        app.toasts.push_with_detail(
            ToastKind::Error,
            title,
            "The file must be inside the repository.",
        );
        return;
    };
    with_dialog_repo(app, request.workdir.as_deref(), |app, state| {
        let relative = parts.join("/");
        let full = parts
            .iter()
            .fold(state.repo.workdir().to_path_buf(), |path, part| {
                path.join(part)
            });
        if full.exists() {
            app.toasts
                .push_with_detail(ToastKind::Error, title, "A file with that name exists.");
            return;
        }
        if let Some(parent) = full.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::File::create(&full).is_err() {
            app.toasts
                .push_with_detail(ToastKind::Error, title, "The file could not be written.");
            return;
        }
        state.refresh_status();
        crate::ui::panels::open_file_editor(cx, app, state, &relative);
    });
}

/// Renaming a branch asks for the new name in the prompt bar.
fn rename_branch_prompt(cx: &mut Cx<'_>, app: &mut Application) {
    let Some((answer, request)) = answered_name_prompt(cx, app) else {
        return;
    };
    if answer == Answer::No || request.text_a == request.ref_name {
        return;
    }
    let (from, to) = (request.ref_name.clone(), request.text_a.clone());
    with_dialog_repo(app, request.workdir.as_deref(), |app, state| {
        let result = state.repo.rename_branch(&from, &to);
        let done = format!("Renamed {from} to {to}");
        if app
            .run(state, "Rename branch", result, Some(&done))
            .is_some()
        {
            let (undo_from, undo_to) = (from.clone(), to.clone());
            state.push_undo(
                UndoEntry::new(format!("rename {from}"), move |repo| {
                    repo.rename_branch(&undo_to, &undo_from)
                })
                .with_redo(move |repo| repo.rename_branch(&from, &to)),
            );
        }
    });
}

/// Editing a stash's message asks for the new one in the same prompt bar.
fn stash_message_prompt(cx: &mut Cx<'_>, app: &mut Application) {
    let Some((answer, request)) = answered_name_prompt(cx, app) else {
        return;
    };
    if answer == Answer::No {
        return;
    }
    with_dialog_repo(app, request.workdir.as_deref(), |app, state| {
        let result = state.repo.stash_set_message(&request.sha, &request.text_a);
        app.run(
            state,
            "Edit stash message",
            result,
            Some("Stash message updated"),
        );
    });
}

fn in_quotes(text: &str) -> String {
    format!("\"{text}\"")
}

/// What the confirmation prompt for `d` says, its confirm button's label and whether the action is a
/// destructive one (the button is red).
fn confirm_texts(d: &Dialog) -> (String, String, bool) {
    const DESTRUCTIVE: &str = "This is a destructive operation, are you sure you want to delete";
    match d.kind {
        Some(DialogKind::Confirm) => (
            d.message.clone(),
            if d.confirm_label.is_empty() {
                "OK".to_owned()
            } else {
                d.confirm_label.clone()
            },
            d.danger,
        ),
        Some(DialogKind::DiscardAll) => (
            "This will discard all staged and unstaged changes, including new untracked files. Are you sure \
             you want to discard all changes?"
                .to_owned(),
            "Reset All".to_owned(),
            true,
        ),
        Some(DialogKind::DeleteBranch) => {
            let message = if !d.ref_remote.is_empty() {
                format!(
                    "{DESTRUCTIVE} {}?",
                    in_quotes(&format!("{}/{}", d.ref_remote, d.ref_name))
                )
            } else if d.flag {
                format!(
                    "{DESTRUCTIVE} {} at this node, including remotes?",
                    in_quotes(&d.ref_name)
                )
            } else {
                format!("{DESTRUCTIVE} {}?", in_quotes(&d.ref_name))
            };
            (message, "Delete".to_owned(), true)
        }
        Some(DialogKind::DeleteTag) => {
            let message = if d.flag {
                format!(
                    "{DESTRUCTIVE} {} from your remote repos?",
                    in_quotes(&d.ref_name)
                )
            } else {
                format!("{DESTRUCTIVE} {}?", in_quotes(&d.ref_name))
            };
            (message, "Delete".to_owned(), true)
        }
        _ => {
            let shown = if d.text_b.is_empty() {
                &d.ref_name
            } else {
                &d.text_b
            };
            (
                format!(
                    "This is a destructive operation, are you sure you want to delete stash with message {}?",
                    in_quotes(shown)
                ),
                "Delete".to_owned(),
                true,
            )
        }
    }
}

/// Destructive confirmations are the prompt bar over the toolbar, not a modal.
fn confirm_prompt(cx: &mut Cx<'_>, app: &mut Application) {
    let (message, label, danger) = confirm_texts(&app.dialog);
    let answer = prompt_bar::draw_prompt_bar(cx, &message, &label, danger);
    if answer == Answer::Pending {
        return;
    }
    let request = std::mem::take(&mut app.dialog);
    if answer == Answer::No {
        return;
    }
    if request.kind == Some(DialogKind::Confirm) {
        if let Some(on_confirm) = request.on_confirm {
            on_confirm(cx, app);
        }
        return;
    }
    with_dialog_repo(
        app,
        request.workdir.as_deref(),
        |app, state| match request.kind {
            Some(DialogKind::DiscardAll) => {
                let result = state.repo.discard_all();
                app.run(state, "Discard all", result, Some("Changes discarded"));
            }
            Some(DialogKind::DeleteBranch) => delete_branch(app, state, &request),
            Some(DialogKind::DeleteTag) => delete_tag(app, state, &request),
            _ => delete_stash(app, state, &request),
        },
    );
}

fn delete_branch(app: &mut Application, state: &mut RepoState, request: &Dialog) {
    if !request.ref_remote.is_empty() {
        app.run_git_cli(
            state,
            "Delete remote branch",
            ["push", &request.ref_remote, "--delete", &request.ref_name],
        );
        return;
    }
    let name = request.ref_name.clone();
    let upstream = request.text_a.clone();
    let sha = request.sha.clone();
    let result = state.repo.delete_branch(&name);
    if app
        .run(
            state,
            "Delete branch",
            result,
            Some(&format!("Deleted {name}")),
        )
        .is_none()
    {
        return;
    }
    let redo_name = name.clone();
    state.push_undo(
        UndoEntry::new(format!("delete branch {name}"), move |repo| {
            repo.set_branch_target(&name, &sha)
        })
        .with_redo(move |repo| repo.delete_branch(&redo_name)),
    );
    if request.flag
        && let Some((remote, branch)) = upstream.split_once('/')
    {
        app.run_git_cli(
            state,
            "Delete remote branch",
            ["push", remote, "--delete", branch],
        );
    }
}

fn delete_tag(app: &mut Application, state: &mut RepoState, request: &Dialog) {
    let name = request.ref_name.clone();
    let sha = request.sha.clone();
    let remote = state
        .history
        .remotes
        .first()
        .map(|remote| remote.name.clone())
        .unwrap_or_default();
    let result = state.repo.delete_tag(&name);
    if app
        .run(
            state,
            "Delete tag",
            result,
            Some(&format!("Deleted tag {name}")),
        )
        .is_none()
    {
        return;
    }
    let (redo_name, remote_tag) = (name.clone(), format!("refs/tags/{name}"));
    state.push_undo(
        UndoEntry::new(format!("delete tag {name}"), move |repo| {
            repo.create_tag(&name, &sha, None)
        })
        .with_redo(move |repo| repo.delete_tag(&redo_name)),
    );
    if request.flag && !remote.is_empty() {
        app.run_git_cli(
            state,
            "Delete remote tag",
            ["push", &remote, "--delete", &remote_tag],
        );
    }
}

fn delete_stash(app: &mut Application, state: &mut RepoState, request: &Dialog) {
    // The stash is found again by its commit: one pushed since the prompt opened shifts the indexes.
    let Some(index) = state.repo.stash_index_of(&request.sha) else {
        app.toasts.push_with_detail(
            ToastKind::Error,
            "Delete stash failed",
            "The stash is gone.",
        );
        return;
    };
    let result = state.repo.stash_drop(index);
    app.run(state, "Delete stash", result, Some("Stash deleted"));
}

/// The name of a reset mode in the toast.
fn reset_mode_name(mode: ResetMode) -> &'static str {
    match mode {
        ResetMode::Soft => "Soft",
        ResetMode::Mixed => "Mixed",
        ResetMode::Hard => "Hard",
    }
}

/// Resets run straight away.
fn reset_now(_cx: &mut Cx<'_>, app: &mut Application) {
    let request = std::mem::take(&mut app.dialog);
    with_dialog_repo(app, request.workdir.as_deref(), |app, state| {
        let (sha, mode) = (request.sha.clone(), request.reset_mode);
        let previous = state.history.head_sha.clone();
        let result = state.repo.reset(&sha, mode);
        let done = format!("{} reset to {}", reset_mode_name(mode), short_sha(&sha));
        if app.run(state, "Reset", result, Some(&done)).is_none() {
            return;
        }
        let undo_mode = if mode == ResetMode::Hard {
            ResetMode::Mixed
        } else {
            mode
        };
        let redo_sha = sha.clone();
        state.push_undo(
            UndoEntry::new("reset", move |repo| match &previous {
                Some(previous) => repo.reset(previous, undo_mode),
                None => Err(Error::Message("nothing to restore".to_owned())),
            })
            .with_redo(move |repo| repo.reset(&redo_sha, mode)),
        );
    });
}

/// Branches and lightweight tags are named inline in the graph, like the ref zone input. `true` when the
/// dialog turned into that (and closed).
fn start_inline_ref(app: &mut Application) -> bool {
    let d = &app.dialog;
    let inline = d.kind == Some(DialogKind::CreateBranch)
        || (d.kind == Some(DialogKind::CreateTag) && !d.flag);
    if !inline {
        return false;
    }
    let tag = d.kind == Some(DialogKind::CreateTag);
    let checkout = d.kind == Some(DialogKind::CreateBranch) && d.flag;
    let wanted = d.sha.clone();
    let workdir = d.workdir.clone();
    let mut started = false;
    with_dialog_repo(app, workdir.as_deref(), |_, state| {
        let sha = if wanted.is_empty() {
            state.history.head_sha.clone().unwrap_or_default()
        } else {
            wanted
        };
        let Some(&row) = state.history.row_by_sha.get(&sha) else {
            return;
        };
        state.select_only(&sha);
        state.inline_ref = Some(InlineRef {
            sha,
            tag,
            checkout,
            ..InlineRef::default()
        });
        state.scroll_to_row = Some(row);
        started = true;
    });
    if started {
        app.dialog.close();
    }
    started
}

/// The modal popup of `kind`: the title bar, the contents, a 0.15s fade in.
fn draw_modal(cx: &mut Cx<'_>, app: &mut Application, kind: DialogKind) {
    let ui = cx.ui;
    let p = cx.palette;
    let title = if kind == DialogKind::Confirm {
        format!("{}###confirm_dialog", app.dialog.title)
    } else {
        modal::title(kind).to_owned()
    };
    if app.dialog.just_opened {
        ui.open_popup(&title);
        app.dialog.just_opened = false;
    }
    let fade = cx.smooth_over("##dialog_fade", true, 0.15);
    let (viewport_pos, viewport_size) = raw::viewport(ui);
    raw::set_next_window_pos_appearing(ui, viewport_pos + viewport_size / 2.0, vec2(0.5, 0.5));
    raw::set_next_window_size(ui, vec2(cx.px(modal::width(kind)), 0.0));
    // The window's own look, held while the contents draw; the contents put the application's back.
    let normal = ui.clone_style();
    let normal_popup_bg = cx.style_color(StyleColor::PopupBg);
    let normal_title_bg = cx.style_color(StyleColor::TitleBgActive);
    let _padding = ui.push_style_var(StyleVar::WindowPadding([cx.px(20.0), cx.px(16.0)]));
    let _rounding = ui.push_style_var(StyleVar::WindowRounding(cx.px(6.0)));
    let _border = ui.push_style_var(StyleVar::WindowBorderSize(1.0));
    let _alpha = ui.push_style_var(StyleVar::Alpha(fade));
    let _popup_bg = ui.push_style_color(StyleColor::PopupBg, p.panel_bg1);
    let _title_bg = ui.push_style_color(StyleColor::TitleBgActive, p.panel_bg1);
    let mut open = true;
    let popup = ui
        .begin_modal_popup_config(&title)
        .opened(&mut open)
        .flags(WindowFlags::NO_RESIZE | WindowFlags::NO_MOVE)
        .begin();
    let Some(popup) = popup else {
        app.dialog.close();
        return;
    };
    {
        let _alpha = ui.push_style_var(StyleVar::Alpha(1.0));
        let _padding = ui.push_style_var(StyleVar::WindowPadding(normal.window_padding()));
        let _rounding = ui.push_style_var(StyleVar::WindowRounding(normal.window_rounding()));
        let _border = ui.push_style_var(StyleVar::WindowBorderSize(normal.window_border_size()));
        let _popup_bg = ui.push_style_color(StyleColor::PopupBg, normal_popup_bg);
        let _title_bg = ui.push_style_color(StyleColor::TitleBgActive, normal_title_bg);
        let _frame = ui.push_style_var(StyleVar::FramePadding([cx.px(8.0), cx.px(6.0)]));
        modal::body(cx, app, kind);
    }
    drop(popup);
    if !ui.is_popup_open(&title) {
        app.dialog.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(kind: DialogKind) -> Dialog {
        let mut d = Dialog::default();
        d.open(kind);
        d
    }

    #[test]
    fn deleting_a_branch_names_what_goes() {
        let mut d = dialog(DialogKind::DeleteBranch);
        d.ref_name = "topic".to_owned();
        let (message, label, danger) = confirm_texts(&d);
        assert_eq!(
            message,
            "This is a destructive operation, are you sure you want to delete \"topic\"?"
        );
        assert_eq!((label.as_str(), danger), ("Delete", true));
        d.flag = true;
        assert!(
            confirm_texts(&d)
                .0
                .ends_with("\"topic\" at this node, including remotes?")
        );
        d.ref_remote = "origin".to_owned();
        assert!(confirm_texts(&d).0.ends_with("\"origin/topic\"?"));
    }

    #[test]
    fn deleting_a_tag_or_a_stash_names_what_goes() {
        let mut d = dialog(DialogKind::DeleteTag);
        d.ref_name = "v1".to_owned();
        assert!(confirm_texts(&d).0.ends_with("\"v1\"?"));
        d.flag = true;
        assert!(
            confirm_texts(&d)
                .0
                .ends_with("\"v1\" from your remote repos?")
        );
        let mut d = dialog(DialogKind::DeleteStash);
        d.ref_name = "stash@{0}".to_owned();
        assert!(
            confirm_texts(&d)
                .0
                .ends_with("stash with message \"stash@{0}\"?")
        );
        d.text_b = "WIP".to_owned();
        assert!(confirm_texts(&d).0.ends_with("stash with message \"WIP\"?"));
    }

    #[test]
    fn a_generic_confirmation_uses_its_own_words() {
        let mut d = dialog(DialogKind::Confirm);
        d.message = "Sure?".to_owned();
        assert_eq!(
            confirm_texts(&d),
            ("Sure?".to_owned(), "OK".to_owned(), false)
        );
        d.confirm_label = "Force Push".to_owned();
        d.danger = true;
        assert_eq!(
            confirm_texts(&d),
            ("Sure?".to_owned(), "Force Push".to_owned(), true)
        );
        let (_, label, danger) = confirm_texts(&dialog(DialogKind::DiscardAll));
        assert_eq!((label.as_str(), danger), ("Reset All", true));
    }

    #[test]
    fn new_files_stay_inside_the_repository() {
        assert_eq!(
            new_file_components("", "a.txt"),
            Some(vec!["a.txt".to_owned()])
        );
        assert_eq!(
            new_file_components("src", "ui\\b.rs"),
            Some(vec!["src".to_owned(), "ui".to_owned(), "b.rs".to_owned()])
        );
        assert_eq!(
            new_file_components("src", "../b.rs"),
            Some(vec!["b.rs".to_owned()])
        );
        assert_eq!(new_file_components("", "../outside"), None);
        assert_eq!(new_file_components("", "/abs"), None);
        assert_eq!(new_file_components("", "C:\\x"), None);
        assert_eq!(new_file_components("", "dir/"), None);
        assert_eq!(new_file_components("", "."), None);
        assert_eq!(new_file_components("", ""), None);
    }

    #[test]
    fn reset_modes_have_names() {
        assert_eq!(reset_mode_name(ResetMode::Soft), "Soft");
        assert_eq!(reset_mode_name(ResetMode::Mixed), "Mixed");
        assert_eq!(reset_mode_name(ResetMode::Hard), "Hard");
    }
}
