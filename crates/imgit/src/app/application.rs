//! The application: the open repository tabs, the modals, the preferences, the profiles, the terminal panel and
//! the frame that draws them (`draw_frame`, in `frame.rs`).
//!
//! The C++ `Application` is split by topic into submodules that each add an `impl Application`:
//!
//! | file | what |
//! |---|---|
//! | `tabs.rs` | tabs, tab groups, aliases, submodules, the detach helpers (`with_tab`, `with_repo`) |
//! | `git_ops.rs` | `run`, `run_git_cli*`, force push, undo / redo, auto fetch, polling the git commands |
//! | `credentials.rs`, `clone.rs`, `background.rs` | the credentials prompt, `git clone` with progress, jobs |
//! | `terminal.rs` | the terminal panel's tabs and sessions |
//! | `shortcuts.rs` | the keyboard shortcuts |
//! | `frame.rs`, `menu_bar.rs` | `draw_frame`, the menu bar, status bar, toasts, `draw_repo_tab` |
//!
//! # The active tab is detached while its panels draw
//!
//! A panel takes `app: &mut Application` *and* the tab's `state: &mut RepoState`. Since the state lives in
//! `app.tabs`, [`Application::with_tab`] takes it out of its [`Tab`] for the duration of the closure and puts it
//! back afterwards (also when the closure returns early or closes the tab). While it is out, the tab's `state`
//! is `None`: `active()`, `find_repo_state()` and `repo_states()` skip it, and `with_repo()` on its working
//! directory does nothing. Use the `state` you were handed instead.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use imgit_app::connections::Connections;
use imgit_app::preferences::{pref, preferences, ssh_command};
use imgit_app::profiles::{Profile, Profiles};
use imgit_git::cli;
use imgit_git::repository::{
    global_user_email, global_user_name, set_global_identity, set_profile_identity,
};
use imgit_platform::{process, shell as platform_shell};

use crate::app::background::BackgroundJob;
use crate::app::clone::PendingClone;
use crate::app::credentials::CredentialPrompt;
use crate::app::dialog::{Dialog, FuzzyFinder, ProfileDialog};
use crate::app::settings::Settings;
use crate::app::tabs::{PendingSubmodule, Tab, TabGroup};
use crate::app::terminal::TerminalPanel;
use crate::app::toasts::{ToastKind, Toasts};
use crate::ui::panels::PanelStates;
use crate::ui::{Cx, raw};
use imgit_app::RepoState;

/// The version the About dialog, the status bar and the menus show.
pub const VERSION: &str = "1.0.0";

/// How many repositories the recent list keeps.
pub const MAX_RECENT: usize = 12;

/// Files that belong to a profile; before profiles existed they sat in the settings folder itself.
const PROFILE_FILES: [&str; 6] = [
    "settings.txt",
    "preferences.txt",
    "recent.txt",
    "tabs.txt",
    "favorites.txt",
    "aliases.txt",
];

/// The settings folder: `%APPDATA%\.imgit` on Windows, `~/.imgit` elsewhere.
#[must_use]
pub fn config_directory() -> PathBuf {
    let directory = imgit_platform::system::app_data_directory().join(".imgit");
    // Where ImGit kept its settings before profiles.
    let legacy = imgit_platform::system::config_directory().join("imgit");
    if !directory.exists() && legacy.is_dir() {
        let _ = fs::rename(&legacy, &directory);
    }
    directory
}

/// A `%.0f`-less way to show the zoom: 1.1 -> "110%".
#[must_use]
pub fn zoom_percent(zoom: f32) -> i32 {
    (zoom * 100.0).round() as i32
}

pub struct Application {
    /// The repository tabs. The active one's `state` is `None` while its panels draw (see the module docs).
    pub tabs: Vec<Tab>,
    /// `None`: the "New Tab" page.
    pub active_tab: Option<usize>,
    /// The "+" button's "New Tab" entry in the tabs bar.
    pub new_tab_open: bool,
    /// The tabs bar folder's Repository Management view.
    pub repo_management_open: bool,
    /// Popovers of the tabs bar asked for from elsewhere (a menu, the command palette); opened on its next
    /// draw.
    pub tabs_list_requested: bool,
    pub notifications_requested: bool,
    pub toasts: Toasts,
    pub dialog: Dialog,
    pub profile_dialog: ProfileDialog,
    pub fuzzy: FuzzyFinder,
    pub terminal: TerminalPanel,
    pub profiles: Profiles,
    pub settings: Settings,
    /// Newest first.
    pub recent_repos: Vec<PathBuf>,
    /// `git clone` in flight; the destination opens as a tab when it finishes.
    pub pending_clone: Option<PendingClone>,
    /// A git command refused for want of a username / password waits here for the user.
    pub credential_prompt: Option<CredentialPrompt>,
    /// The credentials git accepted this session (ImGit keeps them for the session unless remembered).
    pub credentials: Vec<cli::Credential>,
    /// Preferences > Connections: the hosting services reachable over HTTPS / SSH and their checks.
    pub connections: Connections,
    /// Paths of closed tabs, most recent last (File > Reopen Closed Tab).
    pub closed_tabs: Vec<PathBuf>,
    /// Repository tab groups: named, coloured runs of tabs that the tabs bar marks with a label in front of
    /// them. Saved with the tabs.
    pub tab_groups: Vec<TabGroup>,
    /// Repository aliases by [`alias_key`](crate::app::persistence::alias_key) of the working directory.
    pub repo_aliases: BTreeMap<String, String>,
    /// The Preferences view (replaces the tab content).
    pub preferences_open: bool,
    pub preferences_page: String,
    pub show_left_panel: bool,
    pub show_detail_panel: bool,
    pub show_terminal_panel: bool,
    pub show_toolbar: bool,
    /// Every panel's state between frames (the C++ function-local statics).
    pub panels: PanelStates,

    // Not for the panels.
    pub(crate) config_directory: PathBuf,
    pub(crate) background: Vec<BackgroundJob>,
    /// The refs hidden from the graph (the eye toggles) per repository, kept in `hidden_refs.txt`; a tab
    /// opening the repository starts from them and `sync_hidden_refs` saves what the tabs changed.
    pub(crate) hidden_refs: BTreeMap<String, HashSet<String>>,
    /// `restore_tabs` is opening the remembered tabs.
    pub(crate) restoring_tabs: bool,
    /// This session was started with a repository on the command line (also how "Open in a new window"
    /// starts one): its tabs are not the remembered set, so it never writes `tabs.txt`.
    pub(crate) started_with_repository: bool,
    pub(crate) next_tab_id: u64,
    /// A submodule `open_submodule` made, waiting for its parent's tab to be put back.
    pub(crate) pending_submodule: Option<PendingSubmodule>,
    /// The tab whose submodule `close_submodule` is closing, finished when the tab's state is put back.
    pub(crate) closing_submodule: Option<u64>,
    /// Submodules closed this frame, released at the start of the next one.
    #[allow(clippy::vec_box)] // the boxes come from the tabs and go back to nothing
    pub(crate) closed_submodules: Vec<Box<RepoState>>,
    pub(crate) pending_profile: Option<String>,
    /// The toasts pushed so far, as of the last frame (a rise makes the window ask for attention).
    pub(crate) toasts_pushed_seen: usize,
    /// The next custom-themes reload is one the user asked for: it reports success.
    pub(crate) report_theme_reload: bool,
}

impl Application {
    /// The application with the profile in the settings folder ([`config_directory`]) loaded: its settings,
    /// preferences, recent repositories, aliases and hidden refs.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config_directory(config_directory())
    }

    /// The application with the profiles in `config_directory`.
    #[must_use]
    pub fn with_config_directory(config_directory: PathBuf) -> Self {
        let _ = fs::create_dir_all(&config_directory);
        let profiles = Profiles::load(
            &config_directory,
            &global_user_name().unwrap_or_default(),
            &global_user_email().unwrap_or_default(),
            &PROFILE_FILES,
        );
        let mut app = Self {
            tabs: Vec::new(),
            active_tab: None,
            new_tab_open: false,
            repo_management_open: false,
            tabs_list_requested: false,
            notifications_requested: false,
            toasts: Toasts::default(),
            dialog: Dialog::default(),
            profile_dialog: ProfileDialog::default(),
            fuzzy: FuzzyFinder::default(),
            terminal: TerminalPanel::default(),
            profiles,
            settings: Settings::default(),
            recent_repos: Vec::new(),
            pending_clone: None,
            credential_prompt: None,
            credentials: Vec::new(),
            connections: Connections::new(),
            closed_tabs: Vec::new(),
            tab_groups: Vec::new(),
            repo_aliases: BTreeMap::new(),
            preferences_open: false,
            preferences_page: "General".to_owned(),
            show_left_panel: true,
            show_detail_panel: true,
            show_terminal_panel: false,
            show_toolbar: true,
            panels: PanelStates::default(),
            config_directory,
            background: Vec::new(),
            hidden_refs: BTreeMap::new(),
            restoring_tabs: false,
            started_with_repository: false,
            next_tab_id: 1,
            pending_submodule: None,
            closing_submodule: None,
            closed_submodules: Vec::new(),
            pending_profile: None,
            toasts_pushed_seen: 0,
            report_theme_reload: false,
        };
        app.apply_profile_identity(false);
        app.settings.load(&app.config_path("settings.txt"));
        let _ = preferences().load(app.config_path("preferences.txt"));
        // Preferences > SSH, read at each git run (on its thread): the chosen key, and whether the credential
        // manager may show its own sign-in window.
        cli::set_environment_provider(Some(Box::new(|| {
            let preferences = preferences();
            let mut environment = Vec::new();
            if let Some(command) = ssh_command(
                &preferences.get(pref::SSH_PRIVATE_KEY),
                preferences.flag(pref::SSH_AGENT),
            ) {
                environment.push(("GIT_SSH_COMMAND".to_owned(), command));
            }
            if preferences.flag(pref::CREDENTIAL_MANAGER) {
                environment.push(("GCM_INTERACTIVE".to_owned(), String::new()));
            }
            environment
        })));
        app.load_recent();
        app.load_aliases();
        app.load_hidden_refs();
        app
    }

    /// Writes the settings, the open tabs and the recent repositories (done again at exit).
    pub fn save_state(&self) {
        self.settings.save(&self.config_path("settings.txt"));
        if self.settings.remember_tabs {
            self.save_tabs();
        }
        self.save_recent();
    }

    /// A file of the current profile (settings, tabs, recent repositories...).
    #[must_use]
    pub fn config_path(&self, file: &str) -> PathBuf {
        self.profiles.directory().join(file)
    }

    /// A file shared by every profile (the avatar cache).
    #[must_use]
    pub fn app_path(&self, file: &str) -> PathBuf {
        self.config_directory.join(file)
    }

    // Profiles

    /// Switches to another profile once the frame is drawn: its settings, tabs and author replace the current
    /// ones.
    pub fn switch_profile(&mut self, guid: &str) {
        if guid != self.profiles.current().guid && self.profiles.find(guid).is_some() {
            self.pending_profile = Some(guid.to_owned());
        }
    }

    pub(crate) fn apply_profile_switch(&mut self, cx: &mut Cx<'_>) {
        let Some(guid) = self.pending_profile.take() else {
            return;
        };
        // The profile being left keeps its settings and tabs.
        self.save_state();
        // From here the tabs are the new profile's remembered ones, not the command-line repository.
        self.started_with_repository = false;
        self.tabs.clear();
        self.tab_groups.clear();
        self.closed_tabs.clear();
        self.recent_repos.clear();
        self.active_tab = None;
        self.new_tab_open = false;
        self.repo_management_open = false;
        self.dialog.close();

        if let Err(error) = self.profiles.set_current(&guid) {
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Could not switch profile",
                error.to_string(),
            );
        }
        self.apply_profile_identity(self.profiles.keep_git_config());
        self.settings = Settings::default();
        self.settings.load(&self.config_path("settings.txt"));
        let _ = preferences().load(self.config_path("preferences.txt"));
        self.load_recent();
        self.load_aliases();
        self.load_hidden_refs();
        self.request_custom_themes(cx.state, false);
        self.apply_settings(cx);
        if self.settings.remember_tabs {
            self.restore_tabs();
        }
    }

    /// The current profile's author signs commits and, with "Keep my .gitconfig updated", goes to the global
    /// `.gitconfig`.
    fn apply_profile_identity(&mut self, write_git_config: bool) {
        let profile = self.profiles.current().clone();
        set_profile_identity(&profile.user_name, &profile.user_email);
        if write_git_config
            && let Err(error) = set_global_identity(&profile.user_name, &profile.user_email)
        {
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Could not update .gitconfig",
                error.to_string(),
            );
        }
    }

    /// Creates a profile (from a copy of the current one's settings when asked) and switches to it.
    pub fn add_profile(&mut self, values: Profile, copy_current: bool) {
        if copy_current {
            // The copy starts from what is open now.
            self.save_state();
        }
        match self.profiles.create(values, copy_current) {
            Ok(guid) => self.switch_profile(&guid),
            Err(error) => self.toasts.push_with_detail(
                ToastKind::Error,
                "Could not create the profile",
                error.to_string(),
            ),
        }
    }

    pub fn edit_profile(&mut self, profile: &Profile) {
        if let Err(error) = self.profiles.update(profile) {
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Could not save the profile",
                error.to_string(),
            );
            return;
        }
        if profile.guid == self.profiles.current().guid {
            self.apply_profile_identity(self.profiles.keep_git_config());
            self.for_each_repo_state_mut(RepoState::refresh);
        }
    }

    pub fn delete_profile(&mut self, guid: &str) {
        if let Err(error) = self.profiles.remove(guid) {
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Could not delete the profile",
                error.to_string(),
            );
        }
    }

    pub fn set_keep_git_config(&mut self, keep: bool) {
        let _ = self.profiles.set_keep_git_config(keep);
    }

    /// Runs `f` with one panel's persistent state taken out of `self.panels`, so that `f` can use the whole
    /// application meanwhile (a panel's draw function: `app.panels.graph_view` would otherwise stay borrowed).
    /// The state is put back afterwards.
    ///
    /// ```ignore
    /// app.with_panel_state(|panels| &mut panels.graph_view, |app, graph| {
    ///     graph.scroll = 0.0;
    ///     app.run_git_cli(state, "Fetch", ["fetch"]);
    /// });
    /// ```
    ///
    /// While `f` runs the slot holds a default state: do not reach the same panel's state through `app` in it.
    pub fn with_panel_state<S: Default, R>(
        &mut self,
        select: impl Fn(&mut PanelStates) -> &mut S,
        f: impl FnOnce(&mut Self, &mut S) -> R,
    ) -> R {
        let mut state = std::mem::take(select(&mut self.panels));
        let result = f(self, &mut state);
        *select(&mut self.panels) = state;
        result
    }

    // Settings, zoom, themes

    /// The zoom, 50% to 200%.
    pub fn set_zoom(&mut self, cx: &mut Cx<'_>, zoom: f32) {
        self.settings.zoom = zoom.clamp(0.5, 2.0);
        self.apply_settings(cx);
    }

    /// Applies the zoom and the theme of the settings (from the next frame on), the commit limit to the open
    /// repositories, and saves the settings.
    pub fn apply_settings(&mut self, cx: &mut Cx<'_>) {
        self.apply_settings_to(cx.state);
    }

    /// [`Self::apply_settings`] without a frame (the start-up).
    pub fn apply_settings_to(&mut self, ui: &mut crate::ui::UiState) {
        ui.set_zoom(self.settings.zoom);
        ui.request_theme(&self.settings.theme);
        let limit = self.commit_limit();
        self.for_each_repo_state_mut(|state| state.commit_limit = limit);
        self.settings.save(&self.config_path("settings.txt"));
    }

    /// How many commits a tab's graph loads at first: Initial Commits in Graph, or all of them (Show All Commits
    /// in Graph).
    #[must_use]
    pub fn commit_limit(&self) -> usize {
        // Show All Commits in Graph: no limit.
        if preferences().flag(pref::SHOW_ALL_COMMITS) {
            usize::MAX
        } else {
            usize::try_from(self.settings.initial_commits).unwrap_or(500)
        }
    }

    /// Reads the profile's custom themes (its "themes" folder) again from the next frame on, reports files that
    /// could not be used (and, with `report`, that it worked) and applies the theme.
    pub fn reload_custom_themes(&mut self, cx: &mut Cx<'_>, report: bool) {
        self.request_custom_themes(cx.state, report);
        self.apply_settings(cx);
    }

    /// Asks the UI to load the custom themes of the current profile before the next frame.
    pub fn request_custom_themes(&mut self, ui: &mut crate::ui::UiState, report: bool) {
        self.report_theme_reload = report;
        ui.request_custom_theme_reload(self.config_path("themes"));
    }

    /// Reports the outcome of a custom-themes reload (`UiState::take_theme_reload`).
    pub(crate) fn report_theme_reload(&mut self, problems: &[String]) {
        for problem in problems {
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Custom theme not loaded",
                problem.clone(),
            );
        }
        if std::mem::take(&mut self.report_theme_reload) && problems.is_empty() {
            self.toasts
                .push(ToastKind::Success, "Custom themes reloaded");
        }
    }

    // Window-level actions

    /// Closes the window after this frame.
    pub fn quit(&mut self, cx: &mut Cx<'_>) {
        cx.shell.quit();
    }

    /// Starts another instance and closes this one. The state is written now, not at exit: the new instance
    /// reads these files as soon as it starts.
    pub fn relaunch(&mut self, cx: &mut Cx<'_>) {
        self.save_state();
        let _ = process::spawn_detached(
            &[process::executable_path().to_string_lossy().into_owned()],
            None,
        );
        cx.shell.quit();
    }

    /// Enters or leaves full screen after this frame.
    pub fn toggle_full_screen(&mut self, cx: &mut Cx<'_>) {
        cx.shell.toggle_full_screen();
    }

    /// Puts `text` on the clipboard and toasts it.
    pub fn copy_to_clipboard(&mut self, cx: &Cx<'_>, text: &str) {
        raw::set_clipboard_text(cx.ui, text);
        let shown = if text.chars().count() > 80 {
            let head: String = text.chars().take(77).collect();
            format!("{head}...")
        } else {
            text.to_owned()
        };
        self.toasts
            .push_with_detail(ToastKind::Info, "Copied to clipboard", shown);
    }

    // External tools

    /// Preferences > External Tools: opens `path` in the chosen editor.
    pub fn open_in_editor(&mut self, path: &Path) {
        let editor = preferences().get(pref::EDITOR);
        let command: Vec<String> = match editor.as_str() {
            "Visual Studio Code" => vec!["code".to_owned(), path.to_string_lossy().into_owned()],
            "Rider" => vec!["rider".to_owned(), path.to_string_lossy().into_owned()],
            "<Custom>" => {
                let command = imgit_app::preferences::custom_tool_command(
                    &preferences().get(pref::EDITOR_COMMAND),
                    path,
                );
                if command.is_empty() {
                    self.toasts.push_with_detail(
                        ToastKind::Error,
                        "No editor command",
                        "Set the custom editor's command in Preferences > External Tools.",
                    );
                    return;
                }
                command
            }
            _ => {
                let _ = platform_shell::open_in_editor(path);
                return;
            }
        };
        if platform_shell::launch(&command, None, editor != "<Custom>").is_err() {
            self.toasts.push_with_detail(
                ToastKind::Error,
                format!("Could not start {editor}"),
                format!("{} was not found.", command[0]),
            );
        }
    }

    /// Preferences > External Tools: a terminal at `path` (the in-app one for the open repository, when that
    /// is the choice; `has_repository` is whether one is open).
    pub fn open_terminal(&mut self, has_repository: bool, path: &Path) {
        let choice = preferences().get(pref::TERMINAL);
        let command: Vec<String> = match choice.as_str() {
            "In-App Terminal" => {
                self.show_terminal_panel = true;
                self.terminal.focus_requested = true;
                if !has_repository {
                    self.toasts.push(
                        ToastKind::Info,
                        "Open a repository to use the in-app terminal",
                    );
                }
                return;
            }
            "Command Prompt" => vec!["cmd.exe".to_owned(), "/K".to_owned()],
            "Git Bash" => {
                let root = git_for_windows_root();
                vec![
                    root.join("git-bash.exe").to_string_lossy().into_owned(),
                    format!("--cd={}", path.display()),
                ]
            }
            "<Custom>" => {
                let command = imgit_app::preferences::custom_tool_command(
                    &preferences().get(pref::TERMINAL_COMMAND),
                    path,
                );
                if command.is_empty() {
                    self.toasts.push_with_detail(
                        ToastKind::Error,
                        "No terminal command",
                        "Set the custom terminal's command in Preferences > External Tools.",
                    );
                    return;
                }
                command
            }
            _ => {
                let _ = platform_shell::open_external_terminal(path);
                return;
            }
        };
        if process::spawn_detached(&command, Some(path)).is_err() {
            self.toasts.push_with_detail(
                ToastKind::Error,
                format!("Could not start {choice}"),
                format!("{} was not found.", command[0]),
            );
        }
    }
}

impl Default for Application {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        self.save_state();
    }
}

/// Git for Windows' folder (`<git>/mingw64/libexec/git-core` -> `<git>`), empty when git is elsewhere.
fn git_for_windows_root() -> PathBuf {
    let output = cli::run(Path::new(""), ["--exec-path"], cli::RunOptions::default());
    let folder = output.text.trim();
    if output.exit_code != 0 || folder.is_empty() {
        return PathBuf::new();
    }
    Path::new(folder)
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}
