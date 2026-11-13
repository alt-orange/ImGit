//! ImGit's Git layer: libgit2 (through `git2`) for everything local, the user's `git` executable for network
//! operations so the configured credential helpers and SSH setup are honoured.

pub mod cli;
pub mod graph;
pub mod repository;
pub mod types;

pub use repository::{Error, Repository, Result};
pub use types::*;
