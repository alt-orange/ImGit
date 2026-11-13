//! The index and commits: staging, discarding, committing.

use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::Path;

use super::diff::file_diff_options;
use super::{
    CommitOptions, DiffOptions, Error, LineSelection, Repository, Result, force_checkout,
    git_available, lossy, sha_of, trim_line_end,
};
use crate::types::{FileChange, FileStatus, LineOrigin, Sha};

impl Repository {
    /// Stages the files as they are on disk; a file that is gone is staged as a deletion.
    pub fn stage<S: AsRef<str>>(&self, paths: &[S]) -> Result<()> {
        let mut index = self.repo.index()?;
        for path in paths {
            let path = path.as_ref();
            // symlink_metadata: a dangling symlink is still a file to stage.
            match std::fs::symlink_metadata(self.workdir.join(path)) {
                Ok(_) => index.add_path(Path::new(path))?,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    index.remove_path(Path::new(path))?;
                }
                Err(source) => {
                    return Err(Error::Io {
                        path: path.to_owned(),
                        source,
                    });
                }
            }
        }
        index.write()?;
        Ok(())
    }

    /// Stages every change of the working directory: new files, modifications and deletions.
    pub fn stage_all(&self) -> Result<()> {
        let mut index = self.repo.index()?;
        let everything = std::iter::empty::<&str>;
        index.add_all(everything(), git2::IndexAddOption::DEFAULT, None)?;
        index.update_all(everything(), None)?;
        index.write()?;
        Ok(())
    }

    /// Puts each path back to HEAD's entry in the index, or takes it out when HEAD does not have it. The
    /// working directory is not touched.
    pub fn unstage<S: AsRef<str>>(&self, paths: &[S]) -> Result<()> {
        let mut index = self.repo.index()?;
        let Some(tree) = self.head_tree()? else {
            for path in paths {
                let _ = index.remove_path(Path::new(path.as_ref()));
            }
            index.write()?;
            return Ok(());
        };
        // By exact path: libgit2's reset reads the paths as wildmatch patterns ([ ] * ?).
        for path in paths {
            let path = path.as_ref();
            ignore_not_found(index.conflict_remove(Path::new(path)))?;
            match tree.get_path(Path::new(path)) {
                Ok(entry) => index.add(&git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: u32::try_from(entry.filemode()).unwrap_or_default(),
                    uid: 0,
                    gid: 0,
                    file_size: 0,
                    id: entry.id(),
                    flags: 0,
                    flags_extended: 0,
                    path: path.as_bytes().to_vec(),
                })?,
                Err(error) if error.code() == git2::ErrorCode::NotFound => {
                    ignore_not_found(index.remove_path(Path::new(path)))?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        index.write()?;
        Ok(())
    }

    /// Unstages everything that is staged (both names of a staged rename).
    pub fn unstage_all(&self) -> Result<()> {
        let status = self.load_status()?;
        let paths: Vec<&str> = status
            .staged
            .iter()
            .flat_map(|change| std::iter::once(&change.path).chain(&change.old_path))
            .map(String::as_str)
            .collect();
        if paths.is_empty() {
            return Ok(());
        }
        self.unstage(&paths)
    }

    /// Throws away unstaged changes: untracked files are deleted, the others are written again from the index.
    pub fn discard(&self, changes: &[FileChange]) -> Result<()> {
        let index = self.repo.index()?;
        let mut tracked: Vec<&str> = Vec::new();
        for change in changes {
            if change.status == FileStatus::Untracked {
                remove_all(&self.workdir.join(&change.path));
            } else if let Some(old_path) = &change.old_path
                && *old_path != change.path
                && index.get_path(Path::new(&change.path), 0).is_none()
            {
                // An unstaged rename: the new name is not in the index, so it goes and the old name comes back.
                let _ = std::fs::remove_file(self.workdir.join(&change.path));
                tracked.push(old_path);
            } else {
                tracked.push(&change.path);
            }
        }
        if tracked.is_empty() {
            return Ok(());
        }
        // The paths are literal, not patterns: [ ] * ? may be part of a file name.
        let mut checkout = force_checkout();
        checkout.recreate_missing(true).disable_pathspec_match(true);
        for path in tracked {
            checkout.path(path);
        }
        self.repo.checkout_index(None, Some(&mut checkout))?;
        Ok(())
    }

    /// Reset All: staged changes too, new files included (everything is unstaged first, then discarded).
    pub fn discard_all(&self) -> Result<()> {
        self.unstage_all()?;
        let status = self.load_status()?;
        self.discard(&status.unstaged)
    }

    /// Commits the index on top of `parents` and moves HEAD's branch there. `author` (name, email and date)
    /// when it is not the committer: the original author of a commit replayed.
    pub(super) fn commit_index(
        &self,
        message: &str,
        parents: &[git2::Oid],
        author: Option<&git2::Signature<'_>>,
    ) -> Result<git2::Oid> {
        let repo = &self.repo;
        let tree = repo.find_tree(repo.index()?.write_tree()?)?;
        let committer = self.default_signature()?;
        let parents = parents
            .iter()
            .map(|&parent| repo.find_commit(parent))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let parents: Vec<&git2::Commit<'_>> = parents.iter().collect();
        Ok(repo.commit(
            Some("HEAD"),
            author.unwrap_or(&committer),
            &committer,
            message,
            &tree,
            &parents,
        )?)
    }

    /// Commits the index; `amend` replaces HEAD's commit instead (its tree and message, keeping its author and
    /// parents). In the middle of a merge the commit gets the merged heads as further parents, and the commit
    /// concludes the merge, cherry-pick or revert in progress. No hook runs: see [`Self::commit_with_git`].
    pub fn commit(&self, message: &str, amend: bool) -> Result<Sha> {
        let repo = &self.repo;
        let head = self.head_commit()?;
        if amend {
            let head = head.ok_or_else(|| Error::message("nothing to amend"))?;
            let tree = repo.find_tree(repo.index()?.write_tree()?)?;
            let committer = self.default_signature()?;
            let id = head.amend(
                Some("HEAD"),
                None,
                Some(&committer),
                None,
                Some(message),
                Some(&tree),
            )?;
            return Ok(sha_of(id));
        }
        let mut parents: Vec<git2::Oid> = head.iter().map(git2::Commit::id).collect();
        if repo.state() == git2::RepositoryState::Merge {
            // MERGE_HEAD as the second parent.
            parents.extend(self.merge_heads());
        }
        let id = self.commit_index(message, &parents, None)?;
        self.finish_operation_state();
        Ok(sha_of(id))
    }

    /// The same through `git commit -F -`: hooks run (unless skipped) and the commit can be GPG-signed. The
    /// author is the one [`Self::commit`] would use (the profile's identity in repositories without their
    /// own).
    pub fn commit_with_git(&self, message: &str, options: &CommitOptions) -> Result<Sha> {
        if !git_available() {
            return Err(Error::message("git was not found on PATH"));
        }
        let identity = self.default_signature()?;
        let mut args = vec![
            "-c".to_owned(),
            format!("user.name={}", lossy(identity.name_bytes())),
            "-c".to_owned(),
            format!("user.email={}", lossy(identity.email_bytes())),
            "commit".to_owned(),
            "-F".to_owned(),
            "-".to_owned(),
        ];
        let mut flag = |flag: &str| args.push(flag.to_owned());
        if options.amend {
            flag("--amend");
        }
        flag(if options.sign { "-S" } else { "--no-gpg-sign" });
        if options.skip_hooks {
            flag("--no-verify");
        }
        if options.strip_comments {
            flag("--cleanup=strip");
        }
        let output = self.git(&args, message);
        self.refresh_index();
        if !output.success() {
            let text = trim_line_end(&output.text);
            return Err(Error::Cli {
                output: if text.is_empty() {
                    "git commit failed".to_owned()
                } else {
                    text.to_owned()
                },
            });
        }
        Ok(sha_of(self.repo.refname_to_id("HEAD")?))
    }

    /// Whether [`Self::commit`] would skip something `git commit` does: a pre-commit / prepare-commit-msg /
    /// commit-msg / post-commit hook in the hooks directory (`core.hooksPath` honoured).
    #[must_use]
    pub fn has_commit_hooks(&self) -> bool {
        let hooks = match self.config_string("core.hooksPath") {
            Some(configured) => self.workdir.join(configured),
            None => self.repo.commondir().join("hooks"),
        };
        [
            "pre-commit",
            "prepare-commit-msg",
            "commit-msg",
            "post-commit",
        ]
        .iter()
        .any(|name| hooks.join(name).is_file())
    }

    /// Stages (or, `unstage`, unstages) only the given changed lines of `path`: a patch of them is applied to
    /// the index with `git apply --cached`. Unselected deletions stay, unselected additions are left out.
    pub fn apply_lines(&self, path: &str, lines: &[LineSelection], unstage: bool) -> Result<()> {
        if lines.is_empty() {
            return Ok(());
        }
        if !git_available() {
            return Err(Error::message("git was not found on PATH"));
        }
        // The diff the lines were picked from, with git's 3 lines of context: HEAD -> index to unstage, index
        // -> working directory to stage.
        let mut options = file_diff_options(path, DiffOptions::default());
        options.include_untracked(true).show_untracked_content(true);
        let diff = self.workdir_diff(unstage, &mut options)?;
        if diff.deltas().len() == 0 {
            return Err(Error::message("the file has no changes left"));
        }
        let patch = git2::Patch::from_diff(&diff, 0)?
            .ok_or_else(|| Error::message("could not build patch"))?;
        let delta = patch.delta();
        if delta.flags().contains(git2::DiffFlags::BINARY) {
            return Err(Error::message("lines of a binary file cannot be staged"));
        }
        let selected = |origin: LineOrigin, lineno: Option<u32>| -> bool {
            lineno.is_some_and(|lineno| lines.contains(&LineSelection { origin, lineno }))
        };

        // The hunks with their changed lines told apart: picked or not.
        let mut hunks: Vec<(git2::DiffHunk<'_>, Vec<PatchLine<'_>>)> = Vec::new();
        for hunk_index in 0..patch.num_hunks() {
            let (hunk, line_count) = patch.hunk(hunk_index)?;
            let mut hunk_lines = Vec::with_capacity(line_count);
            for line_index in 0..line_count {
                let line = patch.line_in_hunk(hunk_index, line_index)?;
                let (origin, picked) = match line.origin_value() {
                    git2::DiffLineType::Context => (LineOrigin::Context, false),
                    git2::DiffLineType::Addition => (
                        LineOrigin::Added,
                        selected(LineOrigin::Added, line.new_lineno()),
                    ),
                    git2::DiffLineType::Deletion => (
                        LineOrigin::Deleted,
                        selected(LineOrigin::Deleted, line.old_lineno()),
                    ),
                    _ => continue, // the end-of-file newline markers: PatchLine::write adds them
                };
                hunk_lines.push(PatchLine {
                    origin,
                    picked,
                    content: line.content(),
                });
            }
            hunks.push((hunk, hunk_lines));
        }

        // Every change picked: the whole file (new, deleted and mode changes included) the usual way.
        let all = hunks
            .iter()
            .flat_map(|(_, hunk_lines)| hunk_lines)
            .all(|line| line.origin == LineOrigin::Context || line.picked);
        if all {
            return if unstage {
                self.unstage(&[path])
            } else {
                self.stage(&[path])
            };
        }

        // Staging applies the patch to the index as is (its old side is the index); unstaging applies it in
        // reverse (its new side is the index). Each hunk keeps that side's start and derives the other one.
        let mut body = String::new();
        let mut offset = 0_i64; // the new side's start minus the old side's, from the hunks before
        for (hunk, hunk_lines) in &hunks {
            let mut text = String::new();
            let (mut old_count, mut new_count) = (0_i64, 0_i64);
            let mut changes = false;
            for line in hunk_lines {
                let origin = match (line.origin, line.picked) {
                    (LineOrigin::Context, _) => LineOrigin::Context,
                    (origin, true) => {
                        changes = true;
                        origin
                    }
                    // Not picked: an addition stays in the index when unstaging, a deletion when staging.
                    (LineOrigin::Added, false) if unstage => LineOrigin::Context,
                    (LineOrigin::Deleted, false) if !unstage => LineOrigin::Context,
                    _ => continue,
                };
                line.write(origin, &mut text);
                old_count += i64::from(origin != LineOrigin::Added);
                new_count += i64::from(origin != LineOrigin::Deleted);
            }
            if !changes {
                continue;
            }
            // A side with no lines starts at the line before (git's convention), one with lines at its first.
            let anchor_start = i64::from(if unstage {
                hunk.new_start()
            } else {
                hunk.old_start()
            });
            let (anchor_count, other_count) = if unstage {
                (new_count, old_count)
            } else {
                (old_count, new_count)
            };
            let mut other_start = if unstage {
                anchor_start - offset
            } else {
                anchor_start + offset
            };
            if anchor_count == 0 && other_count > 0 {
                other_start += 1;
            } else if anchor_count > 0 && other_count == 0 {
                other_start -= 1;
            }
            let (old_start, new_start) = if unstage {
                (other_start, anchor_start)
            } else {
                (anchor_start, other_start)
            };
            let _ = writeln!(
                body,
                "@@ -{old_start},{old_count} +{new_start},{new_count} @@"
            );
            body.push_str(&text);
            offset += new_count - old_count;
        }
        if body.is_empty() {
            return Ok(());
        }

        // A new file keeps its "new file" header when staged in part; a partly staged deletion is a plain
        // change.
        let mut patch_text = format!("diff --git a/{path} b/{path}\n");
        let created =
            !unstage && matches!(delta.status(), git2::Delta::Untracked | git2::Delta::Added);
        if created {
            let mode = u32::from(delta.new_file().mode());
            let _ = writeln!(patch_text, "new file mode {mode:o}\n--- /dev/null");
        } else {
            let _ = writeln!(patch_text, "--- a/{path}");
        }
        let _ = writeln!(patch_text, "+++ b/{path}");
        patch_text.push_str(&body);

        let mut args = vec!["apply", "--cached", "--whitespace=nowarn"];
        if unstage {
            args.push("--reverse");
        }
        args.push("-");
        let output = self.git(args, &patch_text);
        self.refresh_index();
        if !output.success() {
            return Err(Error::cli(&output, "git apply failed"));
        }
        Ok(())
    }

    /// Changes only HEAD's message, keeping its tree.
    pub fn reword_head(&self, message: &str) -> Result<Sha> {
        let head = self
            .head_commit()?
            .ok_or_else(|| Error::message("no commit to reword"))?;
        let committer = self.default_signature()?;
        let id = head.amend(
            Some("HEAD"),
            None,
            Some(&committer),
            None,
            Some(message),
            None,
        )?;
        Ok(sha_of(id))
    }
}

/// A line of the diff a partial patch is built from.
struct PatchLine<'a> {
    origin: LineOrigin,
    /// A changed line the selection holds.
    picked: bool,
    /// With its line ending, when it has one.
    content: &'a [u8],
}

impl PatchLine<'_> {
    /// Appends the line to a patch as an `origin` line.
    fn write(&self, origin: LineOrigin, patch: &mut String) {
        patch.push(origin.as_char());
        patch.push_str(&String::from_utf8_lossy(self.content));
        if !self.content.ends_with(b"\n") {
            patch.push_str("\n\\ No newline at end of file\n");
        }
    }
}

fn ignore_not_found(result: std::result::Result<(), git2::Error>) -> Result<()> {
    match result {
        Err(error) if error.code() != git2::ErrorCode::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Deletes a file or a whole directory; nothing happens when it cannot be deleted.
fn remove_all(path: &Path) {
    let is_dir = std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir());
    let _ = if is_dir {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
}
