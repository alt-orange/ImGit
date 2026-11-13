//! `RepoState` against scratch repositories. The global git configuration is only ever read.

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use imgit_app::repo_state::{
    DiffMode, DiffSource, DiffTarget, FileEdit, RepoState, UndoEntry, WIP_SHA,
};
use imgit_git::repository::Outcome;
use imgit_git::{
    FileStatus, LineOrigin, NodeType, RebaseAction, RefType, RepoState as OperationState,
    Repository, ResetMode, Sha,
};

/// A repository tab on a temporary folder, on `main`, with its own identity.
struct Scratch {
    dir: tempfile::TempDir,
    state: RepoState,
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
        Self {
            dir,
            state: RepoState::new(repo),
        }
    }

    /// A repository with `count` commits on `main`, each adding a line to `a.txt`: "commit 1" is the root.
    fn with_commits(count: usize) -> (Self, Vec<Sha>) {
        let mut scratch = Self::new();
        let mut text = String::new();
        let shas = (1..=count)
            .map(|number| {
                text.push_str(&format!("line {number}\n"));
                scratch.commit_file("a.txt", &text, &format!("commit {number}"))
            })
            .collect();
        scratch.state.refresh();
        (scratch, shas)
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.root().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    /// Writes, stages and commits one file; the state is not refreshed.
    fn commit_file(&mut self, path: &str, text: &str, message: &str) -> Sha {
        self.write(path, text);
        self.state.repo.stage(&[path]).unwrap();
        self.state.repo.commit(message, false).unwrap()
    }

    fn row_shas(&self) -> Vec<&str> {
        self.state
            .history
            .commits
            .iter()
            .map(|commit| commit.sha.as_str())
            .collect()
    }
}

/// Calls `step` until it says it is done, for at most half a minute.
fn wait_until(mut step: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !step() {
        assert!(Instant::now() < deadline, "the background job never ended");
        thread::sleep(Duration::from_millis(5));
    }
}

// Loading

#[test]
fn an_empty_repository_loads() {
    let scratch = Scratch::new();
    let state = &scratch.state;
    assert_eq!(state.load_error, None);
    assert!(state.history.commits.is_empty());
    assert!(state.history.unborn);
    assert!(state.single_selected().is_none());
    assert!(state.head_ref().is_none());
    assert!(state.commits_above(None, None).is_empty());
    assert!(!state.busy());
    assert!(std::ptr::eq(state.root(), state));
}

#[test]
fn refresh_loads_the_history_newest_first() {
    let (scratch, shas) = Scratch::with_commits(3);
    let state = &scratch.state;
    assert_eq!(state.load_error, None);
    assert_eq!(scratch.row_shas(), [&shas[2], &shas[1], &shas[0]]);
    assert_eq!(state.history.head_sha.as_ref(), Some(&shas[2]));
    assert_eq!(state.history.head_branch.as_deref(), Some("main"));
    assert_eq!(state.layout.rows.len(), 3);
    assert!(state.status.is_empty());
    assert_eq!(state.operation_state, OperationState::None);
    assert_eq!(state.commit_for(&shas[1]).unwrap().summary, "commit 2");
    assert!(state.commit_for(WIP_SHA).is_none());
    assert!(state.commit_for("no such sha").is_none());
}

#[test]
fn the_commit_limit_cuts_the_walk() {
    let (mut scratch, shas) = Scratch::with_commits(3);
    scratch.state.commit_limit = 2;
    scratch.state.refresh();
    assert_eq!(scratch.row_shas(), [&shas[2], &shas[1]]);
    assert!(scratch.state.history.has_more);
}

#[test]
fn the_wip_row_comes_first_while_the_tree_is_dirty() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    scratch.write("new.txt", "untracked\n");
    scratch.state.refresh();
    {
        let state = &scratch.state;
        assert_eq!(scratch.row_shas(), [WIP_SHA, &shas[1], &shas[0]]);
        let wip = &state.history.commits[0];
        assert_eq!(wip.node_type, NodeType::WorkDir);
        assert_eq!(wip.summary, "// WIP");
        assert_eq!(wip.parents, [shas[1].as_str()]);
        assert_eq!(wip.author.name, "Tester");
        assert!(wip.time > 0);
        // The rows are indexed again behind it.
        assert_eq!(state.history.row_by_sha[WIP_SHA], 0);
        assert_eq!(state.history.row_by_sha[&shas[1]], 1);
        assert_eq!(state.history.row_by_sha[&shas[0]], 2);
        assert_eq!(state.layout.rows.len(), 3);
        assert_eq!(state.status.unstaged.len(), 1);
        assert_eq!(state.status.unstaged[0].stats.unwrap().additions, 1);
    }

    // Clean again: the cheap status read drops the row.
    scratch.state.select_only(WIP_SHA);
    fs::remove_file(scratch.root().join("new.txt")).unwrap();
    scratch.state.refresh_status();
    assert_eq!(scratch.row_shas(), [&shas[1], &shas[0]]);
    assert_eq!(scratch.state.history.row_by_sha[&shas[1]], 0);
    assert!(scratch.state.status.is_empty());
    // The selection of the row went with it.
    assert!(scratch.state.selected.is_empty());
    assert!(!scratch.state.is_selected(WIP_SHA));
    assert_eq!(scratch.state.focused, None);

    // And the cheap read brings it back.
    scratch.write("a.txt", "changed\n");
    scratch.state.refresh_status();
    assert_eq!(scratch.row_shas()[0], WIP_SHA);
}

#[test]
fn the_wip_row_of_an_unborn_branch_has_no_parent() {
    let mut scratch = Scratch::new();
    scratch.write("a.txt", "one\n");
    scratch.state.refresh();
    assert_eq!(scratch.row_shas(), [WIP_SHA]);
    assert!(scratch.state.history.commits[0].parents.is_empty());
}

#[test]
fn refresh_status_keeps_the_rows_while_only_the_files_change() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    scratch.write("a.txt", "changed\n");
    scratch.state.refresh();
    scratch.state.select_only(&shas[0]);
    assert!(
        scratch
            .state
            .open_diff(DiffTarget::new(DiffSource::Unstaged, "a.txt"))
    );
    scratch.state.load_compare_files(&shas[0]);
    let first = scratch.state.diff.clone().unwrap();

    // Nothing changed: nothing is reloaded.
    scratch.state.refresh_status();
    assert_eq!(scratch.state.diff.as_ref(), Some(&first));
    assert_eq!(scratch.state.compare_sha.as_ref(), Some(&shas[0]));

    // Another change: the working-directory diff and the comparison are stale, the rows are not.
    scratch.write("a.txt", "changed\nagain\n");
    scratch.state.refresh_status();
    let second = scratch.state.diff.as_ref().unwrap();
    assert_ne!(second.revision, first.revision);
    assert_eq!(second.stats.additions, 2);
    assert_eq!(scratch.state.compare_sha, None);
    assert_eq!(scratch.state.selected, [shas[0].as_str()]);
    assert_eq!(scratch.row_shas(), [WIP_SHA, &shas[1], &shas[0]]);

    // A change that leaves a commit's diff alone.
    assert!(scratch.state.open_diff(DiffTarget::new(
        DiffSource::Commit(shas[1].clone()),
        "a.txt",
    )));
    let commit_diff = scratch.state.diff.clone().unwrap();
    scratch.write("a.txt", "third\n");
    scratch.state.refresh_status();
    assert_eq!(scratch.state.diff.as_ref(), Some(&commit_diff));
}

#[test]
fn a_refresh_keeps_what_still_exists() {
    let (mut scratch, shas) = Scratch::with_commits(3);
    let state = &mut scratch.state;
    state.select(&shas[2], false, false);
    state.select(&shas[1], true, false);
    state.load_commit_files(&shas[2]);
    state.commit_summary = "half written".to_owned();
    // The newest commit goes away.
    state.repo.reset(&shas[1], ResetMode::Hard).unwrap();
    state.refresh();
    assert_eq!(state.selected, [shas[1].as_str()]);
    assert!(!state.is_selected(&shas[2]));
    assert!(state.is_selected(&shas[1]));
    assert_eq!(state.focused.as_ref(), Some(&shas[1]));
    assert_eq!(state.commit_summary, "half written");
    // The files of the commit shown were read again (it is still an object).
    assert_eq!(state.files_for_sha.as_ref(), Some(&shas[2]));
    assert_eq!(state.commit_files.len(), 1);

    // The focus goes with its row.
    state.select_only(&shas[1]);
    state.repo.reset(&shas[0], ResetMode::Hard).unwrap();
    state.refresh();
    assert!(state.selected.is_empty());
    assert_eq!(state.focused, None);
}

#[test]
fn a_history_that_cannot_be_loaded_reports_why() {
    let (mut scratch, _) = Scratch::with_commits(1);
    // HEAD's branch points at an object that does not exist.
    let git_dir = scratch.state.repo.git_dir().to_path_buf();
    fs::write(
        git_dir.join("refs/heads/main"),
        "0123456789012345678901234567890123456789\n",
    )
    .unwrap();
    scratch.state.refresh();
    let state = &scratch.state;
    if state.load_error.is_some() {
        assert!(state.history.commits.is_empty());
        assert!(state.layout.rows.is_empty());
        assert!(state.child_ref_rows.is_empty());
    } else {
        // libgit2 walked around the broken ref: nothing to report.
        assert_eq!(state.layout.rows.len(), state.history.commits.len());
    }
}

// Inherited refs and the layout

#[test]
fn rows_inherit_the_branches_of_the_child_above() {
    let (mut scratch, shas) = Scratch::with_commits(4);
    // Rows: 0 = commit 4 (main), 1 = commit 3 (tag), 2 = commit 2 (branch `old`), 3 = commit 1.
    scratch.state.repo.create_tag("v1", &shas[2], None).unwrap();
    scratch
        .state
        .repo
        .create_branch("old", &shas[1], false)
        .unwrap();
    scratch.state.refresh();
    let state = &scratch.state;
    assert_eq!(state.child_ref_rows, [None, Some(0), Some(0), Some(2)]);
    // A row with refs of its own shows no ghost refs; a tag passes on what it inherited.
    assert_eq!(state.ghost_ref_rows, [None, None, None, Some(2)]);
}

#[test]
fn the_wip_row_has_no_refs_to_pass_on() {
    let (mut scratch, _) = Scratch::with_commits(2);
    scratch.write("a.txt", "changed\n");
    scratch.state.refresh();
    let state = &scratch.state;
    assert_eq!(state.child_ref_rows, [None, None, Some(1)]);
    assert_eq!(state.ghost_ref_rows, [None, None, Some(1)]);
}

#[test]
fn a_merge_passes_its_branch_to_its_first_parent_only() {
    let (mut scratch, shas) = Scratch::with_commits(1);
    scratch
        .state
        .repo
        .create_branch("side", &shas[0], true)
        .unwrap();
    let side_1 = scratch.commit_file("side.txt", "1\n", "side 1");
    let side_2 = scratch.commit_file("side.txt", "1\n2\n", "side 2");
    scratch.state.repo.checkout_branch("main").unwrap();
    let main_2 = scratch.commit_file("main.txt", "1\n", "main 2");
    assert_eq!(
        scratch.state.repo.merge("side", true).unwrap(),
        Outcome::Done
    );
    scratch.state.refresh();
    let state = &scratch.state;
    let row = |sha: &str| state.history.row_by_sha[sha];
    let merge = state.history.head_sha.clone().unwrap();
    assert_eq!(state.commit_for(&merge).unwrap().node_type, NodeType::Merge);
    assert_eq!(row(&merge), 0);
    assert_eq!(state.child_ref_rows[row(&main_2)], Some(0));
    // `side` has its own branch: its tip inherits main's through the merge, the commit under it side's.
    assert_eq!(state.child_ref_rows[row(&side_2)], Some(0));
    assert_eq!(state.ghost_ref_rows[row(&side_2)], None);
    assert_eq!(state.child_ref_rows[row(&side_1)], Some(row(&side_2)));
    assert_eq!(state.ghost_ref_rows[row(&side_1)], Some(row(&side_2)));
    // The fork point is the first parent of both chains: the earlier row wins.
    let earlier = row(&main_2).min(row(&side_1));
    let expected = if earlier == row(&main_2) {
        0
    } else {
        row(&side_2)
    };
    assert_eq!(state.child_ref_rows[row(&shas[0])], Some(expected));
}

#[test]
fn the_pinned_branch_takes_the_first_column() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    scratch
        .state
        .repo
        .create_branch("side", &shas[0], true)
        .unwrap();
    let side = scratch.commit_file("side.txt", "1\n", "side 1");
    scratch.state.repo.checkout_branch("main").unwrap();
    let main = scratch.commit_file("main.txt", "1\n", "main 3");
    scratch.state.refresh();
    let column =
        |state: &RepoState, sha: &str| state.layout.rows[state.history.row_by_sha[sha]].column;
    assert_eq!(column(&scratch.state, &main), 0);
    assert_ne!(column(&scratch.state, &side), 0);

    scratch.state.pinned_branch = Some("refs/heads/side".to_owned());
    scratch.state.refresh();
    assert_eq!(column(&scratch.state, &side), 0);
    assert_ne!(column(&scratch.state, &main), 0);

    // A pinned branch that is gone pins nothing.
    scratch.state.pinned_branch = Some("refs/heads/gone".to_owned());
    scratch.state.refresh();
    assert_eq!(column(&scratch.state, &main), 0);
}

#[test]
fn hidden_and_solo_refs_shape_the_walk() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    scratch
        .state
        .repo
        .create_branch("side", &shas[0], true)
        .unwrap();
    let side = scratch.commit_file("side.txt", "1\n", "side 1");
    scratch
        .state
        .repo
        .create_branch("third", &shas[0], true)
        .unwrap();
    let third = scratch.commit_file("third.txt", "1\n", "third 1");
    scratch.state.repo.checkout_branch("main").unwrap();
    scratch.state.refresh();
    assert!(scratch.state.commit_for(&side).is_some());
    assert!(scratch.state.commit_for(&third).is_some());

    scratch
        .state
        .hidden_refs
        .insert("refs/heads/side".to_owned());
    scratch.state.refresh();
    assert!(scratch.state.commit_for(&side).is_none());
    assert!(scratch.state.find_ref("refs/heads/side").unwrap().hidden);
    // A hidden ref is not shown at its commit.
    assert!(scratch.state.refs_at(&side).is_empty());

    scratch.state.hidden_refs.clear();
    scratch.state.solo_ref = Some("refs/heads/side".to_owned());
    scratch.state.refresh();
    assert!(scratch.state.commit_for(&side).is_some());
    // Only the soloed ref is walked (and HEAD, always a row).
    assert!(scratch.state.commit_for(&third).is_none());
    assert!(scratch.state.commit_for(&shas[1]).is_some());
}

// Selection

#[test]
fn selection_single_toggle_and_range() {
    let (mut scratch, shas) = Scratch::with_commits(5);
    // Rows top down: e d c b a.
    let [a, b, c, d, e] = &shas[..] else {
        unreachable!()
    };
    let state = &mut scratch.state;
    assert!(state.single_selected().is_none());

    state.select(c, false, false);
    assert_eq!(state.selected, [c.as_str()]);
    assert_eq!(state.focused.as_ref(), Some(c));
    assert_eq!(state.single_selected().unwrap().sha, *c);
    assert!(state.is_selected(c));

    // Ctrl+click adds in click order and moves the focus; again removes.
    state.range_cursor = Some(d.clone());
    state.select(a, true, false);
    assert_eq!(state.selected, [c.clone(), a.clone()]);
    assert_eq!(state.focused.as_ref(), Some(a));
    assert_eq!(state.range_cursor, None);
    assert!(state.single_selected().is_none());
    state.select(c, true, false);
    assert_eq!(state.selected, [a.as_str()]);
    assert!(!state.is_selected(c));
    assert_eq!(state.focused.as_ref(), Some(c));
    assert_eq!(state.single_selected().unwrap().sha, *a);

    // Shift+click: the rows between the focus (c) and the click, top down, replacing the selection. The
    // focus stays the anchor, the range cursor is left to the caller.
    state.range_cursor = Some(e.clone());
    state.select(e, false, true);
    assert_eq!(state.selected, [e.clone(), d.clone(), c.clone()]);
    assert_eq!(state.focused.as_ref(), Some(c));
    assert_eq!(state.range_cursor.as_ref(), Some(e));
    state.select(b, false, true);
    assert_eq!(state.selected, [c.clone(), b.clone()]);

    // Ctrl+Shift+click adds the range to what is selected, each row once.
    state.select(e, true, true);
    assert_eq!(state.selected, [c.clone(), b.clone(), e.clone(), d.clone()]);
    assert!([b, c, d, e].iter().all(|sha| state.is_selected(sha)));
    assert!(!state.is_selected(a));

    // select_only resets everything.
    state.select_only(a);
    assert_eq!(state.selected, [a.as_str()]);
    assert_eq!(state.focused.as_ref(), Some(a));
    assert_eq!(state.range_cursor, None);
    assert!(!state.is_selected(e));
}

#[test]
fn a_range_without_an_anchor_is_a_plain_click() {
    let (mut scratch, shas) = Scratch::with_commits(3);
    let state = &mut scratch.state;
    state.select(&shas[0], false, true);
    assert_eq!(state.selected, [shas[0].as_str()]);
    // An anchor that is not a row (any more): a toggle.
    state.focused = Some("gone".to_owned());
    state.select(&shas[2], true, true);
    assert_eq!(state.selected, [shas[0].clone(), shas[2].clone()]);
    assert_eq!(state.focused.as_ref(), Some(&shas[2]));
}

// Search

#[test]
fn search_matches_and_stepping() {
    let mut scratch = Scratch::new();
    let fix_parser = scratch.commit_file("a.txt", "1\n", "Fix the parser");
    let docs = scratch.commit_file("a.txt", "2\n", "Write docs\n\nMentions the PARSER too.");
    let other = scratch.commit_file("a.txt", "3\n", "Unrelated");
    let fix_lexer = scratch.commit_file("a.txt", "4\n", "fix the lexer");
    scratch.state.refresh();
    let state = &mut scratch.state;
    // Rows: 0 fix_lexer, 1 other, 2 docs, 3 fix_parser.

    // Closed: nothing is searched.
    state.search_text = "parser".to_owned();
    state.run_search();
    assert!(state.search_matches.is_empty());
    assert_eq!(state.search_index, None);

    // Summary and body, case ignored; the first match is selected and scrolled to.
    state.search_open = true;
    state.run_search();
    assert_eq!(state.search_matches, [2, 3]);
    assert_eq!(state.search_index, Some(0));
    assert_eq!(state.search_match_sha.as_ref(), Some(&docs));
    assert_eq!(state.selected, [docs.as_str()]);
    assert_eq!(state.scroll_to_row, Some(2));

    // Stepping wraps around both ends.
    state.select_search_match(1);
    assert_eq!(state.search_index, Some(1));
    assert_eq!(state.selected, [fix_parser.as_str()]);
    assert_eq!(state.scroll_to_row, Some(3));
    state.select_search_match(1);
    assert_eq!(state.search_index, Some(0));
    state.select_search_match(-1);
    assert_eq!(state.search_index, Some(1));
    assert_eq!(state.search_match_sha.as_ref(), Some(&fix_parser));

    // The match the user is on survives a new match set that still has it...
    state.search_text = "FIX".to_owned();
    state.scroll_to_row = None;
    state.run_search();
    assert_eq!(state.search_matches, [0, 3]);
    assert_eq!(state.search_index, Some(1));
    assert_eq!(state.search_match_sha.as_ref(), Some(&fix_parser));
    assert_eq!(state.scroll_to_row, None);
    // ...and a refresh.
    state.refresh();
    assert_eq!(state.search_index, Some(1));

    // One that lost it moves to its first match.
    state.search_text = "lexer".to_owned();
    state.run_search();
    assert_eq!(state.search_matches, [0]);
    assert_eq!(state.search_index, Some(0));
    assert_eq!(state.selected, [fix_lexer.as_str()]);

    // The author's name and email, and the sha.
    state.search_text = "tester@example".to_owned();
    state.run_search();
    assert_eq!(state.search_matches, [0, 1, 2, 3]);
    state.search_text = "TESTER".to_owned();
    state.run_search();
    assert_eq!(state.search_matches.len(), 4);
    state.search_text = other[..10].to_uppercase();
    state.run_search();
    assert_eq!(state.search_matches, [1]);
    assert_eq!(state.selected, [other.as_str()]);

    // No match: the selection stays, stepping does nothing.
    state.search_text = "no such thing".to_owned();
    state.run_search();
    assert!(state.search_matches.is_empty());
    assert_eq!(state.search_index, None);
    assert_eq!(state.search_match_sha, None);
    state.select_search_match(1);
    assert_eq!(state.search_index, None);
    assert_eq!(state.selected, [other.as_str()]);

    // An empty text forgets the match.
    state.search_text.clear();
    state.run_search();
    assert_eq!(state.search_match_sha, None);
}

// Detail panel

#[test]
fn commit_files_are_cached_per_commit() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    let third = {
        scratch.write("b.txt", "one\ntwo\n");
        scratch.write("a.txt", "line 1\n");
        scratch.state.repo.stage(&["a.txt", "b.txt"]).unwrap();
        scratch.state.repo.commit("commit 3", false).unwrap()
    };
    scratch.state.refresh();
    let state = &mut scratch.state;

    state.load_commit_files(&third);
    assert_eq!(state.files_for_sha.as_ref(), Some(&third));
    assert_eq!(state.commit_files_error, None);
    let mut files: Vec<_> = state
        .commit_files
        .iter()
        .map(|file| (file.path.as_str(), file.status, file.stats.unwrap()))
        .collect();
    files.sort_by_key(|file| file.0);
    assert_eq!(files.len(), 2);
    assert_eq!((files[0].0, files[0].1), ("a.txt", FileStatus::Modified));
    assert_eq!((files[0].2.additions, files[0].2.deletions), (0, 1));
    assert_eq!((files[1].0, files[1].1), ("b.txt", FileStatus::Added));
    assert_eq!((files[1].2.additions, files[1].2.deletions), (2, 0));

    // Asked again: the cache answers (what is in it stays).
    state.commit_files.clear();
    state.load_commit_files(&third);
    assert!(state.commit_files.is_empty());

    // Another commit replaces it.
    state.load_commit_files(&shas[0]);
    assert_eq!(state.commit_files.len(), 1);
    assert_eq!(state.commit_files[0].status, FileStatus::Added);

    // The WIP row has no commit files.
    state.load_commit_files(WIP_SHA);
    assert_eq!(state.files_for_sha.as_deref(), Some(WIP_SHA));
    assert!(state.commit_files.is_empty());
    assert_eq!(state.commit_files_error, None);

    // A commit that does not exist.
    state.load_commit_files("0123456789012345678901234567890123456789");
    assert!(state.commit_files.is_empty());
    assert!(state.commit_files_error.is_some());
}

#[test]
fn compare_files_against_the_working_directory() {
    let (mut scratch, shas) = Scratch::with_commits(3);
    scratch.write("z.txt", "new\n");
    scratch.write("b.txt", "new\n");
    scratch.state.refresh();
    let state = &mut scratch.state;
    state.load_compare_files(&shas[0]);
    assert_eq!(state.compare_sha.as_ref(), Some(&shas[0]));
    assert_eq!(state.compare_error, None);
    let files: Vec<_> = state
        .compare_files
        .iter()
        .map(|file| (file.path.as_str(), file.status))
        .collect();
    assert_eq!(
        files,
        [
            ("a.txt", FileStatus::Modified),
            ("b.txt", FileStatus::Untracked),
            ("z.txt", FileStatus::Untracked),
        ]
    );

    state.load_compare_files("not a sha");
    assert!(state.compare_files.is_empty());
    assert!(state.compare_error.is_some());
}

#[test]
fn open_diff_for_each_source() {
    let (mut scratch, shas) = Scratch::with_commits(3);
    // Staged: a fourth line; unstaged on top of it: a fifth.
    scratch.write("a.txt", "line 1\nline 2\nline 3\nstaged\n");
    scratch.state.repo.stage(&["a.txt"]).unwrap();
    scratch.write("a.txt", "line 1\nline 2\nline 3\nstaged\nunstaged\n");
    scratch.state.refresh();
    let state = &mut scratch.state;
    let added = |state: &RepoState| -> Vec<String> {
        state
            .diff
            .as_ref()
            .unwrap()
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.origin == LineOrigin::Added)
            .map(|line| line.text.clone())
            .collect()
    };

    state.file_edit = Some(FileEdit::default());
    assert!(state.open_diff(DiffTarget {
        source: DiffSource::Commit(shas[1].clone()),
        path: "a.txt".to_owned(),
        status: FileStatus::Modified,
    }));
    assert_eq!(state.file_edit, None);
    assert_eq!(state.diff_error, None);
    assert_eq!(added(state), ["line 2"]);
    assert_eq!(state.diff.as_ref().unwrap().path, "a.txt");
    assert!(state.diff.as_ref().unwrap().revision.is_some());

    assert!(state.open_diff(DiffTarget::new(DiffSource::Staged, "a.txt")));
    assert_eq!(added(state), ["staged"]);

    assert!(state.open_diff(DiffTarget::new(DiffSource::Unstaged, "a.txt")));
    assert_eq!(added(state), ["unstaged"]);

    assert!(state.open_diff(DiffTarget::new(
        DiffSource::Compare(shas[0].clone()),
        "a.txt",
    )));
    assert_eq!(added(state), ["line 2", "line 3", "staged", "unstaged"]);

    // A source that cannot be read: no diff, the reason instead.
    assert!(state.open_diff(DiffTarget::new(
        DiffSource::Commit("not a sha".to_owned()),
        "a.txt",
    )));
    assert_eq!(state.diff, None);
    assert!(state.diff_error.is_some());
    assert!(state.diff_target.is_some());

    // The next one that can clears it.
    assert!(state.open_diff(DiffTarget::new(DiffSource::Staged, "a.txt")));
    assert_eq!(state.diff_error, None);
    assert!(state.diff.is_some());

    state.left_open_in_diff = true;
    state.file_edit = Some(FileEdit::default());
    assert!(state.close_diff());
    assert_eq!(state.diff, None);
    assert_eq!(state.diff_target, None);
    assert_eq!(state.diff_error, None);
    assert_eq!(state.file_edit, None);
    assert!(!state.left_open_in_diff);
    // Nothing open: nothing to reload.
    state.reload_diff();
    assert_eq!(state.diff, None);
}

#[test]
fn unsaved_editor_text_blocks_leaving_the_file_view() {
    let (mut scratch, _shas) = Scratch::with_commits(2);
    let state = &mut scratch.state;
    let target = || DiffTarget::new(DiffSource::Unstaged, "a.txt");
    assert!(state.open_diff(target()));
    state.file_edit = Some(FileEdit {
        path: "a.txt".to_owned(),
        saved: "old\n".to_owned(),
        text: "new\n".to_owned(),
        ..FileEdit::default()
    });
    assert!(state.has_unsaved_edit());

    // Every way out refuses and leaves everything as it was.
    assert!(!state.open_diff(DiffTarget::new(DiffSource::Staged, "a.txt")));
    assert!(!state.close_diff());
    assert!(!state.open_merge_tool("a.txt").unwrap());
    assert_eq!(state.diff_target, Some(target()));
    assert_eq!(
        state.file_edit.as_ref().map(|edit| edit.text.as_str()),
        Some("new\n")
    );

    // Text equal to the disk's is not unsaved.
    state.file_edit.as_mut().unwrap().saved = "new\n".to_owned();
    assert!(!state.has_unsaved_edit());
    assert!(state.close_diff());
    assert_eq!(state.file_edit, None);

    // After the user confirmed the loss, the edit goes and navigation works again.
    assert!(state.open_diff(target()));
    state.file_edit = Some(FileEdit {
        path: "a.txt".to_owned(),
        saved: "old\n".to_owned(),
        text: "new\n".to_owned(),
        ..FileEdit::default()
    });
    state.discard_file_edit();
    assert!(!state.has_unsaved_edit());
    assert!(state.close_diff());
}

#[test]
fn diff_modes_pick_the_context() {
    let mut scratch = Scratch::new();
    let lines: Vec<String> = (1..=40).map(|number| format!("line {number}")).collect();
    scratch.commit_file("a.txt", &(lines.join("\n") + "\n"), "first");
    let mut changed = lines.clone();
    changed[19] = "changed".to_owned();
    let second = scratch.commit_file("a.txt", &(changed.join("\n") + "\n"), "second");
    scratch.state.refresh();
    let state = &mut scratch.state;
    let line_count = |state: &RepoState| -> usize {
        let diff = state.diff.as_ref().unwrap();
        diff.hunks.iter().map(|hunk| hunk.lines.len()).sum()
    };

    assert_eq!(DiffMode::default(), DiffMode::Inline);
    assert_eq!(state.diff_mode, DiffMode::Inline);
    assert!(state.open_diff(DiffTarget::new(DiffSource::Commit(second), "a.txt")));
    // Inline: the whole file, the changed line on both sides.
    assert_eq!(line_count(state), 41);

    state.diff_mode = DiffMode::Hunk;
    state.reload_diff();
    // Three lines of context around the change.
    assert_eq!(line_count(state), 8);

    // The file view shows the whole file whatever the mode.
    state.file_view = true;
    state.reload_diff();
    assert_eq!(line_count(state), 41);
    state.file_view = false;

    state.diff_mode = DiffMode::Split;
    state.reload_diff();
    assert_eq!(line_count(state), 41);

    assert_eq!(DiffMode::Hunk.context_lines(false), 3);
    assert!(DiffMode::Hunk.context_lines(true) > 1_000_000);
    assert!(DiffMode::Inline.context_lines(false) > 1_000_000);
}

#[test]
fn whitespace_only_changes_can_be_left_out() {
    let (mut scratch, _) = Scratch::with_commits(1);
    scratch.write("a.txt", "line   1\n");
    scratch.state.refresh();
    let state = &mut scratch.state;
    assert!(state.open_diff(DiffTarget::new(DiffSource::Unstaged, "a.txt")));
    assert!(state.diff_whitespace);
    assert_eq!(state.diff.as_ref().unwrap().stats.additions, 1);

    state.diff_whitespace = false;
    state.reload_diff();
    assert_eq!(state.diff_error, None);
    assert_eq!(state.diff.as_ref().unwrap().stats.additions, 0);
}

#[test]
fn a_refresh_reloads_the_open_diff() {
    let (mut scratch, _) = Scratch::with_commits(1);
    scratch.write("a.txt", "line 1\nmore\n");
    scratch.state.refresh();
    assert!(
        scratch
            .state
            .open_diff(DiffTarget::new(DiffSource::Unstaged, "a.txt"))
    );
    let before = scratch.state.diff.as_ref().unwrap().revision;
    scratch.state.refresh();
    let after = scratch.state.diff.as_ref().unwrap().revision;
    assert!(after.is_some());
    assert_ne!(before, after);
}

#[test]
fn the_merge_tool_opens_on_a_conflicted_file() {
    let (mut scratch, shas) = Scratch::with_commits(1);
    scratch
        .state
        .repo
        .create_branch("side", &shas[0], true)
        .unwrap();
    let theirs = scratch.commit_file("a.txt", "theirs\n", "side change");
    scratch.state.repo.checkout_branch("main").unwrap();
    let ours = scratch.commit_file("a.txt", "ours\n", "main change");
    assert_eq!(
        scratch.state.repo.merge("side", false).unwrap(),
        Outcome::Conflicts
    );
    scratch.state.refresh();
    let state = &mut scratch.state;
    assert_eq!(state.operation_state, OperationState::Merge);
    assert_eq!(state.status.conflicted.len(), 1);
    assert_eq!(state.history.commits[0].sha, WIP_SHA);

    // Not a conflicted file: nothing opens, the diff stays.
    assert!(state.open_diff(DiffTarget::new(DiffSource::Commit(ours.clone()), "a.txt")));
    assert!(state.open_merge_tool("missing.txt").is_err());
    assert!(state.merge_tool.is_none());
    assert!(state.diff_target.is_some());

    assert!(state.open_merge_tool("a.txt").unwrap());
    assert!(state.diff_target.is_none());
    assert!(state.diff.is_none());
    let tool = state.merge_tool.as_ref().unwrap();
    assert_eq!(tool.path, "a.txt");
    assert_eq!(tool.ours, ["ours"]);
    assert_eq!(tool.theirs, ["theirs"]);
    assert_eq!(tool.base, ["line 1"]);
    assert_eq!(tool.conflict_count(), 1);
    assert_eq!(tool.ours_sha, ours);
    assert_eq!(tool.ours_label, "main");
    assert_eq!(tool.theirs_sha, theirs);
    assert_eq!(tool.theirs_label, "side");
}

// File history

#[test]
fn the_file_history_loads_in_the_background() {
    let (mut scratch, shas) = Scratch::with_commits(3);
    scratch.commit_file("other.txt", "1\n", "another file");
    scratch.state.refresh();
    let state = &mut scratch.state;

    // Nothing open: nothing to poll or select.
    state.poll_file_history();
    state.select_file_history_entry(Some(0));
    state.reload_file_history_diff();
    assert!(state.file_history.is_none());

    state.open_file_history("a.txt", None, None, true);
    {
        let view = state.file_history.as_ref().unwrap();
        assert!(view.history_loading);
        assert!(view.blame);
        assert!(view.file_view);
        assert_eq!(view.selected, None);
    }
    wait_until(|| {
        state.poll_file_history();
        !state.file_history.as_ref().unwrap().history_loading
    });
    {
        let view = state.file_history.as_ref().unwrap();
        assert_eq!(view.history_error, None);
        let entries: Vec<_> = view
            .history
            .entries
            .iter()
            .map(|entry| (entry.commit.sha.as_str(), entry.status))
            .collect();
        assert_eq!(
            entries,
            [
                (shas[2].as_str(), FileStatus::Modified),
                (shas[1].as_str(), FileStatus::Modified),
                (shas[0].as_str(), FileStatus::Added),
            ]
        );
        // The newest entry is selected, shown and scrolled to; its blame is on its way.
        assert_eq!(view.selected, Some(0));
        assert_eq!(view.selected_sha.as_ref(), Some(&shas[2]));
        assert_eq!(view.scroll_to, Some(0));
        assert_eq!(view.blame_sha.as_ref(), Some(&shas[2]));
        // The file view: the whole file.
        let diff = view.diff.as_ref().unwrap();
        assert_eq!(diff.hunks.iter().map(|h| h.lines.len()).sum::<usize>(), 3);
    }
    wait_until(|| {
        state.poll_file_history();
        !state.file_history.as_ref().unwrap().blame_loading
    });
    {
        let view = state.file_history.as_ref().unwrap();
        assert_eq!(view.blame_error, None);
        let blame = view.blame_data.as_ref().unwrap();
        let by_line: Vec<_> = (1..=3)
            .map(|line| blame.hunk_at(line).unwrap().sha.as_str())
            .collect();
        assert_eq!(by_line, [&shas[0], &shas[1], &shas[2]]);
    }

    // Another entry: its diff at once, its blame later.
    state.select_file_history_entry(Some(2));
    {
        let view = state.file_history.as_ref().unwrap();
        assert_eq!(view.selected, Some(2));
        assert_eq!(view.selected_sha.as_ref(), Some(&shas[0]));
        assert_eq!(view.blame_data, None);
        assert!(view.blame_loading);
        assert_eq!(view.diff.as_ref().unwrap().stats.additions, 1);
    }
    wait_until(|| {
        state.poll_file_history();
        !state.file_history.as_ref().unwrap().blame_loading
    });
    assert_eq!(
        state
            .file_history
            .as_ref()
            .unwrap()
            .blame_data
            .as_ref()
            .unwrap()
            .hunks
            .len(),
        1
    );

    // The diff view in hunk mode: three lines of context (here: the two other lines).
    state.select_file_history_entry(Some(1));
    state.diff_mode = DiffMode::Hunk;
    state.file_history.as_mut().unwrap().file_view = false;
    let before = state
        .file_history
        .as_ref()
        .unwrap()
        .diff
        .as_ref()
        .unwrap()
        .revision;
    state.reload_file_history_diff();
    {
        let view = state.file_history.as_ref().unwrap();
        assert_ne!(view.diff.as_ref().unwrap().revision, before);
        assert_eq!(view.diff_error, None);
    }

    // Not an entry: nothing selected.
    state.select_file_history_entry(Some(9));
    {
        let view = state.file_history.as_ref().unwrap();
        assert_eq!(view.selected, None);
        assert_eq!(view.diff, None);
    }
    state.select_file_history_entry(None);
    assert_eq!(state.file_history.as_ref().unwrap().selected, None);

    state.close_file_history();
    assert!(state.file_history.is_none());
    state.close_file_history();
}

#[test]
fn the_file_history_selects_the_entry_asked_for() {
    let (mut scratch, shas) = Scratch::with_commits(3);
    let state = &mut scratch.state;
    // From the second commit, on it: the third is not part of the walk.
    state.open_file_history("a.txt", Some(shas[1].clone()), Some(shas[0].clone()), false);
    wait_until(|| {
        state.poll_file_history();
        !state.file_history.as_ref().unwrap().history_loading
    });
    let view = state.file_history.as_ref().unwrap();
    assert_eq!(view.history.entries.len(), 2);
    assert_eq!(view.selected, Some(1));
    assert_eq!(view.scroll_to, Some(1));
    assert_eq!(view.selected_sha.as_ref(), Some(&shas[0]));
    assert!(!view.blame);

    // Reopening replaces the view (and its jobs) at once.
    state.open_file_history("a.txt", None, Some("not in the history".to_owned()), false);
    state.open_file_history("a.txt", None, Some("not in the history".to_owned()), false);
    wait_until(|| {
        state.poll_file_history();
        !state.file_history.as_ref().unwrap().history_loading
    });
    // An entry that is not there: the newest.
    let view = state.file_history.as_ref().unwrap();
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.selected_sha.as_ref(), Some(&shas[2]));
}

#[test]
fn a_file_history_that_cannot_be_read_reports_why() {
    let (mut scratch, _) = Scratch::with_commits(1);
    let state = &mut scratch.state;
    state.open_file_history("a.txt", Some("not a sha".to_owned()), None, false);
    wait_until(|| {
        state.poll_file_history();
        !state.file_history.as_ref().unwrap().history_loading
    });
    let view = state.file_history.as_ref().unwrap();
    assert!(view.history_error.is_some());
    assert!(view.history.entries.is_empty());
    assert_eq!(view.selected, None);
}

// Refs

#[test]
fn refs_at_find_ref_and_head_ref() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    let repo = &scratch.state.repo;
    let tip = &shas[1];
    repo.create_tag("v1", tip, None).unwrap();
    repo.create_branch("another", tip, false).unwrap();
    repo.create_branch("zeta", tip, false).unwrap();
    repo.create_branch("old", &shas[0], false).unwrap();
    // A remote-tracking branch, as a fetch would leave it.
    git2::Repository::open(scratch.root())
        .unwrap()
        .reference(
            "refs/remotes/origin/main",
            git2::Oid::from_str(tip).unwrap(),
            true,
            "test",
        )
        .unwrap();
    scratch.state.refresh();
    let state = &scratch.state;

    let head = state.head_ref().unwrap();
    assert_eq!(head.full_name, "refs/heads/main");
    assert!(head.is_head);

    // The checked-out branch first (though "another" sorts before it), then local, remote, tags.
    let at_tip: Vec<_> = state
        .refs_at(tip)
        .iter()
        .map(|r| (r.name.as_str(), r.ref_type))
        .collect();
    assert_eq!(at_tip[0], ("main", RefType::LocalBranch));
    assert_eq!(at_tip.len(), 5);
    let kinds: Vec<_> = at_tip.iter().map(|r| r.1).collect();
    assert_eq!(
        kinds,
        [
            RefType::LocalBranch,
            RefType::LocalBranch,
            RefType::LocalBranch,
            RefType::RemoteBranch,
            RefType::Tag,
        ]
    );
    assert_eq!(at_tip[3].0, "origin/main");
    assert_eq!(at_tip[4].0, "v1");

    assert_eq!(state.refs_at(&shas[0])[0].name, "old");
    assert!(state.refs_at("no such sha").is_empty());
    assert!(state.refs_at(WIP_SHA).is_empty());

    assert_eq!(state.find_ref("refs/tags/v1").unwrap().target, *tip);
    assert_eq!(state.find_ref("refs/heads/old").unwrap().target, shas[0]);
    assert!(state.find_ref("old").is_none());
}

#[test]
fn a_detached_head_has_no_head_ref() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    scratch.state.repo.checkout_commit(&shas[0]).unwrap();
    scratch.state.refresh();
    assert!(scratch.state.head_ref().is_none());
    assert!(scratch.state.history.detached);
    // The rebase plan names HEAD by its short sha then.
    scratch
        .state
        .start_rebase_plan(&shas[0], "base", None, None);
    assert_eq!(scratch.state.rebase.head_label, &shas[0][..7]);
    assert!(!scratch.state.rebase.active);
}

// Rebase plan

#[test]
fn commits_above_walks_the_first_parents() {
    let (mut scratch, shas) = Scratch::with_commits(4);
    let summaries = |commits: Vec<&imgit_git::Commit>| -> Vec<String> {
        commits
            .iter()
            .map(|commit| commit.summary.clone())
            .collect()
    };
    {
        let state = &scratch.state;
        // Oldest first, the base left out.
        assert_eq!(
            summaries(state.commits_above(Some(&shas[0]), None)),
            ["commit 2", "commit 3", "commit 4"]
        );
        assert_eq!(
            summaries(state.commits_above(Some(&shas[2]), None)),
            ["commit 4"]
        );
        // HEAD itself: nothing above it.
        assert!(state.commits_above(Some(&shas[3]), None).is_empty());
        // From another tip.
        assert_eq!(
            summaries(state.commits_above(Some(&shas[0]), Some(&shas[2]))),
            ["commit 2", "commit 3"]
        );
        // No base: down to the root, the root included.
        assert_eq!(
            summaries(state.commits_above(None, None)),
            ["commit 1", "commit 2", "commit 3", "commit 4"]
        );
        // A base that is not on the chain (here: above the tip).
        assert!(
            state
                .commits_above(Some(&shas[3]), Some(&shas[1]))
                .is_empty()
        );
        assert!(state.commits_above(Some("no such sha"), None).is_empty());
        assert!(state.commits_above(Some(&shas[0]), Some("gone")).is_empty());
    }

    // The WIP row is not on HEAD's chain.
    scratch.write("a.txt", "dirty\n");
    scratch.state.refresh();
    assert_eq!(scratch.state.commits_above(Some(&shas[1]), None).len(), 2);

    // A chain that leaves the loaded rows.
    scratch.state.commit_limit = 2;
    scratch.state.refresh();
    assert!(scratch.state.commits_above(Some(&shas[0]), None).is_empty());
    assert_eq!(scratch.state.commits_above(Some(&shas[2]), None).len(), 1);
}

#[test]
fn commits_above_stops_at_a_merge() {
    let (mut scratch, shas) = Scratch::with_commits(1);
    scratch
        .state
        .repo
        .create_branch("side", &shas[0], true)
        .unwrap();
    scratch.commit_file("side.txt", "1\n", "side 1");
    scratch.state.repo.checkout_branch("main").unwrap();
    let main_2 = scratch.commit_file("main.txt", "1\n", "main 2");
    scratch.state.repo.merge("side", true).unwrap();
    let after = scratch.commit_file("main.txt", "1\n2\n", "after the merge");
    scratch.state.refresh();
    let state = &scratch.state;
    let merge = state.commit_for(&after).unwrap().parents[0].clone();
    assert_eq!(state.commits_above(Some(&merge), None).len(), 1);
    assert!(state.commits_above(Some(&main_2), None).is_empty());
    assert!(state.commits_above(Some(&shas[0]), None).is_empty());
    assert!(state.commits_above(None, None).is_empty());
}

#[test]
fn the_rebase_plan_starts_swaps_and_resets() {
    let (mut scratch, shas) = Scratch::with_commits(4);
    let state = &mut scratch.state;
    assert!(!state.rebase.active);

    state.start_rebase_plan(&shas[0], "base label", None, None);
    let plan = &mut state.rebase;
    assert!(plan.active);
    assert_eq!(plan.base, shas[0]);
    assert_eq!(plan.base_label, "base label");
    assert_eq!(plan.head_label, "main");
    assert_eq!((plan.selected, plan.editing), (None, None));
    let order = |plan: &imgit_app::repo_state::RebasePlan| -> Vec<String> {
        plan.steps.iter().map(|row| row.summary.clone()).collect()
    };
    assert_eq!(order(plan), ["commit 2", "commit 3", "commit 4"]);
    assert_eq!(
        plan.steps
            .iter()
            .map(|row| row.step.sha.as_str())
            .collect::<Vec<_>>(),
        [&shas[1], &shas[2], &shas[3]]
    );
    assert!(
        plan.steps
            .iter()
            .all(|row| row.step.action == RebaseAction::Pick && row.author.name == "Tester")
    );
    assert_eq!(plan.steps, plan.original_steps);

    // A step moves with its summary and author.
    plan.swap_steps(0, 2);
    assert_eq!(order(plan), ["commit 4", "commit 3", "commit 2"]);
    assert_eq!(plan.steps[0].step.sha, shas[3]);
    plan.steps[1].step.action = RebaseAction::Drop;
    plan.selected = Some(1);
    plan.editing = Some(2);
    plan.reset();
    assert_eq!(order(plan), ["commit 2", "commit 3", "commit 4"]);
    assert_eq!(plan.steps, plan.original_steps);
    assert_eq!((plan.selected, plan.editing), (None, None));
    assert!(plan.active);

    // The plan the state rebases with.
    let steps: Vec<_> = plan.steps.iter().map(|row| row.step.clone()).collect();
    state.repo.rewrite_history(&shas[0], &steps).unwrap();

    // Nothing above the base: the plan stays closed, a plan that was open is gone.
    state.refresh();
    let head = state.history.head_sha.clone().unwrap();
    state.start_rebase_plan(&head, "head", None, None);
    assert!(!state.rebase.active);
    assert!(state.rebase.steps.is_empty());
    assert_eq!(state.rebase.base, head);
}

#[test]
fn the_rebase_plan_of_another_branch() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    scratch
        .state
        .repo
        .create_branch("side", &shas[0], true)
        .unwrap();
    let side_1 = scratch.commit_file("side.txt", "1\n", "side 1");
    let side_2 = scratch.commit_file("side.txt", "1\n2\n", "side 2");
    scratch.state.repo.checkout_branch("main").unwrap();
    scratch.state.refresh();
    let state = &mut scratch.state;

    // `side` (from where it forked) onto main's tip.
    state.start_rebase_plan(&shas[1], "main", Some(&shas[0]), Some(&side_2));
    assert!(state.rebase.active);
    assert_eq!(state.rebase.base, shas[1]);
    assert_eq!(state.rebase.head_label, "side");
    assert_eq!(
        state
            .rebase
            .steps
            .iter()
            .map(|row| row.step.sha.as_str())
            .collect::<Vec<_>>(),
        [&side_1, &side_2]
    );

    // A tip without a local branch: HEAD's branch names the plan.
    state.start_rebase_plan(&shas[0], "base", None, Some(&side_1));
    assert_eq!(state.rebase.head_label, "main");
    assert_eq!(state.rebase.steps.len(), 1);

    // The base is not under the tip without the fork point: nothing to rebase.
    state.start_rebase_plan(&shas[1], "main", None, Some(&side_2));
    assert!(!state.rebase.active);
}

// Status poll

#[test]
fn the_status_poll_picks_up_an_outside_commit() {
    let (mut scratch, shas) = Scratch::with_commits(1);
    let mut now = 100.0;
    // A poll right after the refresh: nothing changed, nothing moves.
    scratch.state.poll_status(now, 0.5);
    assert_eq!(scratch.state.last_status_poll, now);
    wait_until(|| {
        now += 0.01;
        scratch.state.poll_status(now, 1000.0);
        // The read's end restarts the interval.
        scratch.state.last_status_poll == now
    });
    assert_eq!(scratch.row_shas(), [&shas[0]]);
    let collected_at = now;

    // Someone else commits.
    let outside = {
        let other = Repository::open(scratch.root()).unwrap();
        scratch.write("a.txt", "outside\n");
        other.stage(&["a.txt"]).unwrap();
        other.commit("made outside", false).unwrap()
    };

    // Within the interval nothing is read.
    for _ in 0..5 {
        now += 0.01;
        scratch.state.poll_status(now, 1000.0);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(scratch.state.last_status_poll, collected_at);
    assert_eq!(scratch.row_shas(), [&shas[0]]);

    // Past it the read starts, and a later call applies it.
    now += 1.0;
    scratch.state.poll_status(now, 0.5);
    assert_eq!(scratch.state.last_status_poll, now);
    assert_eq!(scratch.row_shas(), [&shas[0]]);
    wait_until(|| {
        now += 0.01;
        scratch.state.poll_status(now, 1000.0);
        scratch.state.history.head_sha.as_ref() == Some(&outside)
    });
    assert_eq!(scratch.row_shas(), [&outside, &shas[0]]);
    assert_eq!(scratch.state.last_status_poll, now);
    assert_eq!(scratch.state.load_error, None);
}

#[test]
fn the_status_poll_picks_up_outside_edits() {
    let (mut scratch, shas) = Scratch::with_commits(1);
    scratch.write("a.txt", "edited outside\n");
    let mut now = 10.0;
    wait_until(|| {
        now += 1.0;
        scratch.state.poll_status(now, 0.5);
        !scratch.state.status.is_empty()
    });
    assert_eq!(scratch.row_shas(), [WIP_SHA, &shas[0]]);
    assert_eq!(scratch.state.status.unstaged[0].path, "a.txt");
    assert!(scratch.state.status.unstaged[0].stats.is_some());

    // A second edit changes the files, not the rows.
    scratch.write("b.txt", "another\n");
    wait_until(|| {
        now += 1.0;
        scratch.state.poll_status(now, 0.5);
        scratch.state.status.unstaged.len() == 2
    });
    assert_eq!(scratch.row_shas(), [WIP_SHA, &shas[0]]);
}

#[test]
fn dropping_a_tab_does_not_wait_for_its_jobs() {
    let (mut scratch, _) = Scratch::with_commits(3);
    scratch.state.poll_status(1.0, 0.5);
    scratch.state.open_file_history("a.txt", None, None, true);
    drop(scratch);
}

// Undo / redo

#[test]
fn the_undo_stack_keeps_the_last_fifty() {
    let (mut scratch, _) = Scratch::with_commits(1);
    let state = &mut scratch.state;
    state
        .redo_stack
        .push(UndoEntry::new("redoable", |_| Ok(())));
    for number in 0..55 {
        state.push_undo(UndoEntry::new(format!("entry {number}"), |_| Ok(())));
    }
    assert_eq!(state.undo_stack.len(), 50);
    assert_eq!(state.undo_stack[0].label, "entry 5");
    assert_eq!(state.undo_stack[49].label, "entry 54");
    // A new action forgets what could be redone.
    assert!(state.redo_stack.is_empty());
}

#[test]
fn an_undo_entry_acts_on_the_repository_it_is_given() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    let state = &mut scratch.state;
    let calls = Rc::new(Cell::new(0));
    let (undo_calls, redo_calls) = (Rc::clone(&calls), Rc::clone(&calls));
    let (before, after) = (shas[0].clone(), shas[1].clone());
    state.push_undo(
        UndoEntry::new("commit", move |repo| {
            undo_calls.set(undo_calls.get() + 1);
            repo.reset(&before, ResetMode::Hard)
        })
        .with_redo(move |repo| {
            redo_calls.set(redo_calls.get() + 10);
            repo.reset(&after, ResetMode::Hard)
        })
        .with_redo_mode(ResetMode::Soft),
    );
    let plain = UndoEntry::new("plain", |_| Ok(()));
    assert!(plain.redo.is_none());
    assert_eq!(plain.redo_mode, ResetMode::Hard);

    let mut entry = state.undo_stack.pop().unwrap();
    assert_eq!(entry.label, "commit");
    assert_eq!(entry.redo_mode, ResetMode::Soft);
    (entry.undo)(&state.repo).unwrap();
    state.refresh();
    assert_eq!(state.history.head_sha.as_ref(), Some(&shas[0]));
    (entry.redo.as_mut().unwrap())(&state.repo).unwrap();
    state.refresh();
    assert_eq!(state.history.head_sha.as_ref(), Some(&shas[1]));
    assert_eq!(calls.get(), 11);
    state.redo_stack.push(entry);
    assert!(format!("{:?}", state.redo_stack[0]).contains("commit"));
}

// Hard reset and its undo

#[test]
fn a_hard_reset_stashes_the_changes_it_wipes_and_its_undo_restores_everything() {
    use imgit_app::repo_state::{hard_reset_keeping_changes, undo_hard_reset};

    let (scratch, shas) = Scratch::with_commits(3);
    let repo = &scratch.state.repo;
    // A tracked change, a staged new file and an untracked one.
    scratch.write("a.txt", "line 1\nline 2\nline 3\nedited\n");
    scratch.write("new.txt", "staged\n");
    repo.stage(&["new.txt"]).unwrap();
    scratch.write("loose.txt", "untracked\n");

    let saved = hard_reset_keeping_changes(repo, &shas[0]).unwrap();
    let saved = saved.expect("the changes were stashed");
    assert_eq!(
        fs::read_to_string(scratch.root().join("a.txt")).unwrap(),
        "line 1\n"
    );
    assert!(!scratch.root().join("new.txt").exists());
    assert_eq!(repo.stash_index_of(&saved), Some(0));

    undo_hard_reset(repo, &shas[2], Some(&saved)).unwrap();
    assert_eq!(head_of(&scratch), shas[2]);
    assert_eq!(
        fs::read_to_string(scratch.root().join("a.txt")).unwrap(),
        "line 1\nline 2\nline 3\nedited\n"
    );
    assert_eq!(
        fs::read_to_string(scratch.root().join("new.txt")).unwrap(),
        "staged\n"
    );
    assert_eq!(
        fs::read_to_string(scratch.root().join("loose.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(repo.stash_index_of(&saved), None, "the stash was popped");
}

#[test]
fn a_hard_reset_of_a_clean_tree_stashes_nothing_and_leaves_untracked_files() {
    use imgit_app::repo_state::{hard_reset_keeping_changes, undo_hard_reset};

    let (scratch, shas) = Scratch::with_commits(2);
    let repo = &scratch.state.repo;
    scratch.write("loose.txt", "untracked\n");
    assert_eq!(hard_reset_keeping_changes(repo, &shas[0]).unwrap(), None);
    assert!(scratch.root().join("loose.txt").exists());
    assert_eq!(head_of(&scratch), shas[0]);
    undo_hard_reset(repo, &shas[1], None).unwrap();
    assert_eq!(head_of(&scratch), shas[1]);
    assert!(scratch.root().join("loose.txt").exists());
}

#[test]
fn an_undo_whose_pop_conflicts_says_so() {
    use imgit_app::repo_state::{hard_reset_keeping_changes, undo_hard_reset};

    let (mut scratch, shas) = Scratch::with_commits(3);
    scratch.write("a.txt", "line 1\nline 2\nmine\n");
    let saved = hard_reset_keeping_changes(&scratch.state.repo, &shas[0])
        .unwrap()
        .unwrap();
    // The undo lands on a commit the stashed changes do not fit on: the pop cannot apply cleanly.
    let other = scratch.commit_file("a.txt", "line 1\nother\nelse\n", "diverge");
    let repo = &scratch.state.repo;
    let result = undo_hard_reset(repo, &other, Some(&saved));
    let error = result.unwrap_err().to_string();
    assert!(error.contains("conflicts in 1 file"), "{error}");
    assert!(
        error.contains("ImGit: auto stash before hard reset"),
        "{error}"
    );
    // The changes are in the conflicted file, between its markers.
    let text = fs::read_to_string(scratch.root().join("a.txt")).unwrap();
    assert!(text.contains("mine") && text.contains("<<<<<<<"), "{text}");
}

/// The sha HEAD points at, read behind the repository back.
fn head_of(scratch: &Scratch) -> String {
    let repo = git2::Repository::open(scratch.root()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    head.to_string()
}

#[test]
fn a_rebuild_drops_the_comparison_and_shifts_a_pending_scroll() {
    let (mut scratch, shas) = Scratch::with_commits(2);
    scratch.write("a.txt", "changed\n");
    scratch.state.refresh();
    assert_eq!(scratch.row_shas(), [WIP_SHA, &shas[1], &shas[0]]);
    scratch.state.load_compare_files(&shas[0]);
    assert_eq!(scratch.state.compare_sha.as_ref(), Some(&shas[0]));
    scratch.state.scroll_to_row = Some(2); // the root commit

    // The working directory is clean again: the WIP row goes and the rows move up.
    scratch.write("a.txt", "line 1\nline 2\n");
    scratch.state.refresh_status();
    assert_eq!(scratch.row_shas(), [&shas[1], &shas[0]]);
    assert_eq!(scratch.state.compare_sha, None);
    assert!(scratch.state.compare_files.is_empty());
    assert_eq!(scratch.state.scroll_to_row, Some(1));

    // A request for the WIP row itself has no row left.
    scratch.write("a.txt", "changed\n");
    scratch.state.refresh_status();
    scratch.state.scroll_to_row = Some(0);
    scratch.write("a.txt", "line 1\nline 2\n");
    scratch.state.refresh_status();
    assert_eq!(scratch.state.scroll_to_row, None);

    // And it comes back: the rows move down again, the old comparison does not.
    scratch.state.scroll_to_row = Some(0);
    scratch.write("a.txt", "changed\n");
    scratch.state.refresh_status();
    assert_eq!(scratch.row_shas(), [WIP_SHA, &shas[1], &shas[0]]);
    assert_eq!(scratch.state.compare_sha, None);
    assert_eq!(scratch.state.scroll_to_row, Some(1));
}
