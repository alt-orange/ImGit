//! `git clone` in flight, with its progress.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use imgit_app::job::Job;
use imgit_git::cli::{self, CloneProgress, Credential};

use crate::app::application::Application;
use crate::app::git_ops::credentials_for;
use crate::app::toasts::ToastKind;

/// The arguments of `git clone`: `--` ends the options, so a url or folder that starts with `-` is not one.
fn clone_args(url: &str, destination: &str) -> Vec<String> {
    ["clone", "--progress", "--", url, destination]
        .map(str::to_owned)
        .into()
}

/// `path` made absolute against the process's folder (unchanged when that cannot be told).
fn absolute(path: &str) -> String {
    std::path::absolute(path).map_or_else(
        |_| path.to_owned(),
        |absolute| absolute.to_string_lossy().into_owned(),
    )
}

/// A running clone; the destination opens as a tab when it finishes. Its progress is written by the clone's
/// thread and read by the Clone dialog's progress bar.
pub struct PendingClone {
    pub job: Job<cli::Output>,
    pub destination: String,
    pub parent: String,
    pub url: String,
    pub attempt: Option<Credential>,
    pub progress: Arc<Mutex<CloneProgress>>,
}

impl Application {
    /// Starts `git clone <url> <destination>` in `parent`. `attempt`: a credential from the credentials prompt,
    /// tried before the session's own.
    pub fn start_clone(
        &mut self,
        parent: String,
        url: String,
        destination: String,
        attempt: Option<Credential>,
    ) {
        let url = url.trim().to_owned();
        if url.is_empty() {
            self.toasts.push_with_detail(
                ToastKind::Error,
                "Clone failed",
                "There is no repository url to clone",
            );
            return;
        }
        // git runs in `parent`: a relative destination would be resolved against it a second time, and the
        // repository opened afterwards would be looked up against the process's own folder.
        let parent = absolute(&parent);
        let destination = absolute(&destination);
        // Only the cloned host's credential reaches git.
        let credentials = credentials_for(&self.credentials, [url.as_str()], attempt.as_ref());
        let progress = Arc::new(Mutex::new(CloneProgress::default()));
        let sink = Arc::clone(&progress);
        let (work_parent, work_url, work_destination) =
            (parent.clone(), url.clone(), destination.clone());
        let job = Job::spawn(move || {
            let mut on_output = |chunk: &str| {
                sink.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .feed(chunk);
            };
            cli::run(
                Path::new(&work_parent),
                clone_args(&work_url, &work_destination),
                cli::RunOptions {
                    credentials: &credentials,
                    on_output: Some(&mut on_output),
                    ..cli::RunOptions::default()
                },
            )
        });
        self.pending_clone = Some(PendingClone {
            job,
            destination,
            parent,
            url,
            attempt,
            progress,
        });
    }

    /// A snapshot of the running clone's progress (empty when none runs).
    #[must_use]
    pub fn clone_progress(&self) -> CloneProgress {
        self.pending_clone
            .as_ref()
            .map(|clone| {
                clone
                    .progress
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
            })
            .unwrap_or_default()
    }

    /// The clone, when it has finished: opens the repository, or asks for the credentials and starts again.
    pub(crate) fn poll_clone(&mut self) {
        let Some(clone) = self.pending_clone.as_mut() else {
            return;
        };
        let output = match clone.job.poll() {
            Some(output) => output,
            None if clone.job.is_running() => return,
            None => cli::Output {
                exit_code: -1,
                text: "git did not finish".to_owned(),
            },
        };
        let Some(clone) = self.pending_clone.take() else {
            return;
        };
        let PendingClone {
            destination,
            parent,
            url,
            attempt,
            ..
        } = clone;
        if output.exit_code == 0 {
            if let Some(attempt) = &attempt {
                self.keep_credential(attempt);
            }
            self.toasts
                .push_with_detail(ToastKind::Success, "Clone complete", destination.clone());
            self.open_repository(&destination);
            self.offer_recursive_submodule_init();
            return;
        }
        let retry_destination = destination;
        let asked = self.prompt_for_credentials(
            &output.text,
            attempt.as_ref(),
            Box::new(move |app, credential| {
                app.start_clone(parent, url, retry_destination, Some(credential));
            }),
        );
        if !asked {
            self.toasts
                .push_with_detail(ToastKind::Error, "Clone failed", output.text.trim());
        }
    }
}
