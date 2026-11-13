//! The plain data the Git layer hands to the rest of the app.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

/// A commit id as 40 hex digits. Kept as text because graph rows also carry ids that are not objects (the
/// working-directory row).
pub type Sha = String;

/// The graph node kinds: a plain commit, a merge (2+ parents), a stash entry and the uncommitted working
/// directory (drawn as the "// WIP" row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NodeType {
    #[default]
    Commit,
    Merge,
    Stash,
    WorkDir,
}

impl NodeType {
    /// Dashed lines: edges that belong to the working directory or a stash.
    #[must_use]
    pub const fn is_dashed(self) -> bool {
        matches!(self, Self::WorkDir | Self::Stash)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RefType {
    #[default]
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Author {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ref {
    pub ref_type: RefType,
    /// "main", "origin/main", "v1.0", "stash@{0}"
    pub name: String,
    /// "refs/heads/main"
    pub full_name: String,
    /// Remote branches only: "origin".
    pub remote: String,
    pub target: Sha,
    /// The checked-out branch.
    pub is_head: bool,
    /// Local branches: "origin/main" when tracking.
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    /// Tags.
    pub annotated: bool,
    /// Stashes.
    pub stash_index: Option<usize>,
    /// Stash / annotated tag message.
    pub message: String,
    /// Not pushed to the graph walk.
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Commit {
    pub sha: Sha,
    pub parents: Vec<Sha>,
    pub author: Author,
    /// Unix seconds, the committer's.
    pub time: i64,
    /// The author's (kept by an amend or rebase).
    pub author_time: i64,
    pub summary: String,
    pub body: String,
    pub node_type: NodeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Submodule {
    pub name: String,
    pub path: String,
    pub url: String,
    /// The commit the repository records for it (the index's, else HEAD's).
    pub recorded: Option<Sha>,
    /// The commit its working directory has checked out; `None` when it is not initialized.
    pub checked_out: Option<Sha>,
}

impl Submodule {
    /// Checked out at the commit the repository records.
    #[must_use]
    pub fn in_sync(&self) -> bool {
        self.checked_out.is_some() && self.checked_out == self.recorded
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Worktree {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    /// Topological, newest first; may contain a `WorkDir` row and `Stash` rows.
    pub commits: Vec<Commit>,
    pub refs: Vec<Ref>,
    /// Indices into `refs`.
    pub refs_by_sha: HashMap<Sha, Vec<usize>>,
    /// Indices into `commits`.
    pub row_by_sha: HashMap<Sha, usize>,
    pub remotes: Vec<Remote>,
    pub submodules: Vec<Submodule>,
    pub worktrees: Vec<Worktree>,
    /// `None` when detached or unborn.
    pub head_branch: Option<String>,
    pub head_sha: Option<Sha>,
    pub detached: bool,
    pub unborn: bool,
    /// The commit limit cut the walk short.
    pub has_more: bool,
}

impl History {
    /// The refs pointing at `sha`.
    pub fn refs_at<'a>(&'a self, sha: &str) -> impl Iterator<Item = &'a Ref> {
        self.refs_by_sha
            .get(sha)
            .into_iter()
            .flatten()
            .map(|&index| &self.refs[index])
    }

    #[must_use]
    pub fn commit(&self, sha: &str) -> Option<&Commit> {
        self.row_by_sha.get(sha).map(|&row| &self.commits[row])
    }
}

/// What to walk: everything but the hidden refs, or only the soloed one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryOptions {
    pub max_commits: usize,
    /// Full names.
    pub hidden_refs: HashSet<String>,
    /// Full name.
    pub solo_ref: Option<String>,
    /// Walk only HEAD's branch, its upstream and the default branch (origin/HEAD's target, else main / master,
    /// locally and on the remote); a solo ref wins.
    pub smart_visibility: bool,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            max_commits: 2000,
            hidden_refs: HashSet::new(),
            solo_ref: None,
            smart_visibility: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChange,
    Untracked,
    Conflicted,
    #[default]
    Unknown,
}

/// Lines added and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineStats {
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileChange {
    pub path: String,
    /// Renames.
    pub old_path: Option<String>,
    pub status: FileStatus,
    /// `None` until they have been counted.
    pub stats: Option<LineStats>,
}

/// One repository as the Repository Management lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoSummary {
    /// `None` when HEAD is detached or unborn.
    pub branch: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    /// Working directory files, staged or not.
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
    /// Origin's owner on its hosting service ("github.com/<owner>/...").
    pub owner: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkDirStatus {
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
}

impl WorkDirStatus {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.conflicted.is_empty()
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.staged.len() + self.unstaged.len() + self.conflicted.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatsSource {
    Commit,
    Staged,
    Unstaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineOrigin {
    #[default]
    Context,
    Added,
    Deleted,
}

impl LineOrigin {
    /// The line's prefix in a unified diff.
    #[must_use]
    pub const fn as_char(self) -> char {
        match self {
            Self::Context => ' ',
            Self::Added => '+',
            Self::Deleted => '-',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffLine {
    pub origin: LineOrigin,
    /// 1-based; `None` for an added line.
    pub old_lineno: Option<u32>,
    /// 1-based; `None` for a deleted line.
    pub new_lineno: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffHunk {
    /// "@@ -1,4 +1,6 @@ ..."
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// Identifies one loaded text: the views key the work they cache on it. No two loads share one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revision(u64);

impl Revision {
    /// A revision no text loaded before has had.
    #[must_use]
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// The revision as a number (never 0), for caches keyed outside this crate.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    /// `Revision::next()` when loaded; `None` for one built by hand.
    pub revision: Option<Revision>,
    pub path: String,
    pub hunks: Vec<DiffHunk>,
    pub binary: bool,
    pub stats: LineStats,
}

/// One commit of a file's history and what it did to the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistoryEntry {
    pub commit: Commit,
    /// `Added` / `Modified` / `Deleted` / `Renamed`.
    pub status: FileStatus,
    /// The file's path at this commit.
    pub path: String,
    /// `Renamed`: its path in the parent.
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileHistory {
    /// Newest first.
    pub entries: Vec<FileHistoryEntry>,
    /// The walk stopped at its limit.
    pub has_more: bool,
}

/// One run of lines a blame attributes to the same commit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameHunk {
    /// 1-based line in the blamed revision.
    pub start_line: u32,
    pub line_count: u32,
    pub sha: Sha,
    pub author: Author,
    /// The author's.
    pub time: i64,
    pub summary: String,
    /// The file's path in that commit.
    pub orig_path: String,
    pub orig_start_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Blame {
    /// In line order.
    pub hunks: Vec<BlameHunk>,
}

impl Blame {
    /// The hunk holding `line` (1-based), `None` past the end.
    #[must_use]
    pub fn hunk_at(&self, line: u32) -> Option<&BlameHunk> {
        let after = self.hunks.partition_point(|hunk| hunk.start_line <= line);
        let hunk = self.hunks.get(after.checked_sub(1)?)?;
        (line < hunk.start_line + hunk.line_count).then_some(hunk)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ResetMode {
    Soft,
    #[default]
    Mixed,
    Hard,
}

/// The operation a repository is in the middle of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RepoState {
    #[default]
    None,
    Merge,
    Revert,
    CherryPick,
    Rebase,
    Other,
}

/// One row of an interactive rebase plan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RebaseAction {
    #[default]
    Pick,
    /// The new message; `None` keeps the original.
    Reword(Option<String>),
    /// The combined commit's message; `None` keeps the original.
    Squash(Option<String>),
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseStep {
    pub sha: Sha,
    pub action: RebaseAction,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blame_hunk_at_finds_the_covering_hunk() {
        let hunk = |start_line, line_count| BlameHunk {
            start_line,
            line_count,
            ..BlameHunk::default()
        };
        let blame = Blame {
            hunks: vec![hunk(1, 3), hunk(4, 2)],
        };
        assert_eq!(blame.hunk_at(0), None);
        assert_eq!(blame.hunk_at(3).map(|h| h.start_line), Some(1));
        assert_eq!(blame.hunk_at(4).map(|h| h.start_line), Some(4));
        assert_eq!(blame.hunk_at(6), None);
    }

    #[test]
    fn revisions_are_unique() {
        assert_ne!(Revision::next(), Revision::next());
    }
}
