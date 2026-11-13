//! `AppFrame`: the application as a [`Frame`] of the shell: it owns the application, the themes and the UI state,
//! builds each frame's [`Cx`], and carries out the dev commands (`--select-row`, `--open-file`).

use dear_imgui_rs::Ui;
use imgit_app::repo_state::{DiffSource, DiffTarget};
use imgit_git::{FileStatus, NodeType};

use crate::app::application::Application;
use crate::dev::DevCommand;
use crate::shell::{Frame, ShellContext};
use crate::theme::Themes;
use crate::ui::panels::menus::text_field_menu;
use crate::ui::{Cx, UiState};

/// The application on the shell's window.
pub struct AppFrame {
    app: Application,
    themes: Themes,
    ui_state: UiState,
}

impl AppFrame {
    /// Starts the application on `shell`'s window: applies the saved zoom and theme, and opens the repository
    /// named on the command line, else the remembered tabs, else the most recent repository (as `main()` of the
    /// C++ build did).
    #[must_use]
    pub fn new(mut app: Application, shell: &mut ShellContext<'_>) -> Self {
        let mut ui_state = UiState::new();
        ui_state.text_field_menu = Some(text_field_menu);
        ui_state
            .avatars
            .set_cache_directory(Some(app.app_path("gravatars")));
        // The saved zoom and theme (custom themes first) apply with the first frame.
        app.request_custom_themes(&mut ui_state, false);
        app.apply_settings_to(&mut ui_state);

        let options = shell.options;
        match &options.repo {
            Some(repo) => app.open_repository(&repo.to_string_lossy()),
            None if app.settings.remember_tabs => app.restore_tabs(),
            None => {}
        }
        if options.terminal {
            app.show_terminal_panel = true;
        }
        if app.tabs.is_empty()
            && options.repo.is_none()
            && let Some(recent) = app.recent_repos.first().cloned()
        {
            app.open_repository(&recent.to_string_lossy());
        }
        Self {
            app,
            themes: Themes::new(),
            ui_state,
        }
    }
}

impl Frame for AppFrame {
    fn draw(&mut self, ui: &Ui, shell: &mut ShellContext<'_>) {
        self.ui_state.begin_frame(ui, &mut self.themes, shell);
        let mut cx = Cx::new(ui, shell, self.themes.palette(), &mut self.ui_state);
        self.app.draw_frame(&mut cx);
    }

    fn dev_command(&mut self, command: DevCommand, _shell: &mut ShellContext<'_>) {
        let Some(state) = self.app.active_mut() else {
            return;
        };
        match command {
            DevCommand::SelectRow(row) => {
                if let Some(sha) = state
                    .history
                    .commits
                    .get(row)
                    .map(|commit| commit.sha.clone())
                {
                    state.select_only(&sha);
                    state.scroll_to_row = Some(row);
                }
            }
            DevCommand::OpenFile {
                path,
                file_history,
                blame,
            } => {
                let sha = state
                    .single_selected()
                    .filter(|commit| commit.node_type != NodeType::WorkDir)
                    .map(|commit| commit.sha.clone());
                if file_history {
                    state.open_file_history(path, sha.clone(), sha, blame);
                } else if let Some(sha) = sha {
                    state.open_diff(DiffTarget {
                        source: DiffSource::Commit(sha),
                        path,
                        status: FileStatus::Modified,
                    });
                } else {
                    state.open_diff(DiffTarget {
                        source: DiffSource::Unstaged,
                        path,
                        status: FileStatus::Modified,
                    });
                }
            }
        }
    }

    /// Screenshots wait for the avatar downloads and the File History view's background jobs.
    fn busy(&self) -> bool {
        self.ui_state.busy()
            || self.app.panels.graph_view.busy() // the CHANGES column's counts
            || self.app.active().is_some_and(|state| {
                state
                    .file_history
                    .as_ref()
                    .is_some_and(|view| view.history_loading || view.blame_loading)
            })
    }
}
