//! The repository tabs: opening, closing, moving, grouping, naming; submodules open in a tab; the helpers that
//! lend a tab's [`RepoState`] to a closure; the aliases of repositories and the persistence of the tabs.

use std::fs;
use std::path::{Path, PathBuf};

use imgit_app::RepoState;
use imgit_app::preferences::{pref, preferences};
use imgit_app::profiles::write_text_file;
use imgit_git::Repository;

use crate::app::application::{Application, MAX_RECENT};
use crate::app::dialog::DialogKind;
use crate::app::persistence::{
    TabEntry, TabsFile, alias_key, format_aliases, format_hidden_refs, format_recent,
    parse_aliases, parse_hidden_refs, parse_recent,
};
use crate::app::toasts::ToastKind;
use crate::theme::GRAPH_COLOR_COUNT;
use crate::ui::Cx;

/// A named, coloured run of tabs that the tabs bar marks with a label in front of them. A collapsed group shows
/// only that label (and the active tab, when it is one of them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroup {
    pub name: String,
    /// A graph lane colour index.
    pub color: usize,
    pub collapsed: bool,
}

/// One repository tab.
///
/// `state` is `None` only while the tab's panels draw (see [`Application::with_tab`]); the other fields are
/// cached from it, so the tabs bar and the menus can use a tab whose state is out.
pub struct Tab {
    /// Names the tab through reorders and closes (a `with_tab` finds it again by it).
    pub id: u64,
    /// The repository shown (a submodule while one is open in the tab). Its `parent` chain leads to the
    /// repository the tab was opened on.
    pub state: Option<Box<RepoState>>,
    /// The working directory of the repository shown.
    pub workdir: PathBuf,
    /// Its folder's name (what the tab is called while a submodule is open).
    pub repo_name: String,
    /// The working directory of the repository the tab was opened on (under any submodule open in it).
    pub root_workdir: PathBuf,
    /// Its `.git` folder, shared with its linked worktrees (forward slashes, no trailing one).
    pub root_common_dir: String,
    /// The tab's own name instead of its repository's.
    pub name: Option<String>,
    /// The tab group.
    pub group: Option<String>,
    /// A submodule is open in the tab.
    pub submodule: bool,
}

impl Tab {
    pub(crate) fn new(id: u64, state: Box<RepoState>) -> Self {
        let root = state.root();
        let root_workdir = root.repo.workdir().to_path_buf();
        let root_common_dir = root.repo.common_dir();
        let mut tab = Self {
            id,
            workdir: PathBuf::new(),
            repo_name: String::new(),
            root_workdir,
            root_common_dir,
            name: None,
            group: None,
            submodule: false,
            state: None,
        };
        tab.attach(state);
        tab
    }

    /// Puts `state` in the tab and caches what the tabs bar needs of it.
    pub(crate) fn attach(&mut self, state: Box<RepoState>) {
        self.workdir = state.repo.workdir().to_path_buf();
        self.repo_name = state.repo.name().to_owned();
        self.submodule = state.parent.is_some();
        self.state = Some(state);
    }
}

/// A submodule opened by [`Application::open_submodule`], waiting for the tab's state to come back.
pub(crate) struct PendingSubmodule {
    pub tab_id: u64,
    pub submodule: Box<RepoState>,
}

/// Where a repository state sits: tab `tab` (by index), `depth` parents below the state shown.
struct Slot {
    tab: usize,
    depth: usize,
}

impl Application {
    // Looking states up

    /// The active tab's state; `None` on the New Tab page and **while the state is out** (see
    /// [`Self::with_tab`]).
    #[must_use]
    pub fn active(&self) -> Option<&RepoState> {
        self.tabs.get(self.active_tab?)?.state.as_deref()
    }

    /// [`Self::active`], mutable.
    pub fn active_mut(&mut self) -> Option<&mut RepoState> {
        let index = self.active_tab?;
        self.tabs.get_mut(index)?.state.as_deref_mut()
    }

    /// The working directory of the repository shown in the active tab: available **while its state is out**
    /// too (the tab caches it); `None` on the New Tab page.
    #[must_use]
    pub fn active_workdir(&self) -> Option<&Path> {
        self.tabs
            .get(self.active_tab?)
            .map(|tab| tab.workdir.as_path())
    }

    /// Every repository state: each tab's, and the repositories an open submodule waits in front of. A tab whose
    /// state is out is skipped.
    pub fn repo_states(&self) -> impl Iterator<Item = &RepoState> {
        self.tabs.iter().flat_map(|tab| {
            std::iter::successors(tab.state.as_deref(), |state| state.parent.as_deref())
        })
    }

    /// Calls `f` with every repository state (those [`Self::repo_states`] lists).
    pub fn for_each_repo_state_mut(&mut self, mut f: impl FnMut(&mut RepoState)) {
        fn visit(state: &mut RepoState, f: &mut impl FnMut(&mut RepoState)) {
            f(state);
            if let Some(parent) = state.parent.as_deref_mut() {
                visit(parent, f);
            }
        }
        for tab in &mut self.tabs {
            if let Some(state) = tab.state.as_deref_mut() {
                visit(state, &mut f);
            }
        }
    }

    /// The state of the repository at `workdir`, shown or behind an open submodule; `None` when none, and
    /// while it is out.
    #[must_use]
    pub fn find_repo_state(&self, workdir: &Path) -> Option<&RepoState> {
        self.repo_states()
            .find(|state| state.repo.workdir() == workdir)
    }

    /// The working directories of the repository states [`Self::repo_states`] lists.
    #[must_use]
    pub fn repo_workdirs(&self) -> Vec<PathBuf> {
        self.repo_states()
            .map(|state| state.repo.workdir().to_path_buf())
            .collect()
    }

    fn find_slot(&self, workdir: &Path) -> Option<Slot> {
        self.tabs.iter().enumerate().find_map(|(tab, entry)| {
            std::iter::successors(entry.state.as_deref(), |state| state.parent.as_deref())
                .position(|state| state.repo.workdir() == workdir)
                .map(|depth| Slot { tab, depth })
        })
    }

    /// The tab index of the repository whose working directory is `workdir` (the one shown, not one behind an
    /// open submodule).
    #[must_use]
    pub fn tab_index_of(&self, workdir: &Path) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.workdir == workdir)
    }

    // Detaching states

    /// Runs `f` with tab `index`'s state taken out of the tab, so that it can also get `&mut Application`. The
    /// state is put back afterwards, wherever `f` returns; when `f` closed the tab it is dropped. `None` (and `f`
    /// not run) when there is no such tab or its state is already out.
    ///
    /// ```ignore
    /// app.with_tab(index, |app, state| {
    ///     let result = state.repo.stage_all();
    ///     app.run(state, "Stage all", result, None);
    /// });
    /// ```
    ///
    /// While `f` runs, `app.tabs[index].state` is `None`: do not look the same repository up through the
    /// application (`active()`, `with_repo()`...) in `f`; use `state`.
    pub fn with_tab<R>(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut Self, &mut RepoState) -> R,
    ) -> Option<R> {
        let tab = self.tabs.get_mut(index)?;
        let id = tab.id;
        let mut state = tab.state.take()?;
        let result = f(self, &mut state);
        self.put_back(id, state);
        Some(result)
    }

    /// [`Self::with_tab`] for the active tab.
    pub fn with_active<R>(&mut self, f: impl FnOnce(&mut Self, &mut RepoState) -> R) -> Option<R> {
        self.with_tab(self.active_tab?, f)
    }

    /// Runs `f` with the state of the repository at `workdir` (a tab's, or one behind an open submodule) taken
    /// out as [`Self::with_tab`] does. A no-op returning `None` when no such repository is open **or its state
    /// is out** (the tab's panels are drawing it). This is what a deferred action uses: it captured the
    /// `workdir` (an owned `PathBuf`), not a reference to the state.
    pub fn with_repo<R>(
        &mut self,
        workdir: &Path,
        f: impl FnOnce(&mut Self, &mut RepoState) -> R,
    ) -> Option<R> {
        let slot = self.find_slot(workdir)?;
        if slot.depth == 0 {
            return self.with_tab(slot.tab, f);
        }
        // Behind an open submodule: cut the chain at that state and join it again afterwards.
        let tab = &mut self.tabs[slot.tab];
        let id = tab.id;
        let mut child = tab.state.as_deref_mut()?;
        for _ in 1..slot.depth {
            child = child.parent.as_deref_mut()?;
        }
        let mut state = child.parent.take()?;
        let result = f(self, &mut state);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id)
            && let Some(mut child) = tab.state.as_deref_mut()
        {
            for _ in 1..slot.depth {
                match child.parent.as_deref_mut() {
                    Some(next) => child = next,
                    None => return Some(result),
                }
            }
            child.parent = Some(state);
        }
        Some(result)
    }

    /// Returns a state taken out by `with_tab` to its tab, finishing a submodule `f` opened or closed.
    fn put_back(&mut self, id: u64, mut state: Box<RepoState>) {
        if self
            .pending_submodule
            .as_ref()
            .is_some_and(|pending| pending.tab_id == id)
            && let Some(pending) = self.pending_submodule.take()
        {
            let mut submodule = pending.submodule;
            submodule.parent = Some(state);
            state = submodule;
        } else if self.closing_submodule == Some(id) {
            self.closing_submodule = None;
            if let Some(mut parent) = state.parent.take() {
                // The submodule may have moved meanwhile (a checkout, a commit).
                parent.refresh();
                // The submodule's state goes at the start of the next frame, once nothing drawing it refers to
                // it.
                self.closed_submodules
                    .push(std::mem::replace(&mut state, parent));
            }
        }
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.attach(state);
        }
    }

    // Opening and closing

    /// Opens the repository at `path` in a tab (or shows its tab when it is open already): a toast when it
    /// cannot be opened.
    pub fn open_repository(&mut self, path: &str) {
        self.repo_management_open = false;
        self.new_tab_open = false;
        let path = path.trim();
        if path.is_empty() {
            return;
        }
        let repo = match Repository::open(path) {
            Ok(repo) => repo,
            Err(error) => {
                self.toasts.push_with_detail(
                    ToastKind::Error,
                    "Could not open repository",
                    error.to_string(),
                );
                return;
            }
        };
        let workdir = repo.workdir().to_path_buf();
        // A tab showing one of its submodules still holds the repository.
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.workdir == workdir || tab.root_workdir == workdir)
        {
            self.active_tab = Some(index);
            return;
        }
        let mut state = Box::new(RepoState::new(repo));
        self.configure_new_state(&mut state);
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab::new(id, state));
        let index = self.tabs.len() - 1;
        self.active_tab = Some(index);
        self.group_worktree_tab(index);

        self.recent_repos.retain(|recent| *recent != workdir);
        self.recent_repos.insert(0, workdir);
        self.recent_repos.truncate(MAX_RECENT);
        self.save_recent();
    }

    /// The settings a tab starts from, and the refs hidden last time.
    fn configure_new_state(&self, state: &mut RepoState) {
        state.commit_limit = self.commit_limit();
        state.push_after_commit = self.settings.push_after_commit;
        let key = alias_key(&state.repo.workdir().to_string_lossy());
        if let Some(hidden) = self.hidden_refs.get(&key) {
            state.hidden_refs.clone_from(hidden);
            state.refresh(); // the constructor loaded the graph with every ref
        }
    }

    /// After a clone: when the repository in the active tab has submodules, asks whether to initialize them
    /// recursively.
    pub fn offer_recursive_submodule_init(&mut self) {
        let Some(tab) = self.active_tab.and_then(|index| self.tabs.get(index)) else {
            return;
        };
        let workdir = tab.workdir.clone();
        if !workdir.join(".gitmodules").exists() {
            return;
        }
        self.dialog.open(DialogKind::Confirm);
        self.dialog.workdir = Some(workdir.clone());
        self.dialog.title = "Initialize submodules".to_owned();
        self.dialog.message =
            "This repository has submodules. Initialize them recursively?".to_owned();
        self.dialog.confirm_label = "Initialize".to_owned();
        self.dialog.danger = false;
        self.dialog.on_confirm = Some(Box::new(move |_, app| {
            app.with_repo(&workdir, |app, state| {
                app.run_git_cli(
                    state,
                    "Initialize submodules",
                    ["submodule", "update", "--init", "--recursive"],
                );
            });
        }));
    }

    /// Open Repo: a native folder picker starting at the last opened repository; a folder that is not a
    /// repository gets an "Open Failed" toast.
    pub fn browse_for_repository(&mut self, cx: &Cx<'_>) {
        let start = self.recent_repos.first().cloned();
        let Some(picked) =
            imgit_platform::shell::pick_folder_dialog(Some(cx.shell.window), start.as_deref())
        else {
            return;
        };
        if Repository::open(&picked).is_err() {
            let name = picked
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Open Failed",
                format!("Could not find a compatible repository at '{name}'."),
            );
            return;
        }
        self.open_repository(&picked.to_string_lossy());
    }

    /// Opens the submodule at `path` (relative to the working directory of `state`, the repository shown in its
    /// tab) in that tab; `state` waits behind it until [`Self::close_submodule`]. Only from inside
    /// [`Self::with_tab`] (`state` is the detached state): the swap happens when the closure returns.
    pub fn open_submodule(&mut self, state: &RepoState, path: &str) {
        let Some(tab) = self.tab_index_of(state.repo.workdir()) else {
            return;
        };
        // A submodule that is not checked out has no .git of its own: opening its folder would find the parent.
        let folder = state.repo.workdir().join(path);
        if !folder.join(".git").exists() {
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Could not open submodule",
                format!("'{path}' is not initialized."),
            );
            return;
        }
        let repo = match Repository::open(&folder) {
            Ok(repo) => repo,
            Err(error) => {
                self.toasts.push_with_detail(
                    ToastKind::Error,
                    "Could not open submodule",
                    error.to_string(),
                );
                return;
            }
        };
        let mut submodule = Box::new(RepoState::new(repo));
        self.configure_new_state(&mut submodule);
        self.pending_submodule = Some(PendingSubmodule {
            tab_id: self.tabs[tab].id,
            submodule,
        });
    }

    /// Back from the submodule shown in `state`'s tab to the repository it belongs to (not while it runs a git
    /// command). Only from inside [`Self::with_tab`]: the swap happens when the closure returns.
    pub fn close_submodule(&mut self, state: &RepoState) {
        let Some(tab) = self.tab_index_of(state.repo.workdir()) else {
            return;
        };
        // A command running in the submodule would finish with nowhere to report to.
        if state.parent.is_none() || state.busy() {
            return;
        }
        // A dialog opened for the submodule goes with it.
        if self.dialog.is_open() && self.dialog.workdir.as_deref() == Some(state.repo.workdir()) {
            self.dialog.close();
        }
        self.closing_submodule = Some(self.tabs[tab].id);
    }

    /// Closes tab `index`.
    pub fn close_tab(&mut self, index: usize) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        self.closed_tabs.push(tab.root_workdir.clone());
        // A dialog opened for this repository (its on_confirm may hold the tab) goes with it.
        if self.dialog.is_open() && self.dialog.workdir.as_deref() == Some(tab.workdir.as_path()) {
            self.dialog.close();
        }
        self.tabs.remove(index);
        self.drop_empty_tab_groups();
        // New Tab (None) stays where it is; a repository tab right of the closed one keeps showing the same
        // repository.
        let Some(active) = self.active_tab else {
            return;
        };
        if self.tabs.is_empty() {
            self.active_tab = None;
        } else if active > index || active >= self.tabs.len() {
            self.active_tab = Some(active.saturating_sub(1));
        }
    }

    /// Moves the repository tab `from` to `to` (a drag along the tabs bar), keeping the active tab active.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let count = self.tabs.len();
        if from == to || from >= count || to >= count {
            return;
        }
        let moved = self.tabs.remove(from);
        self.tabs.insert(to, moved);
        self.active_tab = self.active_tab.map(|active| {
            if active == from {
                to
            } else if from < active && active <= to {
                active - 1
            } else if to <= active && active < from {
                active + 1
            } else {
                active
            }
        });
        if self.settings.remember_tabs {
            self.save_tabs();
        }
    }

    /// File > Reopen Closed Tab.
    pub fn reopen_closed_tab(&mut self) {
        if let Some(path) = self.closed_tabs.pop() {
            self.open_repository(&path.to_string_lossy());
        }
    }

    /// Selects the tab `direction` (+1 / -1) steps away, wrapping round.
    pub fn select_adjacent_tab(&mut self, direction: isize) {
        let count = self.tabs.len() as isize;
        if count == 0 {
            return;
        }
        let from = self.active_tab.unwrap_or(0) as isize;
        self.active_tab = Some((from + direction).rem_euclid(count) as usize);
    }

    // Tab groups

    #[must_use]
    pub fn find_tab_group(&self, name: &str) -> Option<&TabGroup> {
        self.tab_groups.iter().find(|group| group.name == name)
    }

    pub fn find_tab_group_mut(&mut self, name: &str) -> Option<&mut TabGroup> {
        self.tab_groups.iter_mut().find(|group| group.name == name)
    }

    pub(crate) fn drop_empty_tab_groups(&mut self) {
        let tabs = &self.tabs;
        self.tab_groups.retain(|group| {
            tabs.iter()
                .any(|tab| tab.group.as_deref() == Some(group.name.as_str()))
        });
    }

    /// Moves the tab into `group` (created with the next colour when new) beside the group's other tabs; `None`
    /// takes it out of its group. A group left without tabs goes.
    pub fn set_tab_group(&mut self, index: usize, group: Option<&str>) {
        if index >= self.tabs.len() {
            return;
        }
        let group = group.filter(|name| !name.is_empty());
        if let Some(name) = group
            && self.find_tab_group(name).is_none()
        {
            // The default group colour (Preferences > UI Customization), else the next colour round the lanes'
            // wheel after the groups already there.
            let color = self.settings.tab_group_color.unwrap_or_else(|| {
                self.tab_groups
                    .iter()
                    .map(|existing| existing.color + 1)
                    .fold(0, usize::max)
            });
            self.tab_groups.push(TabGroup {
                name: name.to_owned(),
                color: color % GRAPH_COLOR_COUNT,
                collapsed: false,
            });
        }
        self.tabs[index].group = group.map(str::to_owned);
        if let Some(name) = group {
            // Beside the group's last other tab, so the group stays one run.
            let last = self
                .tabs
                .iter()
                .enumerate()
                .filter(|(i, tab)| *i != index && tab.group.as_deref() == Some(name))
                .map(|(i, _)| i)
                .next_back();
            if let Some(last) = last {
                self.move_tab(index, if last < index { last + 1 } else { last });
            }
        }
        self.drop_empty_tab_groups();
        if self.settings.remember_tabs {
            self.save_tabs();
        }
    }

    /// Renames a group; nothing when `to` is empty or taken.
    pub fn rename_tab_group(&mut self, from: &str, to: &str) {
        if to.is_empty() || from == to || self.find_tab_group(to).is_some() {
            return;
        }
        let Some(group) = self.find_tab_group_mut(from) else {
            return;
        };
        group.name = to.to_owned();
        for tab in &mut self.tabs {
            if tab.group.as_deref() == Some(from) {
                tab.group = Some(to.to_owned());
            }
        }
        if self.settings.remember_tabs {
            self.save_tabs();
        }
    }

    /// Takes every tab out of the group and drops it.
    pub fn ungroup_tabs(&mut self, name: &str) {
        for tab in &mut self.tabs {
            if tab.group.as_deref() == Some(name) {
                tab.group = None;
            }
        }
        self.drop_empty_tab_groups();
        if self.settings.remember_tabs {
            self.save_tabs();
        }
    }

    /// Closes every tab of the group.
    pub fn close_tab_group(&mut self, name: &str) {
        for index in (0..self.tabs.len()).rev() {
            if self.tabs[index].group.as_deref() == Some(name) {
                self.close_tab(index);
            }
        }
    }

    /// The tab's own name instead of its repository's; empty: back to the repository's.
    pub fn rename_tab(&mut self, index: usize, name: &str) {
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let name = name.trim();
        tab.name = (!name.is_empty()).then(|| name.to_owned());
        if self.settings.remember_tabs {
            self.save_tabs();
        }
    }

    /// What a repository tab is called: its own name, else its repository's alias or folder name (in a
    /// submodule, the submodule's).
    #[must_use]
    pub fn tab_label(&self, index: usize) -> String {
        let tab = &self.tabs[index];
        if tab.submodule {
            return tab.repo_name.clone(); // a submodule open in the tab
        }
        tab.name
            .clone()
            .unwrap_or_else(|| self.repo_display_name(&tab.workdir))
    }

    /// Automatically groups worktree tabs together: a repository whose .git folder another tab shares (one is a
    /// linked worktree of the other) joins that tab's group, or both go into one named after the main
    /// repository. Not while the remembered tabs are restored: they keep their own groups.
    fn group_worktree_tab(&mut self, index: usize) {
        if self.restoring_tabs
            || !preferences().flag(pref::GROUP_WORKTREES)
            || index >= self.tabs.len()
        {
            return;
        }
        let workdir = self.tabs[index].workdir.clone();
        let common = self.tabs[index].root_common_dir.clone();
        for i in 0..self.tabs.len() {
            if i == index || self.tabs[i].root_common_dir != common {
                continue;
            }
            let mut group = self.tabs[i].group.clone().unwrap_or_default();
            if group.is_empty() {
                // <main>/.git -> <main>
                group = Path::new(&common)
                    .parent()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if group.is_empty() {
                    group.clone_from(&self.tabs[i].repo_name);
                }
                self.set_tab_group(i, Some(&group));
            }
            // set_tab_group moves tabs: this one is found again by its working directory.
            if let Some(at) = self.tab_index_of(&workdir) {
                self.set_tab_group(at, Some(&group));
            }
            self.active_tab = self.tab_index_of(&workdir);
            return;
        }
    }

    // Aliases

    /// Gives the repository an alias, shown wherever its name is; an empty alias removes it.
    pub fn set_repo_alias(&mut self, workdir: &Path, alias: &str) {
        let alias = alias.trim();
        let key = alias_key(&workdir.to_string_lossy());
        if alias.is_empty() {
            self.repo_aliases.remove(&key);
        } else {
            self.repo_aliases.insert(key, alias.to_owned());
        }
        self.save_aliases();
    }

    /// The repository's alias, else its folder's name.
    #[must_use]
    pub fn repo_display_name(&self, workdir: &Path) -> String {
        let key = alias_key(&workdir.to_string_lossy());
        if let Some(alias) = self.repo_aliases.get(&key) {
            return alias.clone();
        }
        key.rsplit_once('/')
            .map_or(key.as_str(), |(_, name)| name)
            .to_owned()
    }

    // Files of the profile

    fn read_profile_file(&self, file: &str) -> String {
        fs::read(self.config_path(file))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    }

    pub(crate) fn load_recent(&mut self) {
        self.recent_repos = parse_recent(&self.read_profile_file("recent.txt"), Path::exists);
    }

    pub fn save_recent(&self) {
        let _ = write_text_file(
            &self.config_path("recent.txt"),
            &format_recent(&self.recent_repos),
        );
    }

    pub(crate) fn load_aliases(&mut self) {
        self.repo_aliases = parse_aliases(&self.read_profile_file("aliases.txt"));
    }

    fn save_aliases(&self) {
        let _ = write_text_file(
            &self.config_path("aliases.txt"),
            &format_aliases(&self.repo_aliases),
        );
    }

    pub(crate) fn load_hidden_refs(&mut self) {
        self.hidden_refs = parse_hidden_refs(&self.read_profile_file("hidden_refs.txt"));
    }

    /// Saves the refs the tabs hid or showed since the last time.
    pub(crate) fn sync_hidden_refs(&mut self) {
        let mut changed = false;
        for state in self.tabs.iter().flat_map(|tab| {
            std::iter::successors(tab.state.as_deref(), |state| state.parent.as_deref())
        }) {
            let key = alias_key(&state.repo.workdir().to_string_lossy());
            let unchanged = match self.hidden_refs.get(&key) {
                Some(saved) => *saved == state.hidden_refs,
                None => state.hidden_refs.is_empty(),
            };
            if unchanged {
                continue;
            }
            if state.hidden_refs.is_empty() {
                self.hidden_refs.remove(&key);
            } else {
                self.hidden_refs.insert(key, state.hidden_refs.clone());
            }
            changed = true;
        }
        if changed {
            let _ = write_text_file(
                &self.config_path("hidden_refs.txt"),
                &format_hidden_refs(&self.hidden_refs),
            );
        }
    }

    /// The open tabs, as `tabs.txt` has them.
    fn tabs_file(&self) -> TabsFile {
        TabsFile {
            groups: self.tab_groups.clone(),
            tabs: self
                .tabs
                .iter()
                .map(|tab| TabEntry {
                    path: tab.root_workdir.to_string_lossy().into_owned(),
                    name: tab.name.clone(),
                    group: tab.group.clone(),
                })
                .collect(),
            active: Some(self.active_tab.map_or(-1, |index| index as i32)),
        }
    }

    pub(crate) fn save_tabs(&self) {
        let _ = write_text_file(&self.config_path("tabs.txt"), &self.tabs_file().to_text());
    }

    /// Opens the tabs of `tabs.txt` again, with their names and groups, and selects the one that was active.
    pub fn restore_tabs(&mut self) {
        let file = TabsFile::parse(&self.read_profile_file("tabs.txt"));
        self.tab_groups = file.groups;
        self.restoring_tabs = true;
        for entry in file.tabs {
            if !Path::new(&entry.path).exists() {
                continue;
            }
            let before = self.tabs.len();
            self.open_repository(&entry.path);
            if self.tabs.len() == before {
                continue;
            }
            if let Some(opened) = self.tabs.last_mut() {
                opened.name = entry.name;
                opened.group = entry.group;
            }
        }
        self.restoring_tabs = false;
        self.drop_empty_tab_groups();
        if let Some(wanted) = file.active
            && wanted >= -1
            && wanted < self.tabs.len() as i32
        {
            self.active_tab = usize::try_from(wanted).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An application on a scratch settings folder with `names` open as tabs (empty repositories).
    struct Fixture {
        app: Application,
        _scratch: tempfile::TempDir,
    }

    fn fixture(names: &[&str]) -> Fixture {
        let scratch = tempfile::tempdir().unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.settings.remember_tabs = false;
        for name in names {
            let folder = scratch.path().join(name);
            Repository::init(&folder).unwrap();
            app.open_repository(&folder.to_string_lossy());
        }
        Fixture {
            app,
            _scratch: scratch,
        }
    }

    fn labels(app: &Application) -> Vec<String> {
        (0..app.tabs.len()).map(|i| app.tab_label(i)).collect()
    }

    fn groups(app: &Application) -> Vec<Option<&str>> {
        app.tabs.iter().map(|tab| tab.group.as_deref()).collect()
    }

    #[test]
    fn repositories_open_once_and_become_active() {
        let mut f = fixture(&["a", "b"]);
        assert_eq!(labels(&f.app), ["a", "b"]);
        assert_eq!(f.app.active_tab, Some(1));
        let path = f.app.tabs[0].root_workdir.to_string_lossy().into_owned();
        f.app.open_repository(&path);
        assert_eq!(f.app.tabs.len(), 2);
        assert_eq!(f.app.active_tab, Some(0));
        assert_eq!(f.app.recent_repos.len(), 2);
        f.app.open_repository("   ");
        f.app.open_repository("/definitely/not/a/repository");
        assert_eq!(f.app.tabs.len(), 2);
        assert_eq!(f.app.toasts.history()[0].title, "Could not open repository");
    }

    #[test]
    fn closing_tabs_keeps_the_active_one() {
        let mut f = fixture(&["a", "b", "c"]);
        f.app.active_tab = Some(2);
        f.app.close_tab(0);
        assert_eq!(labels(&f.app), ["b", "c"]);
        assert_eq!(f.app.active_tab, Some(1));
        f.app.close_tab(1);
        assert_eq!(f.app.active_tab, Some(0));
        f.app.close_tab(0);
        assert_eq!(f.app.active_tab, None);
        assert_eq!(f.app.closed_tabs.len(), 3);
        f.app.reopen_closed_tab();
        assert_eq!(labels(&f.app), ["b"]);
        // New Tab stays where it is.
        f.app.active_tab = None;
        f.app.close_tab(0);
        assert_eq!(f.app.active_tab, None);
    }

    #[test]
    fn moving_a_tab_keeps_the_active_tab() {
        let mut f = fixture(&["a", "b", "c", "d"]);
        f.app.active_tab = Some(1);
        f.app.move_tab(0, 2);
        assert_eq!(labels(&f.app), ["b", "c", "a", "d"]);
        assert_eq!(f.app.active_tab, Some(0));
        f.app.move_tab(3, 0);
        assert_eq!(labels(&f.app), ["d", "b", "c", "a"]);
        assert_eq!(f.app.active_tab, Some(1));
        f.app.move_tab(1, 3);
        assert_eq!(labels(&f.app), ["d", "c", "a", "b"]);
        assert_eq!(f.app.active_tab, Some(3));
        f.app.move_tab(1, 1);
        f.app.move_tab(9, 0);
        assert_eq!(labels(&f.app), ["d", "c", "a", "b"]);
    }

    #[test]
    fn tabs_are_grouped_beside_each_other() {
        let mut f = fixture(&["a", "b", "c", "d"]);
        f.app.set_tab_group(0, Some("Work"));
        f.app.set_tab_group(2, Some("Work"));
        // c moved next to a.
        assert_eq!(labels(&f.app), ["a", "c", "b", "d"]);
        assert_eq!(groups(&f.app), [Some("Work"), Some("Work"), None, None]);
        assert_eq!(f.app.tab_groups.len(), 1);
        f.app.set_tab_group(3, Some("Play"));
        // Colours go round the lane wheel after the groups already there.
        assert_eq!(f.app.find_tab_group("Work").map(|g| g.color), Some(0));
        assert_eq!(f.app.find_tab_group("Play").map(|g| g.color), Some(1));
        f.app.set_tab_group(3, Some("Work"));
        assert_eq!(labels(&f.app), ["a", "c", "d", "b"]);
        // Play lost its last tab and went.
        assert!(f.app.find_tab_group("Play").is_none());
        f.app.set_tab_group(0, None);
        assert_eq!(groups(&f.app), [None, Some("Work"), Some("Work"), None]);
    }

    #[test]
    fn a_default_group_colour_is_taken_from_the_settings() {
        let mut f = fixture(&["a", "b"]);
        f.app.settings.tab_group_color = Some(GRAPH_COLOR_COUNT + 3);
        f.app.set_tab_group(0, Some("One"));
        assert_eq!(f.app.tab_groups[0].color, 3);
    }

    #[test]
    fn groups_rename_ungroup_and_close() {
        let mut f = fixture(&["a", "b", "c"]);
        f.app.set_tab_group(0, Some("G"));
        f.app.set_tab_group(1, Some("G"));
        f.app.set_tab_group(2, Some("H"));
        f.app.rename_tab_group("G", "H");
        assert!(f.app.find_tab_group("G").is_some(), "the name is taken");
        f.app.rename_tab_group("G", "");
        f.app.rename_tab_group("Missing", "X");
        f.app.rename_tab_group("G", "Renamed");
        assert_eq!(
            groups(&f.app),
            [Some("Renamed"), Some("Renamed"), Some("H")]
        );
        f.app.find_tab_group_mut("Renamed").unwrap().collapsed = true;
        f.app.ungroup_tabs("H");
        assert_eq!(groups(&f.app), [Some("Renamed"), Some("Renamed"), None]);
        assert_eq!(f.app.tab_groups.len(), 1);
        f.app.close_tab_group("Renamed");
        assert_eq!(labels(&f.app), ["c"]);
        assert!(f.app.tab_groups.is_empty());
    }

    #[test]
    fn tabs_are_named_by_name_alias_or_folder() {
        let mut f = fixture(&["a"]);
        let workdir = f.app.tabs[0].workdir.clone();
        assert_eq!(f.app.tab_label(0), "a");
        f.app.set_repo_alias(&workdir, "  Alias ");
        assert_eq!(f.app.tab_label(0), "Alias");
        f.app.rename_tab(0, " Own name ");
        assert_eq!(f.app.tab_label(0), "Own name");
        f.app.rename_tab(0, "  ");
        assert_eq!(f.app.tab_label(0), "Alias");
        f.app.set_repo_alias(&workdir, "");
        assert_eq!(f.app.tab_label(0), "a");
    }

    #[test]
    fn tabs_survive_a_restart() {
        let scratch = tempfile::tempdir().unwrap();
        let config = scratch.path().join("config");
        for name in ["a", "b", "c"] {
            Repository::init(scratch.path().join(name)).unwrap();
        }
        let first = {
            let mut app = Application::with_config_directory(config.clone());
            for name in ["a", "b", "c"] {
                app.open_repository(&scratch.path().join(name).to_string_lossy());
            }
            app.set_tab_group(0, Some("Work"));
            app.set_tab_group(1, Some("Work"));
            app.find_tab_group_mut("Work").unwrap().collapsed = true;
            app.rename_tab(2, "Third");
            app.active_tab = Some(1);
            app.set_repo_alias(&app.tabs[0].workdir.clone(), "First");
            app.save_state();
            (labels(&app), groups(&app).len())
        };
        let mut again = Application::with_config_directory(config);
        again.restore_tabs();
        assert_eq!(labels(&again), first.0);
        assert_eq!(labels(&again), ["First", "b", "Third"]);
        assert_eq!(groups(&again), [Some("Work"), Some("Work"), None]);
        assert!(again.find_tab_group("Work").unwrap().collapsed);
        assert_eq!(again.active_tab, Some(1));
        assert_eq!(first.1, 3);
        assert_eq!(again.recent_repos.len(), 3);
    }

    #[test]
    fn the_state_of_a_tab_is_out_while_it_is_lent() {
        let mut f = fixture(&["a", "b"]);
        let workdir_b = f.app.tabs[1].workdir.clone();
        let workdir_a = f.app.tabs[0].workdir.clone();
        let seen = f.app.with_tab(1, |app, state| {
            assert!(app.tabs[1].state.is_none());
            assert!(app.active().is_none());
            assert!(app.find_repo_state(&workdir_b).is_none());
            assert!(app.find_repo_state(&workdir_a).is_some());
            assert_eq!(app.repo_states().count(), 1);
            assert!(app.with_repo(&workdir_b, |_, _| ()).is_none());
            assert!(app.with_tab(1, |_, _| ()).is_none());
            // Other tabs can be lent meanwhile.
            assert_eq!(
                app.with_repo(&workdir_a, |_, other| other.repo.workdir().to_path_buf()),
                Some(workdir_a.clone())
            );
            state.repo.workdir().to_path_buf()
        });
        assert_eq!(seen, Some(workdir_b.clone()));
        assert!(f.app.tabs.iter().all(|tab| tab.state.is_some()));
        assert!(f.app.with_tab(7, |_, _| ()).is_none());
    }

    #[test]
    fn a_state_goes_back_to_its_tab_after_the_tabs_were_reordered_or_closed() {
        let mut f = fixture(&["a", "b", "c"]);
        let moved = f.app.with_tab(0, |app, state| {
            app.move_tab(0, 2);
            state.repo.name().to_owned()
        });
        assert_eq!(moved.as_deref(), Some("a"));
        assert_eq!(labels(&f.app), ["b", "c", "a"]);
        assert!(f.app.tabs.iter().all(|tab| tab.state.is_some()));
        f.app.with_tab(2, |app, _| app.close_tab(2));
        assert_eq!(labels(&f.app), ["b", "c"]);
        assert!(f.app.tabs.iter().all(|tab| tab.state.is_some()));
    }

    #[test]
    fn a_dialog_for_a_closed_tab_goes_with_it() {
        let mut f = fixture(&["a", "b"]);
        f.app.dialog.open(DialogKind::Confirm);
        f.app.dialog.workdir = Some(f.app.tabs[1].workdir.clone());
        f.app.close_tab(0);
        assert!(f.app.dialog.is_open());
        f.app.close_tab(0);
        assert!(!f.app.dialog.is_open());
    }

    #[test]
    fn adjacent_tabs_wrap_round() {
        let mut f = fixture(&["a", "b", "c"]);
        f.app.active_tab = Some(2);
        f.app.select_adjacent_tab(1);
        assert_eq!(f.app.active_tab, Some(0));
        f.app.select_adjacent_tab(-1);
        assert_eq!(f.app.active_tab, Some(2));
        f.app.active_tab = None;
        f.app.select_adjacent_tab(1);
        assert_eq!(f.app.active_tab, Some(1));
    }

    #[test]
    fn hidden_refs_are_saved_and_given_to_new_tabs() {
        let mut f = fixture(&["a"]);
        f.app
            .active_mut()
            .unwrap()
            .hidden_refs
            .insert("refs/heads/x".to_owned());
        f.app.sync_hidden_refs();
        let saved = fs::read_to_string(f.app.config_path("hidden_refs.txt")).unwrap();
        assert!(saved.ends_with("\trefs/heads/x\n"), "{saved}");
        let workdir = f.app.tabs[0].root_workdir.clone();
        f.app.close_tab(0);
        f.app.open_repository(&workdir.to_string_lossy());
        assert!(f.app.active().unwrap().hidden_refs.contains("refs/heads/x"));
    }
}
