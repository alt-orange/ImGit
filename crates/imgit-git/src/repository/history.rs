//! Reading the history: the graph's rows and refs, one file's history, blame.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::{
    FNV_OFFSET, Repository, Result, author_of, fnv1a, git_available, is_missing, lossy, parse_oid,
    read_commit, sha_of, stash_summary, unique_oids,
};
use crate::types::{
    Author, Blame, BlameHunk, Commit, FileHistory, FileHistoryEntry, FileStatus, History,
    HistoryOptions, NodeType, Ref, RefType, Remote, Sha, Submodule, Worktree,
};

impl Repository {
    /// A fingerprint of HEAD, every reference and the stash list: it changes whenever a ref moves, appears or
    /// disappears (a commit, checkout, fetch, tag or stash made outside ImGit) without walking the history.
    pub fn refs_signature(&self) -> Result<u64> {
        let repo = &self.repo;
        // The sum of one hash per "name target" pair: order-free, since the refdb may list loose and packed
        // refs in any order, and fixed-size however many refs there are.
        let mut signature = 0_u64;
        let mut add = |name: &[u8], target: &[u8]| {
            let line = fnv1a(fnv1a(FNV_OFFSET, name), b" ");
            signature = signature.wrapping_add(fnv1a(line, target));
        };
        // A ref by what it names, not where that lands: a checkout between two branches at one commit must
        // count.
        let mut add_ref = |reference: &git2::Reference<'_>| match reference.symbolic_target_bytes()
        {
            Some(target) => add(reference.name_bytes(), target),
            None => {
                let target = reference.target().map(sha_of).unwrap_or_default();
                add(reference.name_bytes(), target.as_bytes());
            }
        };
        add_ref(&repo.find_reference("HEAD")?);
        // Every ref under refs/ (libgit2 leaves out loose refs it cannot parse, as load_history does).
        for reference in repo.references()? {
            add_ref(&reference?);
        }
        // Stashes: dropping one below the top leaves refs/stash in place, only its reflog shrinks.
        for (index, (id, _)) in self.stash_entries()?.iter().enumerate() {
            add(
                format!("stash@{{{index}}}").as_bytes(),
                sha_of(*id).as_bytes(),
            );
        }
        Ok(signature)
    }

    /// The graph's rows and everything the left panel lists: refs, remotes, submodules, worktrees.
    ///
    /// The rows are in `git rev-list --date-order` from the visible refs in name order, then HEAD, then the
    /// stashes: commits with the same date keep git's queue order, and each stash is a row of its own (a dashed
    /// node on its base commit) that takes its place by date. Hidden refs are listed but not walked. The
    /// working-directory row is not part of the history: the application puts it first.
    pub fn load_history(&self, options: &HistoryOptions) -> Result<History> {
        let repo = &self.repo;
        let mut history = History::default();
        let max_commits = options.max_commits;

        // HEAD
        match repo.head() {
            Ok(head) => {
                history.detached = repo.head_detached().unwrap_or(false);
                history.head_sha = head.target().map(sha_of);
                if !history.detached {
                    history.head_branch = git2::Branch::wrap(head).name_bytes().ok().map(lossy);
                }
            }
            Err(error) if is_missing(&error) => history.unborn = true,
            Err(error) => return Err(error.into()),
        }

        // The refs Smart Branch Visibility keeps.
        let smart = if options.smart_visibility {
            self.smart_refs(history.head_branch.as_deref())
        } else {
            HashSet::new()
        };
        let walkable = |full_name: &str| {
            if let Some(solo) = &options.solo_ref {
                return full_name == solo;
            }
            if options.smart_visibility {
                return smart.contains(full_name);
            }
            !options.hidden_refs.contains(full_name)
        };

        // What the libgit2 walk starts from, should git not be there to order the rows.
        let mut tips: Vec<git2::Oid> = Vec::new();
        if let Some(head) = history
            .head_sha
            .as_deref()
            .and_then(|sha| parse_oid(sha).ok())
        {
            tips.push(head);
        }

        // Branches
        for item in repo.branches(None)? {
            let Ok((branch, branch_type)) = item else {
                break;
            };
            let reference = branch.get();
            if reference.kind() != Some(git2::ReferenceType::Direct) {
                continue; // origin/HEAD
            }
            let (Ok(name), Some(target)) = (branch.name_bytes(), reference.target()) else {
                continue;
            };
            let mut out = Ref {
                name: lossy(name),
                full_name: lossy(reference.name_bytes()),
                target: sha_of(target),
                ..Ref::default()
            };
            if branch_type == git2::BranchType::Remote {
                out.ref_type = RefType::RemoteBranch;
                if let Some((remote, _)) = out.name.split_once('/') {
                    out.remote = remote.to_owned();
                }
            } else {
                out.ref_type = RefType::LocalBranch;
                out.is_head = branch.is_head();
                if let Ok(upstream) = branch.upstream() {
                    out.upstream = upstream.name_bytes().ok().map(lossy);
                    if let Some(upstream_target) = upstream.get().target()
                        && let Ok((ahead, behind)) =
                            repo.graph_ahead_behind(target, upstream_target)
                    {
                        out.ahead = ahead;
                        out.behind = behind;
                    }
                }
            }
            out.hidden = !walkable(&out.full_name);
            if !out.hidden {
                tips.push(target);
            }
            history.refs.push(out);
        }

        // Tags
        let _ = repo.tag_foreach(|id, name| {
            let full_name = lossy(name);
            let mut out = Ref {
                ref_type: RefType::Tag,
                name: full_name
                    .strip_prefix("refs/tags/")
                    .unwrap_or(&full_name)
                    .to_owned(),
                ..Ref::default()
            };
            let Ok(object) = repo.find_object(id, None) else {
                return true;
            };
            let target = if let Some(tag) = object.as_tag() {
                out.annotated = true;
                out.message = tag.message_bytes().map(lossy).unwrap_or_default();
                match tag.peel() {
                    Ok(peeled) if peeled.kind() == Some(git2::ObjectType::Commit) => peeled.id(),
                    _ => return true,
                }
            } else if object.kind() == Some(git2::ObjectType::Commit) {
                id
            } else {
                return true;
            };
            out.target = sha_of(target);
            out.hidden = !walkable(&full_name);
            out.full_name = full_name;
            if !out.hidden {
                tips.push(target);
            }
            history.refs.push(out);
            true
        });

        // Submodules and worktrees (left panel sections)
        history.submodules = self.read_submodules();
        if let Ok(names) = repo.worktrees() {
            for name in names.iter_bytes() {
                let name = lossy(name);
                let path = repo
                    .find_worktree(&name)
                    .map(|worktree| worktree.path().to_string_lossy().into_owned())
                    .unwrap_or_default();
                history.worktrees.push(Worktree { name, path });
            }
        }

        // Stashes (not walked: they are inserted as rows next to their base commit below)
        let stashes: Vec<Ref> = self
            .stash_entries()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, (id, message))| Ref {
                ref_type: RefType::Stash,
                stash_index: Some(index),
                name: format!("stash@{{{index}}}"),
                full_name: "refs/stash".to_owned(),
                target: sha_of(id),
                message,
                ..Ref::default()
            })
            .collect();

        // Remotes
        if let Ok(names) = repo.remotes() {
            for name in names.iter_bytes() {
                let name = lossy(name);
                let url = repo
                    .find_remote(&name)
                    .map(|remote| lossy(remote.url_bytes()))
                    .unwrap_or_default();
                history.remotes.push(Remote { name, url });
            }
        }

        // A stash commit's other parents (its index and untracked-files commits) are never rows.
        let mut stash_internals: HashSet<Sha> = HashSet::new();
        let mut stash_by_sha: HashMap<&str, &Ref> = HashMap::new();
        for stash in &stashes {
            let Ok(commit) = self.lookup_commit(&stash.target) else {
                continue;
            };
            stash_by_sha.entry(&stash.target).or_insert(stash);
            stash_internals.extend(commit.parent_ids().skip(1).map(sha_of));
        }

        let ordered = self.rev_list_order(&history, options, &stashes, stash_internals.len());
        if ordered.is_empty() {
            // Without git: libgit2's topological walk, stashes inserted right above their base commit.
            self.walk_history(&mut history, &tips, &stashes, max_commits)?;
        } else {
            history.commits.reserve(max_commits.min(ordered.len()));
            for sha in &ordered {
                if stash_internals.contains(sha) {
                    continue;
                }
                if history.commits.len() >= max_commits {
                    history.has_more = true;
                    break;
                }
                let Ok(commit) = self.lookup_commit(sha) else {
                    continue;
                };
                let mut out = read_commit(&commit);
                if let Some(stash) = stash_by_sha.get(sha.as_str()) {
                    // A stash row: a dashed node on its base commit.
                    out.parents.truncate(1);
                    out.summary = stash_summary(&stash.message).to_owned();
                    out.body.clear();
                    out.node_type = NodeType::Stash;
                    history.refs.push((*stash).clone());
                }
                history.commits.push(out);
            }
        }

        for (index, reference) in history.refs.iter().enumerate() {
            history
                .refs_by_sha
                .entry(reference.target.clone())
                .or_default()
                .push(index);
        }
        for (row, commit) in history.commits.iter().enumerate() {
            history.row_by_sha.entry(commit.sha.clone()).or_insert(row);
        }
        Ok(history)
    }

    /// The refs Smart Branch Visibility walks: HEAD's branch, its upstream and the default branch (what the
    /// remote's HEAD points at, else a local main or master), locally and on the remote.
    fn smart_refs(&self, head_branch: Option<&str>) -> HashSet<String> {
        let repo = &self.repo;
        let mut smart = HashSet::new();
        let mut remote = "origin".to_owned();
        if let Some(branch) = head_branch {
            let local = format!("refs/heads/{branch}");
            if let Ok(upstream) = repo.branch_upstream_name(&local) {
                smart.insert(lossy(&upstream));
            }
            if let Ok(upstream_remote) = repo.branch_upstream_remote(&local) {
                remote = lossy(&upstream_remote);
            }
            smart.insert(local);
        }
        let prefix = format!("refs/remotes/{remote}/");
        let target = repo
            .find_reference(&format!("{prefix}HEAD"))
            .ok()
            .and_then(|head| {
                let symbolic = lossy(head.symbolic_target_bytes()?);
                symbolic.strip_prefix(&prefix).map(str::to_owned)
            })
            .filter(|target| !target.is_empty())
            .or_else(|| {
                ["main", "master"]
                    .into_iter()
                    .find(|name| repo.find_reference(&format!("refs/heads/{name}")).is_ok())
                    .map(str::to_owned)
            });
        if let Some(target) = target {
            smart.insert(format!("refs/heads/{target}"));
            smart.insert(format!("{prefix}{target}"));
        }
        smart
    }

    /// The rows' order as git lists them; empty when git is not there or has nothing to list.
    fn rev_list_order(
        &self,
        history: &History,
        options: &HistoryOptions,
        stashes: &[Ref],
        stash_internals: usize,
    ) -> Vec<Sha> {
        if !git_available() {
            return Vec::new();
        }
        let mut tips: Vec<&Ref> = history.refs.iter().filter(|r| !r.hidden).collect();
        tips.sort_by(|a, b| a.full_name.cmp(&b.full_name));
        let mut input = String::new();
        let mut feed = |sha: &str| {
            input.push_str(sha);
            input.push('\n');
        };
        for tip in tips {
            feed(&tip.target);
        }
        // The checked-out commit is always a row, even when its branch is hidden or another ref is soloed.
        if let Some(head) = &history.head_sha {
            feed(head);
        }
        if options.solo_ref.is_none() {
            for stash in stashes {
                feed(&stash.target);
            }
        }
        if input.is_empty() {
            return Vec::new();
        }
        let max_count = options
            .max_commits
            .saturating_add(1)
            .saturating_add(stash_internals);
        let listed = self.git(
            [
                "rev-list",
                "--date-order",
                "--ignore-missing",
                &format!("--max-count={max_count}"),
                "--stdin",
            ],
            &input,
        );
        if !listed.success() {
            return Vec::new();
        }
        listed
            .text
            .lines()
            .filter(|line| {
                line.len() == 40
                    && line
                        .bytes()
                        .all(|digit| matches!(digit, b'0'..=b'9' | b'a'..=b'f'))
            })
            .map(str::to_owned)
            .collect()
    }

    /// The rows from libgit2's topological walk of `tips`, the stashes inserted right above their base commit.
    fn walk_history(
        &self,
        history: &mut History,
        tips: &[git2::Oid],
        stashes: &[Ref],
        max_commits: usize,
    ) -> Result<()> {
        let repo = &self.repo;
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        for tip in unique_oids(tips.iter().copied()) {
            let _ = walk.push(tip);
        }
        history.commits.reserve(max_commits.min(4096));
        for (walked, id) in walk.map_while(std::result::Result::ok).enumerate() {
            if walked >= max_commits {
                history.has_more = true;
                break;
            }
            if let Ok(commit) = repo.find_commit(id) {
                history.commits.push(read_commit(&commit));
            }
        }

        let index_by_sha: HashMap<&str, usize> = history
            .commits
            .iter()
            .enumerate()
            .map(|(row, commit)| (commit.sha.as_str(), row))
            .collect();
        let mut inserts: Vec<(usize, Commit)> = Vec::new();
        for stash in stashes {
            let Ok(commit) = self.lookup_commit(&stash.target) else {
                continue;
            };
            let Ok(base) = commit.parent_id(0).map(sha_of) else {
                continue;
            };
            let Some(&base_row) = index_by_sha.get(base.as_str()) else {
                continue;
            };
            let author = commit.author();
            inserts.push((
                base_row,
                Commit {
                    sha: stash.target.clone(),
                    parents: vec![base],
                    author: author_of(&author),
                    time: commit.time().seconds(),
                    author_time: author.when().seconds(),
                    summary: stash_summary(&stash.message).to_owned(),
                    body: String::new(),
                    node_type: NodeType::Stash,
                },
            ));
            history.refs.push(stash.clone());
        }
        // Insert from the bottom up so earlier indices stay valid.
        inserts.sort_by_key(|(row, _)| std::cmp::Reverse(*row));
        for (row, commit) in inserts {
            history.commits.insert(row, commit);
        }
        Ok(())
    }

    /// The commits from `from` (HEAD when `None`) that changed `path`, newest first, following the file across
    /// renames; `max_commits` caps the walk (`has_more` when it stopped there).
    pub fn file_history(
        &self,
        path: &str,
        from: Option<&str>,
        max_commits: usize,
    ) -> Result<FileHistory> {
        let repo = &self.repo;
        let mut result = FileHistory::default();
        let start = match from {
            Some(sha) => parse_oid(sha)?,
            None => match self.head_commit()? {
                Some(head) => head.id(),
                None => return Ok(result), // an unborn branch has no history
            },
        };
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        walk.push(start)?;

        // The id of the blob (or tree) `tracked` names in `tree`, none when the tree has no such path.
        let entry_id = |tree: &git2::Tree<'_>, tracked: &str| {
            tree.get_path(Path::new(tracked))
                .ok()
                .map(|entry| entry.id())
        };

        // The path followed back through renames: once a commit is found to have renamed the file, the older
        // commits are searched for its previous name.
        let mut tracked = path.to_owned();
        for (walked, id) in walk.map_while(std::result::Result::ok).enumerate() {
            if walked >= max_commits {
                result.has_more = true;
                break;
            }
            let Ok(commit) = repo.find_commit(id) else {
                continue;
            };
            let Ok(tree) = commit.tree() else {
                continue;
            };
            let own = entry_id(&tree, &tracked);
            if commit.parent_count() == 0 {
                if own.is_some() {
                    result.entries.push(FileHistoryEntry {
                        commit: read_commit(&commit),
                        status: FileStatus::Added,
                        path: tracked.clone(),
                        old_path: None,
                    });
                }
                continue;
            }
            // A commit that leaves the file as its first parent had it did not change it. Every parent is
            // walked, so a merge counts only against its first parent (`git log --full-history`): one that took
            // the other side's version changed the file on this branch and is listed, its diff against the
            // first parent.
            let parent_tree = commit.parent(0).and_then(|parent| parent.tree()).ok();
            let parent_entry = parent_tree
                .as_ref()
                .and_then(|parent_tree| entry_id(parent_tree, &tracked));
            if parent_tree.is_some() && own == parent_entry {
                continue;
            }
            let mut entry = FileHistoryEntry {
                commit: read_commit(&commit),
                status: FileStatus::Modified,
                path: tracked.clone(),
                old_path: None,
            };
            if own.is_none() {
                entry.status = FileStatus::Deleted;
            } else if parent_entry.is_none()
                && let Some(parent_tree) = &parent_tree
            {
                // New over the first parent: a rename when a file of the parent's went away with its content.
                entry.status = FileStatus::Added;
                if let Some(old_path) = renamed_from(repo, parent_tree, &tree, &tracked) {
                    entry.status = FileStatus::Renamed;
                    entry.old_path = Some(old_path.clone());
                    tracked = old_path;
                }
            }
            result.entries.push(entry);
        }
        Ok(result)
    }

    /// The blame of `path` as of `sha` (HEAD when `None`): which commit last changed each line.
    pub fn blame_file(&self, path: &str, sha: Option<&str>) -> Result<Blame> {
        let repo = &self.repo;
        let mut options = git2::BlameOptions::new();
        if let Some(sha) = sha {
            options.newest_commit(parse_oid(sha)?);
        }
        let blame = repo.blame_file(Path::new(path), Some(&mut options))?;
        // The commits' summaries, read once each.
        let mut commits: HashMap<git2::Oid, Commit> = HashMap::new();
        let mut result = Blame::default();
        result.hunks.reserve(blame.len());
        for hunk in blame.iter() {
            let id = hunk.final_commit_id();
            let commit = commits.entry(id).or_insert_with(|| {
                repo.find_commit(id)
                    .map(|commit| read_commit(&commit))
                    .unwrap_or_default()
            });
            let (author, time) = match hunk.final_signature() {
                Some(signature) => (author_of(&signature), signature.when().seconds()),
                None => (commit.author.clone(), commit.author_time),
            };
            result.hunks.push(BlameHunk {
                start_line: to_u32(hunk.final_start_line()),
                line_count: to_u32(hunk.lines_in_hunk()),
                sha: sha_of(id),
                author,
                time,
                summary: commit.summary.clone(),
                orig_path: hunk.path().map_or_else(
                    || path.to_owned(),
                    |orig| orig.to_string_lossy().into_owned(),
                ),
                orig_start_line: to_u32(hunk.orig_start_line()),
            });
        }
        Ok(result)
    }

    /// Who a commit made now would be by: the profile's identity in a repository without its own, else the
    /// configured `user.name` / `user.email`.
    pub fn default_author(&self) -> Result<Author> {
        Ok(author_of(&self.default_signature()?))
    }

    /// HEAD's whole message; empty on an unborn branch.
    pub fn head_commit_message(&self) -> Result<String> {
        Ok(self
            .head_commit()?
            .map(|head| lossy(head.message_bytes()))
            .unwrap_or_default())
    }

    /// The submodules as `load_history` lists them (each one's own HEAD is read, so it opens each one).
    #[must_use]
    pub fn submodules(&self) -> Vec<Submodule> {
        self.read_submodules()
    }

    /// One commit, read without the history walk (a submodule's checked-out commit).
    pub fn load_commit(&self, sha: &str) -> Result<Commit> {
        Ok(read_commit(&self.lookup_commit(sha)?))
    }
}

/// The path the file at `path` in `tree` had in `parent_tree`, when the commit renamed it.
fn renamed_from(
    repo: &git2::Repository,
    parent_tree: &git2::Tree<'_>,
    tree: &git2::Tree<'_>,
    path: &str,
) -> Option<String> {
    let mut diff = repo
        .diff_tree_to_tree(Some(parent_tree), Some(tree), None)
        .ok()?;
    let mut find = git2::DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find)).ok()?;
    diff.deltas()
        .filter(|delta| delta.status() == git2::Delta::Renamed)
        .find(|delta| delta.new_file().path_bytes() == Some(path.as_bytes()))
        .and_then(|delta| delta.old_file().path_bytes().map(lossy))
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
