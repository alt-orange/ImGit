//! The menus of the tabs bar, the graph header, the left panel's sections, remotes, worktrees and local
//! folders, the terminal and read-only / editable text.

use std::path::Path;

use dear_imgui_rs::Key;
use imgit_app::RepoState;
use imgit_git::{Ref, RefType, Worktree};

use super::shared::{confirm, repo_item, toggle_hide_item, toggle_solo_item, workdir_of};
use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::settings::Settings;
use crate::app::toasts::ToastKind;
use crate::ui::menu::{MenuItem, MenuItems, menu_checkbox, menu_item, separated_menu};
use crate::ui::panels::tabs_bar::tab_naming_items;
use crate::ui::panels::terminal_panel::{
    terminal_clear, terminal_copy, terminal_has_selection, terminal_paste, terminal_select_all,
    terminal_write,
};
use crate::ui::panels::workflows::{gitflow_start_items, open_branch_cleanup};
use crate::ui::{Cx, raw};

/// The menu of repository tab `index`.
#[must_use]
pub fn tab_menu(cx: &mut Cx<'_>, app: &mut Application, index: usize) -> MenuItems {
    let count = app.tabs.len();
    let reopen = menu_item("Reopen Closed Tab", |_cx, app| app.reopen_closed_tab())
        .enabled(!app.closed_tabs.is_empty())
        .accelerator(cx.shortcut_label("Ctrl+Shift+T"));
    let naming = tab_naming_items(cx, app, index);
    separated_menu(vec![
        vec![
            menu_item("Close tab", move |_cx, app| app.close_tab(index)),
            menu_item("Close other tabs", move |_cx, app| {
                for i in (0..app.tabs.len()).rev() {
                    if i != index {
                        app.close_tab(i);
                    }
                }
                app.active_tab = if app.tabs.is_empty() { None } else { Some(0) };
            })
            .enabled(count > 1),
            menu_item("Close tabs to the right", move |_cx, app| {
                for i in (index + 1..app.tabs.len()).rev() {
                    app.close_tab(i);
                }
            })
            .enabled(index + 1 < count),
        ],
        naming,
        vec![reopen],
    ])
}

/// Which of the settings' flags a column checkbox flips.
type Flag = fn(&mut Settings) -> &mut bool;

fn save_settings(app: &Application) {
    app.settings.save(&app.config_path("settings.txt"));
}

/// The graph header's column checkboxes (a column cannot be hidden when it is the last one shown), the layout
/// toggles and the resets.
#[must_use]
pub fn graph_header_menu(_cx: &mut Cx<'_>, app: &mut Application) -> MenuItems {
    let s = &app.settings;
    let fixed_shown = |key: &str| !s.hidden_graph_columns.contains(key);
    let shown = [
        fixed_shown("ref"),
        fixed_shown("graph"),
        fixed_shown("message"),
        s.show_author,
        s.show_date,
        s.show_sha,
        s.show_changes,
    ]
    .into_iter()
    .filter(|&on| on)
    .count();
    let toggle = |label: &str, checked: bool, flag: Flag, enabled: bool| {
        menu_checkbox(label, checked, move |_cx, app| {
            let value = flag(&mut app.settings);
            *value = !*value;
            save_settings(app);
        })
        .enabled(enabled)
    };
    let column = |label: &str, checked: bool, flag: Flag| {
        toggle(label, checked, flag, !checked || shown > 1)
    };
    let fixed_column = |label: &str, key: &'static str| {
        let on = fixed_shown(key);
        menu_checkbox(label, on, move |_cx, app| {
            let hidden = &mut app.settings.hidden_graph_columns;
            if !hidden.remove(key) {
                hidden.insert(key.to_owned());
            }
            save_settings(app);
        })
        .enabled(!on || shown > 1)
    };
    let reset = |label: &str, compact: bool| {
        menu_item(label, move |_cx, app| {
            let s = &mut app.settings;
            s.show_author = false;
            s.show_date = false;
            s.show_sha = false;
            s.show_changes = false;
            s.hidden_graph_columns.clear();
            s.compact_graph = compact;
            s.graph_column_order.clear();
            save_settings(app);
        })
    };
    separated_menu(vec![
        vec![
            fixed_column("Branch / Tag", "ref"),
            fixed_column("Graph", "graph"),
            fixed_column("Commit message", "message"),
            column("Author", s.show_author, |s| &mut s.show_author),
            column("Date / Time", s.show_date, |s| &mut s.show_date),
            column("Sha", s.show_sha, |s| &mut s.show_sha),
            column("Changes", s.show_changes, |s| &mut s.show_changes),
        ],
        vec![
            toggle(
                "Compact Graph Column",
                s.compact_graph,
                |s| &mut s.compact_graph,
                true,
            ),
            toggle(
                "Smart Branch Visibility",
                s.smart_branch_visibility,
                |s| &mut s.smart_branch_visibility,
                true,
            ),
        ],
        vec![
            reset("Reset columns to default layout", false),
            reset("Reset columns to compact layout", true),
        ],
    ])
}

/// A linked worktree's row in the left panel: open it, copy its path, remove it (`git worktree remove`).
#[must_use]
pub fn worktree_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    worktree: &Worktree,
) -> MenuItems {
    let workdir = workdir_of(state);
    let (path, name) = (worktree.path.clone(), worktree.name.clone());
    let remove = |label: &str, force: bool| {
        let (path, name) = (path.clone(), name.clone());
        repo_item(label, &workdir, move |_cx, app, state| {
            app.dialog.open(DialogKind::Confirm);
            let workdir = state.repo.workdir().to_path_buf();
            app.dialog.workdir = Some(workdir.clone());
            let quoted = format!("\"{name}\"");
            app.dialog.message = if force {
                format!(
                    "This is a destructive operation, are you sure you want to remove the worktree {quoted} and discard its uncommitted changes?"
                )
            } else {
                format!("Remove the worktree {quoted}? Its folder is deleted; its branch stays.")
            };
            app.dialog.confirm_label = "Remove".to_owned();
            app.dialog.danger = true;
            app.dialog.on_confirm = Some(Box::new(move |_cx, app| {
                app.with_repo(&workdir, |app, tab| {
                    let mut args = vec!["worktree".to_owned(), "remove".to_owned()];
                    if force {
                        args.push("--force".to_owned());
                    }
                    args.push(path);
                    app.run_git_cli(tab, "Remove worktree", args);
                });
            }));
        })
    };
    let (open_path, copy_path) = (worktree.path.clone(), worktree.path.clone());
    separated_menu(vec![
        vec![
            menu_item("Open worktree", move |_cx, app| {
                app.open_repository(&open_path)
            }),
            menu_item("Copy worktree path", move |cx, app| {
                app.copy_to_clipboard(cx, &copy_path);
            }),
        ],
        vec![
            remove("Remove worktree", false),
            remove("Remove worktree and discard its changes", true),
        ],
    ])
}

#[must_use]
pub fn remote_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    name: &str,
    in_left_panel: bool,
) -> MenuItems {
    let workdir = workdir_of(state);
    let full_name = format!("refs/remotes/{name}");
    let url = state
        .history
        .remotes
        .iter()
        .rev()
        .find(|remote| remote.name == name)
        .map(|remote| remote.url.clone())
        .unwrap_or_default();
    let (fetch_name, edit_name, remove_name) = (name.to_owned(), name.to_owned(), name.to_owned());
    let remove_workdir = workdir.clone();
    separated_menu(vec![
        vec![repo_item(
            format!("Fetch {name}"),
            &workdir,
            move |_cx, app, state| {
                app.run_git_cli(state, "Fetch", ["fetch", fetch_name.as_str(), "--prune"]);
            },
        )],
        vec![
            menu_item(format!("Edit {name}"), move |_cx, app| {
                app.dialog.open(DialogKind::EditRemote);
                app.dialog.ref_name.clone_from(&edit_name);
                app.dialog.text_a = edit_name;
                app.dialog.text_b = url;
            }),
            menu_item(format!("Remove {name}"), move |_cx, app| {
                let remote = remove_name.clone();
                confirm(
                    app,
                    "Remove Remote",
                    format!("Are you sure you want to remove the remote {remote}?"),
                    "Remove",
                    true,
                    move |_cx, app| {
                        app.with_repo(&remove_workdir, |app, state| {
                            let result = state.repo.remove_remote(&remote);
                            app.run(
                                state,
                                "Remove remote",
                                result,
                                Some(&format!("Removed remote {remote}")),
                            );
                        });
                    },
                );
            }),
        ],
        vec![
            toggle_hide_item(
                &workdir,
                state,
                &full_name,
                !in_left_panel || state.solo_ref.is_none(),
            ),
            toggle_solo_item(&workdir, state, &full_name, "Solo"),
        ],
    ])
}

#[must_use]
pub fn local_folder_menu(
    _cx: &mut Cx<'_>,
    _app: &mut Application,
    state: &mut RepoState,
    folder: &str,
) -> MenuItems {
    let prefix = format!("{folder}/");
    let branches: Vec<String> = state
        .history
        .refs
        .iter()
        .filter(|reference| {
            reference.ref_type == RefType::LocalBranch && reference.name.starts_with(&prefix)
        })
        .map(|reference| reference.name.clone())
        .collect();
    let label = if branches.len() == 1 {
        format!("Delete 1 branch in folder \"{folder}\"")
    } else {
        format!("Delete {} branches in folder \"{folder}\"", branches.len())
    };
    let folder = folder.to_owned();
    vec![repo_item(
        label,
        state.repo.workdir(),
        move |_cx, app, state| {
            let message = format!("Delete {} branches in folder \"{folder}\"?", branches.len());
            let workdir = state.repo.workdir().to_path_buf();
            confirm(
                app,
                "Delete Branches",
                message,
                "Delete",
                true,
                move |_cx, app| {
                    app.with_repo(&workdir, |app, state| {
                        for branch in &branches {
                            if let Err(error) = state.repo.delete_branch(branch) {
                                app.toasts.push_with_detail(
                                    ToastKind::Error,
                                    format!("Could not delete {branch}"),
                                    error.to_string(),
                                );
                            }
                        }
                        state.refresh();
                    });
                },
            );
        },
    )]
}

/// Whether `reference` is listed in the left panel section `section` ("Local" leaves out the checked-out
/// branch).
fn in_section(section: &str, reference: &Ref) -> bool {
    match section {
        "Local" => reference.ref_type == RefType::LocalBranch && !reference.is_head,
        "Remote" => reference.ref_type == RefType::RemoteBranch,
        "Stashes" => reference.ref_type == RefType::Stash,
        "Tags" => reference.ref_type == RefType::Tag,
        _ => false,
    }
}

/// The "Hide all ..." / "Show all ..." texts of a section.
fn visibility_labels(section: &str) -> Option<(&'static str, &'static str)> {
    match section {
        "Local" => Some(("Hide all local branches", "Show all local branches")),
        "Remote" => Some(("Hide all remotes", "Show all remotes")),
        "Stashes" => Some(("Hide all stashes", "Show all stashes")),
        "Tags" => Some(("Hide all tags", "Show all tags")),
        _ => None,
    }
}

/// The left panel section's menu: hide / show everything in it, maximize it, the section switches and its
/// workflows.
#[must_use]
pub fn left_section_menu(
    cx: &mut Cx<'_>,
    app: &mut Application,
    state: &mut RepoState,
    section: &str,
    sections: &[String],
) -> MenuItems {
    let workdir = workdir_of(state);
    // The section's workflows: Clean Up Branches under LOCAL, Start Feature / Release / Hotfix under GITFLOW.
    let mut workflows = MenuItems::new();
    if section == "Local" {
        workflows.push(repo_item(
            "Clean Up Branches...",
            &workdir,
            |cx, app, state| {
                open_branch_cleanup(cx, app, state);
            },
        ));
    }
    if section == "Local" || section == "Gitflow" {
        workflows.extend(gitflow_start_items(cx, app, state));
    }
    let mut visibility = MenuItems::new();
    if let Some((hide, show)) = visibility_labels(section) {
        let any_visible = state
            .history
            .refs
            .iter()
            .any(|r| in_section(section, r) && !state.hidden_refs.contains(&r.full_name));
        let any_hidden = state
            .history
            .refs
            .iter()
            .any(|r| in_section(section, r) && state.hidden_refs.contains(&r.full_name));
        let set_all = |label: &str, hide: bool, enabled: bool| {
            let section = section.to_owned();
            repo_item(label, &workdir, move |_cx, _app, state| {
                for reference in &state.history.refs {
                    if !in_section(&section, reference) {
                        continue;
                    }
                    if hide {
                        state.hidden_refs.insert(reference.full_name.clone());
                    } else {
                        state.hidden_refs.remove(&reference.full_name);
                    }
                }
                state.solo_ref = None;
                state.refresh();
            })
            .enabled(enabled)
        };
        visibility.push(set_all(hide, true, any_visible));
        visibility.push(set_all(show, false, any_hidden));
    }
    let section_name = section.to_owned();
    let maximize = vec![repo_item(
        "Maximize this section",
        &workdir,
        move |_cx, _app, state| {
            // collapseOtherLeftPanelSections
            state.expanded_left_sections = std::iter::once(section_name).collect();
        },
    )];
    let switches: MenuItems = sections
        .iter()
        .map(|name| {
            let toggled = name.clone();
            menu_checkbox(
                name.clone(),
                !app.settings.hidden_left_sections.contains(name),
                move |_cx, app| {
                    if !app.settings.hidden_left_sections.remove(&toggled) {
                        app.settings.hidden_left_sections.insert(toggled);
                    }
                    save_settings(app);
                },
            )
            .enabled(name != "Local")
        })
        .collect();
    separated_menu(vec![workflows, visibility, maximize, switches])
}

/// The terminal panel's right-click menu.
#[must_use]
pub fn terminal_menu(cx: &mut Cx<'_>, app: &mut Application) -> MenuItems {
    let with_accelerator = |item: MenuItem, accelerator: String| item.accelerator(accelerator);
    let has_selection = terminal_has_selection(app);
    separated_menu(vec![
        vec![
            with_accelerator(
                menu_item("Copy", terminal_copy).enabled(has_selection),
                cx.shortcut_label("Ctrl+C"),
            ),
            with_accelerator(
                menu_item("Paste", terminal_paste),
                cx.shortcut_label("Ctrl+V"),
            ),
            menu_item("Select All", |_cx, app| terminal_select_all(app)),
        ],
        vec![
            with_accelerator(
                menu_item("Delete Line", |_cx, app| {
                    // Windows: Ctrl+Home, PSReadLine's BackwardDeleteLine.
                    terminal_write(app, b"\x1b[1;5H");
                }),
                "Esc".to_owned(),
            ),
            // The shell's own key: the Control key on macOS too.
            with_accelerator(
                menu_item("Clear", |_cx, app| terminal_clear(app)),
                "Ctrl+L".to_owned(),
            ),
        ],
        vec![
            menu_item("Minimize Terminal", |_cx, app| {
                app.terminal.minimized = true
            }),
            menu_item("Kill Terminal", |_cx, app| {
                let workdir = app.active_workdir().map(Path::to_path_buf);
                app.kill_terminals(workdir.as_deref());
            }),
            menu_item("Hide Terminal", |_cx, app| app.show_terminal_panel = false),
        ],
    ])
}

/// A key chord (or key) sent to the focused text field, like Electron's roles do.
fn chord(key: Key, with_modifier: bool) -> impl FnOnce(&mut Cx<'_>, &mut Application) + 'static {
    move |cx, _app| {
        let ui = cx.ui;
        let modifier = cx.shortcut_modifier_event();
        if with_modifier {
            raw::add_key_event(ui, modifier, true);
        }
        raw::add_key_event(ui, key, true);
        raw::add_key_event(ui, key, false);
        if with_modifier {
            raw::add_key_event(ui, modifier, false);
        }
    }
}

/// The menu of a read-only or editable text (the diff and file editors): Electron's roles, which send the
/// editing commands to the focused field.
#[must_use]
pub fn text_input_menu(_cx: &mut Cx<'_>, read_only: bool) -> MenuItems {
    if read_only {
        return vec![
            menu_item("Copy", chord(Key::C, true)),
            menu_item("Select All", chord(Key::A, true)),
        ];
    }
    vec![
        menu_item("Cut", chord(Key::X, true)),
        menu_item("Copy", chord(Key::C, true)),
        menu_item("Paste", chord(Key::V, true)),
        menu_item("Delete", chord(Key::Delete, false)),
        menu_item("Select All", chord(Key::A, true)),
    ]
}
