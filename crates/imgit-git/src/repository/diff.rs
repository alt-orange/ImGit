//! Diffs: what a commit changed, the working directory's changes, one file's hunks, line statistics.

use std::collections::HashMap;

use super::{
    DiffOptions, Error, Repository, Result, change_from_delta, delta_path, lossy, trim_line_end,
};
use crate::types::{
    DiffHunk, DiffLine, FileChange, FileDiff, LineOrigin, LineStats, Revision, StatsSource,
};

impl Repository {
    /// The files a commit changed against its first parent (against nothing for a root commit), renames
    /// detected.
    pub fn commit_changes(&self, sha: &str) -> Result<Vec<FileChange>> {
        let commit = self.lookup_commit(sha)?;
        let tree = commit.tree()?;
        let parent_tree = first_parent_tree(&commit)?;
        let mut diff = self
            .repo
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let _ = diff.find_similar(None);
        Ok(changes_of(&diff))
    }

    /// Every file in the commit's tree (blobs and submodule links), sorted by path.
    pub fn commit_tree_paths(&self, sha: &str) -> Result<Vec<String>> {
        let tree = self.lookup_commit(sha)?.tree()?;
        let mut paths = Vec::new();
        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            if matches!(
                entry.kind(),
                Some(git2::ObjectType::Blob | git2::ObjectType::Commit)
            ) {
                paths.push(format!("{root}{}", lossy(entry.name_bytes())));
            }
            git2::TreeWalkResult::Ok
        })?;
        paths.sort();
        Ok(paths)
    }

    /// One file's diff in a commit, against the commit's first parent.
    pub fn commit_file_diff(
        &self,
        sha: &str,
        path: &str,
        options: DiffOptions,
    ) -> Result<FileDiff> {
        let repo = &self.repo;
        let commit = self.lookup_commit(sha)?;
        let tree = commit.tree()?;
        let parent_tree = first_parent_tree(&commit)?;
        let mut file_options = file_diff_options(path, options);
        let diff =
            repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut file_options))?;
        let added = diff
            .get_delta(0)
            .is_some_and(|delta| delta.status() == git2::Delta::Added);
        if diff.deltas().len() == 1 && !added {
            return file_diff(&diff, 0, path);
        }
        // An added file may really be the new side of a rename: re-run over the whole tree with similarity
        // detection and pick the delta that ends at `path`.
        let mut tree_options = base_diff_options(options);
        let mut full =
            repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut tree_options))?;
        let _ = full.find_similar(None);
        let renamed = full
            .deltas()
            .position(|delta| delta.new_file().path_bytes() == Some(path.as_bytes()));
        match renamed {
            Some(index) => file_diff(&full, index, path),
            None => file_diff(&diff, 0, path),
        }
    }

    /// One file's diff in the working directory: HEAD against the index (`staged`), or the index against the
    /// files on disk (an untracked file shows as all added).
    pub fn workdir_file_diff(
        &self,
        path: &str,
        staged: bool,
        options: DiffOptions,
    ) -> Result<FileDiff> {
        let mut file_options = file_diff_options(path, options);
        file_options
            .include_untracked(true)
            .show_untracked_content(true)
            .recurse_untracked_dirs(true);
        let diff = self.workdir_diff(staged, &mut file_options)?;
        file_diff(&diff, 0, path)
    }

    /// Commit `sha` against the working directory (staged and unstaged changes, untracked files included): the
    /// files that differ.
    pub fn compare_changes(&self, sha: &str) -> Result<Vec<FileChange>> {
        let tree = self.lookup_commit(sha)?.tree()?;
        let mut options = git2::DiffOptions::new();
        options.include_untracked(true).recurse_untracked_dirs(true);
        let mut diff = self
            .repo
            .diff_tree_to_workdir_with_index(Some(&tree), Some(&mut options))?;
        let _ = diff.find_similar(None);
        Ok(changes_of(&diff))
    }

    /// Commit `sha` against the working directory: one file's diff.
    pub fn compare_file_diff(
        &self,
        sha: &str,
        path: &str,
        options: DiffOptions,
    ) -> Result<FileDiff> {
        let tree = self.lookup_commit(sha)?.tree()?;
        let mut file_options = file_diff_options(path, options);
        file_options
            .include_untracked(true)
            .show_untracked_content(true);
        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(Some(&tree), Some(&mut file_options))?;
        file_diff(&diff, 0, path)
    }

    /// Fills the `stats` of `changes` (matched by path) from the given diff source; `sha` is the commit of
    /// [`StatsSource::Commit`] and is not read otherwise. A file the diff does not have, or a binary one, gets
    /// zeros.
    pub fn fill_stats(
        &self,
        changes: &mut [FileChange],
        source: StatsSource,
        sha: Option<&str>,
    ) -> Result<()> {
        let repo = &self.repo;
        let mut options = git2::DiffOptions::new();
        options
            .include_untracked(true)
            .show_untracked_content(true)
            .recurse_untracked_dirs(true);
        let diff = match source {
            StatsSource::Commit => {
                let sha = sha.ok_or_else(|| Error::message("no commit to read the changes of"))?;
                let commit = self.lookup_commit(sha)?;
                let tree = commit.tree()?;
                let parent_tree = first_parent_tree(&commit)?;
                let mut diff =
                    repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut options))?;
                let _ = diff.find_similar(None);
                diff
            }
            StatsSource::Staged => {
                let mut diff = self.workdir_diff(true, &mut options)?;
                let _ = diff.find_similar(None);
                diff
            }
            StatsSource::Unstaged => self.workdir_diff(false, &mut options)?,
        };

        let mut by_path: HashMap<String, LineStats> = HashMap::new();
        for (index, delta) in diff.deltas().enumerate() {
            if delta.flags().contains(git2::DiffFlags::BINARY) {
                continue;
            }
            let Ok(Some(patch)) = git2::Patch::from_diff(&diff, index) else {
                continue;
            };
            let (_, additions, deletions) = patch.line_stats().unwrap_or_default();
            by_path.insert(
                delta_path(&delta),
                LineStats {
                    additions,
                    deletions,
                },
            );
        }
        for change in changes {
            change.stats = Some(by_path.get(&change.path).copied().unwrap_or_default());
        }
        Ok(())
    }

    /// HEAD against the index (`staged`; everything is new on an unborn branch) or the index against the
    /// working directory.
    pub(super) fn workdir_diff(
        &self,
        staged: bool,
        options: &mut git2::DiffOptions,
    ) -> Result<git2::Diff<'_>> {
        let index = self.repo.index()?;
        Ok(if staged {
            let head_tree = self.head_tree()?;
            self.repo
                .diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(options))?
        } else {
            self.repo
                .diff_index_to_workdir(Some(&index), Some(options))?
        })
    }
}

fn changes_of(diff: &git2::Diff<'_>) -> Vec<FileChange> {
    diff.deltas()
        .map(|delta| change_from_delta(&delta))
        .collect()
}

/// The tree a commit's changes are read against: its first parent's, none for a root commit.
fn first_parent_tree<'repo>(commit: &git2::Commit<'repo>) -> Result<Option<git2::Tree<'repo>>> {
    if commit.parent_count() == 0 {
        return Ok(None);
    }
    Ok(Some(commit.parent(0)?.tree()?))
}

fn base_diff_options(options: DiffOptions) -> git2::DiffOptions {
    let mut out = git2::DiffOptions::new();
    out.context_lines(options.context_lines)
        .ignore_whitespace(options.ignore_whitespace);
    out
}

/// Options for the diff of exactly `path` (a literal path, not a pattern: `[ ] * ?` may be part of a name).
pub(super) fn file_diff_options(path: &str, options: DiffOptions) -> git2::DiffOptions {
    let mut out = base_diff_options(options);
    out.pathspec(path).disable_pathspec_match(true);
    out
}

/// The hunks of the diff's delta `index`; an empty diff of `path` when the diff has no such delta.
fn file_diff(diff: &git2::Diff<'_>, index: usize, path: &str) -> Result<FileDiff> {
    let mut result = FileDiff {
        revision: Some(Revision::next()),
        path: path.to_owned(),
        ..FileDiff::default()
    };
    if index >= diff.deltas().len() {
        return Ok(result);
    }
    let Some(patch) = git2::Patch::from_diff(diff, index)? else {
        return Ok(result);
    };
    if patch.delta().flags().contains(git2::DiffFlags::BINARY) {
        result.binary = true;
        return Ok(result);
    }
    let (_, additions, deletions) = patch.line_stats().unwrap_or_default();
    result.stats = LineStats {
        additions,
        deletions,
    };
    for hunk_index in 0..patch.num_hunks() {
        let (hunk, line_count) = patch.hunk(hunk_index)?;
        let mut out = DiffHunk {
            header: trim_line_end(&lossy(hunk.header())).to_owned(),
            lines: Vec::with_capacity(line_count),
        };
        for line_index in 0..line_count {
            let line = patch.line_in_hunk(hunk_index, line_index)?;
            let origin = match line.origin_value() {
                git2::DiffLineType::Context => LineOrigin::Context,
                git2::DiffLineType::Addition => LineOrigin::Added,
                git2::DiffLineType::Deletion => LineOrigin::Deleted,
                _ => continue,
            };
            out.lines.push(DiffLine {
                origin,
                old_lineno: line.old_lineno(),
                new_lineno: line.new_lineno(),
                text: trim_line_end(&lossy(line.content())).to_owned(),
            });
        }
        result.hunks.push(out);
    }
    Ok(result)
}

/// A changed range between two texts, in 0-based lines: `old_count` lines at `old_start` became `new_count`
/// lines at `new_start` (a count of 0 is an insertion / deletion point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LineHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
}

/// The line hunks (no context) turning `old_text` into `new_text`, in order.
#[must_use]
pub fn diff_lines(old_text: &str, new_text: &str) -> Vec<LineHunk> {
    let mut options = git2::DiffOptions::new();
    options.context_lines(0).interhunk_lines(0).force_text(true);
    let Ok(patch) = git2::Patch::from_buffers(
        old_text.as_bytes(),
        None,
        new_text.as_bytes(),
        None,
        Some(&mut options),
    ) else {
        return Vec::new();
    };
    // libgit2's starts are 1-based; a zero count names the line the change comes after.
    let start = |start: u32, count: u32| (if count == 0 { start } else { start - 1 }) as usize;
    (0..patch.num_hunks())
        .filter_map(|index| patch.hunk(index).ok())
        .map(|(hunk, _)| LineHunk {
            old_start: start(hunk.old_start(), hunk.old_lines()),
            old_count: hunk.old_lines() as usize,
            new_start: start(hunk.new_start(), hunk.new_lines()),
            new_count: hunk.new_lines() as usize,
        })
        .collect()
}
