//! Native macOS application menus, backed by the same actions as the in-window menus.

#[must_use]
pub const fn has_native_menu_bar() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(target_os = "macos")]
pub use macos::draw_native_menu_bar;

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;

    use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    use crate::app::action::Action;
    use crate::app::application::Application;
    use crate::app::menu_bar::{BAR_LABELS, bar_menu};
    use crate::ui::Cx;
    use crate::ui::menu::{MenuItemKind, MenuItems};

    #[derive(Default)]
    struct NativeMenu {
        menu: Option<Menu>,
        signature: String,
        actions: Vec<Option<Action>>,
    }

    thread_local! {
        static NATIVE: RefCell<NativeMenu> = RefCell::default();
    }

    fn collect(items: &mut MenuItems, actions: &mut Vec<Option<Action>>, signature: &mut String) {
        for item in items {
            signature.push_str(&format!(
                "{:?}{:?}{:?}{}{}[",
                item.kind, item.label, item.accelerator, item.enabled, item.checked
            ));
            if !item.submenu.is_empty() {
                collect(&mut item.submenu, actions, signature);
            } else if item.kind != MenuItemKind::Separator {
                actions.push(item.click.take());
            }
            signature.push(']');
        }
    }

    fn populate(menu: &Submenu, items: MenuItems, next_id: &mut usize) {
        for item in items {
            if item.kind == MenuItemKind::Separator {
                menu.append(&PredefinedMenuItem::separator())
                    .expect("native separator");
            } else if !item.submenu.is_empty() {
                let submenu = Submenu::new(item.label.replace('&', ""), item.enabled);
                populate(&submenu, item.submenu, next_id);
                menu.append(&submenu).expect("native submenu");
            } else {
                let id = format!("imgit-action-{}", *next_id);
                *next_id += 1;
                // Accelerators are deliberately left to the application's shortcut handler, which
                // also handles text fields and the terminal. Native key equivalents would swallow them.
                let label = item.label.replace('&', "");
                if item.kind == MenuItemKind::Checkbox {
                    menu.append(&CheckMenuItem::with_id(
                        id,
                        label,
                        item.enabled,
                        item.checked,
                        None,
                    ))
                    .expect("native checkbox");
                } else {
                    menu.append(&MenuItem::with_id(id, label, item.enabled, None))
                        .expect("native item");
                }
            }
        }
    }

    pub fn draw_native_menu_bar(cx: &mut Cx<'_>, app: &mut Application) {
        // Run the actions belonging to the displayed menu before refreshing its dynamic contents.
        let clicks: Vec<Action> = NATIVE.with_borrow_mut(|state| {
            let mut clicks = Vec::new();
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if let Some(index) = event
                    .id
                    .0
                    .strip_prefix("imgit-action-")
                    .and_then(|index| index.parse::<usize>().ok())
                    && let Some(action) = state.actions.get_mut(index).and_then(Option::take)
                {
                    clicks.push(action);
                }
            }
            clicks
        });
        for click in clicks {
            click(cx, app);
        }

        let mut menus: Vec<_> = (0..BAR_LABELS.len())
            .map(|index| bar_menu(cx, index))
            .collect();
        let mut signature = String::new();
        let mut actions = Vec::new();
        for items in &mut menus {
            collect(items, &mut actions, &mut signature);
        }
        let rebuild = NATIVE.with_borrow_mut(|state| {
            state.actions = actions;
            state.menu.is_none() || state.signature != signature
        });
        if !rebuild {
            return;
        }
        let menu = Menu::new();
        let application = Submenu::new("ImGit", true);
        application
            .append_items(&[
                &PredefinedMenuItem::about(Some("About ImGit"), None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(Some("Hide ImGit")),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(Some("Quit ImGit")),
            ])
            .expect("application menu");
        menu.append(&application).expect("application submenu");
        let mut next_id = 0;
        let mut help_menu = None;
        for (index, (title, items)) in BAR_LABELS.iter().zip(menus).enumerate() {
            let submenu = Submenu::new(title.replace('&', ""), true);
            populate(&submenu, items, &mut next_id);
            menu.append(&submenu).expect("application submenu");
            if index == 3 {
                help_menu = Some(submenu);
            }
        }
        let window = Submenu::new("Window", true);
        window
            .append_items(&[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::zoom(None),
                &PredefinedMenuItem::fullscreen(None),
            ])
            .expect("window menu");
        menu.insert(&window, 4).expect("window submenu");

        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        if let Some(help) = help_menu {
            help.set_as_help_menu_for_nsapp();
        }
        NATIVE.with_borrow_mut(|state| {
            state.menu = Some(menu);
            state.signature = signature;
        });
    }
}
