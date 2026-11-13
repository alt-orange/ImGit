//! Branches, tags and remotes.

use super::{Repository, Result, lossy, parse_oid, safe_checkout};

impl Repository {
    /// Creates a local branch at `sha` and, `checkout`, switches to it.
    pub fn create_branch(&self, name: &str, sha: &str, checkout: bool) -> Result<()> {
        let commit = self.lookup_commit(sha)?;
        self.repo.branch(name, &commit, false)?;
        if checkout {
            self.checkout_branch(name)?;
        }
        Ok(())
    }

    /// Points (or re-creates) a local branch at `sha` without touching the working directory.
    pub fn set_branch_target(&self, name: &str, sha: &str) -> Result<()> {
        let id = parse_oid(sha)?;
        if let Ok(branch) = self.repo.find_branch(name, git2::BranchType::Local) {
            branch
                .into_reference()
                .set_target(id, "ImGit: move branch")?;
            return Ok(());
        }
        self.repo.branch(name, &self.repo.find_commit(id)?, false)?;
        Ok(())
    }

    /// Switches to a local branch: its tree is checked out (local changes that are in the way make it fail,
    /// the others are kept), then HEAD is pointed at it.
    pub fn checkout_branch(&self, name: &str) -> Result<()> {
        let repo = &self.repo;
        let branch = repo.find_branch(name, git2::BranchType::Local)?;
        let reference = branch.get();
        let target = reference.peel(git2::ObjectType::Commit)?;
        repo.checkout_tree(&target, Some(&mut safe_checkout()))?;
        repo.set_head_bytes(reference.name_bytes())?;
        Ok(())
    }

    /// "origin/feature" -> local "feature" tracking it, checked out.
    pub fn checkout_remote_branch(&self, remote: &str, name: &str) -> Result<()> {
        let repo = &self.repo;
        let remote_name = format!("{remote}/{name}");
        let remote_branch = repo.find_branch(&remote_name, git2::BranchType::Remote)?;
        match repo.find_branch(name, git2::BranchType::Local) {
            Err(_) => {
                let commit = remote_branch.get().peel_to_commit()?;
                let mut local = repo.branch(name, &commit, false)?;
                local.set_upstream(Some(&remote_name))?;
            }
            Ok(local) => {
                // A local branch behind the remote one is fast-forwarded, so HEAD lands on the commit that was
                // clicked. The tree is checked out first: a conflict with the working directory then leaves
                // the branch alone.
                if let (Some(local_id), Some(remote_id)) =
                    (local.get().target(), remote_branch.get().target())
                    && local_id != remote_id
                    && repo.graph_descendant_of(remote_id, local_id) == Ok(true)
                {
                    let target = repo.find_object(remote_id, Some(git2::ObjectType::Commit))?;
                    repo.checkout_tree(&target, Some(&mut safe_checkout()))?;
                    let moved = local
                        .into_reference()
                        .set_target(remote_id, "checkout: fast-forward")?;
                    repo.set_head_bytes(moved.name_bytes())?;
                    return Ok(());
                }
            }
        }
        self.checkout_branch(name)
    }

    /// Checks out a commit, leaving HEAD detached on it.
    pub fn checkout_commit(&self, sha: &str) -> Result<()> {
        let commit = self.lookup_commit(sha)?;
        self.repo
            .checkout_tree(commit.as_object(), Some(&mut safe_checkout()))?;
        self.repo.set_head_detached(commit.id())?;
        Ok(())
    }

    /// Deletes a local branch (libgit2 refuses for the one checked out).
    pub fn delete_branch(&self, name: &str) -> Result<()> {
        self.repo
            .find_branch(name, git2::BranchType::Local)?
            .delete()?;
        Ok(())
    }

    /// The repository's default branch: what `remote`'s HEAD points at (usually "origin"), else gitflow's main
    /// branch, else a local main or master; `None` when there is none.
    #[must_use]
    pub fn default_branch(&self, remote: &str) -> Option<String> {
        let exists = |name: &str| {
            self.repo
                .find_reference(&format!("refs/heads/{name}"))
                .is_ok()
        };
        let prefix = format!("refs/remotes/{remote}/");
        let remote_default = self
            .repo
            .find_reference(&format!("{prefix}HEAD"))
            .ok()
            .and_then(|head| {
                let symbolic = lossy(head.symbolic_target_bytes()?);
                symbolic.strip_prefix(&prefix).map(str::to_owned)
            });
        remote_default
            .filter(|name| exists(name))
            .or_else(|| {
                self.config_string("gitflow.branch.master")
                    .filter(|name| exists(name))
            })
            .or_else(|| {
                ["main", "master"]
                    .into_iter()
                    .find(|name| exists(name))
                    .map(str::to_owned)
            })
    }

    /// Whether the local branch tracks an upstream that no longer exists (git's "[gone]" after a pruning
    /// fetch).
    #[must_use]
    pub fn upstream_gone(&self, name: &str) -> bool {
        // The upstream's name comes from branch.<name>.remote / .merge alone; the ref itself may be gone.
        let Ok(upstream) = self
            .repo
            .branch_upstream_name(&format!("refs/heads/{name}"))
        else {
            return false;
        };
        self.repo
            .find_reference(&lossy(&upstream))
            .is_err_and(|error| error.code() == git2::ErrorCode::NotFound)
    }

    /// Whether the local branch is checked out here or in a linked worktree.
    #[must_use]
    pub fn branch_checked_out(&self, name: &str) -> bool {
        let repo = &self.repo;
        let full_name = format!("refs/heads/{name}");
        if repo.find_reference(&full_name).is_err() {
            return false;
        }
        // git2 does not expose libgit2's own check, which does this: HEAD of the main working directory and
        // of every linked worktree is compared with the branch.
        let has_it_checked_out = |worktree: &git2::Repository| {
            !worktree.is_bare()
                && worktree
                    .find_reference("HEAD")
                    .is_ok_and(|head| head.symbolic_target_bytes() == Some(full_name.as_bytes()))
        };
        if has_it_checked_out(repo) {
            return true;
        }
        if git2::Repository::open(repo.commondir()).is_ok_and(|main| has_it_checked_out(&main)) {
            return true;
        }
        let Ok(names) = repo.worktrees() else {
            return false;
        };
        names.iter_bytes().any(|worktree| {
            repo.find_worktree(&lossy(worktree))
                .and_then(|worktree| git2::Repository::open_from_worktree(&worktree))
                .is_ok_and(|worktree| has_it_checked_out(&worktree))
        })
    }

    /// Renames a local branch (its upstream configuration follows); fails when `new_name` is taken.
    pub fn rename_branch(&self, name: &str, new_name: &str) -> Result<()> {
        self.repo
            .find_branch(name, git2::BranchType::Local)?
            .rename(new_name, false)?;
        Ok(())
    }

    /// Makes the local branch track `upstream` ("origin/main"); `None` stops it tracking anything.
    pub fn set_upstream(&self, name: &str, upstream: Option<&str>) -> Result<()> {
        self.repo
            .find_branch(name, git2::BranchType::Local)?
            .set_upstream(upstream.filter(|upstream| !upstream.is_empty()))?;
        Ok(())
    }

    /// Tags a commit: an annotated tag when there is a `message`, a lightweight one otherwise.
    pub fn create_tag(&self, name: &str, sha: &str, message: Option<&str>) -> Result<()> {
        let commit = self.lookup_commit(sha)?;
        match message.filter(|message| !message.is_empty()) {
            None => self.repo.tag_lightweight(name, commit.as_object(), false)?,
            Some(message) => {
                let tagger = self.default_signature()?;
                self.repo
                    .tag(name, commit.as_object(), &tagger, message, false)?
            }
        };
        Ok(())
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> {
        Ok(self.repo.tag_delete(name)?)
    }

    pub fn add_remote(&self, name: &str, url: &str) -> Result<()> {
        self.repo.remote(name, url)?;
        Ok(())
    }

    pub fn set_remote_url(&self, name: &str, url: &str) -> Result<()> {
        Ok(self.repo.remote_set_url(name, url)?)
    }

    /// Removes a remote with its remote-tracking branches and configuration.
    pub fn remove_remote(&self, name: &str) -> Result<()> {
        Ok(self.repo.remote_delete(name)?)
    }
}
