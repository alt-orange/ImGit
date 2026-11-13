//! `Repository` against scratch repositories. The global git configuration is only ever read.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use imgit_git::cli;
use imgit_git::repository::{
    CommitOptions, DiffOptions, LineHunk, LineSelection, Outcome, diff_lines, set_profile_identity,
};
use imgit_git::{
    FileChange, FileStatus, HistoryOptions, LineOrigin, LineStats, NodeType, RebaseAction,
    RebaseStep, RefType, RepoState, Repository, ResetMode, Sha, StatsSource,
};

/// A repository in a temporary folder, on an unborn `main`, with its own identity.
struct Scratch {
    dir: tempfile::TempDir,
    repo: Repository,
}

impl Scratch {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        fs::create_dir(&root).unwrap();
        let repo = Repository::init(&root).unwrap();
        // Whatever init.defaultBranch says.
        git2::Repository::open(&root)
            .unwrap()
            .set_head("refs/heads/main")
            .unwrap();
        for (key, value) in [
            ("user.name", "Tester"),
            ("user.email", "tester@example.com"),
            ("core.autocrlf", "false"),
            ("commit.gpgsign", "false"),
        ] {
            repo.set_local_config(key, Some(value)).unwrap();
        }
        Self { dir, repo }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn raw(&self) -> git2::Repository {
        git2::Repository::open(self.root()).unwrap()
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.root().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root().join(path)).unwrap()
    }

    fn exists(&self, path: &str) -> bool {
        self.root().join(path).exists()
    }

    /// Writes, stages and commits one file.
    fn commit_file(&self, path: &str, text: &str, message: &str) -> Sha {
        self.write(path, text);
        self.repo.stage(&[path]).unwrap();
        self.repo.commit(message, false).unwrap()
    }

    fn head(&self) -> Sha {
        self.history().head_sha.unwrap()
    }

    fn history(&self) -> imgit_git::History {
        self.repo.load_history(&HistoryOptions::default()).unwrap()
    }

    fn git(&self, args: &[&str]) {
        let output = cli::run(&self.root(), args, cli::RunOptions::default());
        assert!(output.success(), "git {args:?}: {}", output.text);
    }
}

fn paths(changes: &[FileChange]) -> Vec<(&str, FileStatus)> {
    changes
        .iter()
        .map(|change| (change.path.as_str(), change.status))
        .collect()
}

fn added(lineno: u32) -> LineSelection {
    LineSelection {
        origin: LineOrigin::Added,
        lineno,
    }
}

#[test]
fn init_and_open() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    assert_eq!(repo.name(), "work");
    assert!(repo.workdir().ends_with("work"));
    assert_eq!(repo.state(), RepoState::None);
    assert!(repo.common_dir().ends_with("work/.git"));
    assert!(repo.git_dir().ends_with(".git"));

    let history = scratch.history();
    assert!(history.unborn);
    assert!(history.commits.is_empty());
    assert_eq!(history.head_sha, None);
    assert_eq!(repo.head_commit_message().unwrap(), "");
    assert!(repo.load_status().unwrap().is_empty());
    assert!(
        repo.file_history("a.txt", None, 10)
            .unwrap()
            .entries
            .is_empty()
    );

    // From a folder inside the repository.
    scratch.write("deep/inside/file.txt", "x\n");
    let inner = Repository::open(scratch.root().join("deep/inside")).unwrap();
    assert_eq!(inner.name(), "work");
    assert_eq!(inner.common_dir(), repo.common_dir());

    let error = Repository::open(scratch.dir.path().join("nowhere")).unwrap_err();
    assert!(!error.to_string().is_empty());
    assert!(!error.to_string().contains("class="), "{error}");
}

#[test]
fn stage_unstage_commit_amend() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.write("a.txt", "one\n");
    let status = repo.load_status().unwrap();
    assert_eq!(paths(&status.unstaged), [("a.txt", FileStatus::Untracked)]);
    assert!(status.staged.is_empty());

    // On an unborn branch.
    repo.stage(&["a.txt"]).unwrap();
    assert_eq!(
        paths(&repo.load_status().unwrap().staged),
        [("a.txt", FileStatus::Added)]
    );
    repo.unstage(&["a.txt"]).unwrap();
    assert!(repo.load_status().unwrap().staged.is_empty());
    assert!(repo.commit("nothing", true).is_err(), "nothing to amend");

    repo.stage_all().unwrap();
    let first = repo.commit("first\n\nbody text", false).unwrap();
    assert_eq!(first.len(), 40);
    assert!(repo.load_status().unwrap().is_empty());
    assert_eq!(repo.head_commit_message().unwrap(), "first\n\nbody text");
    let commit = repo.load_commit(&first).unwrap();
    assert_eq!(commit.summary, "first");
    assert_eq!(commit.body, "body text");
    assert_eq!(commit.author.name, "Tester");
    assert_eq!(commit.author.email, "tester@example.com");
    assert!(commit.parents.is_empty());
    assert_eq!(commit.node_type, NodeType::Commit);
    assert_eq!(repo.default_author().unwrap().name, "Tester");

    // With HEAD: a modification, a deletion staged by path, and back.
    scratch.write("a.txt", "one\ntwo\n");
    scratch.write("b.txt", "bee\n");
    repo.stage(&["a.txt", "b.txt"]).unwrap();
    let second = repo.commit("second", false).unwrap();
    assert_eq!(repo.load_commit(&second).unwrap().parents, [first.as_str()]);

    fs::remove_file(scratch.root().join("b.txt")).unwrap();
    scratch.write("a.txt", "changed\n");
    repo.stage(&["a.txt", "b.txt"]).unwrap();
    let status = repo.load_status().unwrap();
    assert_eq!(
        paths(&status.staged),
        [
            ("a.txt", FileStatus::Modified),
            ("b.txt", FileStatus::Deleted)
        ]
    );
    repo.unstage(&["b.txt"]).unwrap();
    let status = repo.load_status().unwrap();
    assert_eq!(paths(&status.staged), [("a.txt", FileStatus::Modified)]);
    assert_eq!(paths(&status.unstaged), [("b.txt", FileStatus::Deleted)]);
    repo.unstage_all().unwrap();
    assert!(repo.load_status().unwrap().staged.is_empty());

    // Amend: the tree and message change, the parent stays.
    repo.stage(&["a.txt"]).unwrap();
    let amended = repo.commit("second, amended", true).unwrap();
    assert_ne!(amended, second);
    let commit = repo.load_commit(&amended).unwrap();
    assert_eq!(commit.parents, [first.as_str()]);
    assert_eq!(commit.summary, "second, amended");
    assert_eq!(
        paths(&repo.commit_changes(&amended).unwrap()),
        [
            ("a.txt", FileStatus::Modified),
            ("b.txt", FileStatus::Added)
        ]
    );

    // Reword: only the message.
    let reworded = repo.reword_head("reworded").unwrap();
    assert_eq!(repo.head_commit_message().unwrap(), "reworded");
    assert_eq!(
        repo.commit_tree_paths(&reworded).unwrap(),
        repo.commit_tree_paths(&amended).unwrap()
    );
    assert_eq!(scratch.head(), reworded);
}

#[test]
fn status_categories_and_summary() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.write("keep.txt", "keep\n");
    scratch.write("staged.txt", "a\n");
    scratch.write("unstaged.txt", "a\n");
    scratch.write("gone.txt", "a\n");
    scratch.write(
        "old name.txt",
        "some content that is long enough to be matched\nline two\n",
    );
    repo.stage_all().unwrap();
    repo.commit("base", false).unwrap();

    scratch.write("staged.txt", "b\n");
    fs::rename(
        scratch.root().join("old name.txt"),
        scratch.root().join("new name.txt"),
    )
    .unwrap();
    repo.stage(&["staged.txt", "old name.txt", "new name.txt"])
        .unwrap();
    scratch.write("unstaged.txt", "b\n");
    fs::remove_file(scratch.root().join("gone.txt")).unwrap();
    scratch.write("dir/new [1].txt", "new\n");

    let status = repo.load_status().unwrap();
    assert_eq!(
        paths(&status.staged),
        [
            ("new name.txt", FileStatus::Renamed),
            ("staged.txt", FileStatus::Modified)
        ]
    );
    assert_eq!(status.staged[0].old_path.as_deref(), Some("old name.txt"));
    assert_eq!(
        paths(&status.unstaged),
        [
            ("dir/new [1].txt", FileStatus::Untracked),
            ("gone.txt", FileStatus::Deleted),
            ("unstaged.txt", FileStatus::Modified)
        ]
    );
    assert!(status.conflicted.is_empty());
    assert_eq!(status.count(), 5);

    let summary = repo.summary().unwrap();
    assert_eq!(summary.branch.as_deref(), Some("main"));
    assert_eq!(
        (summary.added, summary.deleted, summary.modified),
        (1, 1, 3)
    );
    assert_eq!(summary.owner, "");

    // Stats of both sides.
    let mut staged = status.staged.clone();
    repo.fill_stats(&mut staged, StatsSource::Staged, None)
        .unwrap();
    assert_eq!(
        staged[1].stats,
        Some(LineStats {
            additions: 1,
            deletions: 1
        })
    );
    assert_eq!(staged[0].stats, Some(LineStats::default()));
    let mut unstaged = status.unstaged.clone();
    repo.fill_stats(&mut unstaged, StatsSource::Unstaged, None)
        .unwrap();
    let stats: Vec<_> = unstaged
        .iter()
        .map(|change| {
            let stats = change.stats.unwrap();
            (stats.additions, stats.deletions)
        })
        .collect();
    assert_eq!(stats, [(1, 0), (0, 1), (1, 1)]);

    // Unstaging everything gives both names of the rename back to the working directory.
    repo.unstage_all().unwrap();
    let status = repo.load_status().unwrap();
    assert!(status.staged.is_empty());
    assert!(
        status
            .unstaged
            .iter()
            .any(|change| change.path == "new name.txt")
    );
}

/// Commits a tree holding `files` on top of `parents` at a fixed time, without touching HEAD.
fn commit_at(
    raw: &git2::Repository,
    time: i64,
    message: &str,
    parents: &[git2::Oid],
    files: &[(&str, &str)],
) -> git2::Oid {
    let base = parents
        .first()
        .map(|parent| raw.find_commit(*parent).unwrap().tree().unwrap());
    let mut builder = raw.treebuilder(base.as_ref()).unwrap();
    for (name, text) in files {
        let blob = raw.blob(text.as_bytes()).unwrap();
        builder.insert(name, blob, 0o100_644).unwrap();
    }
    let tree = raw.find_tree(builder.write().unwrap()).unwrap();
    let signature =
        git2::Signature::new("Timed", "timed@example.com", &git2::Time::new(time, 0)).unwrap();
    let parents: Vec<_> = parents
        .iter()
        .map(|parent| raw.find_commit(*parent).unwrap())
        .collect();
    let parents: Vec<_> = parents.iter().collect();
    raw.commit(None, &signature, &signature, message, &tree, &parents)
        .unwrap()
}

#[test]
fn history_order_with_merge_stash_and_dirty_tree() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let raw = scratch.raw();
    let c1 = commit_at(&raw, 1000, "c1", &[], &[("base.txt", "base\n")]);
    let c2 = commit_at(&raw, 2000, "c2", &[c1], &[("main.txt", "main\n")]);
    let c4 = commit_at(&raw, 2500, "c4", &[c1], &[("side.txt", "side\n")]);
    let c3 = commit_at(&raw, 3000, "c3", &[c1], &[("feature.txt", "feature\n")]);
    let merge = commit_at(
        &raw,
        4000,
        "merge",
        &[c2, c3],
        &[("feature.txt", "feature\n")],
    );
    for (name, target) in [("main", merge), ("feature", c3), ("side", c4)] {
        raw.reference(&format!("refs/heads/{name}"), target, true, "test")
            .unwrap();
    }
    let [c1, c2, c3, c4, merge] = [c1, c2, c3, c4, merge].map(|id| id.to_string());
    repo.reset(&merge, ResetMode::Hard).unwrap();
    assert!(repo.load_status().unwrap().is_empty());

    // A stash (tracked and untracked changes), then a dirty tree on top.
    scratch.write("base.txt", "stashed\n");
    scratch.write("untracked.txt", "stashed too\n");
    let stash = repo.stash_push("my stash").unwrap();
    assert!(repo.load_status().unwrap().is_empty());
    scratch.write("dirty.txt", "dirty\n");
    assert!(!repo.load_status().unwrap().is_empty());

    let history = scratch.history();
    let rows: Vec<&str> = history.commits.iter().map(|c| c.sha.as_str()).collect();
    // Newest first by date, parents after their children; the stash takes its place by date (it is the
    // newest) and its index / untracked-files commits are not rows.
    assert_eq!(rows, [&stash, &merge, &c3, &c4, &c2, &c1]);
    assert!(!history.has_more);
    // The working-directory row is the application's to add: the history never has one.
    assert!(
        history
            .commits
            .iter()
            .all(|commit| commit.node_type != NodeType::WorkDir)
    );

    let stash_row = &history.commits[0];
    assert_eq!(stash_row.node_type, NodeType::Stash);
    assert_eq!(stash_row.parents, [merge.as_str()]);
    assert_eq!(stash_row.summary, "my stash");
    assert_eq!(stash_row.body, "");
    let merge_row = history.commit(&merge).unwrap();
    assert_eq!(merge_row.node_type, NodeType::Merge);
    assert_eq!(merge_row.parents, [c2.clone(), c3.clone()]);
    assert_eq!(merge_row.time, 4000);
    assert_eq!(merge_row.author.name, "Timed");
    assert_eq!(history.row_by_sha[&merge], 1);
    assert_eq!(history.row_by_sha[&c1], 5);

    assert_eq!(history.head_branch.as_deref(), Some("main"));
    assert_eq!(history.head_sha.as_deref(), Some(merge.as_str()));
    assert!(!history.detached && !history.unborn);

    let stash_ref = history.refs_at(&stash).next().unwrap();
    assert_eq!(stash_ref.ref_type, RefType::Stash);
    assert_eq!(stash_ref.name, "stash@{0}");
    assert_eq!(stash_ref.full_name, "refs/stash");
    assert_eq!(stash_ref.stash_index, Some(0));
    assert_eq!(stash_ref.message, "On main: my stash");
    let main_ref = history.refs_at(&merge).next().unwrap();
    assert_eq!((main_ref.name.as_str(), main_ref.is_head), ("main", true));
    assert_eq!(main_ref.full_name, "refs/heads/main");

    // The limit.
    let limited = repo
        .load_history(&HistoryOptions {
            max_commits: 3,
            ..HistoryOptions::default()
        })
        .unwrap();
    assert!(limited.has_more);
    let rows: Vec<&str> = limited.commits.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(rows, [&stash, &merge, &c3]);

    // A hidden ref is listed but not walked.
    let hidden = repo
        .load_history(&HistoryOptions {
            hidden_refs: HashSet::from(["refs/heads/side".to_owned()]),
            ..HistoryOptions::default()
        })
        .unwrap();
    assert!(hidden.commit(&c4).is_none());
    let side = hidden.refs.iter().find(|r| r.name == "side").unwrap();
    assert!(side.hidden);
    assert_eq!(hidden.commits.len(), 5);

    // A solo ref: only it and the checked-out commit are walked, and no stash.
    let solo = repo
        .load_history(&HistoryOptions {
            solo_ref: Some("refs/heads/side".to_owned()),
            ..HistoryOptions::default()
        })
        .unwrap();
    let rows: Vec<&str> = solo.commits.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(rows, [&merge, &c3, &c4, &c2, &c1]);
    assert!(solo.refs.iter().all(|r| r.ref_type != RefType::Stash));
    assert!(solo.refs.iter().all(|r| r.hidden == (r.name != "side")));

    // Smart visibility: HEAD's branch and the default branch, which are both main here.
    let smart = repo
        .load_history(&HistoryOptions {
            smart_visibility: true,
            ..HistoryOptions::default()
        })
        .unwrap();
    assert!(smart.commit(&c4).is_none());
    assert!(smart.commit(&c3).is_some());
}

#[test]
fn refs_with_a_bare_remote() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let bare = scratch.dir.path().join("remote.git");
    git2::Repository::init_bare(&bare).unwrap();
    let url = bare.to_str().unwrap().replace('\\', "/");

    let c1 = scratch.commit_file("a.txt", "1\n", "c1");
    repo.add_remote("origin", &url).unwrap();
    scratch.git(&["push", "-u", "origin", "main"]);
    let c2 = scratch.commit_file("a.txt", "2\n", "c2");

    let history = scratch.history();
    assert_eq!(history.remotes.len(), 1);
    assert_eq!(history.remotes[0].name, "origin");
    assert_eq!(history.remotes[0].url, url);
    let main = history.refs.iter().find(|r| r.name == "main").unwrap();
    assert_eq!(main.ref_type, RefType::LocalBranch);
    assert_eq!(main.upstream.as_deref(), Some("origin/main"));
    assert_eq!((main.ahead, main.behind), (1, 0));
    assert!(main.is_head);
    let remote = history
        .refs
        .iter()
        .find(|r| r.name == "origin/main")
        .unwrap();
    assert_eq!(remote.ref_type, RefType::RemoteBranch);
    assert_eq!(remote.remote, "origin");
    assert_eq!(remote.full_name, "refs/remotes/origin/main");
    assert_eq!(remote.target, c1);
    assert_eq!(repo.summary().unwrap().ahead, 1);

    // Behind, then diverged.
    scratch.git(&["push", "origin", "main"]);
    repo.reset(&c1, ResetMode::Hard).unwrap();
    let main = scratch
        .history()
        .refs
        .into_iter()
        .find(|r| r.name == "main")
        .unwrap();
    assert_eq!((main.ahead, main.behind), (0, 1));
    let c3 = scratch.commit_file("b.txt", "3\n", "c3");
    let main = scratch
        .history()
        .refs
        .into_iter()
        .find(|r| r.name == "main")
        .unwrap();
    assert_eq!((main.ahead, main.behind), (1, 1));
    let summary = repo.summary().unwrap();
    assert_eq!((summary.ahead, summary.behind), (1, 1));
    assert!(repo.is_ancestor(&c1, &c3));
    assert!(!repo.is_ancestor(&c3, &c1));
    assert!(!repo.is_ancestor(&c1, &c1));
    assert_eq!(repo.merge_base(&c2, &c3), Some(c1.clone()));

    // Tags: lightweight and annotated.
    repo.create_tag("light", &c1, None).unwrap();
    repo.create_tag("v1.0", &c3, Some("release notes")).unwrap();
    assert!(repo.create_tag("light", &c3, None).is_err());
    let history = scratch.history();
    let light = history.refs.iter().find(|r| r.name == "light").unwrap();
    assert_eq!(light.ref_type, RefType::Tag);
    assert!(!light.annotated);
    assert_eq!(light.target, c1);
    assert_eq!(light.full_name, "refs/tags/light");
    let annotated = history.refs.iter().find(|r| r.name == "v1.0").unwrap();
    assert!(annotated.annotated);
    assert_eq!(annotated.message.trim_end(), "release notes");
    assert_eq!(annotated.target, c3, "peeled to the commit");
    repo.delete_tag("light").unwrap();
    assert!(scratch.history().refs.iter().all(|r| r.name != "light"));

    // A remote branch checked out as a local one tracking it.
    scratch.git(&["push", "origin", &format!("{c2}:refs/heads/feature")]);
    scratch.git(&["fetch", "origin"]);
    repo.checkout_remote_branch("origin", "feature").unwrap();
    let history = scratch.history();
    assert_eq!(history.head_branch.as_deref(), Some("feature"));
    assert_eq!(history.head_sha.as_deref(), Some(c2.as_str()));
    let feature = history.refs.iter().find(|r| r.name == "feature").unwrap();
    assert_eq!(feature.upstream.as_deref(), Some("origin/feature"));
    assert_eq!(scratch.read("a.txt"), "2\n");

    // A local branch behind its remote one is fast-forwarded by the same call.
    repo.checkout_branch("main").unwrap();
    repo.set_branch_target("feature", &c1).unwrap();
    repo.checkout_remote_branch("origin", "feature").unwrap();
    assert_eq!(scratch.head(), c2);

    // Default branch, gone upstreams, checked-out branches.
    assert_eq!(repo.default_branch("origin").as_deref(), Some("main"));
    repo.set_local_config("gitflow.branch.master", Some("feature"))
        .unwrap();
    assert_eq!(repo.default_branch("origin").as_deref(), Some("feature"));
    assert!(repo.branch_checked_out("feature"));
    assert!(!repo.branch_checked_out("main"));
    assert!(!repo.branch_checked_out("no-such-branch"));
    assert!(!repo.upstream_gone("feature"));
    scratch
        .raw()
        .find_reference("refs/remotes/origin/feature")
        .unwrap()
        .delete()
        .unwrap();
    assert!(repo.upstream_gone("feature"));
    assert!(!repo.upstream_gone("no-such-branch"));
    repo.set_upstream("feature", None).unwrap();
    assert!(!repo.upstream_gone("feature"));
    repo.set_upstream("feature", Some("origin/main")).unwrap();
    let feature = scratch
        .history()
        .refs
        .into_iter()
        .find(|r| r.name == "feature")
        .unwrap();
    assert_eq!(feature.upstream.as_deref(), Some("origin/main"));

    // Remotes.
    repo.set_remote_url("origin", "https://example.com/owner/repo.git")
        .unwrap();
    assert_eq!(repo.summary().unwrap().owner, "owner");
    repo.remove_remote("origin").unwrap();
    assert!(scratch.history().remotes.is_empty());
}

#[test]
fn commit_changes_diffs_and_stats() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.write("a.txt", "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\n");
    scratch.write("old.txt", "a file that will be renamed\nwith two lines\n");
    scratch.write("sub/c.txt", "c\n");
    repo.stage_all().unwrap();
    let first = repo.commit("first", false).unwrap();

    scratch.write(
        "a.txt",
        "one\ntwo\nthree\nFOUR\nfive\nsix\nseven\neight\nnine\n",
    );
    fs::rename(
        scratch.root().join("old.txt"),
        scratch.root().join("new.txt"),
    )
    .unwrap();
    fs::remove_file(scratch.root().join("sub/c.txt")).unwrap();
    scratch.write("bin.dat", "bin\0ary\n");
    repo.stage_all().unwrap();
    let second = repo.commit("second", false).unwrap();

    // The root commit: everything added.
    assert_eq!(
        paths(&repo.commit_changes(&first).unwrap()),
        [
            ("a.txt", FileStatus::Added),
            ("old.txt", FileStatus::Added),
            ("sub/c.txt", FileStatus::Added)
        ]
    );
    let mut changes = repo.commit_changes(&second).unwrap();
    assert_eq!(
        paths(&changes),
        [
            ("a.txt", FileStatus::Modified),
            ("bin.dat", FileStatus::Added),
            ("new.txt", FileStatus::Renamed),
            ("sub/c.txt", FileStatus::Deleted)
        ]
    );
    assert_eq!(changes[2].old_path.as_deref(), Some("old.txt"));
    assert!(changes.iter().all(|change| change.stats.is_none()));
    repo.fill_stats(&mut changes, StatsSource::Commit, Some(&second))
        .unwrap();
    let stats: Vec<_> = changes
        .iter()
        .map(|change| {
            let stats = change.stats.unwrap();
            (stats.additions, stats.deletions)
        })
        .collect();
    assert_eq!(stats, [(2, 1), (0, 0), (0, 0), (0, 1)]);
    assert!(
        repo.fill_stats(&mut changes, StatsSource::Commit, None)
            .is_err()
    );

    assert_eq!(
        repo.commit_tree_paths(&second).unwrap(),
        ["a.txt", "bin.dat", "new.txt"]
    );
    assert_eq!(
        repo.commit_tree_paths(&first).unwrap(),
        ["a.txt", "old.txt", "sub/c.txt"]
    );

    // One file's diff.
    let diff = repo
        .commit_file_diff(&second, "a.txt", DiffOptions::default())
        .unwrap();
    assert!(diff.revision.is_some());
    assert_eq!(diff.path, "a.txt");
    assert!(!diff.binary);
    assert_eq!(
        diff.stats,
        LineStats {
            additions: 2,
            deletions: 1
        }
    );
    assert_eq!(diff.hunks.len(), 1);
    assert_eq!(diff.hunks[0].header, "@@ -1,8 +1,9 @@");
    let lines: Vec<_> = diff.hunks[0]
        .lines
        .iter()
        .map(|line| {
            (
                line.origin.as_char(),
                line.old_lineno,
                line.new_lineno,
                line.text.as_str(),
            )
        })
        .collect();
    assert_eq!(lines[3], ('-', Some(4), None, "four"));
    assert_eq!(lines[4], ('+', None, Some(4), "FOUR"));
    assert_eq!(lines[0], (' ', Some(1), Some(1), "one"));
    assert_eq!(lines.last(), Some(&('+', None, Some(9), "nine")));
    assert_ne!(
        repo.commit_file_diff(&second, "a.txt", DiffOptions::default())
            .unwrap()
            .revision,
        diff.revision
    );

    // Less context: two hunks.
    let tight = DiffOptions {
        context_lines: 0,
        ..DiffOptions::default()
    };
    let diff = repo.commit_file_diff(&second, "a.txt", tight).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    assert_eq!(diff.hunks[0].lines.len(), 2);

    // The new side of a rename is diffed against its old name; a binary file has no hunks.
    let diff = repo
        .commit_file_diff(&second, "new.txt", DiffOptions::default())
        .unwrap();
    assert_eq!(diff.stats, LineStats::default());
    assert!(diff.hunks.is_empty());
    let diff = repo
        .commit_file_diff(&second, "bin.dat", DiffOptions::default())
        .unwrap();
    assert!(diff.binary);
    let diff = repo
        .commit_file_diff(&second, "no such file", DiffOptions::default())
        .unwrap();
    assert!(diff.hunks.is_empty() && !diff.binary);
    let diff = repo
        .commit_file_diff(&first, "sub/c.txt", DiffOptions::default())
        .unwrap();
    assert_eq!(diff.stats.additions, 1);

    // The working directory: staged, unstaged and untracked.
    scratch.write(
        "a.txt",
        "one\ntwo\nthree\nFOUR\nfive\nsix\nseven\neight\nnine\nstaged\n",
    );
    repo.stage(&["a.txt"]).unwrap();
    scratch.write(
        "a.txt",
        "one\ntwo\nthree\nFOUR\nfive\nsix\nseven\neight\nnine\nstaged\n  unstaged\n",
    );
    scratch.write("fresh [x].txt", "fresh\nfile\n");
    let staged = repo
        .workdir_file_diff("a.txt", true, DiffOptions::default())
        .unwrap();
    assert_eq!(staged.stats.additions, 1);
    assert_eq!(staged.hunks[0].lines.last().unwrap().text, "staged");
    let unstaged = repo
        .workdir_file_diff("a.txt", false, DiffOptions::default())
        .unwrap();
    assert_eq!(unstaged.stats.additions, 1);
    assert_eq!(unstaged.hunks[0].lines.last().unwrap().text, "  unstaged");
    let untracked = repo
        .workdir_file_diff("fresh [x].txt", false, DiffOptions::default())
        .unwrap();
    assert_eq!(untracked.stats.additions, 2);
    assert_eq!(untracked.hunks[0].lines[1].new_lineno, Some(2));

    // Whitespace-only changes can be left out.
    scratch.write("new.txt", "a file  that will be renamed\nwith two lines\n");
    let plain = repo
        .workdir_file_diff("new.txt", false, DiffOptions::default())
        .unwrap();
    assert_eq!(plain.stats.additions, 1);
    let ignoring = DiffOptions {
        ignore_whitespace: true,
        ..DiffOptions::default()
    };
    let quiet = repo.workdir_file_diff("new.txt", false, ignoring).unwrap();
    assert!(quiet.hunks.is_empty());

    // A commit against the working directory.
    let compared = repo.compare_changes(&first).unwrap();
    assert_eq!(
        paths(&compared),
        [
            ("a.txt", FileStatus::Modified),
            ("bin.dat", FileStatus::Added),
            ("fresh [x].txt", FileStatus::Untracked),
            ("new.txt", FileStatus::Renamed),
            ("sub/c.txt", FileStatus::Deleted)
        ]
    );
    let diff = repo
        .compare_file_diff(&first, "a.txt", DiffOptions::default())
        .unwrap();
    assert_eq!(
        diff.stats,
        LineStats {
            additions: 4,
            deletions: 1
        }
    );
    let diff = repo
        .compare_file_diff(&first, "fresh [x].txt", DiffOptions::default())
        .unwrap();
    assert_eq!(diff.stats.additions, 2);
}

#[test]
fn discard() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.write("a.txt", "a\n");
    scratch.write("b [1].txt", "b\n");
    scratch.write(
        "moved.txt",
        "content that moves to another name\nsecond line\n",
    );
    repo.stage_all().unwrap();
    repo.commit("base", false).unwrap();

    scratch.write("a.txt", "changed\n");
    fs::remove_file(scratch.root().join("b [1].txt")).unwrap();
    fs::rename(
        scratch.root().join("moved.txt"),
        scratch.root().join("renamed.txt"),
    )
    .unwrap();
    scratch.write("junk/file.txt", "junk\n");
    let status = repo.load_status().unwrap();
    let renamed = status
        .unstaged
        .iter()
        .find(|change| change.path == "renamed.txt")
        .expect("an unstaged rename");
    assert_eq!(renamed.status, FileStatus::Renamed);
    assert_eq!(renamed.old_path.as_deref(), Some("moved.txt"));

    // Only what is asked for.
    let only_a: Vec<_> = status
        .unstaged
        .iter()
        .filter(|change| change.path == "a.txt")
        .cloned()
        .collect();
    repo.discard(&only_a).unwrap();
    assert_eq!(scratch.read("a.txt"), "a\n");
    assert!(!scratch.exists("b [1].txt"));

    repo.discard(&repo.load_status().unwrap().unstaged).unwrap();
    assert!(repo.load_status().unwrap().is_empty());
    assert_eq!(scratch.read("b [1].txt"), "b\n");
    assert!(scratch.exists("moved.txt") && !scratch.exists("renamed.txt"));
    assert!(!scratch.exists("junk/file.txt"));

    // Reset All: staged changes and staged new files go too.
    scratch.write("a.txt", "staged change\n");
    scratch.write("new.txt", "staged new file\n");
    repo.stage_all().unwrap();
    scratch.write("a.txt", "and an unstaged one\n");
    repo.discard_all().unwrap();
    assert!(repo.load_status().unwrap().is_empty());
    assert_eq!(scratch.read("a.txt"), "a\n");
    assert!(!scratch.exists("new.txt"));
}

#[test]
fn branches() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let c1 = scratch.commit_file("a.txt", "1\n", "c1");
    let c2 = scratch.commit_file("a.txt", "2\n", "c2");

    repo.create_branch("topic", &c1, false).unwrap();
    assert_eq!(scratch.history().head_branch.as_deref(), Some("main"));
    assert!(
        repo.create_branch("topic", &c1, false).is_err(),
        "already there"
    );
    repo.create_branch("work", &c1, true).unwrap();
    let history = scratch.history();
    assert_eq!(history.head_branch.as_deref(), Some("work"));
    assert_eq!(history.head_sha.as_deref(), Some(c1.as_str()));
    assert_eq!(scratch.read("a.txt"), "1\n");

    repo.rename_branch("topic", "renamed").unwrap();
    let names: Vec<String> = scratch.history().refs.into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["main", "renamed", "work"]);
    assert!(repo.rename_branch("renamed", "main").is_err());

    repo.checkout_branch("main").unwrap();
    assert_eq!(scratch.read("a.txt"), "2\n");
    assert_eq!(scratch.head(), c2);
    assert!(repo.checkout_branch("missing").is_err());

    // A checkout keeps local changes that are not in the way and refuses when they are.
    scratch.write("local.txt", "untracked\n");
    scratch.write("a.txt", "local change\n");
    assert!(repo.checkout_branch("work").is_err());
    assert_eq!(scratch.history().head_branch.as_deref(), Some("main"));
    repo.discard_all().unwrap();
    scratch.write("local.txt", "untracked\n");
    repo.checkout_branch("work").unwrap();
    assert!(scratch.exists("local.txt"));

    repo.set_branch_target("renamed", &c2).unwrap();
    repo.set_branch_target("fresh", &c2).unwrap();
    let history = scratch.history();
    for name in ["renamed", "fresh"] {
        let branch = history.refs.iter().find(|r| r.name == name).unwrap();
        assert_eq!(branch.target, c2);
    }
    assert_eq!(
        scratch.read("a.txt"),
        "1\n",
        "the working directory is not touched"
    );

    assert!(repo.delete_branch("work").is_err(), "checked out");
    repo.delete_branch("fresh").unwrap();
    assert!(scratch.history().refs.iter().all(|r| r.name != "fresh"));

    repo.checkout_commit(&c2).unwrap();
    let history = scratch.history();
    assert!(history.detached);
    assert_eq!(history.head_branch, None);
    assert_eq!(history.head_sha.as_deref(), Some(c2.as_str()));
    assert_eq!(repo.summary().unwrap().branch, None);
    assert_eq!(scratch.read("a.txt"), "2\n");
}

#[test]
fn reset_modes() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let c1 = scratch.commit_file("a.txt", "1\n", "c1");
    let c2 = scratch.commit_file("a.txt", "2\n", "c2");

    repo.reset(&c1, ResetMode::Soft).unwrap();
    assert_eq!(scratch.head(), c1);
    let status = repo.load_status().unwrap();
    assert_eq!(paths(&status.staged), [("a.txt", FileStatus::Modified)]);
    assert!(status.unstaged.is_empty());

    repo.reset(&c2, ResetMode::Soft).unwrap();
    repo.reset(&c1, ResetMode::Mixed).unwrap();
    let status = repo.load_status().unwrap();
    assert!(status.staged.is_empty());
    assert_eq!(paths(&status.unstaged), [("a.txt", FileStatus::Modified)]);
    assert_eq!(scratch.read("a.txt"), "2\n");

    repo.reset(&c2, ResetMode::Hard).unwrap();
    scratch.write("a.txt", "local\n");
    repo.reset(&c1, ResetMode::Hard).unwrap();
    assert!(repo.load_status().unwrap().is_empty());
    assert_eq!(scratch.read("a.txt"), "1\n");
    assert_eq!(scratch.head(), c1);
    assert!(repo.reset("not a sha", ResetMode::Hard).is_err());
}

/// `main` and `feature` both changed `f.txt` after `base`; `feature` also added `extra.txt` in a commit of its
/// own. Returns (base, main's tip, feature's conflicting commit, feature's clean commit), on `main`.
fn diverged(scratch: &Scratch) -> (Sha, Sha, Sha, Sha) {
    let repo = &scratch.repo;
    let base = scratch.commit_file("f.txt", "line\n", "base");
    repo.create_branch("feature", &base, true).unwrap();
    let theirs = scratch.commit_file("f.txt", "theirs\n", "feature changes f\n\nwith a body\n");
    let clean = scratch.commit_file("extra.txt", "extra\n", "feature adds extra");
    repo.checkout_branch("main").unwrap();
    let ours = scratch.commit_file("f.txt", "ours\n", "main changes f");
    (base, ours, theirs, clean)
}

#[test]
fn cherry_pick_clean_and_conflicting() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let (_, ours, theirs, clean) = diverged(&scratch);

    assert_eq!(repo.cherry_pick(&clean).unwrap(), Outcome::Done);
    assert_eq!(repo.state(), RepoState::None);
    let picked = repo.load_commit(&scratch.head()).unwrap();
    let original = repo.load_commit(&clean).unwrap();
    assert_eq!(picked.parents, [ours]);
    assert_eq!(picked.summary, "feature adds extra");
    assert_eq!(picked.author, original.author);
    assert_eq!(picked.author_time, original.author_time);
    assert_eq!(scratch.read("extra.txt"), "extra\n");
    assert!(repo.load_status().unwrap().is_empty());
    let after_pick = scratch.head();

    assert_eq!(repo.cherry_pick(&theirs).unwrap(), Outcome::Conflicts);
    assert!(!Outcome::Conflicts.is_done());
    assert_eq!(repo.state(), RepoState::CherryPick);
    assert_eq!(repo.incoming_head(), Some(theirs.clone()));
    assert_eq!(scratch.head(), after_pick, "nothing was committed");
    let status = repo.load_status().unwrap();
    assert_eq!(
        paths(&status.conflicted),
        [("f.txt", FileStatus::Conflicted)]
    );
    assert!(scratch.read("f.txt").contains("<<<<<<<"));
    let sides = repo.conflict_sides("f.txt").unwrap();
    assert_eq!(sides.base.as_deref(), Some("line\n"));
    assert_eq!(sides.ours.as_deref(), Some("ours\n"));
    assert_eq!(sides.theirs.as_deref(), Some("theirs\n"));
    assert!(repo.conflict_sides("extra.txt").is_err());

    repo.abort_operation().unwrap();
    assert_eq!(repo.state(), RepoState::None);
    assert_eq!(repo.incoming_head(), None);
    assert!(repo.load_status().unwrap().is_empty());
    assert_eq!(scratch.read("f.txt"), "ours\n");
    assert_eq!(scratch.head(), after_pick);

    // Resolved by hand instead: the commit concludes the cherry-pick.
    assert_eq!(repo.cherry_pick(&theirs).unwrap(), Outcome::Conflicts);
    scratch.write("f.txt", "resolved\n");
    repo.stage(&["f.txt"]).unwrap();
    assert!(repo.load_status().unwrap().conflicted.is_empty());
    let resolved = repo.commit("resolved pick", false).unwrap();
    assert_eq!(repo.state(), RepoState::None);
    assert_eq!(repo.load_commit(&resolved).unwrap().parents, [after_pick]);
}

#[test]
fn revert_clean_and_conflicting() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let c1 = scratch.commit_file("a.txt", "1\n", "c1");
    let c2 = scratch.commit_file("b.txt", "b\n", "add b");
    let c3 = scratch.commit_file("a.txt", "3\n", "c3");

    assert_eq!(repo.revert(&c2).unwrap(), Outcome::Done);
    assert!(!scratch.exists("b.txt"));
    assert_eq!(
        repo.head_commit_message().unwrap(),
        format!("Revert \"add b\"\n\nThis reverts commit {c2}.\n")
    );
    let reverted = repo.load_commit(&scratch.head()).unwrap();
    assert_eq!(reverted.parents, [c3]);
    assert_eq!(reverted.author.name, "Tester");
    assert_eq!(repo.state(), RepoState::None);

    // Reverting c1 would delete a file c3 changed.
    scratch.commit_file("a.txt", "4\n", "c4");
    let before = scratch.head();
    assert_eq!(repo.revert(&c1).unwrap(), Outcome::Conflicts);
    assert_eq!(repo.state(), RepoState::Revert);
    assert_eq!(repo.incoming_head(), Some(c1));
    let sides = repo.conflict_sides("a.txt").unwrap();
    assert_eq!(sides.theirs, None, "the reverted side has no file");
    assert_eq!(sides.ours.as_deref(), Some("4\n"));
    repo.abort_operation().unwrap();
    assert_eq!(repo.state(), RepoState::None);
    assert_eq!(scratch.head(), before);
    assert_eq!(scratch.read("a.txt"), "4\n");
}

#[test]
fn merge_fast_forward_no_ff_and_conflict() {
    // Fast-forward.
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let c1 = scratch.commit_file("a.txt", "1\n", "c1");
    repo.create_branch("feature", &c1, true).unwrap();
    let c2 = scratch.commit_file("b.txt", "b\n", "c2");
    repo.checkout_branch("main").unwrap();
    assert!(!scratch.exists("b.txt"));
    assert_eq!(repo.merge("feature", false).unwrap(), Outcome::Done);
    assert_eq!(scratch.head(), c2);
    assert_eq!(scratch.history().head_branch.as_deref(), Some("main"));
    assert_eq!(scratch.read("b.txt"), "b\n");
    // Up to date.
    assert_eq!(repo.merge("feature", true).unwrap(), Outcome::Done);
    assert_eq!(scratch.head(), c2);
    assert!(repo.merge("no-such-branch", false).is_err());

    // A merge commit although HEAD could fast-forward.
    repo.reset(&c1, ResetMode::Hard).unwrap();
    assert_eq!(repo.merge("feature", true).unwrap(), Outcome::Done);
    let merged = repo.load_commit(&scratch.head()).unwrap();
    assert_eq!(merged.parents, [c1, c2]);
    assert_eq!(merged.node_type, NodeType::Merge);
    assert_eq!(merged.summary, "Merge feature into main");
    assert_eq!(repo.state(), RepoState::None);
    assert!(repo.load_status().unwrap().is_empty());
    assert_eq!(scratch.read("b.txt"), "b\n");

    // Diverged without conflicts: a merge commit either way.
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let base = scratch.commit_file("a.txt", "1\n", "base");
    repo.create_branch("feature", &base, true).unwrap();
    let theirs = scratch.commit_file("b.txt", "b\n", "theirs");
    repo.checkout_branch("main").unwrap();
    let ours = scratch.commit_file("c.txt", "c\n", "ours");
    assert_eq!(repo.merge("feature", false).unwrap(), Outcome::Done);
    assert_eq!(
        repo.load_commit(&scratch.head()).unwrap().parents,
        [ours, theirs]
    );
    assert!(scratch.exists("b.txt") && scratch.exists("c.txt"));

    // Conflicts.
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let (_, ours, _, feature_tip) = diverged(&scratch);
    assert_eq!(repo.merge("feature", false).unwrap(), Outcome::Conflicts);
    assert_eq!(repo.state(), RepoState::Merge);
    assert_eq!(repo.incoming_head(), Some(feature_tip.clone()));
    assert_eq!(scratch.head(), ours);
    let status = repo.load_status().unwrap();
    assert_eq!(
        paths(&status.conflicted),
        [("f.txt", FileStatus::Conflicted)]
    );
    assert_eq!(paths(&status.staged), [("extra.txt", FileStatus::Added)]);
    let sides = repo.conflict_sides("f.txt").unwrap();
    assert_eq!(
        (
            sides.base.as_deref(),
            sides.ours.as_deref(),
            sides.theirs.as_deref()
        ),
        (Some("line\n"), Some("ours\n"), Some("theirs\n"))
    );
    assert!(
        repo.prepared_message()
            .is_some_and(|message| message.starts_with("Merge"))
    );
    assert_eq!(repo.summary().unwrap().modified, 1);

    // Aborted, then merged again and resolved.
    repo.abort_operation().unwrap();
    assert_eq!(repo.state(), RepoState::None);
    assert!(repo.load_status().unwrap().is_empty());
    assert!(!scratch.exists("extra.txt"));
    assert_eq!(repo.prepared_message(), None);

    assert_eq!(repo.merge("feature", false).unwrap(), Outcome::Conflicts);
    scratch.write("f.txt", "both\n");
    repo.stage(&["f.txt"]).unwrap();
    let merged = repo.commit("Merge feature", false).unwrap();
    assert_eq!(repo.state(), RepoState::None);
    assert_eq!(
        repo.load_commit(&merged).unwrap().parents,
        [ours, feature_tip]
    );
    assert!(repo.load_status().unwrap().is_empty());
}

#[test]
fn merge_into_an_unborn_branch() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let raw = scratch.raw();
    let c1 = commit_at(&raw, 1000, "c1", &[], &[("a.txt", "a\n")]);
    raw.reference("refs/heads/other", c1, true, "test").unwrap();
    assert!(scratch.history().unborn);
    assert_eq!(repo.merge("other", false).unwrap(), Outcome::Done);
    let history = scratch.history();
    assert_eq!(history.head_branch.as_deref(), Some("main"));
    assert_eq!(history.head_sha, Some(c1.to_string()));
    assert_eq!(scratch.read("a.txt"), "a\n");
}

#[test]
fn stashes() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.commit_file("a.txt", "a\n", "base");
    assert!(repo.stash_push("nothing to stash").is_err());
    assert_eq!(repo.stash_index_of(&scratch.head()), None);

    scratch.write("a.txt", "first stash\n");
    scratch.write("untracked.txt", "untracked\n");
    let first = repo.stash_push("first").unwrap();
    assert!(repo.load_status().unwrap().is_empty());
    assert!(!scratch.exists("untracked.txt"));
    assert_eq!(repo.stash_index_of(&first), Some(0));

    scratch.write("b.txt", "second stash\n");
    repo.stage(&["b.txt"]).unwrap();
    let second = repo.stash_push("").unwrap();
    assert_eq!(repo.stash_index_of(&second), Some(0));
    assert_eq!(repo.stash_index_of(&first), Some(1));

    let stash_refs = |scratch: &Scratch| -> Vec<(Option<usize>, String, String)> {
        scratch
            .history()
            .refs
            .into_iter()
            .filter(|r| r.ref_type == RefType::Stash)
            .map(|r| (r.stash_index, r.target, r.message))
            .collect()
    };
    let listed = stash_refs(&scratch);
    assert_eq!(listed.len(), 2);
    assert!(listed.contains(&(Some(1), first.clone(), "On main: first".to_owned())));
    let git_message = &listed.iter().find(|(_, sha, _)| *sha == second).unwrap().2;
    assert!(git_message.starts_with("WIP on main: "), "{git_message}");

    // Renaming keeps the place in the list and git's prefix.
    repo.stash_set_message(&first, "renamed").unwrap();
    repo.stash_set_message(&second, "was a WIP").unwrap();
    let listed = stash_refs(&scratch);
    assert!(listed.contains(&(Some(1), first.clone(), "On main: renamed".to_owned())));
    assert!(listed.contains(&(Some(0), second.clone(), "On main: was a WIP".to_owned())));
    assert!(repo.stash_set_message(&scratch.head(), "x").is_err());
    let history = scratch.history();
    assert_eq!(history.commit(&first).unwrap().summary, "renamed");
    assert_eq!(history.commit(&first).unwrap().node_type, NodeType::Stash);

    // Apply keeps the stash.
    repo.stash_apply(1).unwrap();
    assert_eq!(scratch.read("a.txt"), "first stash\n");
    assert!(scratch.exists("untracked.txt"));
    assert_eq!(stash_refs(&scratch).len(), 2);
    repo.discard_all().unwrap();
    assert!(repo.load_status().unwrap().is_empty());

    // Pop drops it and reinstates the staged file as staged.
    repo.stash_pop(0).unwrap();
    assert_eq!(
        paths(&repo.load_status().unwrap().staged),
        [("b.txt", FileStatus::Added)]
    );
    assert_eq!(repo.stash_index_of(&second), None);
    assert_eq!(repo.stash_index_of(&first), Some(0));
    // The index this handle holds was refreshed: a commit right away has the popped file.
    let committed = repo.commit("popped", false).unwrap();
    assert_eq!(
        repo.commit_tree_paths(&committed).unwrap(),
        ["a.txt", "b.txt"]
    );

    repo.stash_drop(0).unwrap();
    assert_eq!(repo.stash_index_of(&first), None);
    assert!(stash_refs(&scratch).is_empty());
    assert!(repo.stash_drop(0).is_err());
}

#[test]
fn rewrite_history_reword_squash_drop() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let base = scratch.commit_file("base.txt", "base\n", "base");
    let one = scratch.commit_file("a.txt", "a\n", "one");
    let two = scratch.commit_file("b.txt", "b\n", "two");
    let three = scratch.commit_file("c.txt", "c\n", "three");
    let four = scratch.commit_file("d.txt", "d\n", "four");
    let step = |sha: &Sha, action| RebaseStep {
        sha: sha.clone(),
        action,
    };

    // A dirty working directory is refused.
    scratch.write("dirty.txt", "x\n");
    let error = repo.rewrite_history(&base, &[]).unwrap_err();
    assert!(error.to_string().contains("must be clean"), "{error}");
    assert_eq!(scratch.head(), four);
    fs::remove_file(scratch.root().join("dirty.txt")).unwrap();

    repo.rewrite_history(
        &base,
        &[
            step(&one, RebaseAction::Reword(Some("ONE".to_owned()))),
            step(&two, RebaseAction::Squash(None)),
            step(&three, RebaseAction::Drop),
            step(&four, RebaseAction::Pick),
        ],
    )
    .unwrap();
    assert_eq!(repo.state(), RepoState::None);
    assert!(repo.load_status().unwrap().is_empty());
    let history = scratch.history();
    assert_eq!(history.head_branch.as_deref(), Some("main"));
    let summaries: Vec<&str> = history.commits.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["four", "ONE", "base"]);
    let squashed = &history.commits[1];
    assert_eq!(squashed.body, "two");
    assert_eq!(squashed.parents, [base.as_str()]);
    assert_eq!(
        repo.commit_tree_paths(&squashed.sha).unwrap(),
        ["a.txt", "b.txt", "base.txt"]
    );
    let top = &history.commits[0];
    assert_eq!(
        top.author_time,
        repo.load_commit(&four).unwrap().author_time
    );
    assert_eq!(
        repo.commit_tree_paths(&top.sha).unwrap(),
        ["a.txt", "b.txt", "base.txt", "d.txt"]
    );
    assert!(!scratch.exists("c.txt"));

    // A squash with its own message; a leading squash has nothing to fold into and is picked.
    let (one, four) = (
        history.commits[1].sha.clone(),
        history.commits[0].sha.clone(),
    );
    repo.rewrite_history(
        &base,
        &[
            step(&one, RebaseAction::Squash(Some("ignored".to_owned()))),
            step(&four, RebaseAction::Squash(Some("everything".to_owned()))),
        ],
    )
    .unwrap();
    let history = scratch.history();
    let summaries: Vec<&str> = history.commits.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["everything", "base"]);
    assert_eq!(
        repo.commit_tree_paths(&history.commits[0].sha).unwrap(),
        ["a.txt", "b.txt", "base.txt", "d.txt"]
    );
}

#[test]
fn rewrite_history_rolls_back_on_conflict() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let base = scratch.commit_file("f.txt", "0\n", "base");
    let one = scratch.commit_file("f.txt", "1\n", "one");
    let two = scratch.commit_file("f.txt", "2\n", "two");
    let error = repo
        .rewrite_history(
            &base,
            &[
                RebaseStep {
                    sha: one,
                    action: RebaseAction::Drop,
                },
                RebaseStep {
                    sha: two.clone(),
                    action: RebaseAction::Pick,
                },
            ],
        )
        .unwrap_err();
    let text = error.to_string();
    assert!(text.starts_with("Conflicts while applying "), "{text}");
    assert!(text.contains(&two[..7]), "{text}");
    assert!(text.ends_with("The branch was restored."), "{text}");
    assert_eq!(scratch.head(), two);
    assert_eq!(repo.state(), RepoState::None);
    assert!(repo.load_status().unwrap().is_empty());
    assert_eq!(scratch.read("f.txt"), "2\n");
}

#[test]
fn blame() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let c1 = scratch.commit_file("f.txt", "one\ntwo\nthree\n", "first");
    let c2 = scratch.commit_file("f.txt", "one\nTWO\nthree\n", "second\n\nbody");

    let blame = repo.blame_file("f.txt", None).unwrap();
    let hunks: Vec<_> = blame
        .hunks
        .iter()
        .map(|hunk| {
            (
                hunk.start_line,
                hunk.line_count,
                hunk.sha.as_str(),
                hunk.summary.as_str(),
            )
        })
        .collect();
    assert_eq!(
        hunks,
        [
            (1, 1, c1.as_str(), "first"),
            (2, 1, c2.as_str(), "second"),
            (3, 1, c1.as_str(), "first")
        ]
    );
    let hunk = blame.hunk_at(2).unwrap();
    assert_eq!(hunk.author.name, "Tester");
    assert_eq!(hunk.orig_path, "f.txt");
    assert_eq!(hunk.orig_start_line, 2);
    assert_eq!(hunk.time, repo.load_commit(&c2).unwrap().author_time);

    // As of the first commit.
    let blame = repo.blame_file("f.txt", Some(&c1)).unwrap();
    assert_eq!(blame.hunks.len(), 1);
    assert_eq!(
        (blame.hunks[0].line_count, blame.hunks[0].sha.as_str()),
        (3, c1.as_str())
    );
    assert!(repo.blame_file("missing.txt", None).is_err());
}

#[test]
fn file_history_across_a_rename() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let text = "a file with enough content\nto be found again\nunder another name\n";
    let added = scratch.commit_file("old.txt", text, "add");
    let unrelated = scratch.commit_file("other.txt", "x\n", "unrelated");
    let changed = scratch.commit_file("old.txt", &format!("{text}more\n"), "change");
    fs::rename(
        scratch.root().join("old.txt"),
        scratch.root().join("new.txt"),
    )
    .unwrap();
    repo.stage(&["old.txt", "new.txt"]).unwrap();
    let renamed = repo.commit("rename", false).unwrap();
    let edited = scratch.commit_file("new.txt", &format!("{text}more\nand more\n"), "edit");
    fs::remove_file(scratch.root().join("new.txt")).unwrap();
    repo.stage(&["new.txt"]).unwrap();
    let deleted = repo.commit("delete", false).unwrap();

    let history = repo.file_history("new.txt", None, 100).unwrap();
    let entries: Vec<_> = history
        .entries
        .iter()
        .map(|entry| {
            (
                entry.commit.sha.as_str(),
                entry.status,
                entry.path.as_str(),
                entry.old_path.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        entries,
        [
            (deleted.as_str(), FileStatus::Deleted, "new.txt", None),
            (edited.as_str(), FileStatus::Modified, "new.txt", None),
            (
                renamed.as_str(),
                FileStatus::Renamed,
                "new.txt",
                Some("old.txt")
            ),
            (changed.as_str(), FileStatus::Modified, "old.txt", None),
            (added.as_str(), FileStatus::Added, "old.txt", None),
        ]
    );
    assert!(!history.has_more);
    assert!(entries.iter().all(|entry| entry.0 != unrelated));

    // From a given commit, and capped.
    let history = repo.file_history("old.txt", Some(&changed), 100).unwrap();
    assert_eq!(history.entries.len(), 2);
    let history = repo.file_history("new.txt", None, 2).unwrap();
    assert!(history.has_more);
    assert_eq!(history.entries.len(), 2);
}

#[test]
fn file_history_of_a_file_renamed_on_a_merged_branch() {
    // A merge counts against its first parent only (`git log --full-history`): here the first parent
    // (main) has the file under its old name, so the merge is the commit that "renamed" it, and the
    // walk then follows the old name down both sides.
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let text = "a file with enough content
to be found again
under another name
";
    let added = scratch.commit_file("old.txt", text, "add");
    scratch.git(&["checkout", "-b", "feature"]);
    scratch.git(&["mv", "old.txt", "new.txt"]);
    scratch.git(&["commit", "-m", "rename"]);
    scratch.write(
        "new.txt",
        &format!(
            "{text}edit
"
        ),
    );
    scratch.git(&["commit", "-a", "-m", "edit"]);
    scratch.git(&["checkout", "main"]);
    scratch.write(
        "other.txt",
        "x
",
    );
    scratch.git(&["add", "other.txt"]);
    scratch.git(&["commit", "-m", "unrelated"]);
    scratch.git(&["merge", "--no-ff", "feature", "-m", "merge feature"]);

    let history = repo.file_history("new.txt", None, 100).unwrap();
    let listed: Vec<_> = history
        .entries
        .iter()
        .map(|entry| {
            (
                entry.commit.summary.as_str(),
                entry.status,
                entry.old_path.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("merge feature", FileStatus::Renamed, Some("old.txt")),
            ("rename", FileStatus::Deleted, None),
            ("add", FileStatus::Added, None),
        ]
    );
    assert_eq!(history.entries[2].commit.sha, added);
    assert!(
        history
            .entries
            .iter()
            .all(|e| e.commit.summary != "unrelated")
    );
}

#[test]
fn diff_lines_between_two_texts() {
    assert!(diff_lines("a\nb\n", "a\nb\n").is_empty());
    assert_eq!(
        diff_lines("a\nb\nc\n", "a\nB\nc\n"),
        [LineHunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1
        }]
    );
    // An insertion and a deletion: the empty side names the line the change comes after.
    assert_eq!(
        diff_lines("a\nb\nc\nd\n", "a\nx\ny\nb\nc\n"),
        [
            LineHunk {
                old_start: 1,
                old_count: 0,
                new_start: 1,
                new_count: 2
            },
            LineHunk {
                old_start: 3,
                old_count: 1,
                new_start: 5,
                new_count: 0
            }
        ]
    );
    assert_eq!(
        diff_lines("", "a\n"),
        [LineHunk {
            old_start: 0,
            old_count: 0,
            new_start: 0,
            new_count: 1
        }]
    );
}

#[test]
fn apply_lines_stages_and_unstages_single_lines() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.commit_file("f.txt", "1\n2\n3\n", "base");
    scratch.write("f.txt", "1\nA\n2\nB\n3\n");
    let staged_lines = |repo: &Repository| -> Vec<String> {
        repo.workdir_file_diff("f.txt", true, DiffOptions::default())
            .unwrap()
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.origin != LineOrigin::Context)
            .map(|line| format!("{}{}", line.origin.as_char(), line.text))
            .collect()
    };

    repo.apply_lines("f.txt", &[], false).unwrap();
    assert!(staged_lines(repo).is_empty());

    // Only "A" (line 2 of the new file).
    repo.apply_lines("f.txt", &[added(2)], false).unwrap();
    assert_eq!(staged_lines(repo), ["+A"]);
    let unstaged = repo
        .workdir_file_diff("f.txt", false, DiffOptions::default())
        .unwrap();
    assert_eq!(unstaged.stats.additions, 1);
    assert_eq!(
        scratch.read("f.txt"),
        "1\nA\n2\nB\n3\n",
        "the file itself is untouched"
    );

    // The rest: every change left is picked, so the file is staged whole.
    let selection: Vec<LineSelection> = unstaged
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter_map(LineSelection::of)
        .collect();
    assert_eq!(selection, [added(4)]);
    repo.apply_lines("f.txt", &selection, false).unwrap();
    assert_eq!(staged_lines(repo), ["+A", "+B"]);
    assert!(repo.load_status().unwrap().unstaged.is_empty());

    // Unstage "B" alone (line 4 of the staged file), then the rest.
    repo.apply_lines("f.txt", &[added(4)], true).unwrap();
    assert_eq!(staged_lines(repo), ["+A"]);
    repo.apply_lines("f.txt", &[added(2)], true).unwrap();
    assert!(staged_lines(repo).is_empty());
    assert!(
        repo.apply_lines("f.txt", &[added(2)], true).is_err(),
        "nothing staged"
    );

    // A deletion and a replacement, staged one line at a time.
    repo.stage(&["f.txt"]).unwrap();
    repo.commit("with letters", false).unwrap();
    scratch.write("f.txt", "1\n2\nb\n3\nnew\n");
    let deleted = LineSelection {
        origin: LineOrigin::Deleted,
        lineno: 2,
    };
    repo.apply_lines("f.txt", &[deleted], false).unwrap();
    assert_eq!(staged_lines(repo), ["-A"]);
    repo.apply_lines("f.txt", &[added(5)], false).unwrap();
    assert_eq!(staged_lines(repo), ["-A", "+new"]);

    // Part of a new file.
    scratch.write("fresh.txt", "x\ny\nz\n");
    repo.apply_lines("fresh.txt", &[added(2)], false).unwrap();
    let diff = repo
        .workdir_file_diff("fresh.txt", true, DiffOptions::default())
        .unwrap();
    assert_eq!(diff.stats.additions, 1);
    assert_eq!(diff.hunks[0].lines[0].text, "y");
}

#[test]
fn refs_signature_follows_the_refs() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let unborn = repo.refs_signature().unwrap();
    let c1 = scratch.commit_file("a.txt", "1\n", "c1");
    let mut seen = vec![unborn];
    let mut changed = |what: &str| {
        let signature = repo.refs_signature().unwrap();
        assert!(
            !seen.contains(&signature),
            "{what} did not change the signature"
        );
        seen.push(signature);
    };
    changed("a commit");
    assert_eq!(
        repo.refs_signature().unwrap(),
        repo.refs_signature().unwrap()
    );

    repo.create_tag("v1", &c1, None).unwrap();
    changed("a tag");
    repo.create_branch("other", &c1, false).unwrap();
    changed("a branch");
    // Two branches at one commit: the checkout still counts.
    repo.checkout_branch("other").unwrap();
    changed("a checkout");
    scratch.write("a.txt", "stash me\n");
    repo.stash_push("one").unwrap();
    changed("a stash");
    scratch.write("a.txt", "stash me too\n");
    repo.stash_push("two").unwrap();
    changed("a second stash");
    // Dropping the one below the top leaves refs/stash where it is.
    repo.stash_drop(1).unwrap();
    changed("a dropped stash");
    repo.delete_tag("v1").unwrap();
    changed("a deleted tag");
    scratch.commit_file("a.txt", "2\n", "c2");
    changed("another commit");
}

#[test]
fn configuration() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    assert_eq!(repo.config_string("user.name").as_deref(), Some("Tester"));
    assert_eq!(repo.config_string("imgit.test-unset"), None);
    assert!(repo.config_bool("imgit.test-flag", true));
    assert!(!repo.config_bool("imgit.test-flag", false));
    repo.set_local_config("imgit.test-flag", Some("true"))
        .unwrap();
    assert!(repo.config_bool("imgit.test-flag", false));
    repo.set_local_config("imgit.test-flag", None).unwrap();
    assert!(!repo.config_bool("imgit.test-flag", false));
    repo.set_local_config("imgit.test-flag", None).unwrap();
    repo.set_local_config("imgit.test-flag", Some("yes"))
        .unwrap();
    repo.set_local_config("imgit.test-flag", Some("")).unwrap();
    assert_eq!(repo.config_string("imgit.test-flag"), None);

    // The commit template, relative to the working directory.
    scratch.write("template.txt", "# a comment\nSubject\n\nBody\n\n");
    repo.set_local_config("commit.template", Some("template.txt"))
        .unwrap();
    assert_eq!(repo.commit_template().as_deref(), Some("Subject\n\nBody"));
    repo.set_local_config("commit.template", Some("missing.txt"))
        .unwrap();
    assert_eq!(repo.commit_template(), None);
    assert_eq!(repo.prepared_message(), None);

    // Hooks: in the hooks directory core.hooksPath names.
    repo.set_local_config("core.hooksPath", Some("my-hooks"))
        .unwrap();
    assert!(!repo.has_commit_hooks());
    scratch.write("my-hooks/pre-push", "#!/bin/sh\n");
    assert!(!repo.has_commit_hooks());
    scratch.write("my-hooks/commit-msg", "#!/bin/sh\n");
    assert!(repo.has_commit_hooks());

    // Without an identity of its own, a repository commits as the profile.
    repo.set_local_config("user.name", None).unwrap();
    repo.set_local_config("user.email", None).unwrap();
    set_profile_identity("Profile", "profile@example.com");
    let author = repo.default_author().unwrap();
    assert_eq!(
        (author.name.as_str(), author.email.as_str()),
        ("Profile", "profile@example.com")
    );
    set_profile_identity("", "");
    repo.set_local_config("user.name", Some("Local")).unwrap();
    repo.set_local_config("user.email", Some("local@example.com"))
        .unwrap();
    set_profile_identity("Profile", "profile@example.com");
    assert_eq!(repo.default_author().unwrap().name, "Local");
    set_profile_identity("", "");
}

#[test]
fn commit_with_git_runs_git_commit() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.write("a.txt", "a\n");
    repo.stage(&["a.txt"]).unwrap();
    let options = CommitOptions {
        strip_comments: true,
        ..CommitOptions::default()
    };
    let first = repo
        .commit_with_git("subject\n\n# a comment\nbody\n", &options)
        .unwrap();
    assert_eq!(first, scratch.head());
    assert_eq!(repo.head_commit_message().unwrap(), "subject\n\nbody\n");
    let commit = repo.load_commit(&first).unwrap();
    assert_eq!(commit.author.name, "Tester");
    assert!(repo.load_status().unwrap().is_empty());

    // Nothing to commit: git's own words.
    let error = repo
        .commit_with_git("empty", &CommitOptions::default())
        .unwrap_err();
    assert!(error.to_string().contains("nothing to commit"), "{error}");
    assert!(!error.to_string().ends_with('\n'));

    // Amend, with a hook that refuses unless it is skipped.
    scratch.write("a.txt", "amended\n");
    repo.stage(&["a.txt"]).unwrap();
    scratch.write(
        ".git/hooks/pre-commit",
        "#!/bin/sh\necho refused by the hook\nexit 1\n",
    );
    // Git only runs executable hooks on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            scratch.root().join(".git/hooks/pre-commit"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    assert!(repo.has_commit_hooks());
    let amend = CommitOptions {
        amend: true,
        ..CommitOptions::default()
    };
    let error = repo.commit_with_git("amended", &amend).unwrap_err();
    assert!(error.to_string().contains("refused by the hook"), "{error}");
    assert_eq!(scratch.head(), first);
    let amended = repo
        .commit_with_git(
            "amended",
            &CommitOptions {
                skip_hooks: true,
                ..amend
            },
        )
        .unwrap();
    assert_ne!(amended, first);
    assert!(repo.load_commit(&amended).unwrap().parents.is_empty());
    assert_eq!(repo.head_commit_message().unwrap(), "amended\n");
    // The index this handle holds follows what git wrote.
    scratch.write("b.txt", "b\n");
    repo.stage(&["b.txt"]).unwrap();
    let next = repo.commit("next", false).unwrap();
    assert_eq!(repo.commit_tree_paths(&next).unwrap(), ["a.txt", "b.txt"]);
    assert_eq!(scratch.read("a.txt"), "amended\n");
}

/// The index's content of `path`.
fn index_blob(scratch: &Scratch, path: &str) -> Option<Vec<u8>> {
    let raw = scratch.raw();
    let index = raw.index().unwrap();
    let entry = index.get_path(std::path::Path::new(path), 0)?;
    Some(raw.find_blob(entry.id).unwrap().content().to_vec())
}

fn deleted(lineno: u32) -> LineSelection {
    LineSelection {
        origin: LineOrigin::Deleted,
        lineno,
    }
}

/// A repository of its own inside the working directory, with a commit.
fn nested_repository(scratch: &Scratch, path: &str) {
    scratch.write(&format!("{path}/inner.txt"), "inner\n");
    let root = scratch.root().join(path);
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Inner",
            "-c",
            "user.email=inner@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "inner history",
        ],
    ] {
        let output = cli::run(&root, &args, cli::RunOptions::default());
        assert!(output.success(), "git {args:?}: {}", output.text);
    }
}

#[test]
fn discard_never_deletes_a_nested_repository() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.commit_file("tracked.txt", "base\n", "base");
    scratch.write("junk.txt", "junk\n");
    scratch.write("plain/x.txt", "x\n");
    scratch.write("wrapper/file.txt", "f\n");
    nested_repository(&scratch, "embedded");
    nested_repository(&scratch, "wrapper/deep");

    let status = repo.load_status().unwrap();
    let error = repo.discard(&status.unstaged).unwrap_err().to_string();
    assert!(error.contains("embedded"), "{error}");
    assert!(error.contains("wrapper/deep"), "{error}");
    assert!(!scratch.exists("junk.txt"));
    assert!(!scratch.exists("plain/x.txt"));
    assert!(!scratch.exists("wrapper/file.txt"));
    assert!(scratch.exists("embedded/.git"));
    assert_eq!(scratch.read("embedded/inner.txt"), "inner\n");
    assert!(scratch.exists("wrapper/deep/.git"));

    // Reset All leaves them alone as well, after doing the rest.
    scratch.write("again.txt", "again\n");
    assert!(repo.discard_all().is_err());
    assert!(!scratch.exists("again.txt"));
    assert!(scratch.exists("embedded/.git"));
    assert!(scratch.exists("wrapper/deep/.git"));
}

#[test]
fn discard_removes_the_folders_it_left_empty() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.commit_file(
        "keep/tracked.txt",
        "base
",
        "base",
    );
    scratch.write(
        "keep/new.txt",
        "n
",
    );
    scratch.write(
        "gone/a/b/one.txt",
        "1
",
    );
    scratch.write(
        "gone/a/two.txt",
        "2
",
    );
    scratch.write(
        "files/x.txt",
        "x
",
    );
    scratch.write(
        "files/y.txt",
        "y
",
    );
    fs::create_dir_all(scratch.root().join("unrelated-empty")).unwrap();

    let status = repo.load_status().unwrap();
    repo.discard(&status.unstaged).unwrap();
    assert!(!scratch.exists("keep/new.txt"));
    assert!(scratch.exists("keep/tracked.txt"));
    assert!(!scratch.exists("gone"), "a folder discard leaves no folder");
    assert!(
        !scratch.exists("files"),
        "a folder discard leaves no folder"
    );
    // Only what the discard emptied goes.
    assert!(scratch.exists("unrelated-empty"));
    assert!(scratch.root().is_dir());

    // A single deleted file: its parents go up to the first one that still holds something.
    scratch.write(
        "keep/deep/er/z.txt",
        "z
",
    );
    let file = FileChange {
        path: "keep/deep/er/z.txt".to_owned(),
        old_path: None,
        status: FileStatus::Untracked,
        stats: None,
    };
    repo.discard(&[file]).unwrap();
    assert!(!scratch.exists("keep/deep"));
    assert!(scratch.exists("keep/tracked.txt"));
}

#[test]
fn apply_lines_keeps_bytes_that_are_not_utf8() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    fs::write(scratch.root().join("f.bin"), b"x\n\xff\ny\n").unwrap();
    repo.stage(&["f.bin"]).unwrap();
    repo.commit("base", false).unwrap();
    fs::write(scratch.root().join("f.bin"), b"x\n\xff\nNEW\n\xfe\ny\n").unwrap();

    repo.apply_lines("f.bin", &[added(3)], false).unwrap();
    assert_eq!(
        index_blob(&scratch, "f.bin").unwrap(),
        b"x\n\xff\nNEW\ny\n".to_vec()
    );
    // And back out.
    repo.apply_lines("f.bin", &[added(3)], true).unwrap();
    assert_eq!(
        index_blob(&scratch, "f.bin").unwrap(),
        b"x\n\xff\ny\n".to_vec()
    );
}

#[test]
fn apply_lines_after_a_last_line_without_newline() {
    // (the file as it is now, the line picked, the index afterwards)
    let cases: [(&str, u32, &[u8]); 3] = [
        ("a\nb\nc\n", 3, b"a\nb\nc\n"),
        // The diff does not re-add the old last line at all.
        ("a\nx\ny\n", 3, b"a\nb\ny\n"),
        // Only the old line's new ending picked.
        ("a\nb\nc\n", 2, b"a\nb\n"),
    ];
    for (text, picked, expected) in cases {
        let scratch = Scratch::new();
        scratch.commit_file("f.txt", "a\nb", "base");
        scratch.write("f.txt", text);
        scratch
            .repo
            .apply_lines("f.txt", &[added(picked)], false)
            .unwrap();
        assert_eq!(index_blob(&scratch, "f.txt").unwrap(), expected, "{text:?}");
    }
}

#[test]
fn unstaging_lines_after_a_last_line_without_newline() {
    // HEAD "a\nb" (no ending), index "a\nb\nc\n": the diff is -b (old 2), +b\n (new 2), +c (new 3).
    let cases: [(&[LineSelection], &[u8]); 7] = [
        (&[added(3)], b"a\nb\n"),
        // The line goes away as a whole.
        (&[added(2)], b"a\nc\n"),
        // The old line cannot come back unterminated with `c` staying after it: nothing is glued or doubled.
        (&[deleted(2)], b"a\nb\nc\n"),
        (&[deleted(2), added(2)], b"a\nb\nc\n"),
        (&[deleted(2), added(3)], b"a\nb\n"),
        // Nothing stays after it: it comes back as it was.
        (&[deleted(2), added(2), added(3)], b"a\nb"),
        (&[added(2), added(3)], b"a\n"),
    ];
    for (picked, expected) in cases {
        let scratch = Scratch::new();
        scratch.commit_file("f.txt", "a\nb", "base");
        scratch.write("f.txt", "a\nb\nc\n");
        scratch.repo.stage(&["f.txt"]).unwrap();
        scratch.repo.apply_lines("f.txt", picked, true).unwrap();
        assert_eq!(
            index_blob(&scratch, "f.txt").unwrap(),
            expected,
            "{picked:?}"
        );
    }
}

#[test]
fn unstaging_lines_when_the_index_loses_the_last_newline() {
    // HEAD "a\nb\n", index "a\nb" (no ending): -b\n (old 2), +b (new 2).
    let cases: [(&[LineSelection], &[u8]); 2] =
        [(&[added(2)], b"a\n"), (&[deleted(2)], b"a\nb\nb")];
    for (picked, expected) in cases {
        let scratch = Scratch::new();
        scratch.commit_file("f.txt", "a\nb\n", "base");
        scratch.write("f.txt", "a\nb");
        scratch.repo.stage(&["f.txt"]).unwrap();
        scratch.repo.apply_lines("f.txt", picked, true).unwrap();
        assert_eq!(
            index_blob(&scratch, "f.txt").unwrap(),
            expected,
            "{picked:?}"
        );
    }
}

#[test]
fn cherry_pick_of_a_change_already_in_head_commits_nothing() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let base = scratch.commit_file("f.txt", "x\n", "base");
    repo.create_branch("feature", &base, true).unwrap();
    let picked = scratch.commit_file("f.txt", "y\n", "same change on feature");
    repo.checkout_branch("main").unwrap();
    let head = scratch.commit_file("f.txt", "y\n", "same change on main");

    let error = repo.cherry_pick(&picked).unwrap_err().to_string();
    assert!(error.contains("already in the current branch"), "{error}");
    assert_eq!(scratch.head(), head);
    assert_eq!(repo.state(), RepoState::None);
    assert_eq!(scratch.read("f.txt"), "y\n");
}

#[test]
fn revert_of_a_change_already_undone_commits_nothing() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.commit_file("f.txt", "x\n", "base");
    let change = scratch.commit_file("f.txt", "y\n", "change");
    let head = scratch.commit_file("f.txt", "x\n", "change back by hand");

    let error = repo.revert(&change).unwrap_err().to_string();
    assert!(error.contains("already in the current branch"), "{error}");
    assert_eq!(scratch.head(), head);
    assert_eq!(repo.state(), RepoState::None);
}

#[test]
fn apply_lines_on_an_untracked_file_in_an_untracked_folder() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.commit_file("base.txt", "base\n", "base");
    scratch.write("dir/new.txt", "x\ny\nz\n");
    repo.apply_lines("dir/new.txt", &[added(2)], false).unwrap();
    assert_eq!(index_blob(&scratch, "dir/new.txt").unwrap(), b"y\n");
}

#[test]
fn commit_after_a_conflicted_cherry_pick_keeps_the_author() {
    for with_git in [false, true] {
        let scratch = Scratch::new();
        let repo = &scratch.repo;
        let base = scratch.commit_file("f.txt", "line\n", "base");
        repo.create_branch("feature", &base, true).unwrap();
        repo.set_local_config("user.name", Some("Other")).unwrap();
        repo.set_local_config("user.email", Some("other@example.com"))
            .unwrap();
        let theirs = scratch.commit_file("f.txt", "theirs\n", "feature changes f");
        repo.set_local_config("user.name", Some("Tester")).unwrap();
        repo.set_local_config("user.email", Some("tester@example.com"))
            .unwrap();
        repo.checkout_branch("main").unwrap();
        scratch.commit_file("f.txt", "ours\n", "main changes f");

        assert_eq!(repo.cherry_pick(&theirs).unwrap(), Outcome::Conflicts);
        scratch.write("f.txt", "resolved\n");
        repo.stage(&["f.txt"]).unwrap();
        let resolved = if with_git {
            repo.commit_with_git("resolved pick", &CommitOptions::default())
                .unwrap()
        } else {
            repo.commit("resolved pick", false).unwrap()
        };
        let original = repo.load_commit(&theirs).unwrap();
        let commit = repo.load_commit(&resolved).unwrap();
        assert_eq!(commit.author.name, "Other", "with_git: {with_git}");
        assert_eq!(commit.author, original.author, "with_git: {with_git}");
        assert_eq!(commit.author_time, original.author_time);
    }
}

#[test]
fn conflict_sides_refuses_files_that_are_not_utf8_text() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    let base = scratch.commit_file("f.dat", "base\n", "base");
    repo.create_branch("feature", &base, true).unwrap();
    fs::write(scratch.root().join("f.dat"), b"\xff\xfe theirs\n").unwrap();
    repo.stage(&["f.dat"]).unwrap();
    let theirs = repo.commit("theirs", false).unwrap();
    repo.checkout_branch("main").unwrap();
    fs::write(scratch.root().join("f.dat"), b"\xff ours\n").unwrap();
    repo.stage(&["f.dat"]).unwrap();
    repo.commit("ours", false).unwrap();

    assert_eq!(repo.cherry_pick(&theirs).unwrap(), Outcome::Conflicts);
    let error = repo.conflict_sides("f.dat").unwrap_err().to_string();
    assert!(error.contains("not UTF-8"), "{error}");
}

#[test]
fn apply_lines_unstages_part_of_a_staged_deletion() {
    let scratch = Scratch::new();
    let repo = &scratch.repo;
    scratch.commit_file("f.txt", "1\n2\n3\n", "base");
    fs::remove_file(scratch.root().join("f.txt")).unwrap();
    repo.stage(&["f.txt"]).unwrap();
    assert_eq!(index_blob(&scratch, "f.txt"), None);

    repo.apply_lines("f.txt", &[deleted(2)], true).unwrap();
    assert_eq!(index_blob(&scratch, "f.txt").unwrap(), b"2\n");
}
