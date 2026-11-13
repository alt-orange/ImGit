//! The command palette (Ctrl+P, `app.fuzzy`).
//
// The command palette (.fuzzy-finder): a modal 82px below the top, 600px wide, with a 38px react-select
// control (2px #4D88FF border + 1px ring) over a 200px list of 40px rows. Its commands are in alphabetical
// order; choosing Checkout / Open Repo / a stash command / Switch to Profile / Switch Theme lists the
// branches / repositories / stashes / profiles / themes next; the file commands (Blame, History, View / Edit /
// Open / Delete File, the external diff tool) list the repository's files.

use std::fs;
use std::path::{Path, PathBuf};

use dear_imgui_rs::{
    ChildFlags, DrawFlags, InputTextFlags, Key, MouseButton, StyleColor, StyleVar, WindowFlags,
};
use imgit_app::repo_state::{DiffSource, DiffTarget};
use imgit_app::{RepoState, WIP_SHA};
use imgit_git::{FileChange, FileStatus, RefType, cli};

use crate::app::action::Action;
use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::theme::Color;
use crate::ui::geom::vec2;
use crate::ui::panels::file_editor::navigate_from_editor;
use crate::ui::panels::{
    checkout_ref, gitflow_finish_items, gitflow_start_items, open_branch_cleanup, open_file_editor,
};
use crate::ui::popover::draw_box_shadow;
use crate::ui::{Cx, raw};

/// The FILE stage lists at most this many matches (the rows are drawn every frame).
const MAX_FILE_MATCHES: usize = 200;

/// What the palette lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Commands,
    Checkout,
    OpenRepo,
    StashApply,
    StashPop,
    SwitchProfile,
    SwitchTheme,
    File,
}

impl Stage {
    fn placeholder(self) -> &'static str {
        match self {
            Self::Checkout => "Checkout: select a branch",
            Self::OpenRepo => "Open Repo: select a repository",
            Self::StashApply => "Stash: Apply: select a stash",
            Self::StashPop => "Stash: Pop: select a stash",
            Self::SwitchProfile => "Search for a profile to switch to",
            Self::SwitchTheme => "Search for a theme to switch to",
            Self::File => "Select a file",
            Self::Commands => "Search for commands and actions (e.g., Open Repo)",
        }
    }
}

/// What the FILE stage does with the file picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FileAction {
    Blame,
    History,
    #[default]
    View,
    Edit,
    Open,
    Delete,
    ExternalDiff,
}

/// The repository's files, read again when HEAD or the status changes.
#[derive(Default)]
struct FileList {
    key: String,
    files: Vec<String>,
}

/// What the palette remembers between frames (the C++ file-level statics).
#[derive(Default)]
pub struct State {
    stage: Stage,
    file_action: FileAction,
    /// Open on the previous frame: the click that opened the palette (the toolbar's Actions button) is not a
    /// click outside it.
    was_open: bool,
    files: FileList,
}

/// One row: what it says and what choosing it does.
struct Entry {
    label: String,
    action: Action,
}

/// HEAD's tree plus the working directory's changed files, sorted and without duplicates.
fn file_list<'a>(tree: Vec<String>, changed: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut files = tree;
    files.extend(changed.map(str::to_owned));
    files.sort();
    files.dedup();
    files
}

/// What a cached [`FileList`] is good for: the repository, its HEAD, and the changed paths (their number and a
/// hash of them, so that replacing one changed file by another is a change too).
fn files_key<'a>(
    workdir: &Path,
    head: Option<&str>,
    changed: impl Iterator<Item = &'a str>,
) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut count = 0_usize;
    for path in changed {
        path.hash(&mut hasher);
        count += 1;
    }
    format!(
        "{}|{}|{count}|{:x}",
        workdir.display(),
        head.unwrap_or(""),
        hasher.finish()
    )
}

/// The repository's files: HEAD's tree plus the working directory's new files, sorted; read again when HEAD or
/// the changed files change.
fn repository_files<'a>(cache: &'a mut FileList, state: &RepoState) -> &'a [String] {
    let status = &state.status;
    let key = files_key(
        state.repo.workdir(),
        state.history.head_sha.as_deref(),
        status
            .unstaged
            .iter()
            .chain(&status.staged)
            .chain(&status.conflicted)
            .map(|change| change.path.as_str()),
    );
    if cache.key != key {
        cache.key = key;
        let tree = state
            .history
            .head_sha
            .as_deref()
            .and_then(|sha| state.repo.commit_tree_paths(sha).ok())
            .unwrap_or_default();
        cache.files = file_list(
            tree,
            status
                .unstaged
                .iter()
                .chain(&status.staged)
                .chain(&status.conflicted)
                .map(|change| change.path.as_str()),
        );
    }
    &cache.files
}

/// The working directory's change of `path` and whether it is in the index; `None` when it has none.
fn workdir_change(state: &RepoState, path: &str) -> Option<(FileChange, bool)> {
    let status = &state.status;
    [
        (&status.conflicted, false),
        (&status.unstaged, false),
        (&status.staged, true),
    ]
    .into_iter()
    .find_map(|(list, staged)| {
        list.iter()
            .find(|change| change.path == path)
            .map(|change| (change.clone(), staged))
    })
}

fn run_file_action(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    action: FileAction,
    path: &str,
) {
    let change = workdir_change(state, path);
    match action {
        FileAction::Blame | FileAction::History => {
            state.open_file_history(path, None, None, action == FileAction::Blame);
        }
        FileAction::View => {
            if let Some((change, staged)) = change {
                let path = path.to_owned();
                // Unsaved text in the file editor is asked about first.
                navigate_from_editor(app, state, move |_app, state| {
                    if state.open_diff(DiffTarget {
                        source: if staged {
                            DiffSource::Staged
                        } else {
                            DiffSource::Unstaged
                        },
                        path,
                        status: change.status,
                    }) {
                        state.file_view = true;
                    }
                });
            } else {
                state.open_file_history(path, None, None, false);
            }
        }
        FileAction::Edit => {
            open_file_editor(cx, app, state, path);
        }
        FileAction::Open => {
            let _ = imgit_platform::shell::open_path(&state.repo.workdir().join(path));
        }
        FileAction::Delete => {
            let workdir = state.repo.workdir().to_path_buf();
            let path = path.to_owned();
            app.dialog.open(DialogKind::Confirm);
            app.dialog.workdir = Some(workdir.clone());
            app.dialog.title = "Delete file".to_owned();
            app.dialog.message = format!(
                "Delete '{path}' from the working directory? A committed file can be brought back by discarding \
                 the deletion."
            );
            app.dialog.confirm_label = "Delete".to_owned();
            app.dialog.danger = true;
            app.dialog.on_confirm = Some(Box::new(move |_cx, app| {
                if let Err(error) = fs::remove_file(workdir.join(&path)) {
                    app.toasts.push_with_detail(
                        ToastKind::Error,
                        format!("Could not delete {path}"),
                        error.to_string(),
                    );
                    return;
                }
                app.with_repo(&workdir, |_, state| state.refresh_status());
            }));
        }
        FileAction::ExternalDiff => match change {
            Some((change, _)) if change.status == FileStatus::Conflicted => {
                app.run_git_cli(
                    state,
                    "External merge tool",
                    ["mergetool", "--no-prompt", "--", path],
                );
            }
            Some((_, staged)) => {
                let mut args = vec!["difftool", "-y"];
                if staged {
                    args.push("--cached");
                }
                args.extend(["--", path]);
                app.run_git_cli_silent(state, "Open in diff tool", args);
            }
            None => app.toasts.push_with_detail(
                ToastKind::Info,
                format!("No changes in {path}"),
                "The external diff tool opens files with uncommitted changes.",
            ),
        },
    }
}

/// Builds the rows of a stage. The commands need the active repository; `state` is `None` without one (its
/// commands then do nothing).
struct Rows {
    entries: Vec<Entry>,
    /// The active repository's working directory, found again by the actions that use it.
    workdir: Option<PathBuf>,
}

impl Rows {
    fn add(
        &mut self,
        label: impl Into<String>,
        action: impl FnOnce(&mut Cx<'_>, &mut Application) + 'static,
    ) {
        self.entries.push(Entry {
            label: label.into(),
            action: Box::new(action),
        });
    }

    /// A command of the active repository (nothing happens without one).
    fn add_repo(
        &mut self,
        label: impl Into<String>,
        action: impl FnOnce(&mut Cx<'_>, &mut Application, &mut RepoState) + 'static,
    ) {
        let workdir = self.workdir.clone();
        self.add(label, move |cx, app| {
            if let Some(workdir) = workdir {
                app.with_repo(&workdir, |app, state| action(cx, app, state));
            }
        });
    }

    /// [`Self::add_repo`] for a command that changes the repository: it only toasts "Wait for <title> to
    /// finish" while a git command is running on it ([`Application::with_idle_repo`]).
    fn add_repo_mutating(
        &mut self,
        label: impl Into<String>,
        action: impl FnOnce(&mut Cx<'_>, &mut Application, &mut RepoState) + 'static,
    ) {
        let workdir = self.workdir.clone();
        self.add(label, move |cx, app| {
            if let Some(workdir) = workdir {
                app.with_idle_repo(&workdir, |app, state| action(cx, app, state));
            }
        });
    }

    /// A command that lists `stage` next.
    fn next(&mut self, label: &str, stage: Stage) {
        self.add(label, move |_cx, app| {
            app.panels.fuzzy_finder.stage = stage;
            app.fuzzy.show();
        });
    }

    /// A command that lists the repository's files, then does `action` with the one picked.
    fn pick_file(&mut self, label: &str, action: FileAction) {
        let has_repo = self.workdir.is_some();
        self.add(label, move |_cx, app| {
            if has_repo {
                app.panels.fuzzy_finder.file_action = action;
                app.panels.fuzzy_finder.stage = Stage::File;
                app.fuzzy.show();
            }
        });
    }

    /// A command that opens the Preferences on `page`.
    fn preferences(&mut self, label: &str, page: &'static str) {
        self.add(label, move |_cx, app| {
            app.preferences_open = true;
            app.preferences_page = page.to_owned();
        });
    }

    /// A command that opens a dialog.
    fn dialog(&mut self, label: &str, kind: DialogKind) {
        self.add(label, move |_cx, app| app.dialog.open(kind));
    }
}

/// The rows of the stage the palette is at.
fn build_entries(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: Option<&mut RepoState>,
    palette: &mut State,
) -> Vec<Entry> {
    let mut rows = Rows {
        entries: Vec::new(),
        workdir: state.as_ref().map(|s| s.repo.workdir().to_path_buf()),
    };
    let stage = palette.stage;
    if stage == Stage::Checkout
        && let Some(state) = &state
    {
        for r in &state.history.refs {
            if matches!(r.ref_type, RefType::LocalBranch | RefType::RemoteBranch) {
                let target = r.clone();
                rows.add_repo(r.name.clone(), move |cx, app, state| {
                    checkout_ref(cx, app, state, &target);
                });
            }
        }
        return rows.entries;
    }
    if stage == Stage::File
        && let Some(state) = &state
    {
        let action = palette.file_action;
        for path in repository_files(&mut palette.files, state) {
            let path = path.clone();
            rows.add_repo(path.clone(), move |cx, app, state| {
                run_file_action(cx, app, state, action, &path);
            });
        }
        return rows.entries;
    }
    if stage == Stage::SwitchProfile {
        for profile in app.profiles.all() {
            let guid = profile.guid.clone();
            rows.add(profile.name.clone(), move |_cx, app| {
                app.switch_profile(&guid);
            });
        }
        return rows.entries;
    }
    if stage == Stage::SwitchTheme {
        for name in cx.state.theme_names.clone() {
            rows.add(name.clone(), move |cx, app| {
                app.settings.theme = name;
                app.apply_settings(cx);
            });
        }
        return rows.entries;
    }
    if stage == Stage::OpenRepo {
        for path in &app.recent_repos {
            let path = path.to_string_lossy().into_owned();
            rows.add(path.clone(), move |_cx, app| app.open_repository(&path));
        }
        rows.add("Browse for a repository...", |cx, app| {
            app.browse_for_repository(cx);
        });
        return rows.entries;
    }
    if matches!(stage, Stage::StashApply | Stage::StashPop)
        && let Some(state) = &state
    {
        let pop = stage == Stage::StashPop;
        for r in &state.history.refs {
            let (RefType::Stash, Some(index)) = (r.ref_type, r.stash_index) else {
                continue;
            };
            let label = if r.message.is_empty() {
                r.name.clone()
            } else {
                r.message.clone()
            };
            rows.add_repo_mutating(label, move |_cx, app, state| {
                let result = if pop {
                    state.repo.stash_pop(index)
                } else {
                    state.repo.stash_apply(index)
                };
                app.run(
                    state,
                    if pop { "Pop stash" } else { "Apply stash" },
                    result,
                    Some(if pop { "Stash popped" } else { "Stash applied" }),
                );
            });
        }
        return rows.entries;
    }

    rows.add("Activity Logs", |_cx, app| {
        app.notifications_requested = true;
    });
    rows.dialog("Add Remote", DialogKind::AddRemote);
    rows.add_repo("Apply patch", |cx, app, state| {
        if let Some(file) =
            imgit_platform::shell::open_file_dialog(Some(cx.shell.window), Some("patch"))
        {
            app.run_git_cli(
                state,
                "Apply patch",
                [
                    "apply".to_owned(),
                    "--".to_owned(),
                    file.to_string_lossy().into_owned(),
                ],
            );
        }
    });
    rows.pick_file("Blame", FileAction::Blame);
    rows.next("Checkout", Stage::Checkout);
    rows.add_repo("Clean Up Branches", |cx, app, state| {
        open_branch_cleanup(cx, app, state);
    });
    rows.dialog("Clone", DialogKind::CloneRepo);
    rows.add("Close Tab", |_cx, app| {
        if let Some(index) = app.active_tab {
            app.close_tab(index);
        }
    });
    rows.preferences("Configure Commit Signing", "Commit Signing");
    rows.preferences("Configure Gitflow", "Gitflow");
    rows.preferences("Configure LFS", "LFS");
    rows.add_repo("Create Annotated Tag", |_cx, app, state| {
        app.dialog.open(DialogKind::CreateTag);
        app.dialog.sha = state.history.head_sha.clone().unwrap_or_default();
        app.dialog.flag = true;
    });
    rows.add_repo("Create Branch", |_cx, app, state| {
        app.dialog.open(DialogKind::CreateBranch);
        app.dialog.flag = true;
        app.dialog.sha = state.history.head_sha.clone().unwrap_or_default();
    });
    rows.add_repo("Create File", |_cx, app, _state| {
        app.dialog.open(DialogKind::CreateFile);
    });
    rows.add_repo("Create Tag", |_cx, app, state| {
        app.dialog.open(DialogKind::CreateTag);
        app.dialog.sha = state.history.head_sha.clone().unwrap_or_default();
    });
    rows.add_repo(
        "Create patch from all working directory changes",
        |cx, _app, state| {
            if let Some(file) = imgit_platform::shell::save_file_dialog(
                Some(cx.shell.window),
                Some("changes.patch"),
                Some("patch"),
            ) {
                let output = cli::run(
                    state.repo.workdir(),
                    ["diff", "HEAD"],
                    cli::RunOptions::default(),
                );
                let _ = fs::write(file, output.text);
            }
        },
    );
    rows.add("Decrease Zoom", |cx, app| {
        app.set_zoom(cx, app.settings.zoom - 0.1);
    });
    rows.pick_file("Delete File", FileAction::Delete);
    rows.dialog("Discard all changes", DialogKind::DiscardAll);
    rows.pick_file("Edit File", FileAction::Edit);
    rows.add_repo("Fetch All", |_cx, app, state| {
        app.run_git_cli(state, "Fetch", ["fetch", "--all", "--prune"]);
    });
    // Gitflow: Finish <the checked-out gitflow branch>, Start Feature / Release / Hotfix (once initialized).
    if let Some(state) = state {
        let branch = state.history.head_branch.clone().unwrap_or_default();
        let finish = gitflow_finish_items(cx, app, state, &branch);
        let start = gitflow_start_items(cx, app, state);
        for flow in finish.into_iter().chain(start) {
            let label = format!("Gitflow: {}", flow.label);
            let click = flow.click;
            rows.add(label, move |cx, app| {
                if let Some(click) = click {
                    click(cx, app);
                }
            });
        }
    }
    rows.pick_file("History", FileAction::History);
    rows.add("Increase Zoom", |cx, app| {
        app.set_zoom(cx, app.settings.zoom + 0.1);
    });
    rows.add_repo("Initialize LFS on this repo", |_cx, app, state| {
        app.run_git_cli(state, "LFS install", ["lfs", "install"]);
    });
    rows.dialog("Initialize a Repo", DialogKind::InitRepo);
    rows.dialog("Keyboard Shortcuts", DialogKind::KeyboardShortcuts);
    rows.pick_file("Open File", FileAction::Open);
    rows.next("Open Repo", Stage::OpenRepo);
    rows.add("Open Repository Management Tab", |_cx, app| {
        app.repo_management_open = true;
    });
    rows.pick_file("Open in External Diff/Merge Tool", FileAction::ExternalDiff);
    rows.add_repo("Open in external editor", |_cx, app, state| {
        app.open_in_editor(state.repo.workdir());
    });
    rows.add_repo("Open in external terminal", |_cx, app, state| {
        app.open_terminal(true, state.repo.workdir());
    });
    rows.add_repo("Open in file manager", |_cx, _app, state| {
        let _ = imgit_platform::shell::open_in_file_manager(state.repo.workdir());
    });
    rows.add_repo("Perform Repo Maintenance", |_cx, app, state| {
        app.run_git_cli(state, "Git maintenance", ["maintenance", "run"]);
    });
    rows.add_repo("Prune Remote Branches", |_cx, app, state| {
        let remote = state
            .history
            .remotes
            .first()
            .map_or_else(|| "origin".to_owned(), |remote| remote.name.clone());
        app.run_git_cli(
            state,
            "Prune",
            [
                "remote".to_owned(),
                "prune".to_owned(),
                "--".to_owned(),
                remote,
            ],
        );
    });
    rows.add_repo("Pull", |_cx, app, state| {
        app.run_git_cli(state, "Pull", ["pull"]);
    });
    rows.add_repo("Push", |_cx, app, state| {
        app.run_git_cli(state, "Push", ["push"]);
    });
    rows.add_repo("Redo", |_cx, app, state| app.redo(state));
    rows.add_repo("Rename Branch", |_cx, app, state| {
        let branch = state.history.head_branch.clone().unwrap_or_default();
        app.dialog.open(DialogKind::RenameBranch);
        app.dialog.ref_name = branch.clone();
        app.dialog.text_a = branch;
    });
    rows.add("Reset Zoom", |cx, app| app.set_zoom(cx, 1.0));
    rows.add_repo("Search Commits", |_cx, _app, state| {
        state.search_open = true;
        state.search_focus_requested = true;
        state.run_search(); // leftover text searches again
    });
    rows.add("Settings", |_cx, app| {
        app.preferences_open = true;
    });
    rows.add_repo_mutating("Stage all changes", |_cx, app, state| {
        let result = state.repo.stage_all();
        app.run(state, "Stage all", result, None);
    });
    rows.next("Stash: Apply", Stage::StashApply);
    rows.add_repo("Stash: Create", |_cx, _app, state| {
        state.composer_stash = true;
    });
    rows.next("Stash: Pop", Stage::StashPop);
    rows.next("Switch Theme", Stage::SwitchTheme);
    rows.next("Switch to Profile", Stage::SwitchProfile);
    rows.add("Toggle Commit Details Panel", |_cx, app| {
        app.show_detail_panel = !app.show_detail_panel;
    });
    rows.add("Toggle Left Panel", |_cx, app| {
        app.show_left_panel = !app.show_left_panel;
    });
    rows.add("Toggle Syntax Highlighting", |_cx, app| {
        app.settings.syntax_highlighting = !app.settings.syntax_highlighting;
        app.settings.save(&app.config_path("settings.txt"));
    });
    rows.add("Toggle Toolbar Labels", |_cx, app| {
        app.settings.show_toolbar_labels = !app.settings.show_toolbar_labels;
        app.settings.save(&app.config_path("settings.txt"));
    });
    rows.add("Toggle terminal panel", |_cx, app| {
        app.show_terminal_panel = !app.show_terminal_panel;
        app.terminal.focus_requested = app.show_terminal_panel;
    });
    rows.add_repo("Undo", |_cx, app, state| app.undo(state));
    rows.add_repo_mutating("Unstage all changes", |_cx, app, state| {
        let result = state.repo.unstage_all();
        app.run(state, "Unstage all", result, None);
    });
    rows.pick_file("View File", FileAction::View);
    rows.add_repo("View Working Directory Changes", |_cx, _app, state| {
        state.select_only(WIP_SHA);
    });
    rows.entries
}

/// The rows matching `query` (all of them while it is empty, in order), the best first (ties keep their
/// order). The FILE stage keeps only its first [`MAX_FILE_MATCHES`].
fn rank<'a>(
    labels: impl Iterator<Item = &'a str>,
    query: &str,
    limit: Option<usize>,
) -> Vec<usize> {
    let mut scored: Vec<(usize, i32)> = labels
        .enumerate()
        .map(|(index, label)| {
            (
                index,
                if query.is_empty() {
                    0
                } else {
                    crate::ui::widgets::fuzzy_score(label, query)
                },
            )
        })
        .filter(|&(_, score)| score >= 0)
        .collect();
    if !query.is_empty() {
        scored.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
    }
    let mut matches: Vec<usize> = scored.into_iter().map(|(index, _)| index).collect();
    if let Some(limit) = limit {
        matches.truncate(limit);
    }
    matches
}

/// The palette while `app.fuzzy.open`.
pub fn draw_fuzzy_finder(cx: &mut Cx<'_>, app: &mut Application) {
    if !app.fuzzy.open {
        let palette = &mut app.panels.fuzzy_finder;
        palette.stage = Stage::Commands;
        palette.was_open = false;
        return;
    }
    let chosen = app.with_panel_state(
        |panels| &mut panels.fuzzy_finder,
        |app, palette| draw_palette(cx, app, palette),
    );
    if let Some((action, before)) = chosen {
        action(cx, app);
        let palette = &mut app.panels.fuzzy_finder;
        if palette.stage == before {
            palette.stage = Stage::Commands;
        }
    }
}

/// Draws the modal; the row that was chosen this frame (clicked, or entered) with the stage it was chosen at.
fn draw_palette(
    cx: &mut Cx<'_>,
    app: &mut Application,
    palette: &mut State,
) -> Option<(Action, Stage)> {
    let ui = cx.ui;
    let p = cx.palette;
    let just_opened = !palette.was_open;
    palette.was_open = true;
    let (work_pos, work_size) = raw::work_area(ui);
    raw::set_next_window_focus(ui);
    let _padding = ui.push_style_var(StyleVar::WindowPadding([0.0, 0.0]));
    let _border = ui.push_style_var(StyleVar::WindowBorderSize(0.0));
    let _spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, 0.0]));
    let mut chosen: Option<(Action, Stage)> = None;
    ui.window("##fuzzy_finder")
        .position(work_pos.into(), dear_imgui_rs::Condition::Always)
        .size(work_size.into(), dear_imgui_rs::Condition::Always)
        .flags(
            WindowFlags::NO_DECORATION
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_BACKGROUND
                | WindowFlags::NO_NAV,
        )
        .build(|| {
            let draw = cx.window_draw_list();
            // .modal backdrop
            draw.add_rect_filled(
                work_pos,
                work_pos + work_size,
                Color::from_rgba8(0, 0, 0, 128),
                0.0,
            );

            let stage = palette.stage;
            let mut entries =
                match app.with_active(|app, state| build_entries(cx, app, Some(state), palette)) {
                    Some(entries) => entries,
                    None => build_entries(cx, app, None, palette),
                };
            let limit = (stage == Stage::File).then_some(MAX_FILE_MATCHES); // type to narrow the list
            let matches = rank(
                entries.iter().map(|entry| entry.label.as_str()),
                &app.fuzzy.text,
                limit,
            );
            app.fuzzy.selected = app.fuzzy.selected.min(matches.len().saturating_sub(1));

            let width = cx.px(600.0);
            let min = vec2(
                (work_pos.x + (work_size.x - width) / 2.0).round(),
                work_pos.y + cx.px(82.0),
            );
            let control_max = vec2(min.x + width, min.y + cx.px(38.0));
            let focus = p.accent;
            // .modal-content shadow, the control (2px border + 1px ring), its arrow.
            let list_h = if matches.is_empty() {
                0.0
            } else {
                cx.px(200.0).min(cx.px(40.0) * matches.len() as f32)
            };
            draw_box_shadow(
                draw,
                min,
                vec2(control_max.x, control_max.y + list_h),
                vec2(0.0, cx.px(5.0)),
                cx.px(15.0),
                0.0,
                Color::new(0.0, 0.0, 0.0, 0.5),
                0.0,
            );
            draw.add_rect_filled_corners(
                vec2(min.x - 1.0, min.y - 1.0),
                vec2(control_max.x + 1.0, control_max.y),
                focus,
                cx.px(5.0),
                DrawFlags::ROUND_TOP,
            );
            draw.add_rect_filled_corners(
                vec2(min.x + cx.px(2.0), min.y + cx.px(2.0)),
                vec2(control_max.x - cx.px(2.0), control_max.y - cx.px(2.0)),
                p.panel_bg1,
                cx.px(2.0),
                DrawFlags::ROUND_TOP,
            );
            let arrow = vec2(control_max.x - cx.px(22.0), min.y + cx.px(17.0));
            draw.add_triangle_filled(
                vec2(arrow.x, arrow.y + cx.px(5.0)),
                vec2(arrow.x + cx.px(10.0), arrow.y + cx.px(5.0)),
                vec2(arrow.x + cx.px(5.0), arrow.y),
                Color::from_rgba8(153, 153, 153, 255),
            );

            // The input: 14px, the text box 7px below the control's inner top (34px line).
            let font = cx.fonts.regular;
            let ascent = 14.0 * 2189.0 / 2048.0 * cx.scale;
            let baseline = (min.y + cx.px(2.0 + (34.0 - 19.0) / 2.0) + ascent.round()).round();
            cx.set_cursor(vec2(min.x + cx.px(12.0), baseline - ascent.ceil()));
            if app.fuzzy.focus_requested {
                ui.set_keyboard_focus_here();
                app.fuzzy.focus_requested = false;
            }
            let text_changed = {
                let _background = ui.push_style_color(StyleColor::FrameBg, Color::TRANSPARENT);
                let _text = ui.push_style_color(StyleColor::Text, p.ink(0.75));
                let _hint = ui.push_style_color(StyleColor::TextDisabled, p.ink(0.4));
                let _frame_padding = ui.push_style_var(StyleVar::FramePadding([0.0, 0.0]));
                let _font = cx.push_font(font, 14.0);
                cx.input_text(
                    "##fuzzy_input",
                    &mut app.fuzzy.text,
                    Some(stage.placeholder()),
                    InputTextFlags::empty(),
                    width - cx.px(50.0),
                )
            };
            if text_changed {
                app.fuzzy.selected = 0;
            }

            let down = ui.is_key_pressed(Key::DownArrow);
            let up = ui.is_key_pressed(Key::UpArrow);
            if down {
                app.fuzzy.selected = (app.fuzzy.selected + 1).min(matches.len().saturating_sub(1));
            }
            if up {
                app.fuzzy.selected = app.fuzzy.selected.saturating_sub(1);
            }
            let execute = ui.is_key_pressed(Key::Enter) || ui.is_key_pressed(Key::KeypadEnter);
            let list_min = vec2(min.x, control_max.y);
            let list_max = vec2(control_max.x, control_max.y + list_h);
            if ui.is_key_pressed(Key::Escape)
                || (!just_opened
                    && ui.is_mouse_clicked(MouseButton::Left)
                    && !ui.is_mouse_hovering_rect([min.x - 1.0, min.y - 1.0], list_max.to_array()))
            {
                app.fuzzy.open = false;
            }

            let mut executed: Option<usize> = None;
            if !matches.is_empty() {
                // .Select-menu-outer: #272A31, 1px rgba(255,255,255,.08) border, bottom corners 4px.
                draw.add_rect_filled_corners(
                    list_min,
                    list_max,
                    p.ink(0.08),
                    cx.px(4.0),
                    DrawFlags::ROUND_BOTTOM,
                );
                draw.add_rect_filled_corners(
                    vec2(list_min.x + 1.0, list_min.y + 1.0),
                    vec2(list_max.x - 1.0, list_max.y - 1.0),
                    p.panel_bg0,
                    cx.px(3.0),
                    DrawFlags::ROUND_BOTTOM,
                );
                cx.set_cursor(vec2(list_min.x + 1.0, list_min.y + 1.0));
                let _scrollbar = ui.push_style_var(StyleVar::ScrollbarSize(cx.px(8.0)));
                ui.child_window("##fuzzy_list")
                    .size([width - 2.0, list_h - 2.0])
                    .child_flags(ChildFlags::NONE)
                    .flags(WindowFlags::NO_BACKGROUND)
                    .build(ui, || {
                        let list = cx.window_draw_list();
                        let row_w = cx.avail().x;
                        for (i, &entry) in matches.iter().enumerate() {
                            let pos = cx.cursor();
                            let hovered = {
                                let _id = ui.push_id(i);
                                if ui.invisible_button("##entry", vec2(row_w, cx.px(40.0))) {
                                    executed = Some(i);
                                }
                                ui.is_item_hovered()
                            };
                            let row_end = vec2(pos.x + row_w, pos.y + cx.px(39.0));
                            if i == app.fuzzy.selected {
                                list.add_rect_filled(pos, row_end, p.selected_row, 0.0);
                                if down || up {
                                    ui.set_scroll_here_y(0.5);
                                }
                            } else if hovered {
                                list.add_rect_filled(pos, row_end, p.hover_row, 0.0);
                            }
                            list.add_rect_filled(
                                vec2(pos.x, pos.y + cx.px(39.0)),
                                vec2(pos.x + row_w, pos.y + cx.px(40.0)),
                                p.ink(0.08),
                                0.0,
                            );
                            cx.draw_text_ellipsis(
                                list,
                                font,
                                14.0,
                                vec2(pos.x + cx.px(10.0), pos.y + cx.px(8.0)),
                                row_w - cx.px(20.0),
                                p.ink(0.75),
                                &entries[entry].label,
                            );
                        }
                    });
            }
            if execute && !matches.is_empty() {
                executed = Some(app.fuzzy.selected);
            }
            if let Some(row) = executed.and_then(|i| matches.get(i)) {
                app.fuzzy.open = false;
                chosen = Some((entries.swap_remove(*row).action, stage));
            }
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_lists_everything_in_order() {
        let labels = ["Undo", "Redo", "Blame"];
        assert_eq!(rank(labels.into_iter(), "", None), [0, 1, 2]);
    }

    #[test]
    fn matches_are_ordered_best_first_and_ties_keep_their_order() {
        // A contained query scores 1000 - length: shorter labels first; equal lengths stay in order.
        let labels = [
            "Open File",
            "Open Repo",
            "Toggle Left Panel",
            "Zoom",
            "Open Repository Management Tab",
        ];
        let ranked = rank(labels.into_iter(), "open", None);
        assert_eq!(ranked, [0, 1, 4]);
        let labels = ["Stash: Pop", "Stash: Apply", "Stage all changes"];
        assert_eq!(rank(labels.into_iter(), "stash", None), [0, 1]);
    }

    #[test]
    fn a_subsequence_matches_below_a_substring() {
        let labels = ["Create Branch", "Checkout"];
        // "cb": a subsequence of "Create Branch" only.
        assert_eq!(rank(labels.into_iter(), "cb", None), [0]);
        let labels = ["a long Checkout row", "Checkout"];
        assert_eq!(rank(labels.into_iter(), "checkout", None), [1, 0]);
    }

    #[test]
    fn the_file_stage_keeps_the_first_matches_only() {
        let labels: Vec<String> = (0..500).map(|i| format!("src/file{i}.rs")).collect();
        let ranked = rank(
            labels.iter().map(String::as_str),
            "",
            Some(MAX_FILE_MATCHES),
        );
        assert_eq!(ranked.len(), MAX_FILE_MATCHES);
        assert_eq!(ranked[0], 0);
        assert!(rank(labels.iter().map(String::as_str), "nothing like it", None).is_empty());
    }

    #[test]
    fn replacing_one_changed_file_by_another_changes_the_files_key() {
        let key = |head: Option<&str>, changed: &[&str]| {
            files_key(Path::new("/repo"), head, changed.iter().copied())
        };
        let before = key(Some("abc"), &["a.txt"]);
        // Same number of changed files, another one: the cached list would be stale.
        assert_ne!(before, key(Some("abc"), &["b.txt"]));
        assert_eq!(before, key(Some("abc"), &["a.txt"]));
        assert_ne!(before, key(Some("def"), &["a.txt"]));
        assert_ne!(before, key(Some("abc"), &["a.txt", "b.txt"]));
        // Paths are told apart where they join.
        assert_ne!(key(None, &["ab", "c"]), key(None, &["a", "bc"]));
        assert_ne!(
            files_key(Path::new("/other"), Some("abc"), ["a.txt"].into_iter()),
            before
        );
    }

    #[test]
    fn the_file_list_is_sorted_and_without_duplicates() {
        let tree = vec!["src/main.rs".to_owned(), "README.md".to_owned()];
        let files = file_list(tree, ["src/new.rs", "README.md"].into_iter());
        assert_eq!(files, ["README.md", "src/main.rs", "src/new.rs"]);
    }

    #[test]
    fn every_stage_has_its_own_placeholder() {
        assert_eq!(
            Stage::Commands.placeholder(),
            "Search for commands and actions (e.g., Open Repo)"
        );
        assert_eq!(Stage::StashPop.placeholder(), "Stash: Pop: select a stash");
        assert_eq!(Stage::File.placeholder(), "Select a file");
    }
}
