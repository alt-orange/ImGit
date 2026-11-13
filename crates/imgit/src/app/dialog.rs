//! The modal currently shown with its form fields, the profile modal and the command palette's state.

use std::path::PathBuf;

use imgit_app::profiles::Profile;
use imgit_git::{ResetMode, Sha};

use crate::app::action::{Action, TextAction};

/// Which dialog is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialogKind {
    CloneRepo,
    InitRepo,
    CreateBranch,
    CreateTag,
    RenameBranch,
    /// The rename prompt bar for any name: `message` / `text_a` / `flag` (empty allowed) / `on_text`.
    TextPrompt,
    DeleteBranch,
    DeleteTag,
    DeleteStash,
    /// `sha`: the stash, `text_a`: its new message (the prompt bar).
    EditStashMessage,
    /// `ref_name` / `ref_remote`: the branch to check out (empty: `sha`, detached), `text_a`: path.
    CreateWorktree,
    DiscardAll,
    Reset,
    PushNewBranch,
    AddRemote,
    AddSubmodule,
    /// `text_a`: the submodule's URL, `text_b`: its path.
    EditSubmodule,
    EditRemote,
    RewordCommit,
    SquashCommits,
    SetUpstream,
    Preferences,
    KeyboardShortcuts,
    About,
    /// Generic: `title` / `message` / `confirm_label` / `on_confirm`.
    Confirm,
    /// The prompt bar: `text_a` the new file's name, `text_b` the folder it goes in (empty: the root).
    CreateFile,
    /// `checks`: the local branches to delete, each with why it is offered.
    BranchCleanup,
}

/// An entry of a list of ticked / unticked entries (`BranchCleanup`): `label` and the `note` after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub label: String,
    pub note: String,
    pub checked: bool,
}

impl Check {
    /// A ticked entry.
    #[must_use]
    pub fn new(label: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            note: note.into(),
            checked: true,
        }
    }
}

/// The modal currently shown, with its form fields. `kind` is `None` while no dialog is open.
#[derive(Default)]
pub struct Dialog {
    pub kind: Option<DialogKind>,
    pub just_opened: bool,
    /// The repository the dialog was opened for (its working directory, set on the first frame it is drawn): the
    /// prompt bar acts on that tab, whichever one is active, and closes when it is gone.
    pub workdir: Option<PathBuf>,
    /// Name / path / url.
    pub text_a: String,
    /// Message / destination.
    pub text_b: String,
    /// "Checkout after create", "also delete remote"...
    pub flag: bool,
    /// Empty: none.
    pub sha: Sha,
    pub ref_name: String,
    pub ref_remote: String,
    pub stash_index: Option<usize>,
    pub page: usize,
    pub reset_mode: ResetMode,
    /// `SquashCommits`.
    pub shas: Vec<Sha>,
    pub checks: Vec<Check>,
    pub error: Option<String>,
    // Confirm
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub danger: bool,
    /// Runs when the user confirms; the dialog is closed first.
    pub on_confirm: Option<Action>,
    /// `TextPrompt`: runs with the entered text.
    pub on_text: Option<TextAction>,
}

impl Dialog {
    /// Closes whatever is open and opens a fresh dialog of `kind`.
    pub fn open(&mut self, kind: DialogKind) {
        *self = Self {
            kind: Some(kind),
            just_opened: true,
            ..Self::default()
        };
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    /// Whether a dialog of any kind is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.kind.is_some()
    }

    /// Whether a dialog of `kind` is open.
    #[must_use]
    pub fn is(&self, kind: DialogKind) -> bool {
        self.kind == Some(kind)
    }
}

/// Which profile modal is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileDialogKind {
    AddEdit,
    Delete,
}

/// The Add / Edit Profile modal and its Delete profile confirmation. `kind` is `None` while closed.
#[derive(Debug, Clone, Default)]
pub struct ProfileDialog {
    pub kind: Option<ProfileDialogKind>,
    pub editing: bool,
    pub just_opened: bool,
    /// The fields; `guid`: the profile edited / deleted.
    pub values: Profile,
    /// The fields' placeholders, used for the ones left empty.
    pub defaults: Profile,
    pub copy_current: bool,
    /// The email the preview shows, updated 500ms after typing stops.
    pub gravatar_email: String,
    pub email_changed_at: f64,
}

impl ProfileDialog {
    pub fn close(&mut self) {
        *self = Self::default();
    }
}

/// The command palette (Ctrl+P): commands, branch checkouts and recent repositories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FuzzyFinder {
    pub open: bool,
    pub focus_requested: bool,
    pub text: String,
    pub selected: usize,
}

impl FuzzyFinder {
    /// Ctrl+P: opens the palette empty, or closes it.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.text.clear();
        self.selected = 0;
        self.focus_requested = self.open;
    }

    /// Opens the palette empty (the Help menu's "Open Command Palette").
    pub fn show(&mut self) {
        self.open = true;
        self.text.clear();
        self.selected = 0;
        self.focus_requested = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_dialog_resets_it() {
        let mut dialog = Dialog::default();
        dialog.open(DialogKind::CreateBranch);
        dialog.text_a = "feature".to_owned();
        dialog.flag = true;
        dialog.on_confirm = Some(Box::new(|_, _| {}));
        assert!(dialog.is(DialogKind::CreateBranch));
        assert!(dialog.just_opened);
        dialog.open(DialogKind::Confirm);
        assert!(dialog.is(DialogKind::Confirm));
        assert!(dialog.text_a.is_empty() && !dialog.flag && dialog.on_confirm.is_none());
        assert_eq!(dialog.reset_mode, ResetMode::Mixed);
        dialog.close();
        assert!(!dialog.is_open() && !dialog.just_opened);
    }

    #[test]
    fn the_palette_toggles_empty() {
        let mut finder = FuzzyFinder {
            text: "old".to_owned(),
            selected: 3,
            ..FuzzyFinder::default()
        };
        finder.toggle();
        assert!(finder.open && finder.focus_requested && finder.text.is_empty());
        assert_eq!(finder.selected, 0);
        finder.toggle();
        assert!(!finder.open && !finder.focus_requested);
    }
}
