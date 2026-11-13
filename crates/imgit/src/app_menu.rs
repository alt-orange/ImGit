//! The application menu (File / Edit / View / Help) where the platform shows it itself: the macOS menu bar.
//! Elsewhere the app draws its own bar (`Application::draw_menu_bar`) and these do nothing.
//!
//! TODO(macOS): port `src/platform/app_menu_mac.mm` (an `NSMenu` built from [`AppMenu`], its clicks queued for
//! the UI thread). Until then every platform draws the in-app bar and [`has_native_menu_bar`] is `false`.

use std::fmt;

/// What kind of row an [`AppMenuItem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppMenuItemKind {
    #[default]
    Normal,
    Checkbox,
    Separator,
}

/// One row of a native menu.
#[derive(Default)]
pub struct AppMenuItem {
    pub kind: AppMenuItemKind,
    /// Without mnemonic markers.
    pub label: String,
    /// The shortcut as shown ("Cmd+Shift+T"), displayed next to the item.
    pub accelerator: String,
    pub enabled: bool,
    pub checked: bool,
    /// Which action to run on the UI thread, by the index [`take_native_menu_clicks`] reports.
    pub click: Option<usize>,
    pub submenu: Vec<AppMenuItem>,
}

impl fmt::Debug for AppMenuItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppMenuItem")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

pub type AppMenu = Vec<AppMenuItem>;

/// True where the platform's own menu bar carries the application menu.
#[must_use]
pub const fn has_native_menu_bar() -> bool {
    false
}

/// Installs one top-level menu per title; the menu `i` is built each time it is about to open. Empty `titles`
/// removes the menus. The shortcuts shown are not acted on by the menus: the keystrokes still reach the window
/// and the app's own shortcut handling.
pub fn set_native_menu_bar(titles: &[&str], build: Option<fn(usize) -> AppMenu>) {
    let _ = (titles, build);
}

/// The clicks made in the native menus since the last call (indices of the actions of the menus that were
/// built), for the UI thread to run during its frame.
#[must_use]
pub fn take_native_menu_clicks() -> Vec<usize> {
    Vec::new()
}
