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
    OperationState, confirm, full_path, repo_item, save_git_output, selected_commits, workdir_of,
};
use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::ui::Cx;
use crate::ui::menu::{MenuItem, MenuItems, menu_item, menu_submenu, separated_menu};
use crate::ui::panels::file_editor::open_file_editor;

/// `pattern` and a line break appended to the repository's `.gitignore` (created when missing).
fn append_to_gitignore(workdir: &Path, lines: &[String]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(workdir.join(".gitignore"))?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    Ok(())
}

/// The item that adds `pattern` to `.gitignore` and says so.
fn ignore_item(label: String, workdir: &Path, pattern: String) -> MenuItem {
    repo_item(label, workdir, move |_cx, app, state| {
        match append_to_gitignore(state.repo.workdir(), std::slice::from_ref(&pattern)) {
            Ok(()) => {
                app.toasts
                    .push_with_detail(ToastKind::Success, "Added to .gitignore", pattern)
            }
            Err(error) => app.toasts.push_with_detail(
                ToastKind::Error,
                "Could not write .gitignore",
                error.to_string(),
            ),
        }
        state.refresh_status();
    })
}

/// "Ignore": the file, its extension, its folder.
fn ignore_submenu(workdir: &Path, path: &str) -> MenuItem {
    let p = Path::new(path);
    let mut items = vec![ignore_item(
        format!(
            "Ignore '{}'",
            p.file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default()
        ),
        workdir,
        path.to_owned(),
    )];
    if let Some(extension) = p.extension() {
        let extension = format!(".{}", extension.to_string_lossy());
        items.push(ignore_item(
            format!("All files with the extension '{extension}'"),
            workdir,
            format!("*{extension}"),
        ));
    }
    if let Some(parent) = p.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        let folder = parent.to_string_lossy().replace('\\', "/");
        items.push(ignore_item(
            format!(
                "All files in '{}/'",
                parent
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            ),
            workdir,
            format!("{folder}/"),
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
        repo_item("Edit file", workdir, move |cx, app, state| {
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
            repo_item(label, workdir, move |_cx, _app, state| {
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

/// A discard of the working directory changes of `paths` (unstaged first when they are staged).
fn discard_paths(app: &mut Application, state: &mut RepoState, paths: &[String], staged: bool) {
    let changes = changes_among(state, paths, staged);
    if staged {
        let _ = state.repo.unstage(paths);
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
        ops.push(repo_item(
            "Open in merge tool",
            &workdir,
            move |_cx, app, state| {
                if let Err(error) = state.open_merge_tool(&target.path) {
                    app.toasts.push_with_detail(
                        ToastKind::Error,
                        "Could not open the merge tool",
                        error.to_string(),
                    );
                    state.open_diff(DiffTarget {
                        source: DiffSource::Unstaged,
                        path: target.path,
                        status: target.status,
                    });
                }
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
            if staged {
                let paths = paths_of(&copy);
                if state.repo.unstage(&paths).is_err() {
                    return;
                }
            }
            let message = format!("Discarded changes to {}", file_name(&copy.path));
            let result = state.repo.discard(std::slice::from_ref(&copy));
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
        open.push(repo_item(
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
            repo_item(
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
        open.push(repo_item(
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
    ops.push(repo_item(
        format!("Ignore {n} files"),
        &workdir,
        move |_cx, app, state| {
            match append_to_gitignore(state.repo.workdir(), &ignore_paths) {
                Ok(()) => app.toasts.push(ToastKind::Success, "Added to .gitignore"),
                Err(error) => app.toasts.push_with_detail(
                    ToastKind::Error,
                    "Could not write .gitignore",
                    error.to_string(),
                ),
            }
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
        vec![repo_item(
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
        ops.push(repo_item(
            format!("Ignore all files in '{base}/'"),
            &workdir,
            move |_cx, app, state| {
                let pattern = format!("{ignore_folder}/");
                match append_to_gitignore(state.repo.workdir(), std::slice::from_ref(&pattern)) {
                    Ok(()) => app.toasts.push_with_detail(
                        ToastKind::Success,
                        "Added to .gitignore",
                        pattern,
                    ),
                    Err(error) => app.toasts.push_with_detail(
                        ToastKind::Error,
                        "Could not write .gitignore",
                        error.to_string(),
                    ),
                }
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
        ops.push(repo_item(
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
            repo_item("Show in graph", &workdir, move |_cx, _app, state| {
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

    #[test]
    fn ignoring_appends_one_line_per_pattern() {
        let scratch = tempfile::tempdir().unwrap();
        append_to_gitignore(scratch.path(), &["a.txt".to_owned()]).unwrap();
        append_to_gitignore(scratch.path(), &["*.log".to_owned(), "build/".to_owned()]).unwrap();
        let text = std::fs::read_to_string(scratch.path().join(".gitignore")).unwrap();
        assert_eq!(text, "a.txt\n*.log\nbuild/\n");
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
