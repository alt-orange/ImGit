//! The File History view of a repository tab and the background jobs that feed it.

use std::path::PathBuf;

use imgit_git::repository::DiffOptions;
use imgit_git::{Blame, FileDiff, FileHistory, FileStatus, Repository, Sha};

use super::RepoState;
use crate::job::Job;

/// The cap of the file history's walk.
const MAX_HISTORY_COMMITS: usize = 20000;

/// What a view shows when its worker died without an answer.
const JOB_FAILED: &str = "The background job stopped unexpectedly";

/// The File History view (over the whole tab): the commits that touched one file, the file as the selected one
/// left it (with the blame of its lines) or that commit's diff of it.
#[derive(Debug)]
pub struct FileHistoryView {
    /// As asked for, at `from`.
    pub path: String,
    /// The commit the walk starts at, `None` for HEAD.
    pub from: Option<Sha>,
    pub history: FileHistory,
    pub history_error: Option<String>,
    pub history_loading: bool,
    /// Index into `history.entries`.
    pub selected: Option<usize>,
    /// The entry to select once the history is loaded (`None` for the newest); then the selected entry's.
    pub selected_sha: Option<Sha>,
    /// File View | Diff View
    pub file_view: bool,
    /// The blame columns (commit and date) next to the lines.
    pub blame: bool,
    /// The selected entry's diff of the file.
    pub diff: Option<FileDiff>,
    pub diff_error: Option<String>,
    /// Of the selected entry.
    pub blame_data: Option<Blame>,
    /// The entry `blame_data` (or the blame job) is for.
    pub blame_sha: Option<Sha>,
    pub blame_error: Option<String>,
    pub blame_loading: bool,
    /// List row to bring into view.
    pub scroll_to: Option<usize>,
    history_job: Job<imgit_git::Result<FileHistory>>,
    blame_job: Job<imgit_git::Result<Blame>>,
}

impl FileHistoryView {
    fn new(path: String, from: Option<Sha>, select: Option<Sha>, blame: bool) -> Self {
        Self {
            path,
            from,
            history: FileHistory::default(),
            history_error: None,
            history_loading: true,
            selected: None,
            selected_sha: select,
            file_view: true,
            blame,
            diff: None,
            diff_error: None,
            blame_data: None,
            blame_sha: None,
            blame_error: None,
            blame_loading: false,
            scroll_to: None,
            history_job: Job::idle(),
            blame_job: Job::idle(),
        }
    }
}

impl RepoState {
    /// Opens the File History view of `path` from `from` (HEAD when `None`), with the entry of `select` (the
    /// newest when `None`) selected and the blame columns on when `blame`.
    pub fn open_file_history(
        &mut self,
        path: impl Into<String>,
        from: Option<Sha>,
        select: Option<Sha>,
        blame: bool,
    ) {
        // A view already open goes with its jobs, which end on their own.
        let mut view = FileHistoryView::new(path.into(), from, select, blame);
        let workdir = self.workdir();
        let path = view.path.clone();
        let from = view.from.clone();
        view.history_job = Job::spawn(move || {
            // A libgit2 repository must not be shared across threads: open another.
            Repository::open(workdir)?.file_history(&path, from.as_deref(), MAX_HISTORY_COMMITS)
        });
        self.file_history = Some(view);
    }

    pub fn close_file_history(&mut self) {
        self.file_history = None;
    }

    /// Selects an entry of the file history: loads its diff and starts its blame. `None`, or an index that is
    /// not an entry, selects nothing.
    pub fn select_file_history_entry(&mut self, index: Option<usize>) {
        let workdir = self.workdir();
        let Some(view) = &mut self.file_history else {
            return;
        };
        let Some((index, entry)) =
            index.and_then(|index| Some((index, view.history.entries.get(index)?)))
        else {
            view.selected = None;
            view.diff = None;
            view.diff_error = None;
            return;
        };
        let (path, sha) = (entry.path.clone(), entry.commit.sha.clone());
        // The blame of the file as this commit left it (a deleted file has none).
        let has_blame = entry.status != FileStatus::Deleted;
        view.selected = Some(index);
        view.selected_sha = Some(sha.clone());
        view.blame_job.cancel();
        view.blame_data = None;
        view.blame_error = None;
        view.blame_sha = Some(sha.clone());
        view.blame_loading = has_blame;
        if has_blame {
            view.blame_job =
                Job::spawn(move || Repository::open(workdir)?.blame_file(&path, Some(&sha)));
        }
        self.reload_file_history_diff();
    }

    /// Reloads the selected entry's diff (after a mode change).
    pub fn reload_file_history_diff(&mut self) {
        let Some(view) = &mut self.file_history else {
            return;
        };
        let Some(entry) = view
            .selected
            .and_then(|index| view.history.entries.get(index))
        else {
            return;
        };
        view.diff = None;
        view.diff_error = None;
        let options = DiffOptions {
            context_lines: self.diff_mode.context_lines(view.file_view),
            ignore_whitespace: !self.diff_whitespace,
        };
        match self
            .repo
            .commit_file_diff(&entry.commit.sha, &entry.path, options)
        {
            Ok(diff) => view.diff = Some(diff),
            Err(error) => view.diff_error = Some(error.to_string()),
        }
    }

    /// Collects the file history's finished background jobs; call every frame the view is shown.
    pub fn poll_file_history(&mut self) {
        let Some(view) = &mut self.file_history else {
            return;
        };
        let loaded = view.history_job.poll();
        if loaded.is_none() && view.history_loading && !view.history_job.is_running() {
            view.history_loading = false;
            view.history_error = Some(JOB_FAILED.to_owned());
        }
        match loaded {
            Some(Ok(history)) => {
                view.history_loading = false;
                view.history = history;
                let entries = &view.history.entries;
                let index = view
                    .selected_sha
                    .as_ref()
                    .and_then(|sha| entries.iter().position(|entry| entry.commit.sha == *sha))
                    .or(if entries.is_empty() { None } else { Some(0) });
                self.select_file_history_entry(index);
                if let (Some(index), Some(view)) = (index, &mut self.file_history) {
                    view.scroll_to = Some(index);
                }
            }
            Some(Err(error)) => {
                view.history_loading = false;
                view.history_error = Some(error.to_string());
            }
            None => {}
        }
        let Some(view) = &mut self.file_history else {
            return;
        };
        match view.blame_job.poll() {
            Some(loaded) => {
                view.blame_loading = false;
                match loaded {
                    Ok(blame) => view.blame_data = Some(blame),
                    Err(error) => view.blame_error = Some(error.to_string()),
                }
            }
            None if view.blame_loading && !view.blame_job.is_running() => {
                view.blame_loading = false;
                view.blame_error = Some(JOB_FAILED.to_owned());
            }
            None => {}
        }
    }

    /// The working directory, for a worker to open its own repository on.
    pub(super) fn workdir(&self) -> PathBuf {
        self.repo.workdir().to_path_buf()
    }
}
