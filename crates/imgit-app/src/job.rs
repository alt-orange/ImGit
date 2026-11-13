//! Work done off the UI thread and collected on a later frame.

use std::fmt;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

/// One value computed on a worker thread.
///
/// The UI thread asks [`poll`](Self::poll) for it once per frame. Dropping a job never waits for its worker:
/// the thread runs to its end on its own and its result is thrown away, so closing a tab during a slow status
/// read or git command does not stall the UI.
pub struct Job<T> {
    /// `None`: idle (never started, or its result was collected).
    receiver: Option<Receiver<T>>,
}

impl<T> Job<T> {
    /// A job that was never started.
    #[must_use]
    pub const fn idle() -> Self {
        Self { receiver: None }
    }

    /// Whether a job was started whose result [`poll`](Self::poll) has not handed out yet (a finished worker
    /// still counts until its result is collected).
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.receiver.is_some()
    }

    /// The result, once: `None` while the worker is still at it, and from then on.
    ///
    /// A worker that panicked has no result: the job then turns idle and `poll` keeps returning `None`.
    pub fn poll(&mut self) -> Option<T> {
        match self.receiver.as_ref()?.try_recv() {
            Ok(value) => {
                self.receiver = None;
                Some(value)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.receiver = None;
                None
            }
        }
    }

    /// Forgets the job; its worker finishes on its own.
    pub fn cancel(&mut self) {
        self.receiver = None;
    }
}

impl<T: Send + 'static> Job<T> {
    /// Runs `work` on a thread of its own.
    #[must_use]
    pub fn spawn(work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            // Nobody is listening any more when the job was dropped.
            let _ = sender.send(work());
        });
        Self {
            receiver: Some(receiver),
        }
    }
}

impl<T> Default for Job<T> {
    fn default() -> Self {
        Self::idle()
    }
}

impl<T> fmt::Debug for Job<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("running", &self.is_running())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    fn wait<T>(job: &mut Job<T>) -> Option<T> {
        let deadline = Instant::now() + Duration::from_secs(30);
        while job.is_running() && Instant::now() < deadline {
            if let Some(value) = job.poll() {
                return Some(value);
            }
            thread::sleep(Duration::from_millis(1));
        }
        None
    }

    #[test]
    fn an_idle_job_has_nothing() {
        let mut job = Job::<i32>::default();
        assert!(!job.is_running());
        assert_eq!(job.poll(), None);
    }

    #[test]
    fn the_result_is_handed_out_once() {
        let (release, gate) = channel::<()>();
        let mut job = Job::spawn(move || {
            gate.recv().unwrap();
            42
        });
        assert!(job.is_running());
        assert_eq!(job.poll(), None);
        assert!(job.is_running());
        release.send(()).unwrap();
        assert_eq!(wait(&mut job), Some(42));
        assert!(!job.is_running());
        assert_eq!(job.poll(), None);
    }

    #[test]
    fn a_panicked_worker_leaves_the_job_idle() {
        let mut job = Job::<i32>::spawn(|| panic!("expected by the test"));
        assert_eq!(wait(&mut job), None);
        assert!(!job.is_running());
    }

    #[test]
    fn dropping_a_running_job_does_not_wait() {
        let (release, gate) = channel::<()>();
        let (done, finished) = channel::<()>();
        let job = Job::spawn(move || {
            let _ = gate.recv();
            let _ = done.send(());
        });
        drop(job); // would deadlock here if it joined: the worker is still waiting for `release`
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(30)).unwrap();
    }
}
