//! Running repository operations and `git` commands for a tab: the result toasts, the commands in flight, the
//! force-push offer, undo / redo, auto fetch and keeping submodules up to date.

use std::path::{Path, PathBuf};

use imgit_app::RepoState;
use imgit_app::job::Job;
use imgit_app::preferences::{pref, preferences, with_prune_preference};
use imgit_app::repo_state::{AsyncOperation, with_changes_stashed};
use imgit_git::{Repository, ResetMode, cli};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;
use crate::app::toasts::ToastKind;

/// How often the active tab's working directory is read again, in seconds.
pub const STATUS_POLL_SECONDS: f64 = 2.0;

/// The success / error toast titles for the toolbar's LFS commands; `None` for other commands.
fn lfs_toast(title: &str, success: bool) -> Option<&'static str> {
    const TOASTS: [[&str; 3]; 5] = [
        [
            "LFS Fetch",
            "LFS fetched successfully",
            "Error on LFS Fetch",
        ],
        [
            "LFS Checkout",
            "LFS checkout successful",
            "Error on LFS Checkout",
        ],
        ["LFS Pull", "LFS pulled successfully", "Error on LFS Pull"],
        ["LFS Push", "LFS pushed successfully", "Error on LFS Push"],
        ["LFS Prune", "LFS pruned successfully", "Error on LFS Prune"],
    ];
    TOASTS
        .iter()
        .find(|toast| toast[0] == title)
        .map(|toast| if success { toast[1] } else { toast[2] })
}

/// The credentials a git command may be given: the session's credentials for the hosts of `urls` (the urls the
/// command can connect to) and `attempt` (from the credentials prompt, tried first, in place of its host's
/// entry). Every other host's credential stays out of git's environment, where hooks and helpers could read it.
pub(crate) fn credentials_for<'a>(
    credentials: &[cli::Credential],
    urls: impl IntoIterator<Item = &'a str>,
    attempt: Option<&cli::Credential>,
) -> Vec<cli::Credential> {
    let hosts: Vec<String> = urls
        .into_iter()
        .filter_map(cli::credential_host)
        .map(|host| host.url)
        .collect();
    let mut result: Vec<cli::Credential> = credentials
        .iter()
        .filter(|credential| {
            hosts
                .iter()
                .any(|host| host.eq_ignore_ascii_case(&credential.url))
        })
        .cloned()
        .collect();
    if let Some(attempt) = attempt {
        result.retain(|credential| credential.url != attempt.url);
        result.insert(0, attempt.clone());
    }
    result
}

/// git's progress lines are separated by '\r'; keeps the last meaningful line of each, one per line.
fn clean_output(text: &str) -> String {
    text.replace('\r', "\n")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `text` cut to `max` characters with "..." in place of the rest.
fn shortened(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        let head: String = text.chars().take(max - 3).collect();
        format!("{head}...")
    } else {
        text.to_owned()
    }
}

impl Application {
    /// Reports the outcome of a repository operation: a failure as a toast "`what` failed" with the error,
    /// a success as `success_message` when given; the tab is reloaded either way. Returns the value on success.
    ///
    /// The C++ took the operation as a closure; here it has run already, so evaluate it first (borrowing
    /// `state.repo` only for that statement):
    ///
    /// ```ignore
    /// let result = state.repo.stage_all();
    /// app.run(state, "Stage all", result, None);
    /// ```
    pub fn run<T>(
        &mut self,
        state: &mut RepoState,
        what: &str,
        result: imgit_git::Result<T>,
        success_message: Option<&str>,
    ) -> Option<T> {
        match result {
            Ok(value) => {
                if let Some(message) = success_message.filter(|message| !message.is_empty()) {
                    self.toasts.push(ToastKind::Success, message);
                }
                state.refresh();
                Some(value)
            }
            Err(error) => {
                self.toasts.push_with_detail(
                    ToastKind::Error,
                    format!("{what} failed"),
                    error.to_string(),
                );
                state.refresh();
                None
            }
        }
    }

    /// The busy guard of every action that changes the repository through libgit2 (checkout, reset, merge,
    /// stash, history rewrites, commits, staging...): while a background git command (fetch, pull, push, rebase)
    /// runs on the same repository it is refused with a toast "Wait for <title> to finish". Returns `true` when
    /// the action must not run. Read-only actions never ask; the git commands started through
    /// [`Self::run_git_cli`] have their own check.
    ///
    /// Menu clicks and confirmed dialogs get it from [`Self::with_idle_repo`]; the panels' own buttons call
    /// this at the start of their action.
    pub fn refuse_while_busy(&mut self, state: &RepoState) -> bool {
        let Some(running) = &state.async_op else {
            return false;
        };
        self.toasts.push(
            ToastKind::Info,
            format!("Wait for {} to finish", running.title),
        );
        true
    }

    /// [`Self::with_repo`] for an action that changes the repository: `f` does not run (and a toast says why)
    /// while a git command is in flight on it, see [`Self::refuse_while_busy`].
    pub fn with_idle_repo<R>(
        &mut self,
        workdir: &Path,
        f: impl FnOnce(&mut Self, &mut RepoState) -> R,
    ) -> Option<R> {
        self.with_repo(workdir, |app, state| {
            (!app.refuse_while_busy(state)).then(|| f(app, state))
        })
        .flatten()
    }

    /// Starts `git <args>` for the tab in the background; the result is reported when it finishes.
    pub fn run_git_cli<A: Into<String>>(
        &mut self,
        state: &mut RepoState,
        title: &str,
        args: impl IntoIterator<Item = A>,
    ) {
        self.run_git_cli_with(
            state,
            title,
            args.into_iter().map(Into::into).collect(),
            false,
            None,
        );
    }

    /// [`Self::run_git_cli`] with no toast when it works (auto fetch, keeping submodules up to date).
    pub fn run_git_cli_silent<A: Into<String>>(
        &mut self,
        state: &mut RepoState,
        title: &str,
        args: impl IntoIterator<Item = A>,
    ) {
        self.run_git_cli_with(
            state,
            title,
            args.into_iter().map(Into::into).collect(),
            true,
            None,
        );
    }

    /// Starts `git <args>` for the tab in the background. `silent`: no "Please wait" toast when one is running
    /// and none on success; `attempt`: a credential from the credentials prompt, tried before the session's own.
    pub fn run_git_cli_with(
        &mut self,
        state: &mut RepoState,
        title: &str,
        args: Vec<String>,
        silent: bool,
        attempt: Option<cli::Credential>,
    ) {
        if let Some(running) = &state.async_op {
            if !silent {
                self.toasts.push_with_detail(
                    ToastKind::Info,
                    "Please wait",
                    format!("{} is still running", running.title),
                );
            }
            return;
        }
        let workdir = state.repo.workdir().to_path_buf();
        let args = with_prune_preference(args, preferences().flag(pref::AUTO_PRUNE));
        let urls = state
            .history
            .remotes
            .iter()
            .map(|remote| remote.url.as_str())
            .chain(
                state
                    .history
                    .submodules
                    .iter()
                    .map(|submodule| submodule.url.as_str()),
            );
        let credentials = credentials_for(&self.credentials, urls, attempt.as_ref());
        let command = args.clone();
        state.async_op = Some(AsyncOperation {
            title: title.to_owned(),
            job: Job::spawn(move || {
                cli::run(
                    &workdir,
                    &command,
                    cli::RunOptions {
                        credentials: &credentials,
                        ..cli::RunOptions::default()
                    },
                )
            }),
            silent,
            args,
            attempt,
            queued: Vec::new(),
        });
    }

    /// Runs several `git` commands one after another in the background (stopping at the first failure); the
    /// result toast reports the last.
    pub fn run_git_cli_sequence(
        &mut self,
        state: &mut RepoState,
        title: &str,
        mut runs: Vec<Vec<String>>,
    ) {
        if runs.is_empty() {
            return;
        }
        if let Some(running) = &state.async_op {
            self.toasts.push_with_detail(
                ToastKind::Info,
                "Please wait",
                format!("{} is still running", running.title),
            );
            return;
        }
        let first = runs.remove(0);
        self.run_git_cli_with(state, title, first, false, None);
        if let Some(operation) = &mut state.async_op {
            operation.queued = runs;
        }
    }

    /// A failed `git push <...>` whose `output` says the remote has commits the branch lacks (non-fast-forward /
    /// fetch first): asks in the prompt bar whether to push again with `--force-with-lease`. `false` when it is
    /// not that refusal, or another prompt is already open.
    pub fn offer_force_push(
        &mut self,
        state: &RepoState,
        output: &str,
        mut args: Vec<String>,
    ) -> bool {
        let rejected = output.contains("! [rejected]")
            && (output.contains("non-fast-forward") || output.contains("fetch first"));
        if !rejected || self.dialog.is_open() || args.first().map(String::as_str) != Some("push") {
            return false;
        }
        let branch = state.head_ref().map_or_else(
            || "this branch".to_owned(),
            |head| format!("'{}'", head.name),
        );
        let workdir = state.repo.workdir().to_path_buf();
        self.dialog.open(DialogKind::Confirm);
        self.dialog.workdir = Some(workdir.clone());
        self.dialog.message = format!(
            "The push was rejected: the remote has commits that {branch} does not. A force push overwrites them \
             with your local history. Force push anyway?"
        );
        self.dialog.confirm_label = "Force Push".to_owned();
        self.dialog.danger = true;
        args.insert(1, "--force-with-lease".to_owned());
        self.dialog.on_confirm = Some(Box::new(move |_, app| {
            app.with_repo(&workdir, |app, state| app.run_git_cli(state, "Push", args));
        }));
        true
    }

    // Undo / redo

    /// Reverses the repository's last undoable action; a redo is kept for it.
    pub fn undo(&mut self, state: &mut RepoState) {
        if state.undo_stack.is_empty() || self.refuse_while_busy(state) {
            return;
        }
        let Some(mut entry) = state.undo_stack.pop() else {
            return;
        };
        let before_sha = state.history.head_sha.clone();
        let before_branch = state.history.head_branch.clone();
        if let Err(error) = (entry.undo)(&state.repo) {
            self.toasts
                .push_with_detail(ToastKind::Error, "Undo Failed", error.to_string());
            state.refresh();
            return;
        }
        self.toasts
            .push_with_detail(ToastKind::Success, "Undo Successful", entry.label.clone());
        state.refresh();
        if entry.redo.is_none()
            && let Some(branch) = before_branch
                .clone()
                .filter(|branch| state.history.head_branch.as_ref() != Some(branch))
        {
            // Undoing a checkout: redo checks the branch out again.
            entry.redo = Some(Box::new(move |repo: &Repository| {
                repo.checkout_branch(&branch)
            }));
        } else if entry.redo.is_none()
            && let Some(sha) = before_sha.filter(|sha| state.history.head_sha.as_ref() != Some(sha))
        {
            // Undoing a commit / merge / cherry pick...: redo moves the branch back to the commit it left (which
            // still exists), keeping uncommitted changes aside like undo does.
            let mode = entry.redo_mode;
            entry.redo = Some(Box::new(move |repo: &Repository| {
                if mode == ResetMode::Hard {
                    // Changes that cannot be set aside stop the redo (a hard reset would wipe them); a pop that
                    // fails or conflicts is reported, naming the stash that holds the changes.
                    with_changes_stashed(repo, "ImGit: auto stash before redo", true, |repo| {
                        repo.reset(&sha, mode)
                    })
                } else {
                    repo.reset(&sha, mode)
                }
            }));
        }
        if entry.redo.is_some() {
            state.redo_stack.push(entry);
        }
    }

    /// Repeats the last undone action.
    pub fn redo(&mut self, state: &mut RepoState) {
        if state.redo_stack.is_empty() || self.refuse_while_busy(state) {
            return;
        }
        let Some(mut entry) = state.redo_stack.pop() else {
            return;
        };
        let Some(redo) = entry.redo.as_mut() else {
            return;
        };
        if let Err(error) = redo(&state.repo) {
            self.toasts
                .push_with_detail(ToastKind::Error, "Redo Failed", error.to_string());
            state.refresh();
            return;
        }
        self.toasts
            .push_with_detail(ToastKind::Success, "Redo Successful", entry.label.clone());
        state.undo_stack.push(entry);
        state.refresh();
    }

    // Background work of every frame

    /// Collects the git commands that finished (also of a repository behind an open submodule), the clone, and
    /// the terminals' output.
    pub(crate) fn poll_async(&mut self) {
        let running: Vec<PathBuf> = self
            .repo_states()
            .filter(|state| state.async_op.is_some())
            .map(|state| state.repo.workdir().to_path_buf())
            .collect();
        for workdir in running {
            self.with_repo(&workdir, |app, state| app.poll_operation(state));
        }
        self.poll_clone();
        self.poll_terminals();
    }

    /// The tab's git command, when it has finished.
    fn poll_operation(&mut self, state: &mut RepoState) {
        let Some(operation) = state.async_op.as_mut() else {
            return;
        };
        let output = match operation.job.poll() {
            Some(output) => output,
            None if operation.job.is_running() => return,
            // The worker died without an answer.
            None => cli::Output {
                exit_code: -1,
                text: "git did not finish".to_owned(),
            },
        };
        let Some(AsyncOperation {
            title,
            silent,
            args,
            attempt,
            mut queued,
            ..
        }) = state.async_op.take()
        else {
            return;
        };
        if output.exit_code == 0
            && let Some(attempt) = &attempt
        {
            self.keep_credential(attempt);
        }
        // The next command of a sequence; the toast waits for the last one.
        if output.exit_code == 0 && !queued.is_empty() {
            let next = queued.remove(0);
            self.run_git_cli_with(state, &title, next, silent, attempt);
            if let Some(operation) = &mut state.async_op {
                operation.queued = queued;
            }
            state.refresh();
            return;
        }
        // Refused for want of credentials: ImGit asks for them and runs the command again.
        if output.exit_code != 0 && !silent {
            let workdir = state.repo.workdir().to_path_buf();
            let retry_title = title.clone();
            let retry_args = args.clone();
            let asked = self.prompt_for_credentials(
                &output.text,
                attempt.as_ref(),
                Box::new(move |app, credential| {
                    app.with_repo(&workdir, |app, state| {
                        app.run_git_cli_with(
                            state,
                            &retry_title,
                            retry_args,
                            false,
                            Some(credential),
                        );
                    });
                }),
            );
            if asked {
                return;
            }
        }
        let text = clean_output(&output.text);
        if output.exit_code == 0 {
            if silent {
            } else if title == "Pull" {
                // "Already Up-to-Date" / "Pulled Successfully".
                if text.contains("Already up to date") {
                    let upstream = state
                        .head_ref()
                        .and_then(|head| head.upstream.clone())
                        .unwrap_or_default();
                    self.toasts.push_with_detail(
                        ToastKind::Info,
                        "Already Up-to-Date",
                        if upstream.is_empty() {
                            "No merge necessary.".to_owned()
                        } else {
                            format!("No merge necessary. Branch {upstream} was not moved.")
                        },
                    );
                } else {
                    self.toasts.push(ToastKind::Success, "Pulled Successfully");
                }
            } else if title == "Push" {
                self.toasts.push(ToastKind::Success, "Pushed Successfully");
            } else if let Some(lfs) = lfs_toast(&title, true) {
                self.toasts.push(ToastKind::Success, lfs);
            } else if title != "Fetch" {
                self.toasts.push_with_detail(
                    ToastKind::Success,
                    format!("{title} complete"),
                    shortened(&text, 200),
                );
            }
        } else if !silent {
            let heading =
                lfs_toast(&title, false).map_or_else(|| format!("{title} failed"), str::to_owned);
            self.toasts
                .push_with_detail(ToastKind::Error, heading, text.clone());
            if title == "Push" {
                self.offer_force_push(state, &text, args);
            }
        }
        state.refresh();
    }

    /// Auto fetch: every Auto-fetch interval, `git fetch --all --prune` for a tab that has remotes.
    pub(crate) fn auto_fetch(&mut self, now: f64, state: &mut RepoState) {
        if self.settings.auto_fetch_minutes <= 0 || state.history.remotes.is_empty() || state.busy()
        {
            return;
        }
        if state.last_auto_fetch == 0.0 {
            state.last_auto_fetch = now; // first fetch one interval after opening
            return;
        }
        if now - state.last_auto_fetch >= f64::from(self.settings.auto_fetch_minutes) * 60.0 {
            state.last_auto_fetch = now;
            self.run_git_cli_silent(state, "Auto-fetch", ["fetch", "--all", "--prune"]);
        }
    }

    /// Keep submodules up to date: once HEAD has moved (a checkout, pull, merge, reset... from ImGit or
    /// elsewhere), the initialized submodules are checked out at the commits it records.
    pub(crate) fn keep_submodules_updated(&mut self) {
        let moved: Vec<PathBuf> = self
            .repo_states()
            .filter(|state| {
                state.async_op.is_none()
                    && state.history.head_sha.is_some()
                    && state.history.head_sha != state.submodules_head
            })
            .map(|state| state.repo.workdir().to_path_buf())
            .collect();
        for workdir in moved {
            self.with_repo(&workdir, |app, state| {
                let Some(head) = state.history.head_sha.clone() else {
                    return;
                };
                let first = state.submodules_head.is_none();
                state.submodules_head = Some(head);
                if !first
                    && preferences().flag(pref::UPDATE_SUBMODULES)
                    && !state.repo.submodules().is_empty()
                {
                    app.run_git_cli_silent(
                        state,
                        "Update submodules",
                        ["submodule", "update", "--recursive"],
                    );
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfs_commands_have_their_own_toast_titles() {
        assert_eq!(
            lfs_toast("LFS Fetch", true),
            Some("LFS fetched successfully")
        );
        assert_eq!(lfs_toast("LFS Prune", false), Some("Error on LFS Prune"));
        assert_eq!(lfs_toast("Fetch", true), None);
    }

    #[test]
    fn git_output_keeps_the_last_state_of_each_progress_line() {
        let text = "Receiving objects:  10%\rReceiving objects: 100%\r\n\n  done  \n";
        assert_eq!(
            clean_output(text),
            "Receiving objects:  10%\nReceiving objects: 100%\ndone"
        );
        assert_eq!(clean_output("\r\n"), "");
        assert_eq!(shortened("abcdef", 6), "abcdef");
        assert_eq!(shortened("abcdefg", 6), "abc...");
    }

    #[test]
    fn credentials_go_only_to_the_hosts_a_command_talks_to() {
        let credential = |url: &str, user: &str| cli::Credential {
            url: url.to_owned(),
            username: user.to_owned(),
            password: String::new(),
            remember: false,
        };
        let session = [
            credential("https://a", "old"),
            credential("https://b", "b"),
            credential("https://c", "c"),
        ];
        let urls = [
            "https://a/repo.git",
            "https://b/x",
            "git@scp:style",
            "/local",
        ];
        // Only the hosts of the urls; the attempt replaces its host's entry and comes first.
        let merged = credentials_for(&session, urls, Some(&credential("https://a", "new")));
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].username, "new");
        assert_eq!(merged[1].url, "https://b");
        let plain = credentials_for(&session, urls, None);
        assert_eq!(plain, session[..2]);
        // No matching remote: nothing, except an attempt for another host.
        assert!(credentials_for(&session, ["https://z/x"], None).is_empty());
        assert!(credentials_for(&session, [], None).is_empty());
        let only_attempt = credentials_for(&session, [], Some(&credential("https://z", "z")));
        assert_eq!(only_attempt.len(), 1);
        assert_eq!(only_attempt[0].url, "https://z");
    }

    #[test]
    fn a_failed_operation_toasts_and_a_successful_one_returns_its_value() {
        let scratch = tempfile::tempdir().unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        let folder = scratch.path().join("repo");
        Repository::init(&folder).unwrap();
        app.open_repository(&folder.to_string_lossy());
        let ok = app.with_active(|app, state| {
            let result = state.repo.stage_all();
            app.run(state, "Stage all", result, Some("Staged"))
        });
        assert_eq!(ok, Some(Some(())));
        assert_eq!(app.toasts.history()[0].title, "Staged");
        let failed = app.with_active(|app, state| {
            let result = state.repo.checkout_branch("nope");
            app.run(state, "Checkout", result, None)
        });
        assert_eq!(failed, Some(None));
        assert_eq!(app.toasts.history()[0].title, "Checkout failed");
        assert_eq!(app.toasts.history()[0].kind, ToastKind::Error);
    }

    /// A git command "in flight" that never ends.
    fn running(title: &str) -> AsyncOperation {
        AsyncOperation {
            title: title.to_owned(),
            job: Job::idle(),
            silent: false,
            args: Vec::new(),
            attempt: None,
            queued: Vec::new(),
        }
    }

    #[test]
    fn changes_wait_for_the_git_command_running_on_the_repository() {
        let scratch = tempfile::tempdir().unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        let folder = scratch.path().join("repo");
        Repository::init(&folder).unwrap();
        app.open_repository(&folder.to_string_lossy());
        let workdir = app.active_workdir().unwrap().to_path_buf();
        app.with_active(|_, state| state.async_op = Some(running("Fetch")));
        let mut ran = false;
        let result = app.with_idle_repo(&workdir, |_, _| ran = true);
        assert!(result.is_none() && !ran);
        assert_eq!(app.toasts.history()[0].title, "Wait for Fetch to finish");
        assert_eq!(app.toasts.history()[0].kind, ToastKind::Info);
        // Looking is never refused.
        assert_eq!(app.with_repo(&workdir, |_, state| state.busy()), Some(true));
        // Once the command ended the action runs.
        app.with_active(|_, state| state.async_op = None);
        assert_eq!(app.with_idle_repo(&workdir, |_, _| 7), Some(7));
    }

    #[test]
    fn undo_and_redo_are_refused_while_a_git_command_runs_and_keep_their_entries() {
        let scratch = tempfile::tempdir().unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        let folder = scratch.path().join("repo");
        Repository::init(&folder).unwrap();
        app.open_repository(&folder.to_string_lossy());
        app.with_active(|app, state| {
            let undone = std::rc::Rc::new(std::cell::Cell::new(0));
            let counter = std::rc::Rc::clone(&undone);
            state.push_undo(imgit_app::repo_state::UndoEntry::new("test", move |_| {
                counter.set(counter.get() + 1);
                Ok(())
            }));
            state.async_op = Some(running("Pull"));
            app.undo(state);
            assert_eq!((undone.get(), state.undo_stack.len()), (0, 1));
            assert_eq!(app.toasts.history()[0].title, "Wait for Pull to finish");
            state.async_op = None;
            app.undo(state);
            assert_eq!((undone.get(), state.undo_stack.len()), (1, 0));
        });
    }

    #[test]
    fn a_rejected_push_offers_a_force_push_once() {
        let scratch = tempfile::tempdir().unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        let folder = scratch.path().join("repo");
        Repository::init(&folder).unwrap();
        app.open_repository(&folder.to_string_lossy());
        let rejected = " ! [rejected]        main -> main (non-fast-forward)";
        let push = || vec!["push".to_owned(), "origin".to_owned(), "main".to_owned()];
        let (first, other, second) = app
            .with_active(|app, state| {
                let other = app.offer_force_push(state, "everything is fine", push());
                let not_a_push = app.offer_force_push(state, rejected, vec!["fetch".to_owned()]);
                let first = app.offer_force_push(state, rejected, push());
                let second = app.offer_force_push(state, rejected, push());
                (first, other || not_a_push, second)
            })
            .unwrap();
        assert!(first && !other && !second);
        assert!(app.dialog.is(DialogKind::Confirm));
        assert!(app.dialog.danger && app.dialog.on_confirm.is_some());
        assert_eq!(app.dialog.confirm_label, "Force Push");
    }
}
