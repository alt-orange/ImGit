//! ImGit's application logic that needs no UI: the preferences store, profiles, custom theme files, the hosting
//! connections, the Gitflow / branch cleanup workflows, date formatting, the merge tool's model and the state
//! of a repository tab.

pub mod connections;
pub mod date_format;
pub mod job;
pub mod md5;
pub mod merge_tool;
pub mod preferences;
pub mod profiles;
pub mod repo_state;
pub mod syntax;
pub mod text;
pub mod theme_file;
pub mod workflows;

pub use job::Job;
pub use md5::md5_hex;
pub use preferences::{Preference, Preferences, pref, preferences};
pub use repo_state::{RepoState, WIP_SHA};
