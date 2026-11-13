//! A libgit2-backed repository. Every operation is synchronous and reports libgit2's error text on failure; the
//! few things libgit2 cannot do (hooks, signing, partial patches, aborting a rebase) go through the `git`
//! executable.

mod config;
mod diff;
mod history;
mod index;
mod refs;
mod rewrite;
mod stash;
mod status;

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::cli;
use crate::types::{
    Author, Commit, DiffLine, FileChange, FileStatus, LineOrigin, NodeType, RepoState, Sha,
    Submodule,
};

pub use config::{
    global_config, global_config_bool, global_user_email, global_user_name, set_global_config,
    set_global_identity,
};
pub use diff::{LineHunk, diff_lines};

/// Why a repository operation failed. `Display` is the text the UI shows.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// libgit2 refused: its own message, without git2's class / code suffix.
    #[error("{}", .0.message())]
    Git(#[from] git2::Error),
    /// A file of the working directory could not be examined.
    #[error("{path}: {source}")]
    Io {
        /// The path as it was given, relative to the working directory.
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// The `git` executable failed: what it printed (or a short description when it printed nothing).
    #[error("{output}")]
    Cli { output: String },
    /// An application-level failure ("nothing to amend", a rolled-back rebase...).
    #[error("{0}")]
    Message(String),
}

impl Error {
    fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    /// A failed `git` command: its output, or `fallback` when it printed nothing.
    fn cli(output: &cli::Output, fallback: &str) -> Self {
        Self::Cli {
            output: if output.text.is_empty() {
                fallback.to_owned()
            } else {
                output.text.clone()
            },
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// How an operation that may stop on conflicts (cherry-pick, revert, merge) ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// It went through (a commit was made, the branch fast-forwarded, or there was nothing to do).
    Done,
    /// It stopped on conflicts: the repository stays in that operation's state until the conflicts are
    /// resolved and committed, or the operation is aborted.
    Conflicts,
}

impl Outcome {
    #[must_use]
    pub const fn is_done(self) -> bool {
        matches!(self, Self::Done)
    }
}

/// How a file diff is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiffOptions {
    /// Lines of context around each change; a huge value yields the whole file as one hunk.
    pub context_lines: u32,
    /// Leave out changes in whitespace only.
    pub ignore_whitespace: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            context_lines: 3,
            ignore_whitespace: false,
        }
    }
}

/// A changed line of a working-directory diff, as the diff view selects it: an added line with its new line
/// number, a deleted one with its old line number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineSelection {
    /// `Added` or `Deleted`; a context line selects nothing.
    pub origin: LineOrigin,
    /// 1-based: the new line number of an added line, the old one of a deleted line.
    pub lineno: u32,
}

impl LineSelection {
    /// The selection of a diff line; `None` for a context line.
    #[must_use]
    pub fn of(line: &DiffLine) -> Option<Self> {
        let lineno = match line.origin {
            LineOrigin::Added => line.new_lineno?,
            LineOrigin::Deleted => line.old_lineno?,
            LineOrigin::Context => return None,
        };
        Some(Self {
            origin: line.origin,
            lineno,
        })
    }
}

/// How `git commit` runs ([`Repository::commit_with_git`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CommitOptions {
    pub amend: bool,
    /// `-S`; off passes `--no-gpg-sign`, so `commit.gpgsign` is the form's default, not an override.
    pub sign: bool,
    /// `--no-verify`
    pub skip_hooks: bool,
    /// `--cleanup=strip`: lines starting with the comment character are left out.
    pub strip_comments: bool,
}

/// The three versions of a conflicted file the index holds (stages 1 to 3). A side that does not have the file
/// (added on one side, deleted on the other) is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConflictSides {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

/// A libgit2-backed repository.
pub struct Repository {
    repo: git2::Repository,
    workdir: PathBuf,
    name: String,
}

impl fmt::Debug for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Repository")
            .field("workdir", &self.workdir)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Repository {
    /// Opens the repository at `path` or, when `path` is inside one, the repository holding it.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let repo =
            git2::Repository::open_ext(path, git2::RepositoryOpenFlags::empty(), &[] as &[&OsStr])?;
        Ok(Self::new(repo))
    }

    /// Creates a repository at `path` (or reopens the one already there).
    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self::new(git2::Repository::init(path)?))
    }

    fn new(repo: git2::Repository) -> Self {
        let workdir = repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf();
        let name = workdir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            repo,
            workdir,
            name,
        }
    }

    /// The working directory; the `.git` folder itself for a bare repository.
    #[must_use]
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// The working directory's folder name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The `.git` folder shared by the repository and its linked worktrees (forward slashes, no trailing one).
    #[must_use]
    pub fn common_dir(&self) -> String {
        let mut dir = self.repo.commondir().to_string_lossy().replace('\\', "/");
        while dir.len() > 1 && dir.ends_with('/') {
            dir.pop();
        }
        dir
    }

    /// This worktree's own `.git` folder (its `info/sparse-checkout`, `HEAD`...).
    #[must_use]
    pub fn git_dir(&self) -> &Path {
        self.repo.path()
    }

    /// The operation the repository is in the middle of.
    #[must_use]
    pub fn state(&self) -> RepoState {
        use git2::RepositoryState as State;
        match self.repo.state() {
            State::Clean => RepoState::None,
            State::Merge => RepoState::Merge,
            State::Revert | State::RevertSequence => RepoState::Revert,
            State::CherryPick | State::CherryPickSequence => RepoState::CherryPick,
            State::Rebase | State::RebaseInteractive | State::RebaseMerge => RepoState::Rebase,
            _ => RepoState::Other,
        }
    }

    // What the operations share.

    fn lookup_commit(&self, sha: &str) -> Result<git2::Commit<'_>> {
        Ok(self.repo.find_commit(parse_oid(sha)?)?)
    }

    /// HEAD's commit, `None` on an unborn branch.
    fn head_commit(&self) -> Result<Option<git2::Commit<'_>>> {
        match self.repo.revparse_single("HEAD") {
            Ok(object) => Ok(Some(object.peel_to_commit()?)),
            Err(error) if is_missing(&error) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// HEAD's tree, `None` on an unborn branch.
    fn head_tree(&self) -> Result<Option<git2::Tree<'_>>> {
        Ok(match self.head_commit()? {
            Some(head) => Some(head.tree()?),
            None => None,
        })
    }

    /// True when the repository's own config (`.git/config`) sets `user.name` or `user.email`.
    fn has_local_identity(&self) -> bool {
        let Ok(local) = self
            .repo
            .config()
            .and_then(|config| config.open_level(git2::ConfigLevel::Local))
        else {
            return false;
        };
        ["user.name", "user.email"]
            .iter()
            .any(|key| local.get_string(key).is_ok())
    }

    /// Who commits: the profile's identity in a repository without its own, else the configured one.
    fn default_signature(&self) -> Result<git2::Signature<'static>> {
        if let Some((name, email)) = profile_identity()
            && !self.has_local_identity()
            && let Ok(signature) = git2::Signature::now(&name, &email)
        {
            return Ok(signature);
        }
        // No user.name / user.email configured: fall back so commits still work.
        Ok(self
            .repo
            .signature()
            .or_else(|_| git2::Signature::now("ImGit", "imgit@localhost"))?)
    }

    /// The submodules with the commit the repository records for each and the one each has checked out.
    fn read_submodules(&self) -> Vec<Submodule> {
        let Ok(submodules) = self.repo.submodules() else {
            return Vec::new();
        };
        submodules
            .iter()
            .map(|submodule| Submodule {
                name: lossy(submodule.name_bytes()),
                path: submodule.path().to_string_lossy().into_owned(),
                url: submodule.opt_url_bytes().map(lossy).unwrap_or_default(),
                recorded: submodule
                    .index_id()
                    .or_else(|| submodule.head_id())
                    .map(|id| id.to_string()),
                // Reads the submodule's own HEAD; none while it is not checked out.
                checked_out: submodule.workdir_id().map(|id| id.to_string()),
            })
            .collect()
    }

    /// The stash list, newest first (`stash@{0}` is the first entry): each stash commit and its message.
    fn stash_entries(&self) -> Result<Vec<(git2::Oid, String)>> {
        match self.repo.find_reference("refs/stash") {
            Ok(_) => {}
            Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        }
        let reflog = self.repo.reflog("refs/stash")?;
        Ok(reflog
            .iter()
            .map(|entry| {
                (
                    entry.id_new(),
                    entry.message_bytes().map(lossy).unwrap_or_default(),
                )
            })
            .collect())
    }

    /// The commits being merged in (`MERGE_HEAD`), in order.
    fn merge_heads(&self) -> Vec<git2::Oid> {
        std::fs::read_to_string(self.repo.path().join("MERGE_HEAD"))
            .map(|text| {
                text.lines()
                    .filter_map(|line| git2::Oid::from_str(line.trim()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Ends the operation a commit just completed, as git's own `commit` does. libgit2's state cleanup would
    /// also delete the rebase-merge / rebase-apply / sequencer directories, which destroys an in-progress
    /// rebase or a multi-commit cherry-pick / revert: a rebase keeps its state (`git rebase --continue` goes on
    /// once the index is clean), a sequence only loses the head file of the step just committed.
    fn finish_operation_state(&self) {
        use git2::RepositoryState as State;
        let head_file = match self.repo.state() {
            State::Merge | State::CherryPick | State::Revert => {
                let _ = self.repo.cleanup_state();
                return;
            }
            State::CherryPickSequence => "CHERRY_PICK_HEAD",
            State::RevertSequence => "REVERT_HEAD",
            _ => return,
        };
        let _ = std::fs::remove_file(self.repo.path().join(head_file));
    }

    /// Reads the index again after something else wrote it (the `git` executable, a second libgit2 handle), so
    /// the next commit is not built from a stale copy.
    fn refresh_index(&self) {
        if let Ok(mut index) = self.repo.index() {
            let _ = index.read(true);
        }
    }

    /// Runs `git` in the working directory.
    fn git<I, S>(&self, args: I, input: &str) -> cli::Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.git_bytes(args, input.as_bytes())
    }

    /// Runs `git` in the working directory with raw bytes on its stdin.
    fn git_bytes<I, S>(&self, args: I, input: &[u8]) -> cli::Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        cli::run(
            &self.workdir,
            args,
            cli::RunOptions {
                input_bytes: Some(input),
                ..cli::RunOptions::default()
            },
        )
    }
}

/// A stash's message as ImGit lists it: git's "On <branch>: " / "WIP on <branch>: " prefix dropped.
#[must_use]
pub fn stash_summary(message: &str) -> &str {
    stash_message_parts(message).map_or(message, |(_, summary)| summary)
}

/// The branch and the summary of a stash message git wrote ("WIP on <branch>: <summary>").
fn stash_message_parts(message: &str) -> Option<(&str, &str)> {
    ["WIP on ", "On "]
        .iter()
        .find_map(|prefix| message.strip_prefix(prefix)?.split_once(": "))
}

/// The first 7 digits of a commit id.
#[must_use]
pub fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// The one-letter badge of a file status.
#[must_use]
pub const fn status_letter(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added | FileStatus::Untracked => "A",
        FileStatus::Modified => "M",
        FileStatus::Deleted => "D",
        FileStatus::Renamed => "R",
        FileStatus::TypeChange => "T",
        FileStatus::Conflicted => "!",
        FileStatus::Unknown => "?",
    }
}

static PROFILE_IDENTITY: RwLock<Option<(String, String)>> = RwLock::new(None);

/// The author / committer of commits in repositories without their own `user.name` / `user.email`: the current
/// profile's. An empty name or email leaves it to the git configuration.
pub fn set_profile_identity(name: &str, email: &str) {
    let identity =
        (!name.is_empty() && !email.is_empty()).then(|| (name.to_owned(), email.to_owned()));
    *PROFILE_IDENTITY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = identity;
}

fn profile_identity() -> Option<(String, String)> {
    PROFILE_IDENTITY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Whether the `git` executable can be run.
fn git_available() -> bool {
    cli::available()
}

fn parse_oid(sha: &str) -> Result<git2::Oid> {
    Ok(git2::Oid::from_str(sha)?)
}

/// HEAD (or whatever was asked for) names nothing yet.
fn is_missing(error: &git2::Error) -> bool {
    matches!(
        error.code(),
        git2::ErrorCode::NotFound | git2::ErrorCode::UnbornBranch
    )
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Text without its line ending.
fn trim_line_end(text: &str) -> &str {
    text.trim_end_matches(['\n', '\r'])
}

/// FNV-1a: `text` folded into `hash`.
fn fnv1a(mut hash: u64, text: &[u8]) -> u64 {
    for &byte in text {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn author_of(signature: &git2::Signature<'_>) -> Author {
    Author {
        name: lossy(signature.name_bytes()),
        email: lossy(signature.email_bytes()),
    }
}

fn read_commit(commit: &git2::Commit<'_>) -> Commit {
    let author = commit.author();
    Commit {
        sha: commit.id().to_string(),
        parents: commit.parent_ids().map(|id| id.to_string()).collect(),
        author: author_of(&author),
        time: commit.time().seconds(),
        author_time: author.when().seconds(),
        summary: commit.summary_bytes().map(lossy).unwrap_or_default(),
        body: commit.body_bytes().map(lossy).unwrap_or_default(),
        node_type: if commit.parent_count() > 1 {
            NodeType::Merge
        } else {
            NodeType::Commit
        },
    }
}

const fn delta_status(status: git2::Delta) -> FileStatus {
    match status {
        git2::Delta::Added => FileStatus::Added,
        git2::Delta::Deleted => FileStatus::Deleted,
        git2::Delta::Modified => FileStatus::Modified,
        git2::Delta::Renamed => FileStatus::Renamed,
        git2::Delta::Typechange => FileStatus::TypeChange,
        git2::Delta::Untracked => FileStatus::Untracked,
        git2::Delta::Conflicted => FileStatus::Conflicted,
        _ => FileStatus::Unknown,
    }
}

/// The path a delta ends at (its old one when the file is gone).
fn delta_path(delta: &git2::DiffDelta<'_>) -> String {
    delta
        .new_file()
        .path_bytes()
        .or_else(|| delta.old_file().path_bytes())
        .map(lossy)
        .unwrap_or_default()
}

fn change_from_delta(delta: &git2::DiffDelta<'_>) -> FileChange {
    let status = delta_status(delta.status());
    FileChange {
        path: delta_path(delta),
        old_path: (status == FileStatus::Renamed)
            .then(|| delta.old_file().path_bytes().map(lossy))
            .flatten(),
        status,
        stats: None,
    }
}

/// A checkout that refuses to overwrite local changes and restores files that went missing.
fn safe_checkout<'a>() -> git2::build::CheckoutBuilder<'a> {
    let mut builder = git2::build::CheckoutBuilder::new();
    builder.safe().recreate_missing(true);
    builder
}

/// A checkout that overwrites whatever is in the way.
fn force_checkout<'a>() -> git2::build::CheckoutBuilder<'a> {
    let mut builder = git2::build::CheckoutBuilder::new();
    builder.force();
    builder
}

/// A safe checkout that writes conflicts into the working directory instead of failing on them.
fn conflict_checkout<'a>() -> git2::build::CheckoutBuilder<'a> {
    let mut builder = git2::build::CheckoutBuilder::new();
    builder.safe().allow_conflicts(true);
    builder
}

/// The distinct ids of `ids`, in first-seen order.
fn unique_oids(ids: impl IntoIterator<Item = git2::Oid>) -> Vec<git2::Oid> {
    let mut seen = HashSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

/// The commit id of a full 40-digit sha, as the graph rows carry it.
fn sha_of(id: git2::Oid) -> Sha {
    id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stash_summary_drops_the_branch_prefix() {
        assert_eq!(
            stash_summary("WIP on main: 1234567 Subject"),
            "1234567 Subject"
        );
        assert_eq!(stash_summary("On feature/x: my stash"), "my stash");
        assert_eq!(stash_summary("custom message"), "custom message");
        assert_eq!(stash_summary("On main"), "On main");
    }

    #[test]
    fn short_sha_takes_seven_digits() {
        assert_eq!(short_sha("0123456789abcdef"), "0123456");
        assert_eq!(short_sha("abc"), "abc");
    }

    #[test]
    fn status_letters() {
        assert_eq!(status_letter(FileStatus::Untracked), "A");
        assert_eq!(status_letter(FileStatus::Conflicted), "!");
        assert_eq!(status_letter(FileStatus::Unknown), "?");
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a(FNV_OFFSET, b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(FNV_OFFSET, b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn line_selection_of_a_diff_line() {
        let line = |origin, old_lineno, new_lineno| DiffLine {
            origin,
            old_lineno,
            new_lineno,
            text: String::new(),
        };
        assert_eq!(
            LineSelection::of(&line(LineOrigin::Added, None, Some(4))),
            Some(LineSelection {
                origin: LineOrigin::Added,
                lineno: 4
            })
        );
        assert_eq!(
            LineSelection::of(&line(LineOrigin::Deleted, Some(2), None)).map(|s| s.lineno),
            Some(2)
        );
        assert_eq!(
            LineSelection::of(&line(LineOrigin::Context, Some(1), Some(1))),
            None
        );
    }
}
