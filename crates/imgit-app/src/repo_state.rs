//! Everything one repository tab holds: the loaded history, its graph layout, the selection, the search, the
//! detail panel's caches, the open diff, the commit form, the undo / redo stacks, the rebase plan, the git
//! command in flight and the background status poll.

mod file_history;
mod rebase;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use imgit_git::repository::{DiffOptions, LineSelection, short_sha};
use imgit_git::{
    Commit, FileChange, FileDiff, FileStatus, History, HistoryOptions, NodeType, Ref, RefType,
    RepoState as OperationState, Repository, ResetMode, Revision, Sha, StatsSource, Submodule,
    WorkDirStatus, cli, graph,
};

use crate::job::Job;
use crate::merge_tool::MergeTool;
use crate::text::contains_ci;

pub use file_history::FileHistoryView;
pub use rebase::{PlanRow, RebasePlan};

/// The sha ImGit gives the uncommitted-changes row.
pub const WIP_SHA: &str = "WIP";

/// Listings with more files are shown without their line counts.
const MAX_FILES_WITH_STATS: usize = 600;
/// The undo stack forgets its oldest entries beyond this many.
const MAX_UNDO: usize = 50;
/// Enough context for a diff to hold the whole file.
const WHOLE_FILE_CONTEXT: u32 = 1 << 24;

/// Where the diff of a file comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiffSource {
    /// What this commit did to the file.
    Commit(Sha),
    /// HEAD against the index.
    Staged,
    /// The index against the working directory.
    Unstaged,
    /// This commit against the working directory.
    Compare(Sha),
}

impl DiffSource {
    /// The commit of a [`Commit`](Self::Commit) or [`Compare`](Self::Compare) source.
    #[must_use]
    pub fn sha(&self) -> Option<&str> {
        match self {
            Self::Commit(sha) | Self::Compare(sha) => Some(sha.as_str()),
            Self::Staged | Self::Unstaged => None,
        }
    }

    /// Whether the working directory is one side of the diff, so that a change in it makes the diff stale.
    #[must_use]
    pub const fn reads_workdir(&self) -> bool {
        !matches!(self, Self::Commit(_))
    }
}

/// The file whose diff is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffTarget {
    pub source: DiffSource,
    pub path: String,
    pub status: FileStatus,
}

impl DiffTarget {
    /// A target whose file status is not known.
    #[must_use]
    pub fn new(source: DiffSource, path: impl Into<String>) -> Self {
        Self {
            source,
            path: path.into(),
            status: FileStatus::Unknown,
        }
    }
}

/// The diff editor modes: the hunk list, the whole file inline (default) or side by side; the file view shows
/// the file itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DiffMode {
    Hunk,
    #[default]
    Inline,
    Split,
}

impl DiffMode {
    /// The context a diff is loaded with: the inline / split / file views show the whole file, the hunk view
    /// 3 lines around each change.
    #[must_use]
    pub const fn context_lines(self, file_view: bool) -> u32 {
        if matches!(self, Self::Hunk) && !file_view {
            3
        } else {
            WHOLE_FILE_CONTEXT
        }
    }
}

/// A long-running `git` CLI command (fetch / pull / push / rebase) in flight.
pub struct AsyncOperation {
    pub title: String,
    /// The command's output once it has ended. A job that turns idle without one (its worker died) has failed.
    pub job: Job<cli::Output>,
    /// Auto-fetch: no toast on success.
    pub silent: bool,
    /// Run again after the credentials prompt.
    pub args: Vec<String>,
    /// The credential entered for this run, kept when it works.
    pub attempt: Option<cli::Credential>,
    /// Commands run one after another once this one succeeds (push a tag to every remote); the toast comes
    /// with the last.
    pub queued: Vec<Vec<String>>,
}

impl fmt::Debug for AsyncOperation {
    /// Without the credential.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncOperation")
            .field("title", &self.title)
            .field("job", &self.job)
            .field("silent", &self.silent)
            .field("args", &self.args)
            .field("queued", &self.queued)
            .finish_non_exhaustive()
    }
}

/// How an [`UndoEntry`] reverses or repeats its action, on the tab's repository.
pub type UndoAction = Box<dyn FnMut(&Repository) -> imgit_git::Result<()>>;

/// The Undo / Redo: each entry knows how to reverse (and, when cheap, repeat) one action.
pub struct UndoEntry {
    pub label: String,
    pub undo: UndoAction,
    /// May be `None`: undo then derives it (back to the branch / commit it left, with `redo_mode`).
    pub redo: Option<UndoAction>,
    pub redo_mode: ResetMode,
}

impl UndoEntry {
    /// An entry whose redo is derived when it is undone, with a hard reset.
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        undo: impl FnMut(&Repository) -> imgit_git::Result<()> + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            undo: Box::new(undo),
            redo: None,
            redo_mode: ResetMode::Hard,
        }
    }

    /// The same entry with its own way to repeat the action.
    #[must_use]
    pub fn with_redo(
        mut self,
        redo: impl FnMut(&Repository) -> imgit_git::Result<()> + 'static,
    ) -> Self {
        self.redo = Some(Box::new(redo));
        self
    }

    /// The same entry with the reset its derived redo makes.
    #[must_use]
    pub const fn with_redo_mode(mut self, mode: ResetMode) -> Self {
        self.redo_mode = mode;
        self
    }
}

impl fmt::Debug for UndoEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UndoEntry")
            .field("label", &self.label)
            .field("redo", &self.redo.is_some())
            .field("redo_mode", &self.redo_mode)
            .finish_non_exhaustive()
    }
}

/// The message of the stash that keeps the uncommitted changes a hard reset would wipe.
pub const AUTO_STASH_BEFORE_RESET: &str = "ImGit: auto stash before hard reset";
/// The message of the stash that keeps the changes present while an undo moves HEAD.
pub const AUTO_STASH_BEFORE_UNDO: &str = "ImGit: auto stash before undo";

/// Whether the working directory holds changes a hard reset (or the checkout of another commit) would
/// overwrite: tracked files' changes, staged or not. Untracked files stay where they are, and a stash cannot
/// be made while files are conflicted.
fn has_overwritable_changes(repo: &Repository) -> bool {
    repo.load_status().is_ok_and(|status| {
        status.conflicted.is_empty()
            && status
                .staged
                .iter()
                .chain(&status.unstaged)
                .any(|change| change.status != FileStatus::Untracked)
    })
}

/// Pops the stash with commit `sha` (`stash_index_of` finds it wherever it sits now; a stash that is gone is
/// not an error: the user dropped it). A pop that fails keeps the stash, and the error names it. A pop that
/// applies with conflicts uses the stash up but leaves the changes in the conflicted files: that is an error
/// too, saying so.
fn pop_stash(repo: &Repository, sha: &str, message: &str) -> imgit_git::Result<()> {
    let Some(index) = repo.stash_index_of(sha) else {
        return Ok(());
    };
    repo.stash_pop(index).map_err(|error| {
        imgit_git::Error::Message(format!(
            "Your uncommitted changes could not be applied again ({error}). They are still saved in              stash@{{{index}}} \"{message}\"."
        ))
    })?;
    let conflicted = repo
        .load_status()
        .map_or(0, |status| status.conflicted.len());
    if conflicted > 0 {
        return Err(imgit_git::Error::Message(format!(
            "Your uncommitted changes were applied again with conflicts in {conflicted} file(s):              resolve them in the working directory (the stash \"{message}\" is used up)."
        )));
    }
    Ok(())
}

/// `action`'s result together with the result of putting the stash back: both failures are reported.
fn both(action: imgit_git::Result<()>, restore: imgit_git::Result<()>) -> imgit_git::Result<()> {
    match (action, restore) {
        (Ok(()), restored) => restored,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(restore)) => Err(imgit_git::Error::Message(format!("{error}. {restore}"))),
    }
}

/// Runs `action` with the working directory's changes stashed (as `message`) and put back afterwards, so that
/// an action that overwrites the working directory (a hard reset) keeps them. With `must_stash`, a stash that
/// cannot be made stops everything before `action` runs; without, `action` runs on without one. A pop that
/// fails (conflicts) is an error naming the stash that still holds the changes.
///
/// # Errors
///
/// `action`'s error, and the failure to put the changes back or to set them aside.
pub fn with_changes_stashed(
    repo: &Repository,
    message: &str,
    must_stash: bool,
    action: impl FnOnce(&Repository) -> imgit_git::Result<()>,
) -> imgit_git::Result<()> {
    let stash = if has_overwritable_changes(repo) {
        match repo.stash_push(message) {
            Ok(sha) => Some(sha),
            Err(error) if must_stash => {
                return Err(imgit_git::Error::Message(format!(
                    "Your uncommitted changes could not be set aside ({error}); nothing was changed."
                )));
            }
            Err(_) => None,
        }
    } else {
        None
    };
    let result = action(repo);
    match stash {
        Some(sha) => both(result, pop_stash(repo, &sha, message)),
        None => result,
    }
}

/// A hard reset to `target` that first stashes the uncommitted changes it would wipe. Returns the stash's
/// sha when there was something to keep.
///
/// # Errors
///
/// When the changes cannot be stashed (nothing was changed) or the reset fails (the changes are put back).
pub fn hard_reset_keeping_changes(
    repo: &Repository,
    target: &str,
) -> imgit_git::Result<Option<Sha>> {
    let stash = if has_overwritable_changes(repo) {
        Some(repo.stash_push(AUTO_STASH_BEFORE_RESET).map_err(|error| {
            imgit_git::Error::Message(format!(
                "Your uncommitted changes could not be set aside ({error}); nothing was reset."
            ))
        })?)
    } else {
        None
    };
    if let Err(error) = repo.reset(target, ResetMode::Hard) {
        if let Some(sha) = &stash {
            return both(Err(error), pop_stash(repo, sha, AUTO_STASH_BEFORE_RESET)).map(|()| None);
        }
        return Err(error);
    }
    Ok(stash)
}

/// The undo of [`hard_reset_keeping_changes`]: a hard reset back to `previous`, then the stashed changes
/// (`saved`) put back. Changes made since are stashed around it, so they survive too.
///
/// # Errors
///
/// As [`with_changes_stashed`]; a failed pop names the stash that still holds the changes.
pub fn undo_hard_reset(
    repo: &Repository,
    previous: &str,
    saved: Option<&str>,
) -> imgit_git::Result<()> {
    with_changes_stashed(repo, AUTO_STASH_BEFORE_UNDO, true, |repo| {
        repo.reset(previous, ResetMode::Hard)?;
        match saved {
            Some(sha) => pop_stash(repo, sha, AUTO_STASH_BEFORE_RESET),
            None => Ok(()),
        }
    })
}

/// The file view's editor (Edit This File): the working directory's copy of the diff target's file as text,
/// "\n" line breaks (written back as "\r\n" when `crlf`); `saved` is the text on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub path: String,
    pub text: String,
    pub saved: String,
    pub crlf: bool,
    /// The editor takes the keyboard focus on its next frame.
    pub focus: bool,
}

impl FileEdit {
    /// Whether the text differs from what is on the disk.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.text != self.saved
    }
}

impl Default for FileEdit {
    fn default() -> Self {
        Self {
            path: String::new(),
            text: String::new(),
            saved: String::new(),
            crlf: false,
            focus: true,
        }
    }
}

/// The inline "enter branch name" / "enter tag name" input in the graph's ref zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRef {
    pub sha: Sha,
    pub tag: bool,
    pub checkout: bool,
    pub name: String,
    /// The input takes the keyboard focus on its next frame.
    pub focus: bool,
}

impl Default for InlineRef {
    fn default() -> Self {
        Self {
            sha: Sha::new(),
            tag: false,
            checkout: false,
            name: String::new(),
            focus: true,
        }
    }
}

/// The working directory as one status read sees it.
struct StatusSnapshot {
    status: imgit_git::Result<WorkDirStatus>,
    operation_state: OperationState,
    /// [`Repository::refs_signature`]; `None`: it could not be read.
    refs_signature: Option<u64>,
    /// Read by the periodic poll only (a refresh loads them with the history): a checkout inside a submodule
    /// moves no ref here, yet changes whether it is in sync.
    submodules: Option<Vec<Submodule>>,
}

/// Status plus each file's line counts. Slow on a big tree or a network share (WSL's /mnt/c takes seconds), so
/// the periodic poll runs it off the UI thread.
///
/// `with_submodules`: also the submodules (each one opened to read its HEAD, as the status read does).
fn read_status(repo: &Repository, with_submodules: bool) -> StatusSnapshot {
    let mut status = repo.load_status();
    if let Ok(status) = &mut status
        && status.count() <= MAX_FILES_WITH_STATS
    {
        let _ = repo.fill_stats(&mut status.staged, StatsSource::Staged, None);
        let _ = repo.fill_stats(&mut status.unstaged, StatsSource::Unstaged, None);
    }
    StatusSnapshot {
        status,
        operation_state: repo.state(),
        refs_signature: repo.refs_signature().ok(),
        submodules: with_submodules.then(|| repo.submodules()),
    }
}

/// [`read_status`] of the repository at `workdir`, opened for the calling (worker) thread.
fn read_status_at(workdir: &std::path::Path) -> StatusSnapshot {
    match Repository::open(workdir) {
        Ok(worker) => read_status(&worker, true),
        Err(error) => StatusSnapshot {
            status: Err(error),
            operation_state: OperationState::None,
            refs_signature: None,
            submodules: None,
        },
    }
}

/// Everything one repository tab holds: the loaded history, its graph layout, selection and panel state.
///
/// Dropping it never waits for a background job still running (status read, file history, git command).
pub struct RepoState {
    pub repo: Repository,
    /// The repository this one is a submodule of, while the submodule is open in its tab.
    pub parent: Option<Box<RepoState>>,
    pub history: History,
    pub status: WorkDirStatus,
    pub layout: graph::Layout,
    /// Per row: the row whose branches it inherits.
    pub child_ref_rows: Vec<Option<usize>>,
    /// Per row without refs: the row whose refs it shows as ghost refs (its `child_ref_rows` entry).
    pub ghost_ref_rows: Vec<Option<usize>>,
    pub operation_state: OperationState,
    pub load_error: Option<String>,
    /// Full ref name forced into column 0.
    pub pinned_branch: Option<String>,

    // Graph walk options
    pub commit_limit: usize,
    /// The HEAD the submodules were last updated for (Keep submodules up to date).
    pub submodules_head: Option<Sha>,
    /// Full names.
    pub hidden_refs: HashSet<String>,
    /// Full name.
    pub solo_ref: Option<String>,
    /// The smart branch visibility preference, as the history was loaded.
    pub smart_branch_visibility: bool,

    // Graph selection and keyboard focus
    /// Ordered by click.
    pub selected: Vec<Sha>,
    /// The anchor of a Shift range.
    pub focused: Option<Sha>,
    /// The moving end of a Shift+arrow range.
    pub range_cursor: Option<Sha>,
    pub scroll_to_row: Option<usize>,
    /// Row under the mouse this frame.
    pub hovered_sha: Option<Sha>,

    // Commit search (Ctrl+F)
    pub search_open: bool,
    pub search_text: String,
    /// Rows, top down.
    pub search_matches: Vec<usize>,
    /// Index into `search_matches`.
    pub search_index: Option<usize>,
    /// The match selected, kept across refreshes.
    pub search_match_sha: Option<Sha>,
    pub search_focus_requested: bool,

    // Detail panel caches
    pub files_for_sha: Option<Sha>,
    pub commit_files: Vec<FileChange>,
    pub commit_files_error: Option<String>,
    /// A commit compared against the working directory (the WIP row and that commit selected); reloaded with
    /// the status.
    pub compare_sha: Option<Sha>,
    pub compare_files: Vec<FileChange>,
    pub compare_error: Option<String>,
    pub diff_target: Option<DiffTarget>,
    /// A conflicted file being resolved.
    pub merge_tool: Option<MergeTool>,
    /// Open over the tab.
    pub file_history: Option<FileHistoryView>,
    pub diff: Option<FileDiff>,
    pub file_edit: Option<FileEdit>,
    pub diff_mode: DiffMode,
    pub file_view: bool,
    /// Show changes in whitespace only (off: they are left out of the diff).
    pub diff_whitespace: bool,
    /// The working-directory diff's lines picked for Stage / Unstage Lines, dropped when the diff reloads.
    pub diff_selected_lines: Vec<LineSelection>,
    /// The diff `diff_selected_lines` were picked in.
    pub diff_selection_revision: Option<Revision>,
    /// The left panel collapses while a file is open.
    pub left_open_in_diff: bool,
    pub diff_error: Option<String>,
    /// Path | Tree toggle
    pub tree_view: bool,
    /// Tree folders start collapsed.
    pub expanded_folders: HashSet<String>,
    /// "Unstaged Files" / "Staged Files" headers.
    pub collapsed_sections: HashSet<String>,

    // Commit form (kept across refreshes)
    pub commit_summary: String,
    pub commit_description: String,
    pub amend: bool,
    pub push_after_commit: bool,
    pub commit_options_open: bool,
    /// Commit options read from the git configuration the first time the form is drawn: GPG signing defaults
    /// to commit.gpgsign, and the form starts from commit.template (again after each commit).
    pub commit_form_ready: bool,
    pub commit_sign: bool,
    pub commit_skip_hooks: bool,
    pub commit_template: Option<String>,
    /// The operation (merge, cherry-pick, revert) whose prepared message (MERGE_MSG) the form was filled from.
    pub prepared_message_for: OperationState,
    /// What the form was filled with, cleared again if left as it was.
    pub prepared_summary: String,
    pub prepared_description: String,
    /// The composer's Stash tab: its own message, stashing every change.
    pub composer_stash: bool,
    pub stash_summary: String,
    pub stash_description: String,
    /// The commit view's message editor (clicking HEAD's message): its own copy of the message.
    pub amending_message: bool,
    /// The commit whose message the detail panel is editing (HEAD or an older one).
    pub amend_sha: Option<Sha>,
    pub amend_focus_requested: bool,
    pub amend_summary: String,
    pub amend_description: String,

    // Left panel
    pub left_filter: String,
    /// Collapsed folders / remotes.
    pub collapsed_nodes: HashSet<String>,
    /// None expanded by default.
    pub expanded_left_sections: HashSet<String>,
    /// Dragged section ends, by section key.
    pub left_section_ys: HashMap<String, f32>,
    /// The ref clicked in the left panel.
    pub left_selected: Option<String>,

    pub inline_ref: Option<InlineRef>,

    // Undo / redo
    pub undo_stack: Vec<UndoEntry>,
    pub redo_stack: Vec<UndoEntry>,

    pub rebase: RebasePlan,

    /// The git command in flight.
    pub async_op: Option<AsyncOperation>,
    /// When the last status read ended (the UI's clock, seconds); 0 asks for one right away.
    pub last_status_poll: f64,
    pub last_auto_fetch: f64,

    /// The `gitflow.*` configuration and when it was read (`ui::panels::workflows`: re-read after a second).
    pub gitflow_config_cache:
        std::cell::RefCell<Option<(std::time::Instant, crate::workflows::gitflow::Config)>>,

    selected_set: HashSet<Sha>,
    status_job: Job<StatusSnapshot>,
    /// Bumped by every status read on the UI thread; a poll started before one is out of date.
    status_generation: u64,
    status_job_generation: u64,
    /// The refs the rows were built from.
    refs_signature: Option<u64>,
}

impl fmt::Debug for RepoState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RepoState")
            .field("repo", &self.repo)
            .field("commits", &self.history.commits.len())
            .field("selected", &self.selected)
            .field("busy", &self.busy())
            .finish_non_exhaustive()
    }
}

impl RepoState {
    /// The tab of `repository`, its history and status loaded.
    #[must_use]
    pub fn new(repository: Repository) -> Self {
        let mut state = Self {
            repo: repository,
            parent: None,
            history: History::default(),
            status: WorkDirStatus::default(),
            layout: graph::Layout::default(),
            child_ref_rows: Vec::new(),
            ghost_ref_rows: Vec::new(),
            operation_state: OperationState::None,
            load_error: None,
            pinned_branch: None,
            commit_limit: 2000,
            submodules_head: None,
            hidden_refs: HashSet::new(),
            solo_ref: None,
            smart_branch_visibility: false,
            selected: Vec::new(),
            focused: None,
            range_cursor: None,
            scroll_to_row: None,
            hovered_sha: None,
            search_open: false,
            search_text: String::new(),
            search_matches: Vec::new(),
            search_index: None,
            search_match_sha: None,
            search_focus_requested: false,
            files_for_sha: None,
            commit_files: Vec::new(),
            commit_files_error: None,
            compare_sha: None,
            compare_files: Vec::new(),
            compare_error: None,
            diff_target: None,
            merge_tool: None,
            file_history: None,
            diff: None,
            file_edit: None,
            diff_mode: DiffMode::Inline,
            file_view: false,
            diff_whitespace: true,
            diff_selected_lines: Vec::new(),
            diff_selection_revision: None,
            left_open_in_diff: false,
            diff_error: None,
            tree_view: true,
            expanded_folders: HashSet::new(),
            collapsed_sections: HashSet::new(),
            commit_summary: String::new(),
            commit_description: String::new(),
            amend: false,
            push_after_commit: false,
            commit_options_open: false,
            commit_form_ready: false,
            commit_sign: false,
            commit_skip_hooks: false,
            commit_template: None,
            prepared_message_for: OperationState::None,
            prepared_summary: String::new(),
            prepared_description: String::new(),
            composer_stash: false,
            stash_summary: String::new(),
            stash_description: String::new(),
            amending_message: false,
            amend_sha: None,
            amend_focus_requested: false,
            amend_summary: String::new(),
            amend_description: String::new(),
            left_filter: String::new(),
            collapsed_nodes: HashSet::new(),
            expanded_left_sections: HashSet::new(),
            left_section_ys: HashMap::new(),
            left_selected: None,
            inline_ref: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            rebase: RebasePlan::default(),
            async_op: None,
            last_status_poll: 0.0,
            last_auto_fetch: 0.0,
            gitflow_config_cache: std::cell::RefCell::new(None),
            selected_set: HashSet::new(),
            status_job: Job::idle(),
            status_generation: 0,
            status_job_generation: 0,
            refs_signature: None,
        };
        state.refresh();
        state
    }

    // Loading

    /// Reloads the history, the status and the layout.
    pub fn refresh(&mut self) {
        // The status (with the refs signature) is read before the history: a ref that moves in between then
        // shows as a change on the next poll instead of hiding behind a signature newer than the rows.
        self.status_generation += 1;
        let snapshot = read_status(&self.repo, false);
        self.rebuild(snapshot);
        self.last_status_poll = 0.0;
    }

    /// Cheap: working directory only; rebuilds the rows when the WIP row appears / disappears or a ref changed.
    pub fn refresh_status(&mut self) {
        self.status_generation += 1;
        let snapshot = read_status(&self.repo, false);
        self.apply_status(snapshot);
    }

    /// The periodic status poll: re-reads the working directory and the refs on a worker thread once `interval`
    /// seconds have passed since the last read finished, and applies the result on a later call. A commit,
    /// checkout, fetch or stash made outside ImGit thus shows up within one interval.
    ///
    /// `now`: the UI's clock, in seconds. Call it every frame.
    pub fn poll_status(&mut self, now: f64, interval: f64) {
        if self.status_job.is_running() {
            if let Some(snapshot) = self.status_job.poll() {
                // The interval counts from the end of a read, so slow reads never queue up.
                self.last_status_poll = now;
                // A refresh on the UI thread since the job started already holds newer state.
                if self.status_job_generation == self.status_generation {
                    self.apply_status(snapshot);
                }
                return;
            }
            if self.status_job.is_running() {
                return;
            }
            // The worker died without an answer: read again once the interval allows.
        }
        if now - self.last_status_poll <= interval {
            return;
        }
        self.last_status_poll = now;
        self.status_job_generation = self.status_generation;
        // A libgit2 repository must not be shared across threads: the worker opens another.
        let workdir = self.workdir();
        self.status_job = Job::spawn(move || read_status_at(&workdir));
    }

    fn apply_status(&mut self, snapshot: StatusSnapshot) {
        let Ok(status) = &snapshot.status else {
            return;
        };
        let had_wip = self.history.row_by_sha.contains_key(WIP_SHA);
        let has_wip = !status.is_empty();
        // A ref moved, appeared or disappeared outside ImGit (a commit, checkout, fetch...): the rows are
        // stale.
        let refs_changed =
            snapshot.refs_signature.is_some() && snapshot.refs_signature != self.refs_signature;
        if had_wip != has_wip || refs_changed {
            // No second status read: this one already predates the history load.
            self.rebuild(snapshot);
            return;
        }
        let StatusSnapshot {
            status: Ok(status),
            operation_state,
            submodules,
            ..
        } = snapshot
        else {
            return;
        };
        // A submodule moved (a checkout or commit inside it), was added or removed: only the list is stale,
        // not the rows.
        if let Some(submodules) = submodules
            && submodules != self.history.submodules
        {
            self.history.submodules = submodules;
        }
        // The same files in the same state, line counts included.
        let changed = status != self.status;
        self.status = status;
        self.operation_state = operation_state;
        if changed {
            if self
                .diff_target
                .as_ref()
                .is_some_and(|target| target.source.reads_workdir())
            {
                self.reload_diff();
            }
            // The comparison against the working directory is read again.
            self.compare_sha = None;
        }
    }

    /// [`refresh`](Self::refresh) from a status already read (one that predates the history load, as
    /// `refresh`'s own does).
    fn rebuild(&mut self, snapshot: StatusSnapshot) {
        self.load_error = None;
        // The comparison against the working directory belongs to the rows as they were: the WIP row going
        // away and coming back must not bring the old file list back for the same commit.
        self.compare_sha = None;
        self.compare_files.clear();
        self.compare_error = None;
        let had_wip = self.history.row_by_sha.contains_key(WIP_SHA);
        self.refs_signature = snapshot.refs_signature;
        let options = HistoryOptions {
            max_commits: self.commit_limit,
            hidden_refs: self.hidden_refs.clone(),
            solo_ref: self.solo_ref.clone(),
            smart_visibility: self.smart_branch_visibility,
        };
        match self.repo.load_history(&options) {
            Ok(history) => self.history = history,
            Err(error) => {
                self.load_error = Some(error.to_string());
                self.history = History::default();
                self.layout = graph::Layout::default();
                self.child_ref_rows.clear();
                self.ghost_ref_rows.clear();
                self.refs_signature = None; // the next poll tries again
                return;
            }
        }
        match snapshot.status {
            Ok(status) => self.status = status,
            Err(error) => {
                self.status = WorkDirStatus::default();
                self.load_error = Some(error.to_string());
            }
        }
        self.operation_state = snapshot.operation_state;
        self.insert_wip_row();
        // A pending scroll request still points at the old rows: the WIP row in front moved them by one.
        let has_wip = self.history.row_by_sha.contains_key(WIP_SHA);
        self.scroll_to_row = self.scroll_to_row.and_then(|row| match (had_wip, has_wip) {
            (false, true) => Some(row + 1),
            (true, false) => row.checked_sub(1),
            _ => Some(row),
        });
        self.rebuild_layout();

        // Drop stale selection, keep the WIP row selected when it still exists.
        let rows = &self.history.row_by_sha;
        self.selected.retain(|sha| rows.contains_key(sha));
        self.selected_set = self.selected.iter().cloned().collect();
        if self
            .focused
            .as_ref()
            .is_some_and(|sha| !rows.contains_key(sha))
        {
            self.focused = None;
        }
        if let Some(reload) = self.files_for_sha.take() {
            self.load_commit_files(&reload);
        }
        self.reload_diff();
        self.run_search();
    }

    /// Puts the uncommitted-changes row in front of the commits when there are changes.
    fn insert_wip_row(&mut self) {
        if self.status.is_empty() {
            return;
        }
        let history = &mut self.history;
        let wip = Commit {
            sha: WIP_SHA.to_owned(),
            // Always the first row: its lane reservation also keeps the checked-out branch leftmost.
            parents: history.head_sha.iter().cloned().collect(),
            author: self.repo.default_author().unwrap_or_default(),
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
                .unwrap_or_default(),
            summary: "// WIP".to_owned(),
            node_type: NodeType::WorkDir,
            ..Commit::default()
        };
        history.commits.insert(0, wip);
        history.row_by_sha = history
            .commits
            .iter()
            .enumerate()
            .map(|(row, commit)| (commit.sha.clone(), row))
            .collect();
    }

    fn rebuild_layout(&mut self) {
        let pinned = self
            .pinned_branch
            .as_deref()
            .and_then(|name| self.find_ref(name))
            .map(|pinned| graph::first_parent_chain(&self.history.commits, &pinned.target))
            .unwrap_or_default();
        self.layout = graph::compute(&self.history.commits, &pinned);

        // Inherited refs: walking the rows top-down, a commit inherits the refs of the child it is the
        // lowest-index parent of (the earliest such child on ties); a child with branches passes its own on,
        // one without (tags only, or no refs) what it inherited.
        struct Pending {
            source: usize,
            parent_index: usize,
            row: usize,
        }
        let commits = &self.history.commits;
        let mut pending: HashMap<&str, Vec<Pending>> = HashMap::new();
        let mut child_ref_rows = vec![None; commits.len()];
        let mut ghost_ref_rows = vec![None; commits.len()];
        for (row, commit) in commits.iter().enumerate() {
            let inherited = pending.remove(commit.sha.as_str()).and_then(|candidates| {
                candidates
                    .iter()
                    .min_by_key(|candidate| (candidate.parent_index, candidate.row))
                    .map(|best| best.source)
            });
            let mut has_refs = false;
            let mut has_branches = false;
            for shown in self.history.refs_at(&commit.sha).filter(|r| !r.hidden) {
                has_refs |= shown.ref_type != RefType::Stash;
                has_branches |=
                    matches!(shown.ref_type, RefType::LocalBranch | RefType::RemoteBranch);
            }
            child_ref_rows[row] = inherited;
            if !has_refs {
                ghost_ref_rows[row] = inherited;
            }
            let Some(source) = (if has_branches { Some(row) } else { inherited }) else {
                continue;
            };
            for (parent_index, parent) in commit.parents.iter().enumerate() {
                pending.entry(parent.as_str()).or_default().push(Pending {
                    source,
                    parent_index,
                    row,
                });
            }
        }
        self.child_ref_rows = child_ref_rows;
        self.ghost_ref_rows = ghost_ref_rows;
    }

    // Detail panel

    /// Loads the files commit `sha` changed (with their line counts, unless there are very many) into
    /// `commit_files`; nothing to do when they are the ones loaded. The WIP row has none: its files are the
    /// status.
    pub fn load_commit_files(&mut self, sha: &str) {
        if self.files_for_sha.as_deref() == Some(sha) {
            return;
        }
        self.files_for_sha = Some(sha.to_owned());
        self.commit_files.clear();
        self.commit_files_error = None;
        if sha == WIP_SHA {
            return;
        }
        match self.repo.commit_changes(sha) {
            Ok(files) => {
                self.commit_files = files;
                if self.commit_files.len() <= MAX_FILES_WITH_STATS {
                    let _ = self.repo.fill_stats(
                        &mut self.commit_files,
                        StatsSource::Commit,
                        Some(sha),
                    );
                }
            }
            Err(error) => self.commit_files_error = Some(error.to_string()),
        }
    }

    /// Loads the files that differ between commit `sha` and the working directory into `compare_files`, sorted
    /// by path.
    pub fn load_compare_files(&mut self, sha: &str) {
        self.compare_sha = Some(sha.to_owned());
        self.compare_files.clear();
        self.compare_error = None;
        match self.repo.compare_changes(sha) {
            Ok(mut files) => {
                files.sort_by(|a, b| a.path.cmp(&b.path));
                self.compare_files = files;
            }
            Err(error) => self.compare_error = Some(error.to_string()),
        }
    }

    /// Opens the merge tool on a conflicted file (closing the diff). `Ok(false)`: unsaved text in the file
    /// editor blocked it and nothing changed (see [`Self::has_unsaved_edit`]).
    ///
    /// # Errors
    ///
    /// When the file's sides cannot be read; nothing changes then.
    pub fn open_merge_tool(&mut self, path: &str) -> imgit_git::Result<bool> {
        if self.has_unsaved_edit() {
            return Ok(false);
        }
        let sides = self.repo.conflict_sides(path)?;
        let mut tool = MergeTool {
            path: path.to_owned(),
            ..MergeTool::default()
        };
        tool.load(
            sides.base.as_deref().unwrap_or_default(),
            sides.ours.as_deref().unwrap_or_default(),
            sides.theirs.as_deref().unwrap_or_default(),
        );
        // A: HEAD on its branch; B: the commit coming in, on the local (else remote) branch pointing at it.
        tool.ours_sha = self.history.head_sha.clone().unwrap_or_default();
        tool.ours_label = self.history.head_branch.clone().unwrap_or_default();
        tool.theirs_sha = self.repo.incoming_head().unwrap_or_default();
        let incoming = self.refs_at(&tool.theirs_sha);
        tool.theirs_label = [RefType::LocalBranch, RefType::RemoteBranch]
            .iter()
            .find_map(|kind| incoming.iter().find(|r| r.ref_type == *kind))
            .map(|branch| branch.name.clone())
            .unwrap_or_default();
        let closed = self.close_diff();
        debug_assert!(closed, "an unsaved edit was refused above");
        self.merge_tool = Some(tool);
        Ok(true)
    }

    /// Whether the file editor holds text that is not on the disk yet: whatever closes the editor would lose
    /// it, so [`Self::open_diff`], [`Self::close_diff`] and [`Self::open_merge_tool`] refuse while this is so.
    #[must_use]
    pub fn has_unsaved_edit(&self) -> bool {
        self.file_edit.as_ref().is_some_and(FileEdit::is_dirty)
    }

    /// Drops the file editor and its unsaved text: only after the user confirmed the loss.
    pub fn discard_file_edit(&mut self) {
        self.file_edit = None;
    }

    /// Opens the diff of `target` (closing the file editor). Returns `false`, changing nothing, while the
    /// editor holds unsaved text: the caller asks the user, calls [`Self::discard_file_edit`] and repeats.
    #[must_use = "false: unsaved text in the file editor blocked the navigation"]
    pub fn open_diff(&mut self, target: DiffTarget) -> bool {
        if self.has_unsaved_edit() {
            return false;
        }
        self.file_edit = None;
        self.diff_target = Some(target);
        self.reload_diff();
        true
    }

    /// Closes the diff (and the file editor). Returns `false`, changing nothing, while the editor holds
    /// unsaved text: the caller asks the user, calls [`Self::discard_file_edit`] and repeats.
    #[must_use = "false: unsaved text in the file editor blocked the navigation"]
    pub fn close_diff(&mut self) -> bool {
        if self.has_unsaved_edit() {
            return false;
        }
        self.left_open_in_diff = false;
        self.file_edit = None;
        self.diff_target = None;
        self.diff = None;
        self.diff_error = None;
        true
    }

    /// Loads the diff target's diff again (after a mode or whitespace change, or a change of the files).
    pub fn reload_diff(&mut self) {
        let Some(target) = &self.diff_target else {
            return;
        };
        self.diff_error = None;
        let options = DiffOptions {
            context_lines: self.diff_mode.context_lines(self.file_view),
            ignore_whitespace: !self.diff_whitespace,
        };
        let repo = &self.repo;
        let path = &target.path;
        let loaded = match &target.source {
            DiffSource::Commit(sha) => repo.commit_file_diff(sha, path, options),
            DiffSource::Staged => repo.workdir_file_diff(path, true, options),
            DiffSource::Unstaged => repo.workdir_file_diff(path, false, options),
            DiffSource::Compare(sha) => repo.compare_file_diff(sha, path, options),
        };
        match loaded {
            Ok(diff) => self.diff = Some(diff),
            Err(error) => {
                self.diff = None;
                self.diff_error = Some(error.to_string());
            }
        }
    }

    // Search

    /// Finds the rows matching `search_text` (in the summary, body, author or sha, case ignored). The match
    /// the user is on survives a refresh (or a narrowing of the text): only a new match set moves the
    /// selection, to its first match.
    pub fn run_search(&mut self) {
        self.search_matches.clear();
        if !self.search_open || self.search_text.is_empty() {
            self.search_index = None;
            self.search_match_sha = None;
            return;
        }
        let text = &self.search_text;
        let commits = &self.history.commits;
        self.search_matches
            .extend(commits.iter().enumerate().filter_map(|(row, commit)| {
                [
                    &commit.summary,
                    &commit.body,
                    &commit.author.name,
                    &commit.author.email,
                    &commit.sha,
                ]
                .iter()
                .any(|field| contains_ci(field, text))
                .then_some(row)
            }));
        self.search_index = self.search_match_sha.as_ref().and_then(|current| {
            self.search_matches
                .iter()
                .position(|&row| commits[row].sha == *current)
        });
        if self.search_index.is_none() {
            self.search_match_sha = None;
            if let Some(&row) = self.search_matches.first() {
                self.go_to_search_match(0, row);
            }
        }
    }

    /// Moves to the next (`direction` 1) or previous (-1) match, around the ends.
    pub fn select_search_match(&mut self, direction: isize) {
        let count = self.search_matches.len().cast_signed();
        if count == 0 {
            return;
        }
        let current = self.search_index.map_or(-1, usize::cast_signed);
        let index = (current + direction).rem_euclid(count).cast_unsigned();
        self.go_to_search_match(index, self.search_matches[index]);
    }

    fn go_to_search_match(&mut self, index: usize, row: usize) {
        let sha = self.history.commits[row].sha.clone();
        self.search_index = Some(index);
        self.select_only(&sha);
        self.search_match_sha = Some(sha);
        self.scroll_to_row = Some(row);
    }

    // Undo / redo

    /// Records an action that can be undone; whatever could be redone is forgotten.
    pub fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_stack.push(entry);
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    // Selection

    /// A click on row `sha`: `range` (Shift) selects the rows from the focused one to it, `toggle` (Ctrl) adds
    /// to or takes from the selection (with `range`: adds the rows), neither selects it alone.
    pub fn select(&mut self, sha: &str, toggle: bool, range: bool) {
        if range
            && let Some(focused) = &self.focused
            && let Some(&a) = self.history.row_by_sha.get(focused)
            && let Some(&b) = self.history.row_by_sha.get(sha)
        {
            if !toggle {
                self.selected.clear();
                self.selected_set.clear();
            }
            for commit in &self.history.commits[a.min(b)..=a.max(b)] {
                if self.selected_set.insert(commit.sha.clone()) {
                    self.selected.push(commit.sha.clone());
                }
            }
            return;
        }
        if toggle {
            if self.selected_set.remove(sha) {
                self.selected.retain(|selected| selected != sha);
            } else {
                self.selected_set.insert(sha.to_owned());
                self.selected.push(sha.to_owned());
            }
            self.focused = Some(sha.to_owned());
            self.range_cursor = None;
            return;
        }
        self.select_only(sha);
    }

    /// Selects row `sha` alone and focuses it.
    pub fn select_only(&mut self, sha: &str) {
        self.selected.clear();
        self.selected.push(sha.to_owned());
        self.selected_set.clear();
        self.selected_set.insert(sha.to_owned());
        self.focused = Some(sha.to_owned());
        self.range_cursor = None;
    }

    #[must_use]
    pub fn is_selected(&self, sha: &str) -> bool {
        self.selected_set.contains(sha)
    }

    /// The loaded row of `sha`.
    #[must_use]
    pub fn commit_for(&self, sha: &str) -> Option<&Commit> {
        self.history.commit(sha)
    }

    /// The one selected row; `None` when none or several are.
    #[must_use]
    pub fn single_selected(&self) -> Option<&Commit> {
        match self.selected.as_slice() {
            [sha] => self.commit_for(sha),
            _ => None,
        }
    }

    /// Whether a git command is in flight.
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.async_op.is_some()
    }

    // Refs

    /// The checked-out branch.
    #[must_use]
    pub fn head_ref(&self) -> Option<&Ref> {
        self.history.refs.iter().find(|r| r.is_head)
    }

    /// The repository the tab was opened on: this one, or the top of the submodules opened in it.
    #[must_use]
    pub fn root(&self) -> &Self {
        let mut state = self;
        while let Some(parent) = &state.parent {
            state = parent;
        }
        state
    }

    /// The refs shown at `sha` (hidden ones left out): the checked-out branch first, then local branches,
    /// remote branches, tags, stashes.
    #[must_use]
    pub fn refs_at(&self, sha: &str) -> Vec<&Ref> {
        let mut refs: Vec<&Ref> = self.history.refs_at(sha).filter(|r| !r.hidden).collect();
        refs.sort_by_key(|r| (!r.is_head, r.ref_type as u8));
        refs
    }

    #[must_use]
    pub fn find_ref(&self, full_name: &str) -> Option<&Ref> {
        self.history.refs.iter().find(|r| r.full_name == full_name)
    }

    /// The first-parent commits from `tip` (HEAD when `None`) down to `base_sha` (the root when `None`),
    /// `base_sha` itself left out, oldest first. Empty when `base_sha` is not on that chain, the chain leaves
    /// the loaded rows, or there is a merge on the way: ImGit refuses to rewrite across merge commits.
    #[must_use]
    pub fn commits_above(&self, base_sha: Option<&str>, tip: Option<&str>) -> Vec<&Commit> {
        let mut result = Vec::new();
        let mut next = tip.or(self.history.head_sha.as_deref());
        while let Some(sha) = next
            && next != base_sha
        {
            let Some(commit) = self.commit_for(sha) else {
                return Vec::new();
            };
            if commit.node_type == NodeType::Merge {
                return Vec::new();
            }
            result.push(commit);
            next = commit.parents.first().map(String::as_str);
        }
        if next != base_sha {
            return Vec::new();
        }
        result.reverse();
        result
    }

    /// Opens the interactive rebase of the commits between `fork_point` (`base_sha` when `None`) and `tip`
    /// (HEAD when `None`) onto `base_sha`. The plan stays inactive when there is nothing to rebase.
    pub fn start_rebase_plan(
        &mut self,
        base_sha: &str,
        base_label: &str,
        fork_point: Option<&str>,
        tip: Option<&str>,
    ) {
        let history = &self.history;
        // The branch being rebased: the local branch at `tip` (HEAD's when there is none).
        let head_label = tip
            .and_then(|tip| {
                self.refs_at(tip)
                    .into_iter()
                    .find(|r| r.ref_type == RefType::LocalBranch)
                    .map(|branch| branch.name.clone())
            })
            .or_else(|| history.head_branch.clone())
            .unwrap_or_else(|| {
                short_sha(history.head_sha.as_deref().unwrap_or_default()).to_owned()
            });
        // An empty base is the root's missing parent.
        let from = Some(fork_point.unwrap_or(base_sha)).filter(|sha| !sha.is_empty());
        let steps: Vec<PlanRow> = self
            .commits_above(from, tip)
            .into_iter()
            .map(|commit| PlanRow {
                step: imgit_git::RebaseStep {
                    sha: commit.sha.clone(),
                    action: imgit_git::RebaseAction::Pick,
                },
                summary: commit.summary.clone(),
                author: commit.author.clone(),
            })
            .collect();
        self.rebase = RebasePlan {
            active: !steps.is_empty(),
            base: base_sha.to_owned(),
            base_label: base_label.to_owned(),
            head_label,
            original_steps: steps.clone(),
            steps,
            selected: None,
            editing: None,
        };
    }
}
