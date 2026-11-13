//! The POSIX side of [`super`]: a pty from the `portable-pty` crate.

use super::Shared;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::fmt::Display;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

pub(super) struct Backend {
    // `Option`s so that `drop` can close them in order.
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn Child + Send + Sync>,
    reaped: bool,
    shared: Arc<Shared>,
    reader: Option<JoinHandle<()>>,
}

fn size_of(columns: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols: columns,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn other(error: impl Display) -> io::Error {
    io::Error::other(format!("{error:#}"))
}

impl Backend {
    pub(super) fn spawn(
        args: &[String],
        workdir: Option<&Path>,
        columns: u16,
        rows: u16,
        shared: Arc<Shared>,
    ) -> io::Result<Self> {
        // Always an explicit, existing folder: portable-pty would silently start in the home folder instead.
        let workdir = match workdir {
            Some(workdir) if workdir.is_dir() => workdir.to_owned(),
            Some(workdir) => {
                let message = format!("{} is not a folder", workdir.display());
                return Err(io::Error::new(io::ErrorKind::NotFound, message));
            }
            None => std::env::current_dir()?,
        };
        let mut command = CommandBuilder::new(&args[0]);
        command.args(&args[1..]);
        command.cwd(workdir);
        command.env("TERM", "xterm-256color");

        let pair = native_pty_system()
            .openpty(size_of(columns, rows))
            .map_err(other)?;
        // The child becomes a session leader with the pty as its controlling terminal.
        let child = pair.slave.spawn_command(command).map_err(other)?;
        // Only the child keeps the terminal's other side open, so its end is seen.
        drop(pair.slave);
        let master = pair.master;
        let reader = master.try_clone_reader().map_err(other)?;
        let writer = master.take_writer().map_err(other)?;
        let descriptor = master.as_raw_fd();
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("pseudo console reader".to_owned())
                .spawn(move || read_output(reader, descriptor, &shared))?
        };
        Ok(Self {
            master: Some(master),
            writer: Some(writer),
            child,
            reaped: false,
            shared,
            reader: Some(thread),
        })
    }

    pub(super) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        writer.write_all(bytes)?;
        writer.flush()
    }

    pub(super) fn resize(&self, columns: u16, rows: u16) -> io::Result<()> {
        // TIOCSWINSZ on the master, which sends the shell SIGWINCH.
        self.master.as_ref().map_or(Ok(()), |master| {
            master.resize(size_of(columns, rows)).map_err(other)
        })
    }

    /// Whether the shell process has ended (reaping it when it has).
    pub(super) fn has_ended(&mut self) -> bool {
        self.reaped = self.reaped || !matches!(self.child.try_wait(), Ok(None));
        self.reaped
    }

    /// The shell's process group. It leads its own session, so the group's id is its own.
    fn process_group(&self) -> Option<libc::pid_t> {
        // Never 0 or 1: kill(0) and kill(-1) signal our own group and everything.
        self.child
            .process_id()
            .and_then(|pid| libc::pid_t::try_from(pid).ok())
            .filter(|&pid| pid > 1)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let group = if self.has_ended() {
            None
        } else {
            self.process_group()
        };
        // SIGHUP to the shell's process group, as a terminal emulator closing its window does.
        if let Some(group) = group {
            // SAFETY: kill only takes integers; the child has not been reaped, so the id still names it.
            unsafe { libc::kill(-group, libc::SIGHUP) };
        }
        // The reader polls the master's descriptor, so it has to end (within its poll interval) before that
        // is closed.
        self.shared.stop.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        self.writer = None;
        self.master = None;
        if let Some(group) = group {
            // This runs on the UI thread: a shell that ignores SIGHUP gets about half a second, then SIGKILL.
            for _ in 0..50 {
                if self.has_ended() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            // SAFETY: as above.
            unsafe { libc::kill(-group, libc::SIGKILL) };
            let _ = self.child.wait();
        }
    }
}

/// The reader thread: appends the terminal's output to `shared.pending` until the output ends or the console is
/// dropped.
fn read_output(mut reader: Box<dyn Read + Send>, descriptor: Option<libc::c_int>, shared: &Shared) {
    let mut buffer = [0u8; 8192];
    while !shared.stop.load(Ordering::Acquire) {
        // A blocking read could not be interrupted by `stop` while a background job keeps the terminal open.
        if let Some(descriptor) = descriptor {
            let mut poll = libc::pollfd {
                fd: descriptor,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `poll` is one live pollfd, as the count says; the backend joins this thread before it
            // closes the descriptor.
            let ready = unsafe { libc::poll(&mut poll, 1, 100) };
            if ready == 0
                || (ready < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted)
            {
                continue;
            }
            if ready < 0 {
                break;
            }
        }
        match reader.read(&mut buffer) {
            // EIO once the last process holding the terminal has ended (Linux), 0 on macOS.
            Ok(0) => break,
            Ok(count) => shared.pending().extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    shared.drained.store(true, Ordering::Release);
}
