//! The menus of files and folders: working directory files, files of a commit, several files, a folder, and a
//! commit of the File History view.

use std::io::Write as _;
use std::path::Path;

use imgit_app::preferences::preferences;
use imgit_app::repo_state::{DiffSource, DiffTarget};
use imgit_app::text::file_name;
use imgit_app::{RepoState, WIP_SHA};
use imgit_git::{FileChange, FileStatus, Sha};

use super::commits::git_args;
use super::shared::{
    OperationState, confirm, full_path, repo_item, repo_view_item, save_git_output,
    selected_commits, workdir_of,
};
use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::ui::Cx;
use crate::ui::menu::{MenuItem, MenuItems, menu_item, menu_submenu, separated_menu};
use crate::ui::panels::file_editor::{navigate_from_editor, open_file_editor};

/// Escapes what `.gitignore` reads as syntax, so that `text` matches only itself: a backslash, the glob
/// characters `*`, `?` and `[`, a leading `#` (a comment) or `!` (a negation) and the trailing spaces git
/// would drop.
fn escape_ignore_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 2);
    for (index, ch) in text.chars().enumerate() {
        if matches!(ch, '\\' | '*' | '?' | '[') || (index == 0 && matches!(ch, '#' | '!')) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    // "Trailing spaces are ignored unless they are quoted with backslash".
    let kept = escaped.trim_end_matches(' ').len();
    let spaces = escaped.len() - kept;
    escaped.truncate(kept);
    escaped.push_str(&"\\ ".repeat(spaces));
    escaped
}

/// The `.gitignore` line for `path` (relative to the working directory, '/' or '\\' separated): the file, or
/// with `folder` the folder and everything in it. Escaped, and anchored with a leading '/' when it sits at the
/// root, so that only that file (folder) is ignored and not every one of that name below it; a path with a
/// folder in it is anchored by git already. `None` for a path no line can name (empty, or with a line break).
fn ignore_line(path: &str, folder: bool) -> Option<String> {
    let path = path.replace('\\', "/");
    let path = path.trim_matches('/');
    if path.is_empty() || path.contains(['\n', '\r']) {
        return None;
    }
    let mut line = String::new();
    if !path.contains('/') {
        line.push('/');
    }
    line.push_str(&escape_ignore_text(path));
    if folder {
        line.push('/');
    }
    Some(line)
}

/// The `.gitignore` line for every file with the extension `extension` (without its dot): `*.rs`.
fn ignore_extension_line(extension: &str) -> Option<String> {
    (!extension.is_empty() && !extension.contains(['\n', '\r', '/']))
        .then(|| format!("*.{}", escape_ignore_text(extension)))
}

/// What to append to a `.gitignore` whose bytes are `existing` so that it holds each of `lines`: the missing
/// ones only (a line that is there already, or an unanchored copy of it, is skipped), each ended like the
/// file's own lines (CRLF when it uses them), after a line break when the last line has none. Returns the
/// bytes and how many lines they hold.
fn gitignore_additions(existing: &[u8], lines: &[String]) -> (Vec<u8>, usize) {
    let eol: &[u8] = if existing.windows(2).any(|pair| pair == b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    };
    let mut present: std::collections::HashSet<Vec<u8>> = existing
        .split(|&byte| byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line).to_vec())
        .collect();
    let mut added = Vec::new();
    let mut count = 0;
    for line in lines {
        // `/name` is covered by `name`, which ignores every file of that name.
        let unanchored = line
            .strip_prefix('/')
            .filter(|rest| !rest.trim_end_matches('/').contains('/'));
        if present.contains(line.as_bytes())
            || unanchored.is_some_and(|rest| present.contains(rest.as_bytes()))
        {
            continue;
        }
        present.insert(line.as_bytes().to_vec());
        added.extend_from_slice(line.as_bytes());
        added.extend_from_slice(eol);
        count += 1;
    }
    if count > 0 && !existing.is_empty() && !existing.ends_with(b"\n") {
        let mut with_break = eol.to_vec();
        with_break.extend(added);
        added = with_break;
    }
    (added, count)
}

/// Appends `lines` to the repository's `.gitignore` (created when missing) as [`gitignore_additions`] says;
/// returns how many were added (0: they were all there).
fn append_to_gitignore(workdir: &Path, lines: &[String]) -> std::io::Result<usize> {
    let path = workdir.join(".gitignore");
    let existing = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let (added, count) = gitignore_additions(&existing, lines);
    if count > 0 {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(&added)?;
    }
    Ok(count)
}

/// The toast of an append: what was added, or that it was there.
fn report_ignored(app: &mut Application, result: std::io::Result<usize>, what: String) {
    match result {
        Ok(0) => app
            .toasts
            .push_with_detail(ToastKind::Info, "Already in .gitignore", what),
        Ok(_) => app
            .toasts
            .push_with_detail(ToastKind::Success, "Added to .gitignore", what),
        Err(error) => app.toasts.push_with_detail(
            ToastKind::Error,
            "Could not write .gitignore",
            error.to_string(),
        ),
    }
}

/// The item that adds the line `pattern` to `.gitignore` and says so.
fn ignore_item(label: String, workdir: &Path, pattern: String) -> MenuItem {
    repo_view_item(label, workdir, move |_cx, app, state| {
        let result = append_to_gitignore(state.repo.workdir(), std::slice::from_ref(&pattern));
        report_ignored(app, result, pattern);
        state.refresh_status();
    })
}

/// "Ignore": the file, its extension, its folder.
fn ignore_submenu(workdir: &Path, path: &str) -> MenuItem {
    let p = Path::new(path);
    let mut items = Vec::new();
    if let Some(line) = ignore_line(path, false) {
        items.push(ignore_item(
            format!(
                "Ignore '{}'",
                p.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            ),
            workdir,
            line,
        ));
    }
    if let Some(extension) = p.extension()
        && let Some(line) = ignore_extension_line(&extension.to_string_lossy())
    {
        items.push(ignore_item(
            format!(
                "All files with the extension '.{}'",
                extension.to_string_lossy()
            ),
            workdir,
            line,
        ));
    }
    if let Some(parent) = p.parent().filter(|parent| !parent.as_os_str().is_empty())
        && let Some(line) = ignore_line(&parent.to_string_lossy(), true)
    {
        items.push(ignore_item(
            format!(
                "All files in '{}/'",
                parent
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            ),
            workdir,
            line,
        ));
    }
    menu_submenu("Ignore", items)
}

/// "Open in external editor", "Open file in default program", "Show in folder": for a file that exists.
fn open_file_items(workdir: &Path, path: &str) -> MenuItems {
    let full = std::path::PathBuf::from(full_path(workdir, path));
    if !full.is_file() {
        return MenuItems::new();
    }
    let (editor, default_program, folder) = (full.clone(), full.clone(), full);
    vec![
        menu_item("Open in external editor", move |_cx, app| {
            app.open_in_editor(&editor);
        }),
        menu_item("Open file in default program", move |_cx, _app| {
            let _ = imgit_platform::shell::open_path(&default_program);
        }),
        menu_item("Show in folder", move |_cx, _app| {
            let _ = imgit_platform::shell::show_in_folder(&folder);
        }),
    ]
}

/// "Edit file" and "Delete file", for a file that exists.
fn edit_delete_items(workdir: &Path, path: &str) -> MenuItems {
    let full = std::path::PathBuf::from(full_path(workdir, path));
    if !full.is_file() {
        return MenuItems::new();
    }
    let (edit_path, delete_path) = (path.to_owned(), path.to_owned());
    let delete_workdir = workdir.to_path_buf();
    vec![
        repo_view_item("Edit file", workdir, move |cx, app, state| {
            open_file_editor(cx, app, state, &edit_path);
        }),
        menu_item("Delete file", move |_cx, app| {
            let path = delete_path.clone();
            confirm(
                app,
                "Delete File",
                format!("Are you sure you want to delete {path}?"),
                "Delete",
                true,
                move |_cx, app| {
                    app.with_repo(&delete_workdir, |app, state| {
                        if let Err(error) = std::fs::remove_file(&full) {
                            app.toasts.push_with_detail(
                                ToastKind::Error,
                                format!("Could not delete {path}"),
                                error.to_string(),
                            );
                        }
                        state.refresh_status();
                    });
                },
            );
        }),
    ]
}

/// File History / File Blame: the File History view of `path` at `sha` (HEAD when empty: the working
/// directory's files).
fn history_items(workdir: &Path, path: &str, sha: &str) -> MenuItems {
    let from: Option<Sha> = Some(sha.to_owned()).filter(|sha| !sha.is_empty() && sha != WIP_SHA);
    [("File History", false), ("File Blame", true)]
        .into_iter()
        .map(|(label, blame)| {
            let (path, from) = (path.to_owned(), from.clone());
            repo_view_item(label, workdir, move |_cx, _app, state| {
                state.open_file_history(path, from.clone(), from, blame);
            })
        })
        .collect()
}

/// The path of the change and, for a rename, the one it came from.
fn paths_of(change: &FileChange) -> Vec<String> {
    let mut paths = vec![change.path.clone()];
    paths.extend(change.old_path.clone());
    paths
}

/// The staged (or unstaged) changes of `state` among `paths`.
fn changes_among(state: &RepoState, paths: &[String], staged: bool) -> Vec<FileChange> {
    let list = if staged {
        &state.status.staged
    } else {
        &state.status.unstaged
    };
    list.iter()
        .filter(|change| paths.contains(&change.path))
        .cloned()
        .collect()
}

/// `changes` that were staged until just now, as the working directory lists them once unstaged: a new file
/// (`Added`) is untracked then, which is what `Repository::discard` deletes.
fn as_unstaged(changes: impl IntoIterator<Item = FileChange>) -> Vec<FileChange> {
    changes
        .into_iter()
        .map(|mut change| {
            if change.status == FileStatus::Added {
                change.status = FileStatus::Untracked;
            }
            change
        })
        .collect()
}

/// A discard of the working directory changes of `paths` (unstaged first when they are staged).
fn discard_paths(app: &mut Application, state: &mut RepoState, paths: &[String], staged: bool) {
    let mut changes = changes_among(state, paths, staged);
    if staged {
        let unstaged = state.repo.unstage(paths);
        // A file that is still staged must not be deleted.
        if app.run(state, "Unstage", unstaged, None).is_none() {
            return;
        }
        changes = as_unstaged(changes);
    }
    let result = state.repo.discard(&changes);
    app.run(state, "Discard", result, None);
}

fn diff_args(staged: bool, paths: &[String]) -> Vec<String> {
    let mut args = git_args(&["diff"]);
    if staged {
        args.push("--cached".to_owned());
    }
    args.push("--".to_owned());
    args.extend(paths.iter().cloned());
    args
}

fn stash_args(paths: &[String]) -> Vec<String> {
    let mut args = git_args(&["stash", "push", "--"]);
    args.extend(paths.iter().cloned());
    args
}

#[must_use]
pub fn workdir_file_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    change: &FileChange,
    staged: bool,
) -> MenuItems {
    let workdir = workdir_of(state);
    let path = change.path.clone();
    let copy = change.clone();
    let added = matches!(change.status, FileStatus::Added | FileStatus::Untracked);
    let conflicted = change.status == FileStatus::Conflicted;
    let mut ops = MenuItems::new();
    if conflicted {
        let target = change.clone();
        ops.push(repo_view_item(
            "Open in merge tool",
            &workdir,
            move |_cx, app, state| {
                navigate_from_editor(app, state, move |app, state| {
                    if let Err(error) = state.open_merge_tool(&target.path) {
                        app.toasts.push_with_detail(
                            ToastKind::Error,
                            "Could not open the merge tool",
                            error.to_string(),
                        );
                        let _ = state.open_diff(DiffTarget {
                            source: DiffSource::Unstaged,
                            path: target.path,
                            status: target.status,
                        });
                    }
                });
            },
        ));
        if preferences().external_merge_tool_enabled() {
            let path = path.clone();
            ops.push(repo_item(
                "Open in external merge tool",
                &workdir,
                move |_cx, app, state| {
                    app.run_git_cli(
                        state,
                        "External merge tool",
                        ["mergetool", "--no-prompt", "--", path.as_str()],
                    );
                },
            ));
        }
        let path = path.clone();
        ops.push(repo_item(
            "Mark resolved",
            &workdir,
            move |_cx, app, state| {
                let result = state.repo.stage(&[path]);
                app.run(state, "Mark resolved", result, None);
            },
        ));
    } else if staged {
        let paths = paths_of(change);
        ops.push(repo_item("Unstage", &workdir, move |_cx, app, state| {
            let result = state.repo.unstage(&paths);
            app.run(state, "Unstage", result, None);
        }));
    } else {
        let path = path.clone();
        ops.push(repo_item("Stage", &workdir, move |_cx, app, state| {
            let result = state.repo.stage(&[path]);
            app.run(state, "Stage", result, None);
        }));
    }
    let discard = |label: &str| {
        let copy = copy.clone();
        repo_item(label, &workdir, move |_cx, app, state| {
            let mut change = copy.clone();
            if staged {
                let paths = paths_of(&copy);
                if state.repo.unstage(&paths).is_err() {
                    return;
                }
                change = as_unstaged([change]).remove(0);
            }
            let message = format!("Discarded changes to {}", file_name(&copy.path));
            let result = state.repo.discard(std::slice::from_ref(&change));
            app.run(state, "Discard", result, Some(&message));
        })
    };
    if !conflicted {
        if added && staged {
            ops.push(discard("Unstage and delete file"));
        } else if !added {
            ops.push(discard("Discard changes"));
        }
    }
    if file_name(&path) != ".gitattributes" && file_name(&path) != ".gitignore" {
        ops.push(ignore_submenu(&workdir, &path));
    }
    if !conflicted && state.repo.state() != OperationState::Merge {
        let paths = [path.clone()];
        ops.push(repo_item("Stash file", &workdir, move |_cx, app, state| {
            app.run_git_cli(state, "Stash file", stash_args(&paths));
        }));
    }
    let history = if added {
        MenuItems::new()
    } else {
        history_items(&workdir, &path, "")
    };
    let mut open = MenuItems::new();
    if preferences().external_diff_tool_enabled() && !conflicted {
        let path = path.clone();
        open.push(repo_view_item(
            "Open in external diff tool",
            &workdir,
            move |_cx, app, state| {
                let mut args = git_args(&["difftool", "-y"]);
                if staged {
                    args.push("--cached".to_owned());
                }
                args.push("--".to_owned());
                args.push(path);
                app.run_git_cli_silent(state, "Open in diff tool", args);
            },
        ));
    }
    open.extend(open_file_items(&workdir, &path));
    let (copy_path, patch_path) = (path.clone(), path.clone());
    let copy_workdir = workdir.clone();
    separated_menu(vec![
        ops,
        history,
        open,
        vec![
            menu_item("Copy file path", move |cx, app| {
                app.copy_to_clipboard(cx, &full_path(&copy_workdir, &copy_path));
            }),
            repo_view_item(
                "Create patch from file changes",
                &workdir,
                move |cx, app, state| {
                    let args = diff_args(staged, std::slice::from_ref(&patch_path));
                    let workdir = state.repo.workdir().to_path_buf();
                    save_git_output(
                        cx,
                        app,
                        &workdir,
                        &format!("{}.patch", file_name(&patch_path)),
                        &args,
                    );
                },
            ),
        ],
        edit_delete_items(&workdir, &path),
    ])
}

#[must_use]
pub fn commit_file_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    sha: &str,
    change: &FileChange,
) -> MenuItems {
    let workdir = workdir_of(state);
    let path = change.path.clone();
    let stash = state
        .commit_for(sha)
        .is_some_and(|commit| commit.node_type == imgit_git::NodeType::Stash);
    let checkout_file = |label: &str, title: &'static str| {
        let (sha, path) = (sha.to_owned(), path.clone());
        repo_item(label, &workdir, move |_cx, app, state| {
            app.run_git_cli(
                state,
                title,
                ["checkout", sha.as_str(), "--", path.as_str()],
            );
        })
    };
    let mut apply = MenuItems::new();
    if stash {
        apply.push(checkout_file("Apply stashed file", "Apply stashed file"));
    }
    let mut restore = MenuItems::new();
    if !stash && selected_commits(state).len() == 1 {
        restore.push(checkout_file(
            "Restore file from this commit",
            "Restore file",
        ));
    }
    let mut open = MenuItems::new();
    if preferences().external_diff_tool_enabled() {
        let (sha, path) = (sha.to_owned(), path.clone());
        open.push(repo_view_item(
            "Open in external diff tool",
            &workdir,
            move |_cx, app, state| {
                app.run_git_cli_silent(
                    state,
                    "Open in diff tool",
                    [
                        "difftool",
                        "-y",
                        format!("{sha}^!").as_str(),
                        "--",
                        path.as_str(),
                    ],
                );
            },
        ));
    }
    open.extend(open_file_items(&workdir, &path));
    let copy_path = path.clone();
    let copy_workdir = workdir.clone();
    separated_menu(vec![
        if stash {
            MenuItems::new()
        } else {
            history_items(&workdir, &path, sha)
        },
        open,
        vec![menu_item("Copy file path", move |cx, app| {
            app.copy_to_clipboard(cx, &full_path(&copy_workdir, &copy_path));
        })],
        edit_delete_items(&workdir, &path),
        apply,
        restore,
    ])
}

#[must_use]
pub fn multiple_files_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    paths: Vec<String>,
    staged: bool,
) -> MenuItems {
    let workdir = workdir_of(state);
    let n = paths.len();
    let mut ops = MenuItems::new();
    let action_paths = paths.clone();
    if staged {
        ops.push(repo_item(
            format!("Unstage {n} files"),
            &workdir,
            move |_cx, app, state| {
                let result = state.repo.unstage(&action_paths);
                app.run(state, "Unstage", result, None);
            },
        ));
    } else {
        ops.push(repo_item(
            format!("Stage {n} files"),
            &workdir,
            move |_cx, app, state| {
                let result = state.repo.stage(&action_paths);
                app.run(state, "Stage", result, None);
            },
        ));
    }
    let discard_paths_list = paths.clone();
    ops.push(repo_item(
        format!("Discard {n} files"),
        &workdir,
        move |_cx, app, state| discard_paths(app, state, &discard_paths_list, staged),
    ));
    let ignore_paths = paths.clone();
    ops.push(repo_view_item(
        format!("Ignore {n} files"),
        &workdir,
        move |_cx, app, state| {
            let lines: Vec<String> = ignore_paths
                .iter()
                .filter_map(|path| ignore_line(path, false))
                .collect();
            let result = append_to_gitignore(state.repo.workdir(), &lines);
            report_ignored(app, result, format!("{} files", lines.len()));
            state.refresh_status();
        },
    ));
    if state.repo.state() != OperationState::Merge {
        let stash_paths = paths.clone();
        ops.push(repo_item(
            format!("Stash {n} files"),
            &workdir,
            move |_cx, app, state| app.run_git_cli(state, "Stash files", stash_args(&stash_paths)),
        ));
    }
    separated_menu(vec![
        ops,
        vec![repo_view_item(
            format!("Create patch from changes in {n} files"),
            &workdir,
            move |cx, app, state| {
                let workdir = state.repo.workdir().to_path_buf();
                save_git_output(
                    cx,
                    app,
                    &workdir,
                    "changes.patch",
                    &diff_args(staged, &paths),
                );
            },
        )],
    ])
}

#[must_use]
pub fn folder_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    folder: &str,
    files: Vec<String>,
    staged: bool,
    uncommitted: bool,
) -> MenuItems {
    let workdir = workdir_of(state);
    let mut ops = MenuItems::new();
    let base = Path::new(folder)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if uncommitted {
        let stage_files = files.clone();
        if staged {
            ops.push(repo_item(
                "Unstage folder",
                &workdir,
                move |_cx, app, state| {
                    let result = state.repo.unstage(&stage_files);
                    app.run(state, "Unstage", result, None);
                },
            ));
        } else {
            ops.push(repo_item(
                "Stage folder",
                &workdir,
                move |_cx, app, state| {
                    let result = state.repo.stage(&stage_files);
                    app.run(state, "Stage", result, None);
                },
            ));
        }
        let discard_files = files.clone();
        ops.push(repo_item(
            "Discard all changes in folder",
            &workdir,
            move |_cx, app, state| discard_paths(app, state, &discard_files, staged),
        ));
        let ignore_folder = folder.to_owned();
        ops.push(repo_view_item(
            format!("Ignore all files in '{base}/'"),
            &workdir,
            move |_cx, app, state| {
                let lines: Vec<String> = ignore_line(&ignore_folder, true).into_iter().collect();
                let result = append_to_gitignore(state.repo.workdir(), &lines);
                report_ignored(app, result, lines.join(""));
                state.refresh_status();
            },
        ));
        if state.repo.state() != OperationState::Merge {
            let stash_files = files.clone();
            ops.push(repo_item(
                "Stash folder",
                &workdir,
                move |_cx, app, state| {
                    app.run_git_cli(state, "Stash folder", stash_args(&stash_files));
                },
            ));
        }
        ops.push(repo_view_item(
            "Create Patch from changes in directory",
            &workdir,
            move |cx, app, state| {
                let workdir = state.repo.workdir().to_path_buf();
                save_git_output(
                    cx,
                    app,
                    &workdir,
                    "changes.patch",
                    &diff_args(staged, &files),
                );
            },
        ));
    }
    let mut open = MenuItems::new();
    let full = std::path::PathBuf::from(full_path(&workdir, folder));
    if full.is_dir() {
        open.push(menu_item("Open folder", move |_cx, _app| {
            let _ = imgit_platform::shell::open_path(&full);
        }));
        let folder = folder.to_owned();
        open.push(menu_item(
            "Create a file in this folder",
            move |_cx, app| {
                app.dialog.open(DialogKind::CreateFile);
                app.dialog.text_b = folder;
            },
        ));
    }
    separated_menu(vec![ops, open])
}

/// A commit row of the File History view.
#[must_use]
pub fn file_history_commit_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    sha: &str,
    path: &str,
) -> MenuItems {
    let workdir = workdir_of(state);
    let (jump_sha, copy_sha) = (sha.to_owned(), sha.to_owned());
    let (copy_path, copy_workdir) = (path.to_owned(), workdir.clone());
    separated_menu(vec![
        vec![
            repo_view_item("Show in graph", &workdir, move |_cx, _app, state| {
                state.close_file_history();
                super::jump_to_commit(state, &jump_sha);
            })
            .enabled(state.history.row_by_sha.contains_key(sha)),
        ],
        vec![
            menu_item("Copy commit sha", move |cx, app| {
                app.copy_to_clipboard(cx, &copy_sha);
            }),
            menu_item("Copy file path", move |cx, app| {
                app.copy_to_clipboard(cx, &full_path(&copy_workdir, &copy_path));
            }),
        ],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, status: FileStatus) -> FileChange {
        FileChange {
            path: path.to_owned(),
            old_path: None,
            status,
            stats: None,
        }
    }

    #[test]
    fn staged_new_files_are_discarded_as_untracked_ones() {
        let converted = as_unstaged([
            change("new.txt", FileStatus::Added),
            change("old.txt", FileStatus::Modified),
        ]);
        assert_eq!(converted[0].status, FileStatus::Untracked);
        assert_eq!(converted[1].status, FileStatus::Modified);
    }

    #[test]
    fn discarding_a_staged_new_file_deletes_it() {
        let scratch = tempfile::tempdir().unwrap();
        let folder = scratch.path().join("repo");
        std::fs::create_dir_all(&folder).unwrap();
        let repo = imgit_git::Repository::init(&folder).unwrap();
        repo.set_local_config("user.name", Some("Test")).unwrap();
        repo.set_local_config("user.email", Some("test@example.com"))
            .unwrap();
        repo.set_local_config("core.autocrlf", Some("false"))
            .unwrap();
        std::fs::write(
            folder.join("old.txt"),
            "old
",
        )
        .unwrap();
        repo.stage(&["old.txt"]).unwrap();
        repo.commit("first", false).unwrap();
        std::fs::write(
            folder.join("new.txt"),
            "new
",
        )
        .unwrap();
        std::fs::write(
            folder.join("old.txt"),
            "changed
",
        )
        .unwrap();
        repo.stage(&["new.txt", "old.txt"]).unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.open_repository(&folder.to_string_lossy());
        app.with_active(|app, state| {
            state.refresh();
            discard_paths(
                app,
                state,
                &["new.txt".to_owned(), "old.txt".to_owned()],
                true,
            );
            assert!(!folder.join("new.txt").exists());
            assert_eq!(
                std::fs::read_to_string(folder.join("old.txt")).unwrap(),
                "old
"
            );
            assert!(state.status.is_empty());
        });
    }

    #[test]
    fn ignoring_appends_one_line_per_pattern() {
        let scratch = tempfile::tempdir().unwrap();
        append_to_gitignore(scratch.path(), &["/a.txt".to_owned()]).unwrap();
        append_to_gitignore(scratch.path(), &["*.log".to_owned(), "build/".to_owned()]).unwrap();
        let text = std::fs::read_to_string(scratch.path().join(".gitignore")).unwrap();
        assert_eq!(text, "/a.txt\n*.log\nbuild/\n");
    }

    #[test]
    fn a_pattern_goes_on_its_own_line_whatever_the_file_ends_with() {
        let lines = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        // The last line has no line break: one is added, not glued to the pattern.
        let (added, count) = gitignore_additions(b"target", &lines(&["/a.txt"]));
        assert_eq!((added.as_slice(), count), (&b"\n/a.txt\n"[..], 1));
        // The file's own CRLF is kept, a missing last one is made up in the same style.
        let (added, _) = gitignore_additions(b"one\r\ntwo", &lines(&["/a", "b/"]));
        assert_eq!(added, b"\r\n/a\r\nb/\r\n");
        let (added, _) = gitignore_additions(b"one\r\ntwo\r\n", &lines(&["/a"]));
        assert_eq!(added, b"/a\r\n");
        // An empty file, or none, needs no leading break.
        assert_eq!(gitignore_additions(b"", &lines(&["/a"])).0, b"/a\n");
    }

    #[test]
    fn a_pattern_that_is_there_already_is_not_added_again() {
        let lines = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        let existing = b"/a.txt\r\nlog.txt\n*.o";
        // Exact, as part of the request twice, or covered by the unanchored form of a root file.
        let (added, count) = gitignore_additions(
            existing,
            &lines(&["/a.txt", "*.o", "/log.txt", "new/", "new/"]),
        );
        assert_eq!((added.as_slice(), count), (&b"\r\nnew/\r\n"[..], 1));
        assert_eq!(
            gitignore_additions(existing, &lines(&["/a.txt"])),
            (Vec::new(), 0)
        );
        // A nested path is not covered by a root file of the same last name.
        assert_eq!(
            gitignore_additions(b"log.txt\n", &lines(&["/src/log.txt"])).1,
            1
        );
        // Reported by the file append: nothing changes on disk.
        let scratch = tempfile::tempdir().unwrap();
        std::fs::write(scratch.path().join(".gitignore"), "/a.txt\n").unwrap();
        assert_eq!(
            append_to_gitignore(scratch.path(), &lines(&["/a.txt"])).unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(scratch.path().join(".gitignore")).unwrap(),
            "/a.txt\n"
        );
    }

    #[test]
    fn ignore_lines_match_that_file_only() {
        // A root file is anchored, so a file of that name in a folder stays tracked.
        assert_eq!(ignore_line("a.txt", false).as_deref(), Some("/a.txt"));
        // A path with a folder in it is anchored by git already.
        assert_eq!(
            ignore_line("src/a.txt", false).as_deref(),
            Some("src/a.txt")
        );
        assert_eq!(
            ignore_line("src\\ui\\a.txt", false).as_deref(),
            Some("src/ui/a.txt")
        );
        assert_eq!(ignore_line("src", true).as_deref(), Some("/src/"));
        assert_eq!(ignore_line("a/b", true).as_deref(), Some("a/b/"));
        // Syntax in the name is escaped.
        assert_eq!(ignore_line("#notes", false).as_deref(), Some("/\\#notes"));
        assert_eq!(
            ignore_line("!keep.txt", false).as_deref(),
            Some("/\\!keep.txt")
        );
        assert_eq!(
            ignore_line("a[1]*?.txt", false).as_deref(),
            Some("/a\\[1]\\*\\?.txt")
        );
        assert_eq!(ignore_line("dir/a#b", false).as_deref(), Some("dir/a#b"));
        assert_eq!(
            ignore_line("trail  ", false).as_deref(),
            Some("/trail\\ \\ ")
        );
        assert_eq!(
            ignore_line("back\\slash", false).as_deref(),
            Some("back/slash")
        );
        // No line can name these.
        assert_eq!(ignore_line("", false), None);
        assert_eq!(ignore_line("/", true), None);
        assert_eq!(ignore_line("a\nb", false), None);
        // The extension keeps its glob and escapes the rest.
        assert_eq!(ignore_extension_line("rs").as_deref(), Some("*.rs"));
        assert_eq!(ignore_extension_line("a[b").as_deref(), Some("*.a\\[b"));
        assert_eq!(ignore_extension_line(""), None);
    }

    #[test]
    fn git_ignores_exactly_what_the_lines_name() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path();
        let git = |args: &[&str]| {
            let output = imgit_git::cli::run(dir, args, imgit_git::cli::RunOptions::default());
            (output.exit_code, output.text)
        };
        assert_eq!(git(&["init", "-q"]).0, 0);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        // (No file called "star*": Windows has none, and `check-ignore` does not need one.)
        for name in ["a.txt", "sub/a.txt", "#hash", "[x]"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let lines: Vec<String> = ["a.txt", "#hash", "[x]", "star*"]
            .iter()
            .filter_map(|name| ignore_line(name, false))
            .collect();
        append_to_gitignore(dir, &lines).unwrap();
        let ignored = |path: &str| git(&["check-ignore", "-q", "--", path]).0 == 0;
        assert!(ignored("a.txt"));
        // The copy in the folder is another file.
        assert!(!ignored("sub/a.txt"));
        assert!(ignored("#hash") && ignored("[x]") && ignored("star*"));
        // The brackets did not become a character class matching "x".
        std::fs::write(dir.join("x"), "x").unwrap();
        assert!(!ignored("x"));
    }

    #[test]
    fn the_ignore_submenu_offers_file_extension_and_folder() {
        let labels = |path: &str| -> Vec<String> {
            ignore_submenu(Path::new("."), path)
                .submenu
                .into_iter()
                .map(|item| item.label)
                .collect()
        };
        assert_eq!(
            labels("src/main.rs"),
            [
                "Ignore 'main.rs'",
                "All files with the extension '.rs'",
                "All files in 'src/'"
            ]
        );
        assert_eq!(labels("Makefile"), ["Ignore 'Makefile'"]);
        assert_eq!(labels(".gitignore"), ["Ignore '.gitignore'"]);
        assert_eq!(
            labels("a/b/c.txt"),
            [
                "Ignore 'c.txt'",
                "All files with the extension '.txt'",
                "All files in 'b/'"
            ]
        );
    }

    #[test]
    fn diff_and_stash_arguments_end_with_the_paths() {
        let paths = vec!["a".to_owned(), "b c".to_owned()];
        assert_eq!(diff_args(false, &paths), ["diff", "--", "a", "b c"]);
        assert_eq!(
            diff_args(true, &paths),
            ["diff", "--cached", "--", "a", "b c"]
        );
        assert_eq!(stash_args(&paths), ["stash", "push", "--", "a", "b c"]);
    }
}
