//! The CHANGES column: files modified / added / deleted per commit, counted on a worker thread for the rows on
//! screen, and drawn as the coloured readout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use imgit_app::Job;
use imgit_git::{FileChange, FileStatus, Repository, Sha, WorkDirStatus};

use crate::theme::{Color, FONT_SIZE_ROW};
use crate::ui::Cx;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};

/// Files changed by a commit or the working directory, by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileCounts {
    pub modified: usize,
    pub added: usize,
    pub deleted: usize,
}

impl FileCounts {
    pub fn count(&mut self, changes: &[FileChange]) {
        for change in changes {
            match change.status {
                FileStatus::Added | FileStatus::Untracked => self.added += 1,
                FileStatus::Deleted => self.deleted += 1,
                _ => self.modified += 1,
            }
        }
    }

    /// The WIP row's counts: every staged, unstaged and conflicted file.
    #[must_use]
    pub fn of_status(status: &WorkDirStatus) -> Self {
        let mut counts = Self::default();
        counts.count(&status.staged);
        counts.count(&status.unstaged);
        counts.count(&status.conflicted);
        counts
    }
}

/// How many commits one worker batch counts.
const BATCH: usize = 48;

type Batch = Vec<(Sha, FileCounts)>;

/// The counts of one repository: kept for good (a commit never changes), a batch read at a time.
#[derive(Debug, Default)]
struct RepoCounts {
    counts: HashMap<Sha, FileCounts>,
    job: Job<Batch>,
}

/// The CHANGES column's counts per commit, by repository (working directory): read on a worker thread (with its
/// own repository handle) for the rows on screen, a batch at a time.
#[derive(Debug, Default)]
pub struct ChangeCounts {
    repos: HashMap<PathBuf, RepoCounts>,
}

/// The commits of `wanted` not counted yet, up to one batch.
fn next_batch<'a>(
    counts: &HashMap<Sha, FileCounts>,
    wanted: impl IntoIterator<Item = &'a Sha>,
) -> Vec<Sha> {
    wanted
        .into_iter()
        .filter(|sha| !counts.contains_key(*sha))
        .take(BATCH)
        .cloned()
        .collect()
}

impl ChangeCounts {
    /// Collects the finished batch of `workdir` and, when none runs, starts one for the `wanted` commits not
    /// yet counted.
    pub fn update(&mut self, workdir: &Path, wanted: &[Sha]) {
        let cache = self.repos.entry(workdir.to_path_buf()).or_default();
        if cache.job.is_running() {
            match cache.job.poll() {
                Some(batch) => cache.counts.extend(batch),
                // Still at it (a worker that died leaves the job idle: the next frame asks again).
                None if cache.job.is_running() => return,
                None => {}
            }
        }
        let batch = next_batch(&cache.counts, wanted);
        if batch.is_empty() {
            return;
        }
        let workdir = workdir.to_path_buf();
        cache.job = Job::spawn(move || {
            let worker = Repository::open(&workdir).ok();
            batch
                .into_iter()
                .map(|sha| {
                    // A commit that cannot be read counts as no changes rather than being asked for again
                    // every frame.
                    let mut counts = FileCounts::default();
                    if let Some(changes) = worker.as_ref().and_then(|w| w.commit_changes(&sha).ok())
                    {
                        counts.count(&changes);
                    }
                    (sha, counts)
                })
                .collect()
        });
    }

    /// The counts of `sha`, once the background count has them.
    #[must_use]
    pub fn get(&self, workdir: &Path, sha: &str) -> Option<&FileCounts> {
        self.repos.get(workdir)?.counts.get(sha)
    }

    /// Whether a count is running (a capture waits for it).
    #[must_use]
    pub fn busy(&self) -> bool {
        self.repos.values().any(|repo| repo.job.is_running())
    }
}

/// "✎ 66  + 34  − 3" (modified / added / deleted files), coloured. Returns the width drawn.
pub fn draw_file_counts(
    cx: &mut Cx<'_>,
    draw: DrawList<'_>,
    pos: Vec2,
    counts: &FileCounts,
    alpha: f32,
) -> f32 {
    let p = cx.palette;
    // .tiny-files-readout: a 10px solid icon in the status colour, 5px, the count in text-normal, 6px.
    let mut x = pos.x;
    let mid = pos.y + cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, "A").y / 2.0;
    let items: [(&str, usize, Color); 3] = [
        ("fas:pencil", counts.modified, p.orange),
        ("fas:plus", counts.added, p.green),
        ("fas:minus", counts.deleted, p.red),
    ];
    for (icon, value, color) in items {
        if value == 0 {
            continue;
        }
        let size = cx.px(10.0);
        let icon_w = cx.fa_icon_width(icon, size);
        cx.draw_fa_icon(
            draw,
            icon,
            vec2(x + icon_w / 2.0, mid),
            size,
            color.with_alpha(color.a * alpha),
        );
        x += icon_w + cx.px(5.0);
        let text = value.to_string();
        cx.draw_text(
            draw,
            cx.fonts.regular,
            FONT_SIZE_ROW,
            vec2(x, pos.y),
            p.text_normal.with_alpha(p.text_normal.a * alpha),
            &text,
        );
        x += cx.text_size(cx.fonts.regular, FONT_SIZE_ROW, &text).x + cx.px(6.0);
    }
    x - pos.x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(status: FileStatus) -> FileChange {
        FileChange {
            path: "f".into(),
            status,
            ..FileChange::default()
        }
    }

    #[test]
    fn counts_group_statuses() {
        let mut counts = FileCounts::default();
        counts.count(&[
            change(FileStatus::Added),
            change(FileStatus::Untracked),
            change(FileStatus::Deleted),
            change(FileStatus::Modified),
            change(FileStatus::Renamed),
            change(FileStatus::Conflicted),
        ]);
        assert_eq!(
            counts,
            FileCounts {
                modified: 3,
                added: 2,
                deleted: 1
            }
        );
    }

    #[test]
    fn status_counts_every_list() {
        let status = WorkDirStatus {
            staged: vec![change(FileStatus::Added)],
            unstaged: vec![change(FileStatus::Modified), change(FileStatus::Deleted)],
            conflicted: vec![change(FileStatus::Conflicted)],
        };
        assert_eq!(
            FileCounts::of_status(&status),
            FileCounts {
                modified: 2,
                added: 1,
                deleted: 1
            }
        );
    }

    #[test]
    fn batches_skip_counted_commits_and_stop_at_the_limit() {
        let mut counts = HashMap::new();
        counts.insert("a".to_owned(), FileCounts::default());
        let wanted: Vec<Sha> = ["a", "b", "c"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(next_batch(&counts, &wanted), ["b", "c"]);
        let many: Vec<Sha> = (0..100).map(|i| i.to_string()).collect();
        assert_eq!(next_batch(&HashMap::new(), &many).len(), BATCH);
    }
}
