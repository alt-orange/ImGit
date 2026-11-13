//! `git clone` in flight, with its progress.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use imgit_app::job::Job;
use imgit_git::cli::{self, CloneProgress, Credential};

use crate::app::application::Application;
use crate::app::toasts::ToastKind;

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
        let mut credentials = self.credentials.clone();
        if let Some(attempt) = &attempt {
            credentials.retain(|credential| credential.url != attempt.url);
            credentials.insert(0, attempt.clone());
        }
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
                ["clone", "--progress", &work_url, &work_destination],
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
