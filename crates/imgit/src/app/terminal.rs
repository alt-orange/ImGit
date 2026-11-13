//! The terminal panel's model: per repository, tabs of shells, each a pseudo console and the screen its output
//! draws; the panel expanded, minimized to its toolbar, or hidden. The panel itself draws it
//! (`ui::panels::terminal_panel`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use imgit_platform::pseudo_console::PseudoConsole;
use imgit_vt::{Point, Screen};

use crate::app::application::Application;
use crate::app::dialog::DialogKind;

/// Columns, rows and scrollback lines of a session's screen until the panel sizes it.
const DEFAULT_COLUMNS: usize = 80;
const DEFAULT_ROWS: usize = 24;
const SCROLLBACK: usize = 5000;

/// The most input kept for a shell that has not started yet.
const MAX_PENDING_INPUT: usize = 64 * 1024;

/// Appends `text` to `pending` without letting it pass [`MAX_PENDING_INPUT`]; what does not fit is dropped.
fn push_pending(pending: &mut String, text: &str) {
    let room = MAX_PENDING_INPUT.saturating_sub(pending.len());
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    pending.push_str(&text[..end]);
}

/// The active tab after the session at `removed` was taken out of `remaining` ones: a tab before the active one
/// shifts it left, so the same shell stays selected; the active shell itself exiting selects the one that took
/// its place (the last when it was the last).
fn active_after_removal(active: usize, removed: usize, remaining: usize) -> usize {
    let active = if removed < active { active - 1 } else { active };
    active.min(remaining.saturating_sub(1))
}

/// A selection in line ids (`Screen::first_line_id()` based) and columns: anchor then head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
}

/// One shell of the terminal panel: a pseudo console and the screen its output draws.
pub struct TerminalSession {
    pub workdir: PathBuf,
    /// `None` until the panel starts the shell (and when it could not: see `failed`).
    pub console: Option<PseudoConsole>,
    pub screen: Screen,
    /// The shell could not be started.
    pub failed: bool,
    /// Typed before the shell started.
    pub pending_input: String,
    /// Lines the view is scrolled up from the bottom.
    pub scroll: usize,
    pub scrolled_seen: u64,
    /// The mouse is selecting.
    pub selecting: bool,
    pub selection: Option<Selection>,
    /// The shell this session runs (its tab's title), fixed when it starts: the Default preference may change
    /// meanwhile. `None` until it starts.
    pub shell_label: Option<String>,
}

impl TerminalSession {
    /// A session whose shell has not started.
    #[must_use]
    pub fn new(workdir: PathBuf) -> Self {
        Self {
            workdir,
            console: None,
            screen: Screen::new(DEFAULT_COLUMNS, DEFAULT_ROWS, SCROLLBACK),
            failed: false,
            pending_input: String::new(),
            scroll: 0,
            scrolled_seen: 0,
            selecting: false,
            selection: None,
            shell_label: None,
        }
    }

    /// Whether the shell is running.
    #[must_use]
    pub fn started(&self) -> bool {
        self.console.is_some()
    }

    /// Types `text` into the shell; before it has started the keystrokes wait in `pending_input` (up to
    /// [`MAX_PENDING_INPUT`]). A shell that could not start takes none.
    pub fn write(&mut self, text: &str) {
        match &mut self.console {
            // A shell that has ended takes no input; the poll closes its tab.
            Some(console) => {
                let _ = console.write(text.as_bytes());
            }
            None if self.failed => {}
            None => push_pending(&mut self.pending_input, text),
        }
    }

    /// The shell's output into the screen, the screen's replies back to the shell. `true` once the shell has
    /// exited (its tab closes).
    fn pump(&mut self) -> bool {
        let Some(console) = &mut self.console else {
            return false;
        };
        let output = console.read();
        if !output.is_empty() {
            self.screen.feed(&output);
            let replies = self.screen.take_replies();
            let _ = console.write(replies.as_bytes());
        }
        console.exited()
    }
}

/// The shells of one repository.
#[derive(Default)]
pub struct TerminalTabs {
    pub sessions: Vec<TerminalSession>,
    pub active: usize,
}

/// The terminal panel: per repository, tabs of shells; expanded, minimized to its toolbar, or hidden
/// (`Application::show_terminal_panel`).
#[derive(Default)]
pub struct TerminalPanel {
    /// By working directory.
    pub repos: HashMap<PathBuf, TerminalTabs>,
    pub minimized: bool,
    pub focus_requested: bool,
    pub focused: bool,
}

/// The folder the terminals of "no repository" open in.
fn current_directory() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

impl Application {
    /// The repository's terminal tabs (the shell starts when the panel first draws it). `workdir`: the
    /// repository's working directory, `None` for the terminal of no repository (the current directory).
    pub fn terminal_tabs(&mut self, workdir: Option<&Path>) -> &mut TerminalTabs {
        let key = workdir.map_or_else(current_directory, Path::to_path_buf);
        self.terminal.repos.entry(key).or_default()
    }

    /// The repository's active terminal; `None` when it has none.
    pub fn active_terminal(&mut self, workdir: Option<&Path>) -> Option<&mut TerminalSession> {
        let tabs = self.terminal_tabs(workdir);
        let last = tabs.sessions.len().checked_sub(1)?;
        tabs.active = tabs.active.min(last);
        let active = tabs.active;
        tabs.sessions.get_mut(active)
    }

    /// Opens a terminal tab for the repository and shows the panel.
    pub fn new_terminal_tab(&mut self, workdir: Option<&Path>) {
        let tabs = self.terminal_tabs(workdir);
        let folder = workdir.map_or_else(current_directory, Path::to_path_buf);
        tabs.sessions.push(TerminalSession::new(folder));
        tabs.active = tabs.sessions.len() - 1;
        self.show_terminal_panel = true;
        self.terminal.minimized = false;
        self.terminal.focus_requested = true;
    }

    /// Closes terminal tab `index` of the repository; the last one hides the panel (`cliPanelViewMode`
    /// "hidden").
    pub fn close_terminal_tab(&mut self, workdir: Option<&Path>, index: usize) {
        let tabs = self.terminal_tabs(workdir);
        if index >= tabs.sessions.len() {
            return;
        }
        let last = tabs.sessions.len() == 1;
        tabs.sessions.remove(index);
        if tabs.active >= index && tabs.active > 0 {
            tabs.active -= 1;
        }
        if last {
            self.show_terminal_panel = false;
        }
    }

    /// Kill All Terminals: asks first when there are two or more, then hides the panel.
    pub fn kill_terminals(&mut self, workdir: Option<&Path>) {
        // By working directory: the confirmation outlives this frame, and the tab may be gone by then.
        let folder = workdir.map_or_else(current_directory, Path::to_path_buf);
        let count = self.terminal_tabs(workdir).sessions.len();
        let kill = move |app: &mut Self| {
            app.terminal
                .repos
                .entry(folder)
                .or_default()
                .sessions
                .clear();
            app.show_terminal_panel = false;
        };
        if count < 2 {
            kill(self);
            return;
        }
        self.dialog.open(DialogKind::Confirm);
        self.dialog.message =
            format!("Kill {count} terminals? Any running processes will be stopped.");
        self.dialog.confirm_label = "Kill All Terminals".to_owned();
        self.dialog.danger = true;
        self.dialog.on_confirm = Some(Box::new(move |_, app| kill(app)));
    }

    /// Types `line` and Enter into the repository's active terminal (opening the panel and a shell if needed).
    pub fn run_terminal_command(&mut self, workdir: Option<&Path>, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if self.active_terminal(workdir).is_none() {
            self.new_terminal_tab(workdir);
        }
        self.show_terminal_panel = true;
        self.terminal.minimized = false;
        if let Some(session) = self.active_terminal(workdir) {
            session.scroll = 0;
            // Before the shell has started the keystrokes wait in the console's input pipe.
            session.write(&format!("{line}\r"));
        }
    }

    /// The shells: their output into the screens, the replies back; a shell that exits closes its tab.
    pub(crate) fn poll_terminals(&mut self) {
        let active_workdir = self
            .active()
            .map(|state| state.repo.workdir().to_path_buf());
        let mut hide = false;
        for (workdir, tabs) in &mut self.terminal.repos {
            let mut i = 0;
            while i < tabs.sessions.len() {
                if tabs.sessions[i].pump() {
                    tabs.sessions.remove(i);
                    tabs.active = active_after_removal(tabs.active, i, tabs.sessions.len());
                    hide |= tabs.sessions.is_empty()
                        && active_workdir
                            .as_deref()
                            .is_some_and(|active| active == workdir);
                    continue;
                }
                i += 1;
            }
        }
        if hide {
            self.show_terminal_panel = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selected_shell_stays_selected_when_an_earlier_one_exits() {
        // Three shells, the third selected; the first exits: the same shell is now the second.
        assert_eq!(active_after_removal(2, 0, 2), 1);
        // A later one exits: nothing moves.
        assert_eq!(active_after_removal(0, 2, 2), 0);
        // The selected one exits: the one that took its place, or the last.
        assert_eq!(active_after_removal(1, 1, 2), 1);
        assert_eq!(active_after_removal(2, 2, 2), 1);
        // The only shell.
        assert_eq!(active_after_removal(0, 0, 0), 0);
    }

    #[test]
    fn a_failed_shell_takes_no_input_and_a_waiting_one_a_bounded_amount() {
        let mut failed = TerminalSession::new(PathBuf::new());
        failed.failed = true;
        failed.write("ls\r");
        assert!(failed.pending_input.is_empty());

        let mut waiting = TerminalSession::new(PathBuf::new());
        waiting.write("ls\r");
        assert_eq!(waiting.pending_input, "ls\r");
        for _ in 0..100 {
            waiting.write(&"é".repeat(1000));
        }
        assert!(waiting.pending_input.len() <= MAX_PENDING_INPUT);
        assert!(waiting.pending_input.len() >= MAX_PENDING_INPUT - 1);
    }
}
