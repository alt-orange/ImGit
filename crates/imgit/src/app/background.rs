//! Jobs on a worker thread whose result runs on the UI thread, in a later frame.

use imgit_app::job::Job;

use crate::app::action::Completion;
use crate::app::application::Application;

/// A job running on a worker thread.
pub struct BackgroundJob {
    pub tag: String,
    job: Job<Completion>,
}

impl Application {
    /// Runs `work` on a worker thread; the [`Completion`] it returns then runs on the UI thread (a later frame),
    /// where it may push toasts or change settings. `tag` names the job for [`Self::background_running`].
    pub fn run_in_background(
        &mut self,
        tag: impl Into<String>,
        work: impl FnOnce() -> Completion + Send + 'static,
    ) {
        self.background.push(BackgroundJob {
            tag: tag.into(),
            job: Job::spawn(work),
        });
    }

    /// Whether a job with `tag` has not finished.
    #[must_use]
    pub fn background_running(&self, tag: &str) -> bool {
        self.background.iter().any(|job| job.tag == tag)
    }

    /// Runs the continuations of the jobs that finished. They leave the list first: one may start another job.
    pub(crate) fn poll_background(&mut self) {
        let mut finished = Vec::new();
        self.background.retain_mut(|entry| {
            if !entry.job.is_running() {
                return false; // the worker died without a result
            }
            match entry.job.poll() {
                Some(completion) => {
                    finished.push(completion);
                    false
                }
                None => entry.job.is_running(),
            }
        });
        for completion in finished {
            completion(self);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::app::toasts::ToastKind;

    #[test]
    fn a_job_completes_on_the_ui_thread() {
        let scratch = tempfile::tempdir().unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.run_in_background("answer", || {
            Box::new(|app: &mut Application| app.toasts.push(ToastKind::Info, "done"))
        });
        assert!(app.background_running("answer"));
        assert!(!app.background_running("other"));
        let deadline = Instant::now() + Duration::from_secs(30);
        while app.background_running("answer") && Instant::now() < deadline {
            app.poll_background();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!app.background_running("answer"));
        assert_eq!(app.toasts.history()[0].title, "done");
    }

    #[test]
    fn a_continuation_may_start_the_next_job() {
        let scratch = tempfile::tempdir().unwrap();
        let mut app = Application::with_config_directory(scratch.path().join("config"));
        app.run_in_background("first", || {
            Box::new(|app: &mut Application| {
                app.run_in_background("second", || Box::new(|_: &mut Application| {}));
            })
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            app.poll_background();
            if app.background_running("second") {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the second job never started");
    }
}
