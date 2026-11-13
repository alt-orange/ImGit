//! Stashes.

use super::{
    Error, Repository, Result, git_available, lossy, parse_oid, sha_of, stash_message_parts,
};
use crate::types::Sha;

impl Repository {
    /// Stashes the working directory's changes, untracked files included, and returns the new stash's commit
    /// (`stash@{0}`). An empty `message` leaves git's "WIP on <branch>: ..." one.
    pub fn stash_push(&self, message: &str) -> Result<Sha> {
        let stasher = self.default_signature()?;
        let message = (!message.is_empty()).then_some(message);
        let id = self.with_stash_handle(|repo| {
            repo.stash_save2(&stasher, message, Some(git2::StashFlags::INCLUDE_UNTRACKED))
        })?;
        Ok(sha_of(id))
    }

    /// The current index of the stash with that commit sha, `None` when it is gone.
    #[must_use]
    pub fn stash_index_of(&self, sha: &str) -> Option<usize> {
        let id = parse_oid(sha).ok()?;
        let entries = self.stash_entries().ok()?;
        entries.iter().position(|(stash, _)| *stash == id)
    }

    /// Applies a stash, keeping it in the list.
    pub fn stash_apply(&self, index: usize) -> Result<()> {
        let applied = self.with_stash_handle(|repo| {
            repo.stash_apply(
                index,
                Some(git2::StashApplyOptions::new().reinstantiate_index()),
            )
        });
        match applied {
            Ok(()) => Ok(()),
            Err(_) => self.stash_apply_cli("apply", index, "could not apply stash"),
        }
    }

    /// Applies a stash and drops it from the list.
    pub fn stash_pop(&self, index: usize) -> Result<()> {
        let popped = self.with_stash_handle(|repo| {
            repo.stash_pop(
                index,
                Some(git2::StashApplyOptions::new().reinstantiate_index()),
            )
        });
        match popped {
            Ok(()) => Ok(()),
            Err(_) => self.stash_apply_cli("pop", index, "could not pop stash"),
        }
    }

    /// Drops a stash from the list.
    pub fn stash_drop(&self, index: usize) -> Result<()> {
        self.with_stash_handle(|repo| repo.stash_drop(index))
    }

    /// Rewrites the message of the stash with that commit sha (its stash reflog entry; its place in the list
    /// stays). `message` is stored as git's "On <branch>: <message>".
    pub fn stash_set_message(&self, sha: &str, message: &str) -> Result<()> {
        let mut reflog = self.repo.reflog("refs/stash")?;
        // The entries newest first, as they are listed; the reflog is rebuilt oldest first with the one
        // message changed (an append adds at the top), keeping each entry's place.
        let mut found = false;
        let entries: Vec<(git2::Oid, git2::Signature<'static>, String)> = reflog
            .iter()
            .map(|entry| {
                let id = entry.id_new();
                let mut text = entry.message_bytes().map(lossy).unwrap_or_default();
                if sha_of(id) == sha {
                    // Keep git's "On <branch>: " (a "WIP on <branch>: " stash becomes "On <branch>: ").
                    text = match stash_message_parts(&text) {
                        Some((branch, _)) if !branch.is_empty() => {
                            format!("On {branch}: {message}")
                        }
                        _ => message.to_owned(),
                    };
                    found = true;
                }
                (id, entry.committer().to_owned(), text)
            })
            .collect();
        if !found {
            return Err(Error::message("the stash is gone"));
        }
        while !reflog.is_empty() {
            reflog.remove(0, false)?;
        }
        for (id, committer, text) in entries.iter().rev() {
            reflog.append(*id, committer, Some(text))?;
        }
        reflog.write()?;
        Ok(())
    }

    /// Runs a stash operation on a second handle of the repository: git2 wants exclusive access for them,
    /// which `&self` cannot give. The index is read again afterwards, since the other handle rewrote it.
    fn with_stash_handle<T>(
        &self,
        operation: impl FnOnce(&mut git2::Repository) -> std::result::Result<T, git2::Error>,
    ) -> Result<T> {
        let mut handle = git2::Repository::open(self.repo.path())?;
        let result = operation(&mut handle);
        drop(handle);
        self.refresh_index();
        Ok(result?)
    }

    /// libgit2 refuses to apply a stash over staged changes; git applies it when they do not overlap,
    /// reinstating the stashed index when it can.
    fn stash_apply_cli(&self, verb: &str, index: usize, fallback: &str) -> Result<()> {
        if !git_available() {
            return Err(Error::message(fallback));
        }
        let stash = format!("stash@{{{index}}}");
        let mut output = self.git(["stash", verb, "--index", &stash], "");
        if !output.success() {
            output = self.git(["stash", verb, &stash], "");
        }
        self.refresh_index();
        if !output.success() {
            return Err(Error::cli(&output, fallback));
        }
        Ok(())
    }
}
