//! The application menu: File / Edit / View / Help, with their labels and accelerators.
//!
//! The items outlive the frame (the menu stays open), so their actions look the active tab up when clicked
//! instead of keeping a reference to it.

use dear_imgui_rs::Key;

use crate::app::application::{Application, VERSION};
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;
use crate::ui::menu::{MenuItem, MenuItems, menu_item, menu_submenu, separated_menu};
use crate::ui::{Cx, raw};

/// The bar's entries: `&` marks the mnemonic letter.
pub const BAR_LABELS: [&str; 4] = ["&File", "&Edit", "&View", "&Help"];

/// The keys that open the entries with Alt.
pub const BAR_KEYS: [Key; 4] = [Key::F, Key::E, Key::V, Key::H];

/// A menu item with its accelerator text in the platform's words.
fn item(
    cx: &Cx<'_>,
    label: &str,
    accelerator: &str,
    click: impl FnOnce(&mut Cx<'_>, &mut Application) + 'static,
) -> MenuItem {
    menu_item(label, click).accelerator(cx.shortcut_label(accelerator))
}

/// A checkbox row, left unchecked whatever the panel state.
fn checkbox(
    cx: &Cx<'_>,
    label: &str,
    accelerator: &str,
    click: impl FnOnce(&mut Cx<'_>, &mut Application) + 'static,
) -> MenuItem {
    item(cx, label, accelerator, click).checkbox(false)
}

/// Types the platform's shortcut for `key` (Ctrl+X on Windows, Cmd+X on macOS): the Edit menu's clipboard rows
/// act on whatever text field has the focus.
fn send_chord(key: Key) -> impl FnOnce(&mut Cx<'_>, &mut Application) + 'static {
    move |cx, _| {
        let modifier = cx.shortcut_modifier_event();
        raw::add_key_event(cx.ui, modifier, true);
        raw::add_key_event(cx.ui, key, true);
        raw::add_key_event(cx.ui, key, false);
        raw::add_key_event(cx.ui, modifier, false);
    }
}

/// Selects terminal tab `index` of the active repository, or (`None`) the one `step` away from the active one.
fn select_terminal_tab(app: &mut Application, index: Option<usize>, step: isize) {
    let workdir = app.active_workdir().map(std::path::Path::to_path_buf);
    let list = app.terminal_tabs(workdir.as_deref());
    let count = list.sessions.len();
    if count == 0 || index.is_some_and(|index| index >= count) {
        return;
    }
    list.active =
        index.unwrap_or_else(|| (list.active as isize + step).rem_euclid(count as isize) as usize);
    app.show_terminal_panel = true;
    app.terminal.minimized = false;
    app.terminal.focus_requested = true;
}

/// The menu of bar entry `index` (File, Edit, View, Help).
#[must_use]
pub fn bar_menu(cx: &Cx<'_>, index: usize) -> MenuItems {
    match index {
        0 => file_menu(cx),
        1 => edit_menu(cx),
        2 => view_menu(cx),
        3 => help_menu(cx),
        _ => MenuItems::new(),
    }
}

fn file_menu(cx: &Cx<'_>) -> MenuItems {
    separated_menu(vec![
        vec![
            item(cx, "New &Tab", "Ctrl+T", |_, app| app.active_tab = None),
            item(cx, "Close Tab", "Ctrl+W", |_, app| {
                if let Some(active) = app.active_tab {
                    app.close_tab(active);
                }
            }),
            item(cx, "Reopen Closed &Tab", "Ctrl+Shift+T", |_, app| {
                app.reopen_closed_tab();
            }),
        ],
        vec![
            item(cx, "Clo&ne Repo...", "Ctrl+N", |_, app| {
                app.dialog.open(DialogKind::CloneRepo);
            }),
            item(cx, "&Init Repo...", "Ctrl+I", |_, app| {
                app.dialog.open(DialogKind::InitRepo);
            }),
            item(cx, "&Open Repo...", "Ctrl+O", |cx, app| {
                app.browse_for_repository(cx);
            }),
            item(cx, "&Open Repo Management", "Alt+Ctrl+O", |_, app| {
                app.repo_management_open = true;
            }),
            item(
                cx,
                "Open Repo in External &Editor",
                "Ctrl+Shift+E",
                |_, app| {
                    if let Some(workdir) = app.active_workdir().map(std::path::Path::to_path_buf) {
                        app.open_in_editor(&workdir);
                    }
                },
            ),
        ],
        vec![
            item(cx, "Open External &Terminal", "Alt+T", |_, app| {
                if let Some(workdir) = app.active_workdir().map(std::path::Path::to_path_buf) {
                    app.open_terminal(true, &workdir);
                }
            }),
            item(cx, "Open in File Manager", "Alt+O", |_, app| {
                if let Some(workdir) = app.active_workdir() {
                    let _ = imgit_platform::shell::open_in_file_manager(workdir);
                }
            }),
        ],
        vec![item(cx, "&Preferences...", "Ctrl+Comma", |_, app| {
            app.preferences_open = true;
        })],
        vec![
            item(
                cx,
                "Check for Update (Last checked a few seconds ago)",
                "",
                |_, app| app.toasts.push(ToastKind::Info, "ImGit is up to date"),
            ),
            item(cx, "E&xit", "Alt+F4", |cx, app| app.quit(cx)),
        ],
    ])
}

fn edit_menu(cx: &Cx<'_>) -> MenuItems {
    separated_menu(vec![
        vec![
            item(cx, "&Undo", "Ctrl+Z", |_, app| {
                app.with_active(|app, state| app.undo(state));
            }),
            item(cx, "&Redo", "Ctrl+Y", |_, app| {
                app.with_active(|app, state| app.redo(state));
            }),
        ],
        vec![
            item(cx, "&Cut", "Ctrl+X", send_chord(Key::X)),
            item(cx, "C&opy", "Ctrl+C", send_chord(Key::C)),
            item(cx, "&Paste", "Ctrl+V", send_chord(Key::V)),
            item(cx, "Select &All", "Ctrl+A", send_chord(Key::A)),
        ],
    ])
}

fn view_menu(cx: &Cx<'_>) -> MenuItems {
    let numbered: MenuItems = (1..=9)
        .map(|i| {
            let mut select = item(cx, "", &format!("Ctrl+{i}"), move |_, app| {
                if i - 1 < app.tabs.len() {
                    app.active_tab = Some(i - 1);
                }
            });
            select.label = format!("Select Tab {i}");
            select
        })
        .collect();
    let tabs_menu = separated_menu(vec![
        vec![
            item(cx, "Select Next Tab", "Ctrl+Tab", |_, app| {
                app.select_adjacent_tab(1)
            }),
            item(cx, "Select Previous Tab", "Ctrl+Shift+Tab", |_, app| {
                app.select_adjacent_tab(-1);
            }),
            item(cx, "Open Tabs List", "Ctrl+Shift+A", |_, app| {
                app.tabs_list_requested = true;
            }),
        ],
        numbered,
    ]);

    let terminal_numbered: MenuItems = (1..=9)
        .map(|i| {
            let mut select = item(cx, "", &format!("Ctrl+Shift+{i}"), move |_, app| {
                select_terminal_tab(app, Some(i - 1), 0);
            });
            select.label = format!("Select Terminal Tab {i}");
            select
        })
        .collect();
    let terminal_menu = separated_menu(vec![
        vec![
            item(
                cx,
                "Select Next Terminal Tab",
                "Ctrl+Page Down",
                |_, app| {
                    select_terminal_tab(app, None, 1);
                },
            ),
            item(
                cx,
                "Select Previous Terminal Tab",
                "Ctrl+Page Up",
                |_, app| {
                    select_terminal_tab(app, None, -1);
                },
            ),
            item(cx, "New Terminal Tab", "Ctrl+Shift+N", |_, app| {
                let workdir = app.active_workdir().map(std::path::Path::to_path_buf);
                app.new_terminal_tab(workdir.as_deref());
            }),
            item(cx, "Close Terminal Tab", "Ctrl+Shift+W", |_, app| {
                let workdir = app.active_workdir().map(std::path::Path::to_path_buf);
                let active = app.terminal_tabs(workdir.as_deref()).active;
                app.close_terminal_tab(workdir.as_deref(), active);
            }),
        ],
        terminal_numbered,
    ]);

    let favorites: MenuItems = (1..=9)
        .map(|i| {
            let mut open = item(cx, "", &format!("Alt+Ctrl+{i}"), move |_, app| {
                app.open_favorite_repository(i - 1);
            });
            open.label = format!("Open Favorite Repo {i}");
            open
        })
        .collect();

    separated_menu(vec![
        vec![
            item(cx, "&Relaunch ImGit Desktop", "Ctrl+Shift+R", |cx, app| {
                app.relaunch(cx)
            }),
            item(cx, "Toggle &Full Screen", "Ctrl+Shift+F", |cx, app| {
                app.toggle_full_screen(cx);
            }),
        ],
        vec![
            item(cx, "&Increase Zoom", "Ctrl+=", |cx, app| {
                app.set_zoom(cx, app.settings.zoom + 0.1);
            }),
            item(cx, "Decrease Z&oom", "Ctrl+-", |cx, app| {
                app.set_zoom(cx, app.settings.zoom - 0.1);
            }),
            item(cx, "&Reset Zoom", "Ctrl+0", |cx, app| app.set_zoom(cx, 1.0)),
            menu_submenu("Tabs", tabs_menu),
            menu_submenu("Terminal Tabs", terminal_menu),
            menu_submenu("Favorites", favorites),
        ],
        vec![
            checkbox(cx, "Show Left Panel", "Ctrl+J", |_, app| {
                app.show_left_panel = !app.show_left_panel;
            }),
            checkbox(cx, "Show Commit Details Panel", "Ctrl+K", |_, app| {
                app.show_detail_panel = !app.show_detail_panel;
            }),
            checkbox(cx, "Show Terminal Panel", "Ctrl+`", |_, app| {
                app.show_terminal_panel = !app.show_terminal_panel;
                app.terminal.focus_requested = app.show_terminal_panel;
            }),
        ],
    ])
}

fn help_menu(cx: &Cx<'_>) -> MenuItems {
    separated_menu(vec![
        vec![
            item(cx, "Open Command Palette", "Ctrl+P", |_, app| {
                app.fuzzy.show()
            }),
            item(cx, "Keyboard Shortcuts", "Ctrl+/", |_, app| {
                app.dialog.open(DialogKind::KeyboardShortcuts);
            }),
        ],
        vec![
            item(cx, "About ImGit Desktop", "", |_, app| {
                app.dialog.open(DialogKind::About);
            }),
            MenuItem::disabled(format!("Version {VERSION} (x64 64-bit)")),
        ],
    ])
}
