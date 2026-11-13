//! A shell attached to a pseudo terminal (ConPTY on Windows, a pty elsewhere) for the terminal panel's sessions:
//! keystrokes go in as VT input, the screen comes back as VT output, read on a background thread.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

#[cfg(unix)]
#[path = "pseudo_console/unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "pseudo_console/windows.rs"]
mod imp;

/// How long after the shell has ended its output counts as complete even though the terminal is still open.
const SETTLE_TIME: Duration = Duration::from_millis(300);

/// What the reader thread shares with the console's owner.
#[derive(Default)]
struct Shared {
    /// The output that arrived since the last [`PseudoConsole::read`].
    pending: Mutex<Vec<u8>>,
    /// Asks the reader to end.
    stop: AtomicBool,
    /// Set once the reader has seen the end of the output.
    drained: AtomicBool,
}

impl Shared {
    fn pending(&self) -> MutexGuard<'_, Vec<u8>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A running shell on a pseudo terminal. Dropping it closes the terminal, which ends the shell: on POSIX with
/// SIGHUP to its process group, as a terminal emulator closing its window does (and SIGKILL about half a second
/// later for a shell that ignores it); on Windows by closing the pseudo console.
pub struct PseudoConsole {
    backend: imp::Backend,
    shared: Arc<Shared>,
    /// When the shell was found to have ended.
    ended_at: Option<Instant>,
}

impl PseudoConsole {
    /// Starts `args` (`args[0]` looked up on PATH) in `workdir` (the current directory when `None`) on a
    /// `columns` x `rows` terminal. On POSIX the shell sees `TERM=xterm-256color`.
    ///
    /// # Errors
    /// The terminal could not be created or the program could not be started.
    pub fn spawn(
        args: &[String],
        workdir: Option<&Path>,
        columns: u16,
        rows: u16,
    ) -> io::Result<Self> {
        if args.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no program to run",
            ));
        }
        let workdir = workdir.filter(|path| !path.as_os_str().is_empty());
        let shared = Arc::new(Shared::default());
        let backend = imp::Backend::spawn(
            args,
            workdir,
            columns.max(1),
            rows.max(1),
            Arc::clone(&shared),
        )?;
        Ok(Self {
            backend,
            shared,
            ended_at: None,
        })
    }

    /// Sends `bytes` (keystrokes as VT input) to the shell.
    ///
    /// # Errors
    /// The terminal no longer takes input (the shell has ended).
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            Ok(())
        } else {
            self.backend.write(bytes)
        }
    }

    /// Tells the shell its terminal is now `columns` x `rows`.
    ///
    /// # Errors
    /// The system refused the size.
    pub fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        self.backend.resize(columns.max(1), rows.max(1))
    }

    /// The output (VT sequences) that arrived since the last call.
    #[must_use]
    pub fn read(&self) -> Vec<u8> {
        std::mem::take(&mut *self.shared.pending())
    }

    /// True once the shell process has ended and all of its output has been read.
    pub fn exited(&mut self) -> bool {
        let ended_at = match self.ended_at {
            Some(ended_at) => ended_at,
            None if self.backend.has_ended() => *self.ended_at.insert(Instant::now()),
            None => return false,
        };
        // The reader only ends once the last process holding the terminal is gone, which a background job the
        // shell left behind (`sleep 1000 &` then `exit`) may never be, and on Windows only once the pseudo
        // console is closed: shortly after the shell itself has ended, its output counts as complete.
        let settled =
            self.shared.drained.load(Ordering::Acquire) || ended_at.elapsed() > SETTLE_TIME;
        settled && self.shared.pending().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|&arg| arg.to_owned()).collect()
    }

    fn shell(script: &str) -> Vec<String> {
        if cfg!(windows) {
            strings(&["cmd.exe", "/d", "/c", script])
        } else {
            strings(&["sh", "-c", script])
        }
    }

    /// Reads until the shell has exited or `wanted` shows up in the output.
    fn read_until(console: &mut PseudoConsole, wanted: &str) -> String {
        let started = Instant::now();
        let mut output = Vec::new();
        loop {
            let done = console.exited();
            output.extend(console.read());
            let text = String::from_utf8_lossy(&output);
            if done || text.contains(wanted) || started.elapsed() > Duration::from_secs(30) {
                return text.into_owned();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_exit(console: &mut PseudoConsole) -> bool {
        let started = Instant::now();
        while !console.exited() {
            let _ = console.read();
            if started.elapsed() > Duration::from_secs(30) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    #[test]
    fn runs_a_command_and_reads_its_output() {
        let mut console = PseudoConsole::spawn(&shell("echo hi"), None, 80, 24).unwrap();
        let text = read_until(&mut console, "hi");
        assert!(text.contains("hi"), "{text:?}");
        assert!(wait_for_exit(&mut console));
        assert!(console.read().is_empty());
    }

    #[test]
    fn takes_input_and_a_new_size() {
        let folder = tempfile::tempdir().unwrap();
        let args = if cfg!(windows) {
            strings(&["cmd.exe", "/d", "/q"])
        } else {
            strings(&["sh"])
        };
        let mut console = PseudoConsole::spawn(&args, Some(folder.path()), 80, 24).unwrap();
        assert!(!console.exited());
        console.resize(100, 30).unwrap();
        // The shell runs in the folder it was given; the caret / quotes keep the typed command from matching.
        let command = if cfg!(windows) {
            "echo im^git& cd\r"
        } else {
            "echo im''git; pwd\r"
        };
        console.write(command.as_bytes()).unwrap();
        let name = folder.path().file_name().unwrap().to_str().unwrap();
        let text = read_until(&mut console, name);
        assert!(text.contains("imgit") && text.contains(name), "{text:?}");
        console.write(b"exit\r").unwrap();
        assert!(wait_for_exit(&mut console));
    }

    #[test]
    fn dropping_it_ends_a_running_shell() {
        let args = if cfg!(windows) {
            strings(&["cmd.exe", "/d"])
        } else {
            strings(&["sh"])
        };
        let started = Instant::now();
        let mut console = PseudoConsole::spawn(&args, None, 80, 24).unwrap();
        assert!(!console.exited());
        drop(console);
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn fails_for_what_cannot_start() {
        assert!(PseudoConsole::spawn(&[], None, 80, 24).is_err());
        assert!(
            PseudoConsole::spawn(&strings(&["imgit-no-such-program-4f2a"]), None, 80, 24).is_err()
        );
        let missing = Path::new("imgit-no-such-folder-4f2a");
        assert!(PseudoConsole::spawn(&shell("exit 0"), Some(missing), 80, 24).is_err());
    }
}
