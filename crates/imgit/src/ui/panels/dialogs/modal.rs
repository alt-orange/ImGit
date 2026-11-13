//! The modal dialogs' contents: what goes inside the popup `draw_dialogs` opens for the kinds that are neither
//! the prompt bar nor the slide panel.

use std::path::{Path, PathBuf};

use dear_imgui_rs::{
    InputTextFlags, Key, StyleColor, TableFlags, TableOptions, TableSizingPolicy, sys,
};
use imgit_app::RepoState;
use imgit_git::repository::short_sha;
use imgit_git::{RebaseAction, RebaseStep, Ref, RefType, Repository, Sha};

use crate::app::application::Application;
use crate::app::dialog::{Dialog, DialogKind};
use crate::app::toasts::ToastKind;
use crate::theme::FONT_SIZE_ROW;
use crate::ui::{Cx, vec2};

use super::{
    create_branch_at, create_tag_at, is_reserved_device_name, leading_dash_error, tip_restore_entry,
};
use crate::ui::panels::menus::fetch_args;

/// The title of the modal for `kind`.
pub fn title(kind: DialogKind) -> &'static str {
    match kind {
        DialogKind::CloneRepo => "Clone a Repository",
        DialogKind::InitRepo => "Init a Repository",
        DialogKind::CreateBranch => "Create Branch",
        DialogKind::CreateTag => "Create Tag",
        DialogKind::CreateWorktree => "Create Worktree",
        DialogKind::AddRemote => "Add Remote",
        DialogKind::EditRemote => "Edit Remote",
        DialogKind::RewordCommit => "Edit Commit Message",
        DialogKind::SquashCommits => "Squash Commits",
        DialogKind::SetUpstream => "Set Upstream",
        DialogKind::Preferences => "Preferences",
        DialogKind::KeyboardShortcuts => "Keyboard Shortcuts",
        DialogKind::About => "About ImGit Desktop",
        DialogKind::BranchCleanup => "Clean Up Branches",
        _ => "",
    }
}

/// The modal's width in logical pixels.
pub fn width(kind: DialogKind) -> f32 {
    match kind {
        DialogKind::Preferences => 720.0,
        DialogKind::KeyboardShortcuts => 560.0,
        DialogKind::SquashCommits | DialogKind::RewordCommit | DialogKind::BranchCleanup => 540.0,
        _ => 480.0,
    }
}

/// `ImGui::GetStyle().ItemSpacing.x`.
fn item_spacing_x(_ui: &dear_imgui_rs::Ui) -> f32 {
    // SAFETY: a frame is open (`_ui`), so the style exists; one float is read.
    unsafe { (*sys::igGetStyle()).ItemSpacing.x }
}

/// A path field with a native folder picker.
fn path_field(cx: &mut Cx<'_>, id: &str, value: &mut String, hint: &str) {
    let ui = cx.ui;
    let browse_w = cx.px(80.0);
    let width = cx.avail().x - browse_w - item_spacing_x(ui);
    cx.input_text(id, value, Some(hint), InputTextFlags::empty(), width);
    ui.same_line();
    if cx.default_button("Browse...", vec2(browse_w, 0.0), true)
        && let Some(picked) = imgit_platform::shell::pick_folder_dialog(Some(cx.shell.window), None)
    {
        *value = picked.to_string_lossy().into_owned();
    }
}

/// The width of a button holding `label`.
fn confirm_width(label: &str) -> f32 {
    (label.len() as f32 * 7.0 + 30.0).max(110.0)
}

/// The Cancel / confirm row; `true` when confirmed (the button, or Enter when `enter_submits`).
fn buttons(
    cx: &mut Cx<'_>,
    confirm_label: &str,
    enabled: bool,
    danger: bool,
    enter_submits: bool,
    show_cancel: bool,
) -> bool {
    let ui = cx.ui;
    ui.dummy([0.0, cx.px(8.0)]);
    let width = cx.px(confirm_width(confirm_label));
    // GetContentRegionMax().x: the right edge of the content, in window coordinates.
    let right = ui.cursor_pos_x() + cx.avail().x;
    let cancel_space = if show_cancel {
        cx.px(110.0) + item_spacing_x(ui)
    } else {
        0.0
    };
    ui.set_cursor_pos_x(right - width - cancel_space);
    let mut cancel = false;
    if show_cancel {
        cancel = cx.default_button("Cancel", vec2(cx.px(110.0), cx.px(32.0)), true);
        ui.same_line();
    }
    let size = vec2(width, cx.px(32.0));
    let mut confirm = if danger {
        cx.danger_button(confirm_label, size, enabled)
    } else {
        cx.primary_button(confirm_label, size, enabled)
    };
    if enter_submits && enabled && ui.is_key_pressed(Key::Enter) && !ui.is_any_item_active() {
        confirm = true;
    }
    if cancel || ui.is_key_pressed(Key::Escape) {
        ui.close_current_popup();
    }
    confirm
}

/// A thin rounded bar filled to `fraction` (0..1) across the content width.
fn progress_bar(cx: &mut Cx<'_>, fraction: f32) {
    let min = cx.cursor();
    let width = cx.avail().x;
    let height = cx.px(6.0);
    let fill = cx.smooth_value("##clone_progress_fill", fraction.clamp(0.0, 1.0));
    let draw = cx.window_draw_list();
    draw.add_rect_filled(
        min,
        vec2(min.x + width, min.y + height),
        cx.palette.input_bg,
        height / 2.0,
    );
    if fill > 0.0 {
        draw.add_rect_filled(
            min,
            vec2(min.x + (width * fill).max(height), min.y + height),
            cx.palette.accent,
            height / 2.0,
        );
    }
    cx.ui.dummy([width, height]);
}

fn error_text(cx: &Cx<'_>, error: Option<&str>) {
    if let Some(error) = error.filter(|error| !error.is_empty()) {
        let _color = cx.ui.push_style_color(StyleColor::Text, cx.palette.red);
        cx.ui.text_wrapped(error);
    }
}

fn shortcut_row(cx: &Cx<'_>, keys: &str, what: &str) {
    let ui = cx.ui;
    ui.table_next_row();
    ui.table_next_column();
    {
        let _color = ui.push_style_color(StyleColor::Text, cx.palette.text_selected);
        ui.text(cx.shortcut_label(keys));
    }
    ui.table_next_column();
    ui.text(what);
}

/// The repository's name for a clone of `url`: its last component without a trailing slash or `.git`.
pub fn clone_name(url: &str) -> String {
    super::slide_panel::name_from_url(url)
}

/// The folder a clone of `url` creates below the folder the user picked: the repository's name, when that is a
/// single normal path component. A url that gives none ("", "git@host:", "https://") would clone into the
/// picked folder itself, and one ending in ".." into its parent; a name that is no folder name (a device name
/// on Windows) cannot be created.
///
/// # Errors
///
/// What the Clone dialog shows when the name is none of that.
pub fn clone_folder_name(url: &str) -> Result<String, String> {
    let name = clone_name(url.trim());
    let mut components = Path::new(&name).components();
    let normal = matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    );
    if name.is_empty() {
        Err("The URL does not name a repository folder to clone into.".to_owned())
    } else if !normal || name.contains(['/', '\\', ':']) {
        Err(format!(
            "'{name}' is not a folder name: the URL must end with the name of the repository."
        ))
    } else if is_reserved_device_name(&name) {
        Err(format!("'{name}' cannot be used as a folder name."))
    } else {
        Ok(name)
    }
}

/// The rebase steps that give the commit `target` the message `message`: the commits of `chain` (oldest first)
/// are picked, `target` reworded.
pub fn reword_plan(chain: &[Sha], target: &str, message: &str) -> Vec<RebaseStep> {
    chain
        .iter()
        .map(|sha| RebaseStep {
            sha: sha.clone(),
            action: if sha == target {
                RebaseAction::Reword(Some(message.to_owned()))
            } else {
                RebaseAction::Pick
            },
        })
        .collect()
}

/// The rebase steps that squash the `selected` commits of `chain` (oldest first) into one with `message`: the
/// first one is reworded, the others squashed into it, the rest picked.
pub fn squash_plan(chain: &[Sha], selected: &[Sha], message: &str) -> Vec<RebaseStep> {
    let mut first = true;
    chain
        .iter()
        .map(|sha| {
            let action = if !selected.contains(sha) {
                RebaseAction::Pick
            } else if std::mem::take(&mut first) {
                RebaseAction::Reword(Some(message.to_owned()))
            } else {
                RebaseAction::Squash(Some(message.to_owned()))
            };
            RebaseStep {
                sha: sha.clone(),
                action,
            }
        })
        .collect()
}

/// Where a worktree of the repository at `workdir` for `leaf` goes by default: next to it, named
/// "<repository>-<leaf>".
pub fn default_worktree_folder(workdir: &Path, leaf: &str) -> Option<PathBuf> {
    let name = workdir.file_name()?.to_string_lossy();
    let leaf = leaf.replace('/', "-");
    Some(workdir.parent()?.join(format!("{name}-{leaf}")))
}

/// The `git worktree add` arguments for the dialog `d`: the branch checked out in a new folder (a remote one
/// through a local branch tracking it unless `local_exists`), or the commit detached.
pub fn worktree_args(d: &Dialog, local_exists: bool) -> Vec<String> {
    // `--` ends the options: a folder (or a branch) that starts with '-' is not one.
    let mut args: Vec<String> = vec!["worktree".into(), "add".into()];
    if d.ref_name.is_empty() {
        args.extend([
            "--detach".into(),
            "--".into(),
            d.text_a.clone(),
            d.sha.clone(),
        ]);
    } else if d.ref_remote.is_empty() || local_exists {
        args.extend(["--".into(), d.text_a.clone(), d.ref_name.clone()]);
    } else {
        args.extend([
            "--track".into(),
            "-b".into(),
            d.ref_name.clone(),
            "--".into(),
            d.text_a.clone(),
            format!("{}/{}", d.ref_remote, d.ref_name),
        ]);
    }
    args
}

/// The contents of the modal for `kind` (the popup is open).
pub fn body(cx: &mut Cx<'_>, app: &mut Application, kind: DialogKind) {
    match kind {
        DialogKind::CloneRepo => clone(cx, app),
        DialogKind::InitRepo => init(cx, app),
        DialogKind::KeyboardShortcuts => shortcuts(cx),
        DialogKind::About => about(cx),
        // Preferences is a view of its own (the old dialog is gone): show that instead.
        DialogKind::Preferences => {
            app.preferences_open = true;
            cx.ui.close_current_popup();
        }
        _ => {
            let workdir = app.dialog.workdir.clone();
            let run = |app: &mut Application, state: &mut RepoState| {
                repository_body(cx, app, state, kind);
            };
            match workdir {
                Some(workdir) => {
                    app.with_repo(&workdir, run);
                }
                None => {
                    app.with_active(run);
                }
            }
        }
    }
}

fn clone(cx: &mut Cx<'_>, app: &mut Application) {
    let ui = cx.ui;
    cx.secondary_text("URL");
    cx.input_text(
        "##clone_url",
        &mut app.dialog.text_a,
        Some("https://github.com/owner/repo.git"),
        InputTextFlags::empty(),
        -1.0,
    );
    ui.dummy([0.0, cx.px(4.0)]);
    cx.secondary_text("Where to clone to");
    path_field(cx, "##clone_dest", &mut app.dialog.text_b, "C:\\projects");
    error_text(cx, app.dialog.error.as_deref());
    // While the clone runs the dialog shows its progress (`flag`: it was seen running here, so the dialog
    // closes when it is done); Hide leaves it running and the toasts report the outcome.
    if app.pending_clone.is_some() {
        app.dialog.flag = true;
        let progress = app.clone_progress();
        ui.dummy([0.0, cx.px(8.0)]);
        cx.secondary_text(if progress.line.is_empty() {
            "Cloning..."
        } else {
            &progress.line
        });
        ui.dummy([0.0, cx.px(4.0)]);
        progress_bar(cx, progress.fraction);
        cx.keep_animating();
        if buttons(cx, "Hide", true, false, false, false) {
            ui.close_current_popup();
        }
        return;
    }
    if app.dialog.flag {
        ui.close_current_popup();
        return;
    }
    let ready = !app.dialog.text_a.is_empty() && !app.dialog.text_b.is_empty();
    if buttons(cx, "Clone", ready, false, true, true) {
        let url = app.dialog.text_a.clone();
        let parent = app.dialog.text_b.clone();
        let name = match clone_folder_name(&url) {
            Ok(name) => name,
            Err(error) => {
                app.dialog.error = Some(error);
                return;
            }
        };
        app.dialog.error = None;
        let destination = Path::new(&parent)
            .join(&name)
            .to_string_lossy()
            .into_owned();
        // git creates the repository folder, not the folder it runs in.
        let _ = std::fs::create_dir_all(&parent);
        app.start_clone(parent, url, destination, None);
        app.toasts.push_with_detail(
            ToastKind::Info,
            format!("Cloning {name}"),
            "This may take a while.",
        );
    }
}

fn init(cx: &mut Cx<'_>, app: &mut Application) {
    cx.secondary_text("New repository folder");
    path_field(
        cx,
        "##init_path",
        &mut app.dialog.text_a,
        "C:\\projects\\new-repo",
    );
    error_text(cx, app.dialog.error.as_deref());
    if buttons(
        cx,
        "Create Repository",
        !app.dialog.text_a.is_empty(),
        false,
        true,
        true,
    ) {
        let path = app.dialog.text_a.clone();
        let _ = std::fs::create_dir_all(&path);
        match Repository::init(&path) {
            Ok(_) => {
                app.open_repository(&path);
                cx.ui.close_current_popup();
            }
            Err(error) => app.dialog.error = Some(error.to_string()),
        }
    }
}

fn shortcuts(cx: &mut Cx<'_>) {
    const ROWS: [(&str, &str); 26] = [
        ("Ctrl + P", "Toggle Command Palette"),
        ("Ctrl + O", "Open Repo"),
        ("Ctrl + T", "Open a new tab"),
        ("Ctrl + W", "Close File/Tab"),
        ("Ctrl + Tab", "Jump to the next open tab"),
        ("Ctrl + 1..9", "Jump to a specific tab"),
        ("Ctrl + Shift + A", "Open tabs list"),
        ("Ctrl + F", "Search commits"),
        ("Ctrl + L", "Fetch All"),
        ("Ctrl + B", "Create Branch"),
        ("Ctrl + Z / Ctrl + Y", "Undo / Redo"),
        ("Ctrl + Shift + S", "Stage All Files"),
        ("Ctrl + Shift + U", "Unstage All Files"),
        ("Ctrl + Shift + M", "Focus commit message"),
        ("Ctrl + Enter", "Commit (when in message input)"),
        ("Ctrl + J", "Toggle Left Panel"),
        ("Ctrl + K", "Toggle Commit Details Panel"),
        ("Ctrl + `", "Toggle Terminal Panel"),
        ("Ctrl + = / Ctrl + -", "Increase / Decrease Zoom"),
        ("Ctrl + 0", "Reset Zoom"),
        ("Up / Down, k / j", "Select Previous / Next Item"),
        ("Shift + Up / Down", "Extend the selection"),
        ("Home / End", "Select First / Last Item"),
        (
            "p / r / s / d",
            "Interactive rebase: pick / reword / squash / drop",
        ),
        ("Esc", "Close View"),
        ("F5", "Reload Window"),
    ];
    let ui = cx.ui;
    {
        let _font = cx.push_font(cx.fonts.regular, FONT_SIZE_ROW);
        if let Some(_table) = ui.begin_table_with_flags(
            "##shortcuts",
            2,
            TableOptions::new()
                .flags(TableFlags::ROW_BG)
                .sizing_policy(TableSizingPolicy::StretchProp),
        ) {
            for (keys, what) in ROWS {
                shortcut_row(cx, keys, what);
            }
        }
    }
    if buttons(cx, "Close", true, false, true, false) {
        ui.close_current_popup();
    }
}

fn about(cx: &mut Cx<'_>) {
    let ui = cx.ui;
    {
        let _font = cx.push_font(cx.fonts.semibold, 20.0);
        ui.text("ImGit Desktop");
    }
    cx.secondary_text("A Git client built on Dear ImGui and libgit2.");
    ui.dummy([0.0, cx.px(6.0)]);
    ui.text("ImGit uses some fine open source projects:");
    for line in [
        "Dear ImGui (MIT)",
        "libgit2 (GPLv2 with linking exception)",
        "GLFW (zlib)",
        "Open Sans (SIL Open Font License)",
        "Font Awesome Free icons (CC BY 4.0)",
    ] {
        ui.bullet_text(line);
    }
    if buttons(cx, "Close", true, false, true, false) {
        ui.close_current_popup();
    }
}

/// The modals that act on the dialog's repository.
fn repository_body(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    kind: DialogKind,
) {
    match kind {
        DialogKind::CreateBranch => create_branch(cx, app, state),
        DialogKind::CreateTag => create_tag(cx, app, state),
        DialogKind::CreateWorktree => create_worktree(cx, app, state),
        DialogKind::AddRemote => add_remote(cx, app, state),
        DialogKind::EditRemote => edit_remote(cx, app, state),
        DialogKind::RewordCommit => reword(cx, app, state),
        DialogKind::SquashCommits => squash(cx, app, state),
        DialogKind::SetUpstream => set_upstream(cx, app, state),
        DialogKind::BranchCleanup => {
            crate::ui::panels::workflows::draw_branch_cleanup(cx, app, state);
        }
        // The prompt bar, the slide panel and the confirmations never get here.
        _ => {}
    }
}

fn create_branch(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    cx.secondary_text(&format!("Branch at {}", short_sha(&app.dialog.sha)));
    if ui.is_window_appearing() {
        ui.set_keyboard_focus_here();
    }
    let submitted = cx.input_text(
        "##branch_name",
        &mut app.dialog.text_a,
        Some("Branch name"),
        InputTextFlags::ENTER_RETURNS_TRUE,
        -1.0,
    );
    cx.checkbox("Checkout after create", &mut app.dialog.flag);
    let invalid = leading_dash_error("branch", &app.dialog.text_a);
    error_text(cx, invalid.as_deref().or(app.dialog.error.as_deref()));
    let enabled = !app.dialog.text_a.is_empty() && invalid.is_none();
    if (buttons(cx, "Create Branch", enabled, false, false, true) || (submitted && enabled))
        && !app.refuse_while_busy(state)
    {
        let (name, sha, checkout) = (
            app.dialog.text_a.clone(),
            app.dialog.sha.clone(),
            app.dialog.flag,
        );
        create_branch_at(cx, app, state, &name, &sha, checkout);
        ui.close_current_popup();
    }
}

fn create_tag(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    cx.secondary_text(&format!("Tag at {}", short_sha(&app.dialog.sha)));
    if ui.is_window_appearing() {
        ui.set_keyboard_focus_here();
    }
    let submitted = cx.input_text(
        "##tag_name",
        &mut app.dialog.text_a,
        Some("Tag name"),
        InputTextFlags::ENTER_RETURNS_TRUE,
        -1.0,
    );
    cx.checkbox("Annotated tag", &mut app.dialog.flag);
    if app.dialog.flag {
        cx.input_multiline(
            "##tag_message",
            &mut app.dialog.text_b,
            Some("Tag message"),
            vec2(-1.0, cx.px(60.0)),
            false,
        );
    }
    let invalid = leading_dash_error("tag", &app.dialog.text_a);
    error_text(cx, invalid.as_deref().or(app.dialog.error.as_deref()));
    let d = &app.dialog;
    let enabled = !d.text_a.is_empty() && (!d.flag || !d.text_b.is_empty()) && invalid.is_none();
    if (buttons(cx, "Create Tag", enabled, false, false, true) || (submitted && enabled))
        && !app.refuse_while_busy(state)
    {
        let d = &app.dialog;
        let message = if d.flag {
            d.text_b.clone()
        } else {
            String::new()
        };
        let (name, sha) = (d.text_a.clone(), d.sha.clone());
        create_tag_at(cx, app, state, &name, &sha, &message);
        ui.close_current_popup();
    }
}

fn create_worktree(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    // git worktree add: the branch checked out in a new folder (a remote one through a local branch tracking
    // it), or the commit detached.
    let d = &mut app.dialog;
    let from = if d.ref_name.is_empty() {
        short_sha(&d.sha).to_owned()
    } else if d.ref_remote.is_empty() {
        d.ref_name.clone()
    } else {
        format!("{}/{}", d.ref_remote, d.ref_name)
    };
    if ui.is_window_appearing() && d.text_a.is_empty() {
        let leaf = if d.ref_name.is_empty() {
            short_sha(&d.sha)
        } else {
            &d.ref_name
        };
        if let Some(folder) = default_worktree_folder(state.repo.workdir(), leaf) {
            d.text_a = folder.to_string_lossy().into_owned();
        }
    }
    cx.secondary_text(&format!("Worktree from {from}"));
    ui.dummy([0.0, cx.px(4.0)]);
    cx.secondary_text("Folder");
    if ui.is_window_appearing() {
        ui.set_keyboard_focus_here();
    }
    let submitted = cx.input_text(
        "##worktree_path",
        &mut app.dialog.text_a,
        Some("Folder for the worktree"),
        InputTextFlags::ENTER_RETURNS_TRUE,
        -1.0,
    );
    error_text(cx, app.dialog.error.as_deref());
    let enabled = !app.dialog.text_a.is_empty();
    if (buttons(cx, "Create Worktree", enabled, false, false, true) || submitted)
        && !app.dialog.text_a.is_empty()
    {
        let d = &app.dialog;
        let local_exists = state
            .history
            .refs
            .iter()
            .any(|r| r.ref_type == RefType::LocalBranch && r.name == d.ref_name);
        let args = worktree_args(d, local_exists);
        app.run_git_cli(state, "Create worktree", args);
        ui.close_current_popup();
    }
}

fn add_remote(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    cx.secondary_text("Name");
    if ui.is_window_appearing() {
        ui.set_keyboard_focus_here();
    }
    cx.input_text(
        "##remote_name",
        &mut app.dialog.text_a,
        Some("origin"),
        InputTextFlags::empty(),
        -1.0,
    );
    cx.secondary_text("URL");
    cx.input_text(
        "##remote_url",
        &mut app.dialog.text_b,
        Some("https://github.com/owner/repo.git"),
        InputTextFlags::empty(),
        -1.0,
    );
    error_text(cx, app.dialog.error.as_deref());
    let enabled = !app.dialog.text_a.is_empty() && !app.dialog.text_b.is_empty();
    if buttons(cx, "Add Remote", enabled, false, true, true) && !app.refuse_while_busy(state) {
        let (name, url) = (app.dialog.text_a.clone(), app.dialog.text_b.clone());
        let result = state.repo.add_remote(&name, &url);
        if app
            .run(
                state,
                "Add remote",
                result,
                Some(&format!("Added remote {name}")),
            )
            .is_some()
        {
            app.run_git_cli(state, "Fetch", fetch_args(&name));
            ui.close_current_popup();
        }
    }
}

fn edit_remote(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    cx.secondary_text("Name");
    {
        let _disabled = ui.begin_disabled();
        cx.input_text(
            "##remote_name",
            &mut app.dialog.text_a,
            None,
            InputTextFlags::empty(),
            -1.0,
        );
    }
    cx.secondary_text("Pull URL");
    cx.input_text(
        "##remote_url",
        &mut app.dialog.text_b,
        Some("https://github.com/owner/repo.git"),
        InputTextFlags::empty(),
        -1.0,
    );
    ui.dummy([0.0, cx.px(6.0)]);
    if cx.danger_button("Remove remote", vec2(cx.px(140.0), cx.px(28.0)), true)
        && !app.refuse_while_busy(state)
    {
        let name = app.dialog.ref_name.clone();
        let result = state.repo.remove_remote(&name);
        app.run(
            state,
            "Remove remote",
            result,
            Some(&format!("Removed remote {name}")),
        );
        ui.close_current_popup();
    }
    let enabled = !app.dialog.text_b.is_empty();
    if buttons(cx, "Save", enabled, false, true, true) && !app.refuse_while_busy(state) {
        let (name, url) = (app.dialog.ref_name.clone(), app.dialog.text_b.clone());
        let result = state.repo.set_remote_url(&name, &url);
        app.run(
            state,
            "Edit remote",
            result,
            Some(&format!("Updated remote {name}")),
        );
        ui.close_current_popup();
    }
}

fn reword(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    cx.secondary_text(&format!("Commit {}", short_sha(&app.dialog.sha)));
    let appearing = ui.is_window_appearing();
    if appearing {
        ui.set_keyboard_focus_here();
    }
    cx.input_multiline(
        "##message",
        &mut app.dialog.text_a,
        Some("Commit message"),
        vec2(-1.0, cx.px(140.0)),
        appearing,
    );
    let enabled = !app.dialog.text_a.is_empty();
    if buttons(cx, "Update Message", enabled, false, false, true) && !app.refuse_while_busy(state) {
        let (sha, message) = (app.dialog.sha.clone(), app.dialog.text_a.clone());
        let undo = tip_restore_entry(state, "edit commit message");
        let ok = if state.history.head_sha.as_deref() == Some(sha.as_str()) {
            let result = state.repo.reword_head(&message);
            app.run(
                state,
                "Edit commit message",
                result,
                Some("Commit message updated"),
            )
            .is_some()
        } else {
            let base = state
                .commit_for(&sha)
                .and_then(|commit| commit.parents.first().cloned());
            let chain: Vec<Sha> = state
                .commits_above(base.as_deref(), None)
                .iter()
                .map(|commit| commit.sha.clone())
                .collect();
            let steps = reword_plan(&chain, &sha, &message);
            let result = state
                .repo
                .rewrite_history(base.as_deref().unwrap_or(""), &steps);
            app.run(
                state,
                "Edit commit message",
                result,
                Some("Commit message updated"),
            )
            .is_some()
        };
        if ok {
            state.push_undo(undo);
        }
        ui.close_current_popup();
    }
}

fn squash(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    ui.text_wrapped(format!(
        "Squash {} commits into one. Commit message:",
        app.dialog.shas.len()
    ));
    cx.input_multiline(
        "##squash_message",
        &mut app.dialog.text_a,
        Some("Commit message"),
        vec2(-1.0, cx.px(160.0)),
        false,
    );
    let enabled = !app.dialog.text_a.is_empty() && !app.dialog.shas.is_empty();
    if buttons(cx, "Squash", enabled, false, false, true) && !app.refuse_while_busy(state) {
        let shas = app.dialog.shas.clone();
        let message = app.dialog.text_a.clone();
        let base = shas
            .first()
            .and_then(|oldest| state.commit_for(oldest))
            .and_then(|oldest| oldest.parents.first().cloned());
        if let Some(base) = base {
            let chain: Vec<Sha> = state
                .commits_above(Some(&base), None)
                .iter()
                .map(|commit| commit.sha.clone())
                .collect();
            let steps = squash_plan(&chain, &shas, &message);
            let undo = tip_restore_entry(state, "squash");
            let result = state.repo.rewrite_history(&base, &steps);
            let done = format!("Squashed {} commits", shas.len());
            if app.run(state, "Squash", result, Some(&done)).is_some() {
                state.push_undo(undo);
            }
        }
        ui.close_current_popup();
    }
}

fn set_upstream(cx: &mut Cx<'_>, app: &mut Application, state: &mut RepoState) {
    let ui = cx.ui;
    cx.secondary_text(&format!("Upstream of {}", app.dialog.ref_name));
    let d = &mut app.dialog;
    let preview = if d.text_a.is_empty() {
        "(none)".to_owned()
    } else {
        d.text_a.clone()
    };
    if let Some(_combo) = ui.begin_combo("##upstream", &preview) {
        if ui
            .selectable_config("(none)")
            .selected(d.text_a.is_empty())
            .build()
        {
            d.text_a.clear();
        }
        let remotes = state
            .history
            .refs
            .iter()
            .filter(|r: &&Ref| r.ref_type == RefType::RemoteBranch);
        for remote in remotes {
            if ui
                .selectable_config(&remote.name)
                .selected(remote.name == d.text_a)
                .build()
            {
                d.text_a.clone_from(&remote.name);
            }
        }
    }
    if buttons(cx, "Set Upstream", true, false, true, true) && !app.refuse_while_busy(state) {
        let (name, upstream) = (app.dialog.ref_name.clone(), app.dialog.text_a.clone());
        let result = state
            .repo
            .set_upstream(&name, (!upstream.is_empty()).then_some(upstream.as_str()));
        let done = if upstream.is_empty() {
            "Upstream cleared".to_owned()
        } else {
            format!("Upstream set to {upstream}")
        };
        app.run(state, "Set upstream", result, Some(&done));
        ui.close_current_popup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shas(names: &[&str]) -> Vec<Sha> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_clone_folder_is_named_after_the_repository() {
        assert_eq!(clone_name("https://github.com/owner/repo.git"), "repo");
        assert_eq!(clone_name("git@host:team/tool.git/"), "tool");
    }

    #[test]
    fn a_clone_needs_a_single_normal_folder_name() {
        assert_eq!(
            clone_folder_name("https://github.com/owner/repo.git"),
            Ok("repo".to_owned())
        );
        assert_eq!(
            clone_folder_name("  git@host:team/tool.git/ "),
            Ok("tool".to_owned())
        );
        assert_eq!(clone_folder_name("C:\\src\\lib"), Ok("lib".to_owned()));
        // Nothing to name the folder after: git would clone into the picked folder itself.
        for url in [
            "",
            "   ",
            "https://",
            "git@host:",
            "/",
            "C:\\",
            "https://host/.git",
        ] {
            assert!(clone_folder_name(url).is_err(), "{url:?}");
        }
        // Dots are not names: ".." is the folder above the picked one.
        for url in [
            "https://host/..",
            "../..",
            "https://host/repo/..",
            "https://host/.",
        ] {
            assert!(clone_folder_name(url).is_err(), "{url:?}");
        }
        // A name Windows reserves for a device.
        assert!(clone_folder_name("https://host/nul.git").is_err());
        assert!(clone_folder_name("https://host/CON").is_err());
        assert_eq!(
            clone_folder_name("https://host/console.git"),
            Ok("console".to_owned())
        );
    }

    #[test]
    fn button_widths_grow_with_the_label() {
        assert_eq!(confirm_width("OK"), 110.0);
        assert_eq!(confirm_width("Create Repository"), 17.0 * 7.0 + 30.0);
    }

    #[test]
    fn rewording_marks_only_the_target() {
        let steps = reword_plan(&shas(&["a", "b", "c"]), "b", "new");
        assert_eq!(steps[0].action, RebaseAction::Pick);
        assert_eq!(
            steps[1].action,
            RebaseAction::Reword(Some("new".to_owned()))
        );
        assert_eq!(steps[2].action, RebaseAction::Pick);
    }

    #[test]
    fn squashing_rewords_the_first_and_squashes_the_rest() {
        let steps = squash_plan(&shas(&["a", "b", "c", "d"]), &shas(&["b", "c"]), "msg");
        let actions: Vec<_> = steps.iter().map(|step| step.action.clone()).collect();
        assert_eq!(
            actions,
            [
                RebaseAction::Pick,
                RebaseAction::Reword(Some("msg".to_owned())),
                RebaseAction::Squash(Some("msg".to_owned())),
                RebaseAction::Pick,
            ]
        );
    }

    #[test]
    fn worktrees_go_next_to_the_repository() {
        let folder = default_worktree_folder(Path::new("/work/app"), "feature/x").unwrap();
        assert_eq!(folder, Path::new("/work").join("app-feature-x"));
        assert_eq!(default_worktree_folder(Path::new("/"), "x"), None);
    }

    #[test]
    fn worktree_arguments_follow_what_is_checked_out() {
        let mut d = Dialog::default();
        d.open(DialogKind::CreateWorktree);
        d.text_a = "../wt".to_owned();
        d.sha = "abc".to_owned();
        assert_eq!(
            worktree_args(&d, false),
            ["worktree", "add", "--detach", "--", "../wt", "abc"]
        );
        d.ref_name = "topic".to_owned();
        assert_eq!(
            worktree_args(&d, false),
            ["worktree", "add", "--", "../wt", "topic"]
        );
        d.ref_remote = "origin".to_owned();
        assert_eq!(
            worktree_args(&d, false),
            [
                "worktree",
                "add",
                "--track",
                "-b",
                "topic",
                "--",
                "../wt",
                "origin/topic"
            ]
        );
        assert_eq!(
            worktree_args(&d, true),
            ["worktree", "add", "--", "../wt", "topic"]
        );
        // A folder that starts with '-' comes after `--`, so git does not read it as an option.
        d.text_a = "-folder".to_owned();
        assert_eq!(
            worktree_args(&d, true),
            ["worktree", "add", "--", "-folder", "topic"]
        );
    }
}
