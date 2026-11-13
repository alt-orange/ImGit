//! Deferred actions: what the C++ kept as `std::function<void()>` capturing the application and a repository
//! by reference (menu item clicks, `Dialog::on_confirm`, the credentials prompt's retry, background-job
//! completions).
//!
//! An action runs later, at a point of the frame where no repository is detached from its tab (a menu's click
//! after the menu closed, a confirmed dialog...), so it captures **owned data only** (shas, names, the tab's
//! working directory) and finds its repository again when it runs:
//!
//! ```ignore
//! let workdir = state.repo.workdir().to_path_buf();
//! let sha = sha.to_owned();
//! menu_item("Revert Commit", move |cx, app| {
//!     app.with_repo(&workdir, |app, state| {
//!         let result = state.repo.revert(&sha);
//!         app.run(state, "Revert", result, Some("Reverted"));
//!     });
//! })
//! ```

use crate::app::application::Application;
use crate::ui::Cx;

/// A deferred action: a menu click, a dialog's confirmation.
pub type Action = Box<dyn FnOnce(&mut Cx<'_>, &mut Application)>;

/// An action taking the text the user entered (the rename prompt bar).
pub type TextAction = Box<dyn FnOnce(&mut Cx<'_>, &mut Application, &str)>;

/// The continuation of a background job: runs on the UI thread, in a later frame, with the application.
pub type Completion = Box<dyn FnOnce(&mut Application) + Send>;

/// A credential the user entered, for the command to run again with.
pub type CredentialAction = Box<dyn FnOnce(&mut Application, imgit_git::cli::Credential)>;
