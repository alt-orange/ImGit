//! The keyboard shortcuts (`handle_shortcuts` of the C++ application), run at the start of every frame.

use dear_imgui_rs::{Key, KeyChord, KeyMods};
use imgit_app::{RepoState, WIP_SHA};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::ui::Cx;
use crate::ui::menu::menu_open;
use crate::ui::panels::repo_management::favorite_repositories;

const CTRL: KeyMods = KeyMods::CTRL;
const SHIFT: KeyMods = KeyMods::SHIFT;
const ALT: KeyMods = KeyMods::ALT;

/// `Key1` .. `Key9`.
const DIGITS: [Key; 9] = [
    Key::Key1,
    Key::Key2,
    Key::Key3,
    Key::Key4,
    Key::Key5,
    Key::Key6,
    Key::Key7,
    Key::Key8,
    Key::Key9,
];

/// Whether exactly `mods` + `key` was pressed this frame (`ImGui::IsKeyChordPressed`).
fn chord(cx: &Cx<'_>, mods: KeyMods, key: Key) -> bool {
    cx.ui
        .is_key_chord_pressed(KeyChord::new(key).with_mods(mods))
}

impl Application {
    /// The shortcuts of the whole application, then (for the active tab) those of the repository. Nothing while
    /// a dialog or a menu is open.
    pub(crate) fn handle_shortcuts(&mut self, cx: &mut Cx<'_>) {
        if self.dialog.is_open() || menu_open(cx) {
            return;
        }
        if chord(cx, CTRL, Key::P) || chord(cx, CTRL | SHIFT, Key::P) {
            self.fuzzy.toggle();
        }
        if self.fuzzy.open {
            return;
        }
        if chord(cx, CTRL, Key::Equal) || chord(cx, CTRL, Key::KeypadAdd) {
            self.set_zoom(cx, self.settings.zoom + 0.1);
        }
        if chord(cx, CTRL, Key::Minus) || chord(cx, CTRL, Key::KeypadSubtract) {
            self.set_zoom(cx, self.settings.zoom - 0.1);
        }
        if chord(cx, CTRL, Key::Key0) {
            self.set_zoom(cx, 1.0);
        }
        if chord(cx, CTRL, Key::GraveAccent) {
            self.show_terminal_panel = !self.show_terminal_panel;
            self.terminal.minimized = false;
            self.terminal.focus_requested = self.show_terminal_panel;
        }
        // The repository shown, when there is one (its state may be out: the tab caches what is needed here).
        let workdir = self
            .active_tab
            .and_then(|index| self.tabs.get(index))
            .map(|tab| tab.workdir.clone());
        // Terminal tabs: new, close, next / previous, by number.
        if let Some(workdir) = workdir.as_deref()
            && self.show_terminal_panel
        {
            let tabs = self.terminal_tabs(Some(workdir));
            let count = tabs.sessions.len();
            let active = tabs.active;
            if chord(cx, CTRL | SHIFT, Key::N) {
                self.new_terminal_tab(Some(workdir));
            }
            if chord(cx, CTRL | SHIFT, Key::W) {
                self.close_terminal_tab(Some(workdir), active);
            }
            let tabs = self.terminal_tabs(Some(workdir));
            if count > 0 && chord(cx, CTRL, Key::PageDown) {
                tabs.active = (tabs.active + 1) % count;
            }
            if count > 0 && chord(cx, CTRL, Key::PageUp) {
                tabs.active = (tabs.active + count - 1) % count;
            }
            for (i, &key) in DIGITS.iter().enumerate().take(count) {
                if chord(cx, CTRL | SHIFT, key) {
                    tabs.active = i;
                }
            }
        }
        // The File menu accelerators work from text fields too.
        if !(self.show_terminal_panel && self.terminal.focused) {
            if chord(cx, CTRL, Key::O) {
                self.browse_for_repository(cx);
            }
            if chord(cx, CTRL | ALT, Key::O) {
                self.repo_management_open = true;
            }
            for (i, &key) in DIGITS.iter().enumerate() {
                if chord(cx, CTRL | ALT, key) {
                    self.open_favorite_repository(i);
                }
            }
            if chord(cx, CTRL, Key::T) {
                self.active_tab = None;
            }
            if chord(cx, CTRL | SHIFT, Key::T) {
                self.reopen_closed_tab();
            }
            if workdir.is_some()
                && chord(cx, CTRL, Key::W)
                && let Some(active) = self.active_tab
            {
                self.close_tab(active);
                return;
            }
        }
        // While a text field or the terminal has the keyboard, keys are typing.
        if cx.ui.io().want_text_input() || (self.show_terminal_panel && self.terminal.focused) {
            return;
        }
        if chord(cx, CTRL, Key::Slash) {
            self.dialog.open(DialogKind::KeyboardShortcuts);
        }
        if chord(cx, CTRL, Key::J) {
            self.show_left_panel = !self.show_left_panel;
        }
        if chord(cx, CTRL, Key::K) {
            self.show_detail_panel = !self.show_detail_panel;
        }
        // From the New Tab page (-1) Ctrl+Tab goes to the first tab, Ctrl+Shift+Tab to the one before the last.
        let count = self.tabs.len() as isize;
        let current = self.active_tab.map_or(-1, |index| index as isize);
        if chord(cx, CTRL, Key::Tab) && count > 0 {
            self.active_tab = Some((current + 1).rem_euclid(count) as usize);
        }
        if chord(cx, CTRL | SHIFT, Key::Tab) && count > 0 {
            self.active_tab = Some((current - 1).rem_euclid(count) as usize);
        }
        for (i, &key) in DIGITS.iter().enumerate().take(self.tabs.len()) {
            if chord(cx, CTRL, key) {
                self.active_tab = Some(i);
            }
        }
        if workdir.is_some() {
            self.with_active(|app, state| app.handle_repo_shortcuts(cx, state));
        }
    }

    /// Opens the `index`th (0-based) favourite repository, in Repository Management's order.
    pub fn open_favorite_repository(&mut self, index: usize) {
        let favorites = favorite_repositories(self);
        match favorites.get(index) {
            Some(path) => self.open_repository(path),
            None => self.toasts.push_with_detail(
                crate::app::toasts::ToastKind::Info,
                format!("No favorite repository {}", index + 1),
                "Star repositories in Repository Management (Alt+Ctrl+O) to open them with Alt+Ctrl+1 to 9.",
            ),
        }
    }

    /// The shortcuts that act on the repository shown.
    fn handle_repo_shortcuts(&mut self, cx: &mut Cx<'_>, state: &mut RepoState) {
        let ui = cx.ui;
        if ui.is_key_pressed(Key::F5) {
            state.refresh();
        }
        if chord(cx, CTRL, Key::L) {
            self.run_git_cli(state, "Fetch", ["fetch", "--all", "--prune"]);
        }
        if chord(cx, CTRL, Key::F) {
            state.search_open = true;
            state.search_focus_requested = true;
        }
        if chord(cx, CTRL, Key::Z) {
            self.undo(state);
        }
        if chord(cx, CTRL, Key::Y) || chord(cx, CTRL | SHIFT, Key::Z) {
            self.redo(state);
        }
        if chord(cx, CTRL | SHIFT, Key::S) {
            let result = state.repo.stage_all();
            self.run(state, "Stage all", result, None);
        }
        if chord(cx, CTRL | SHIFT, Key::U) {
            let result = state.repo.unstage_all();
            self.run(state, "Unstage all", result, None);
        }
        if chord(cx, CTRL, Key::B) {
            let sha = state
                .single_selected()
                .filter(|commit| commit.sha != WIP_SHA)
                .map(|commit| commit.sha.clone())
                .or_else(|| state.history.head_sha.clone())
                .unwrap_or_default();
            self.dialog.open(DialogKind::CreateBranch);
            self.dialog.flag = true;
            self.dialog.sha = sha;
        }
        if ui.is_key_pressed(Key::Escape) {
            if state.search_open {
                state.search_open = false;
                state.run_search();
            } else if state.merge_tool.is_some() && !ui.io().want_text_input() {
                state.merge_tool = None;
            } else if state.file_history.is_some() {
                state.close_file_history();
            } else if state.diff_target.is_some() {
                // Unsaved edits close with the editor's Cancel only.
                if state
                    .file_edit
                    .as_ref()
                    .is_none_or(|edit| edit.text == edit.saved)
                {
                    state.close_diff();
                }
            } else if state.rebase.active && state.rebase.editing.is_some() {
                state.rebase.editing = None; // closes the reword editor only
            } else if state.rebase.active {
                state.rebase = imgit_app::repo_state::RebasePlan::default();
            }
        }
    }
}
