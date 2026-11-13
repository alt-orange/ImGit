//! The working directory's status and the one-line summary of a repository.

use std::collections::BTreeMap;

use super::{Repository, Result, change_from_delta, lossy};
use crate::types::{FileStatus, RepoSummary, WorkDirStatus};

impl Repository {
    /// The staged, unstaged (untracked files included, each one listed) and conflicted files, renames detected
    /// on both sides.
    pub fn load_status(&self) -> Result<WorkDirStatus> {
        use git2::Status;
        let mut options = git2::StatusOptions::new();
        options
            .show(git2::StatusShow::IndexAndWorkdir)
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true)
            .sort_case_insensitively(true);
        let statuses = self.repo.statuses(Some(&mut options))?;

        let staged_flags = Status::INDEX_NEW
            | Status::INDEX_MODIFIED
            | Status::INDEX_DELETED
            | Status::INDEX_RENAMED
            | Status::INDEX_TYPECHANGE;
        let unstaged_flags = Status::WT_NEW
            | Status::WT_MODIFIED
            | Status::WT_DELETED
            | Status::WT_RENAMED
            | Status::WT_TYPECHANGE;

        let mut status = WorkDirStatus::default();
        for entry in statuses.iter() {
            let flags = entry.status();
            if flags.is_empty() {
                continue;
            }
            if flags.contains(Status::CONFLICTED) {
                if let Some(delta) = entry.index_to_workdir().or_else(|| entry.head_to_index()) {
                    let mut change = change_from_delta(&delta);
                    change.status = FileStatus::Conflicted;
                    status.conflicted.push(change);
                }
                continue;
            }
            if let Some(delta) = entry.head_to_index()
                && flags.intersects(staged_flags)
            {
                status.staged.push(change_from_delta(&delta));
            }
            if let Some(delta) = entry.index_to_workdir()
                && flags.intersects(unstaged_flags)
            {
                let mut change = change_from_delta(&delta);
                if flags.contains(Status::WT_NEW) {
                    change.status = FileStatus::Untracked;
                }
                status.unstaged.push(change);
            }
        }
        Ok(status)
    }

    /// The repository as the Repository Management lists it: its branch, how far it is from its upstream, the
    /// files changed in the working directory (each counted once, staged or not) and origin's owner.
    pub fn summary(&self) -> Result<RepoSummary> {
        let mut summary = RepoSummary::default();
        let repo = &self.repo;
        if let Ok(head) = repo.head()
            && head.is_branch()
        {
            let local = head.target();
            let branch = git2::Branch::wrap(head);
            summary.branch = branch.name_bytes().ok().map(lossy);
            if let Ok(upstream) = branch.upstream()
                && let (Some(local), Some(remote)) = (local, upstream.get().target())
                && let Ok((ahead, behind)) = repo.graph_ahead_behind(local, remote)
            {
                summary.ahead = ahead;
                summary.behind = behind;
            }
        }
        if let Ok(status) = self.load_status() {
            // A file both staged and changed again counts once; added and deleted win over modified.
            let mut files: BTreeMap<&str, FileStatus> = BTreeMap::new();
            let changes = status
                .staged
                .iter()
                .chain(&status.unstaged)
                .chain(&status.conflicted);
            for change in changes {
                let known = files.entry(&change.path).or_insert(change.status);
                if matches!(
                    change.status,
                    FileStatus::Added | FileStatus::Untracked | FileStatus::Deleted
                ) {
                    *known = change.status;
                }
            }
            for file_status in files.values() {
                match file_status {
                    FileStatus::Added | FileStatus::Untracked => summary.added += 1,
                    FileStatus::Deleted => summary.deleted += 1,
                    _ => summary.modified += 1,
                }
            }
        }
        if let Ok(origin) = repo.find_remote("origin") {
            summary.owner = remote_owner(&lossy(origin.url_bytes()))
                .unwrap_or_default()
                .to_owned();
        }
        Ok(summary)
    }
}

/// The owner in `https://host/owner/repo(.git)` or `git@host:owner/repo(.git)`.
fn remote_owner(url: &str) -> Option<&str> {
    let start = match url.find("://") {
        Some(scheme) => scheme + 3 + url[scheme + 3..].find('/')?,
        None => url.find(':')?,
    };
    let rest = &url[start + 1..];
    Some(&rest[..rest.find('/')?])
}

#[cfg(test)]
mod tests {
    use super::remote_owner;

    #[test]
    fn owner_of_a_remote_url() {
        assert_eq!(
            remote_owner("https://github.com/owner/repo.git"),
            Some("owner")
        );
        assert_eq!(remote_owner("git@github.com:owner/repo.git"), Some("owner"));
        assert_eq!(remote_owner("ssh://git@host:22/team/repo"), Some("team"));
        assert_eq!(remote_owner("https://host/repo"), None);
        assert_eq!(remote_owner("C:/somewhere/repo"), Some(""));
        assert_eq!(remote_owner("nothing"), None);
    }
}
