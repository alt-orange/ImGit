//! The merge tool's model for one conflicted file: its base, ours (A) and theirs (B) texts split into chunks by a
//! three-way line merge, the lines picked from A and B for each conflict, and the output they make.

use std::ops::Range;

use imgit_git::Revision;
use imgit_git::repository::{LineHunk, diff_lines};

/// One of the two sides being merged: A is ours, B is theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChunkKind {
    /// Unchanged on both sides.
    #[default]
    Stable,
    /// Changed by A only (or by both in the same way): A's lines are taken.
    TakeA,
    /// Changed by B only: B's lines are taken.
    TakeB,
    /// Changed by both: needs a choice.
    Conflict,
}

/// A run of lines the three texts agree on how to merge; `base`, `a` and `b` are its line ranges in each text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MergeChunk {
    pub kind: ChunkKind,
    pub base: Range<usize>,
    pub a: Range<usize>,
    pub b: Range<usize>,
}

impl MergeChunk {
    /// The chunk's lines on `side`.
    #[must_use]
    pub fn side(&self, side: Side) -> Range<usize> {
        match side {
            Side::A => self.a.clone(),
            Side::B => self.b.clone(),
        }
    }
}

/// Where a line of the output came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// Agreed on (or a one-sided change).
    Merged,
    /// Picked for `conflict`: line `line` of `side`.
    Picked {
        side: Side,
        conflict: usize,
        line: usize,
    },
    /// Of a conflict nothing was picked for: the base's lines stand in. A `placeholder` marks where such a
    /// conflict with no base lines sits; it is not written to the file.
    Unresolved { conflict: usize, placeholder: bool },
}

/// One line of the output: its text and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub text: String,
    pub origin: Origin,
}

impl OutputLine {
    fn merged(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            origin: Origin::Merged,
        }
    }

    /// The conflict the line belongs to.
    #[must_use]
    pub fn conflict(&self) -> Option<usize> {
        match self.origin {
            Origin::Merged => None,
            Origin::Picked { conflict, .. } | Origin::Unresolved { conflict, .. } => Some(conflict),
        }
    }

    /// Shown in the output pane but not part of the file.
    #[must_use]
    pub fn is_placeholder(&self) -> bool {
        matches!(
            self.origin,
            Origin::Unresolved {
                placeholder: true,
                ..
            }
        )
    }
}

/// The line ending a file is written back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Eol {
    #[default]
    Lf,
    CrLf,
}

impl Eol {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeTool {
    /// The conflicted file, relative to the working directory.
    pub path: String,
    /// The "Commit `<sha6>` on `<branch>`" parts: short sha and branch.
    pub ours_label: String,
    pub theirs_label: String,
    pub ours_sha: String,
    pub theirs_sha: String,
    /// The three texts' lines, without their line endings.
    pub base: Vec<String>,
    pub ours: Vec<String>,
    pub theirs: Vec<String>,
    /// A new one at each [`load`](Self::load); `None` before the first.
    pub revision: Option<Revision>,
    /// The whole merge, in order.
    pub chunks: Vec<MergeChunk>,
    /// The conflicts, as indices into `chunks`.
    pub conflicts: Vec<usize>,
    /// Per conflict, the picked lines in the order they were picked.
    pub picks: Vec<Vec<(Side, usize)>>,
    /// The line ending written back (CRLF when ours uses it).
    pub eol: Eol,
    /// Whether the file ends with a line break.
    pub final_newline: bool,
    /// The output as it was typed into, which then replaces the picks.
    pub edited: Option<String>,
    /// The conflict the navigator is on.
    pub current: usize,
    /// A conflict the panes should scroll to.
    pub scroll_request: Option<usize>,
}

impl Default for MergeTool {
    fn default() -> Self {
        Self {
            path: String::new(),
            ours_label: String::new(),
            theirs_label: String::new(),
            ours_sha: String::new(),
            theirs_sha: String::new(),
            base: Vec::new(),
            ours: Vec::new(),
            theirs: Vec::new(),
            revision: None,
            chunks: Vec::new(),
            conflicts: Vec::new(),
            picks: Vec::new(),
            eol: Eol::Lf,
            final_newline: true,
            edited: None,
            current: 0,
            scroll_request: None,
        }
    }
}

/// Lines split on '\n' with a trailing '\r' dropped; a final line break does not start another line.
fn split_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|last| last.is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect()
}

impl MergeTool {
    /// Splits the three texts and merges them; nothing is picked afterwards.
    pub fn load(&mut self, base_text: &str, ours_text: &str, theirs_text: &str) {
        self.set_texts(base_text, ours_text, theirs_text);
        self.merge(
            &diff_lines(base_text, ours_text),
            &diff_lines(base_text, theirs_text),
        );
    }

    fn set_texts(&mut self, base_text: &str, ours_text: &str, theirs_text: &str) {
        self.base = split_lines(base_text);
        self.ours = split_lines(ours_text);
        self.theirs = split_lines(theirs_text);
        self.revision = Some(Revision::next());
        let reference = if ours_text.is_empty() {
            theirs_text
        } else {
            ours_text
        };
        self.eol = if reference.contains("\r\n") {
            Eol::CrLf
        } else {
            Eol::Lf
        };
        self.final_newline = reference.is_empty() || reference.ends_with('\n');
    }

    /// The three-way merge of the loaded lines, given each side's changes against the base.
    fn merge(&mut self, a_hunks: &[LineHunk], b_hunks: &[LineHunk]) {
        // Both sides' changes, in base order (A's first where they start on the same line).
        let mut hunks: Vec<(Side, LineHunk)> = a_hunks
            .iter()
            .map(|&hunk| (Side::A, hunk))
            .chain(b_hunks.iter().map(|&hunk| (Side::B, hunk)))
            .collect();
        hunks.sort_by_key(|(_, hunk)| hunk.old_start);

        /// Where the merge has got to in each text.
        #[derive(Default)]
        struct Cursor {
            base: usize,
            a: usize,
            b: usize,
        }
        let mut chunks = Vec::new();
        let mut cursor = Cursor::default();
        let stable = |cursor: &mut Cursor, chunks: &mut Vec<MergeChunk>, until: usize| {
            if until > cursor.base {
                let count = until - cursor.base;
                chunks.push(MergeChunk {
                    kind: ChunkKind::Stable,
                    base: cursor.base..until,
                    a: cursor.a..cursor.a + count,
                    b: cursor.b..cursor.b + count,
                });
                *cursor = Cursor {
                    base: until,
                    a: cursor.a + count,
                    b: cursor.b + count,
                };
            }
        };

        let mut rest = hunks.as_slice();
        while let Some(&(_, first)) = rest.first() {
            // A group: changes whose base ranges overlap or touch.
            let low = first.old_start;
            let mut high = low + first.old_count;
            let mut length = 0;
            // Per side (A, B): whether it changed anything here, and the lines it removed and added.
            let mut changed = [false; 2];
            let mut removed = [0; 2];
            let mut added = [0; 2];
            while let Some(&(side, hunk)) =
                rest.get(length).filter(|(_, hunk)| hunk.old_start <= high)
            {
                high = high.max(hunk.old_start + hunk.old_count);
                changed[side as usize] = true;
                removed[side as usize] += hunk.old_count;
                added[side as usize] += hunk.new_count;
                length += 1;
            }
            rest = &rest[length..];

            stable(&mut cursor, &mut chunks, low);
            let side_count = |side: Side| {
                (high - low + added[side as usize]).saturating_sub(removed[side as usize])
            };
            let a = cursor.a..cursor.a + side_count(Side::A);
            let b = cursor.b..cursor.b + side_count(Side::B);
            let kind = match changed {
                // Both sides made the same change: not a conflict.
                [true, true] if self.ours.get(a.clone()) == self.theirs.get(b.clone()) => {
                    ChunkKind::TakeA
                }
                [true, true] => ChunkKind::Conflict,
                [true, false] => ChunkKind::TakeA,
                [false, _] => ChunkKind::TakeB,
            };
            cursor = Cursor {
                base: high,
                a: a.end,
                b: b.end,
            };
            chunks.push(MergeChunk {
                kind,
                base: low..high,
                a,
                b,
            });
        }
        stable(&mut cursor, &mut chunks, self.base.len());

        self.conflicts = chunks
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.kind == ChunkKind::Conflict)
            .map(|(index, _)| index)
            .collect();
        self.chunks = chunks;
        self.reset();
    }

    #[must_use]
    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }

    /// The chunk of a conflict.
    #[must_use]
    pub fn conflict_chunk(&self, conflict: usize) -> Option<&MergeChunk> {
        self.chunks.get(*self.conflicts.get(conflict)?)
    }

    /// The lines of `side` in a conflict (empty for one there is not).
    fn conflict_lines(&self, conflict: usize, side: Side) -> Range<usize> {
        self.conflict_chunk(conflict)
            .map_or(0..0, |chunk| chunk.side(side))
    }

    /// The conflict whose `side` range holds `line`.
    #[must_use]
    pub fn conflict_at(&self, side: Side, line: usize) -> Option<usize> {
        (0..self.conflict_count())
            .find(|&conflict| self.conflict_lines(conflict, side).contains(&line))
    }

    #[must_use]
    pub fn is_picked(&self, conflict: usize, side: Side, line: usize) -> bool {
        self.picks
            .get(conflict)
            .is_some_and(|picks| picks.contains(&(side, line)))
    }

    /// Picks a line of a conflict, or takes it back.
    pub fn toggle_line(&mut self, conflict: usize, side: Side, line: usize) {
        let Some(picks) = self.picks.get_mut(conflict) else {
            return;
        };
        match picks.iter().position(|&pick| pick == (side, line)) {
            Some(index) => {
                picks.remove(index);
            }
            None => picks.push((side, line)),
        }
    }

    /// Whether every line of `side` in `conflict` (or, with `None`, in every conflict) is picked. An empty side
    /// counts as picked once anything of its conflict was chosen; no conflicts at all is not "every".
    #[must_use]
    pub fn side_fully_picked(&self, conflict: Option<usize>, side: Side) -> bool {
        let check = |conflict: usize| {
            let lines = self.conflict_lines(conflict, side);
            if lines.is_empty() {
                self.picks
                    .get(conflict)
                    .is_some_and(|picks| !picks.is_empty())
            } else {
                lines
                    .into_iter()
                    .all(|line| self.is_picked(conflict, side, line))
            }
        };
        match conflict {
            Some(conflict) => check(conflict),
            None => !self.conflicts.is_empty() && (0..self.conflict_count()).all(check),
        }
    }

    /// Every line of `side` in `conflict` (or, with `None`, in every conflict): picked when not all are, else
    /// none.
    pub fn toggle_side(&mut self, conflict: Option<usize>, side: Side) {
        let select = !self.side_fully_picked(conflict, side);
        let conflicts = match conflict {
            Some(conflict) => conflict..conflict + 1,
            None => 0..self.conflict_count(),
        };
        for conflict in conflicts {
            let lines = self.conflict_lines(conflict, side);
            let Some(picks) = self.picks.get_mut(conflict) else {
                continue;
            };
            picks.retain(|&(picked, _)| picked != side);
            if select {
                picks.extend(lines.map(|line| (side, line)));
            }
        }
    }

    #[must_use]
    pub fn any_picked(&self) -> bool {
        self.picks.iter().any(|picks| !picks.is_empty())
    }

    /// The conflicts nothing is picked for.
    #[must_use]
    pub fn unresolved_count(&self) -> usize {
        self.picks.iter().filter(|picks| picks.is_empty()).count()
    }

    /// Takes back every pick and edit.
    pub fn reset(&mut self) {
        self.picks = vec![Vec::new(); self.conflicts.len()];
        self.edited = None;
    }

    fn lines(&self, side: Side) -> &[String] {
        match side {
            Side::A => &self.ours,
            Side::B => &self.theirs,
        }
    }

    /// The merged lines as the picks stand (the output pane's rows; an edit is not looked at).
    #[must_use]
    pub fn output(&self) -> Vec<OutputLine> {
        let merged = |lines: &[String], range: &Range<usize>| {
            lines
                .get(range.clone())
                .unwrap_or_default()
                .iter()
                .map(|line| OutputLine::merged(line))
                .collect::<Vec<_>>()
        };
        let mut out = Vec::new();
        let mut conflict = 0;
        for chunk in &self.chunks {
            match chunk.kind {
                ChunkKind::Stable => out.extend(merged(&self.base, &chunk.base)),
                ChunkKind::TakeA => out.extend(merged(&self.ours, &chunk.a)),
                ChunkKind::TakeB => out.extend(merged(&self.theirs, &chunk.b)),
                ChunkKind::Conflict => {
                    let picks = self.picks.get(conflict).map_or(&[][..], Vec::as_slice);
                    if picks.is_empty() {
                        let unresolved = |text: &str, placeholder| OutputLine {
                            text: text.to_owned(),
                            origin: Origin::Unresolved {
                                conflict,
                                placeholder,
                            },
                        };
                        let base = self.base.get(chunk.base.clone()).unwrap_or_default();
                        if base.is_empty() {
                            out.push(unresolved("", true));
                        }
                        out.extend(base.iter().map(|line| unresolved(line, false)));
                    }
                    out.extend(picks.iter().filter_map(|&(side, line)| {
                        Some(OutputLine {
                            text: self.lines(side).get(line)?.clone(),
                            origin: Origin::Picked {
                                side,
                                conflict,
                                line,
                            },
                        })
                    }));
                    conflict += 1;
                }
            }
        }
        out
    }

    /// The text written to the file: the edited text, or the output's lines, with the file's own line ending.
    #[must_use]
    pub fn output_text(&self) -> String {
        let eol = self.eol.as_str();
        if let Some(edited) = &self.edited {
            // The editor's '\n' line breaks, in the file's own line ending.
            return edited.replace('\r', "").replace('\n', eol);
        }
        let output = self.output();
        let lines: Vec<&str> = output
            .iter()
            .filter(|line| !line.is_placeholder())
            .map(|line| line.text.as_str())
            .collect();
        let mut text = lines.join(eol);
        if self.final_newline && !lines.is_empty() {
            text.push_str(eol);
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(base: &str, ours: &str, theirs: &str) -> MergeTool {
        let mut tool = MergeTool::default();
        tool.load(base, ours, theirs);
        tool
    }

    fn kinds(tool: &MergeTool) -> Vec<ChunkKind> {
        tool.chunks.iter().map(|chunk| chunk.kind).collect()
    }

    fn texts(tool: &MergeTool) -> Vec<String> {
        tool.output().into_iter().map(|line| line.text).collect()
    }

    #[test]
    fn splits_lines() {
        assert_eq!(split_lines(""), Vec::<String>::new());
        assert_eq!(split_lines("a"), ["a"]);
        assert_eq!(split_lines("a\r\nb\n"), ["a", "b"]);
        assert_eq!(split_lines("a\n\n"), ["a", ""]);
        assert_eq!(split_lines("\n"), [""]);
        assert_eq!(split_lines("a\n\r"), ["a", ""]);
    }

    #[test]
    fn one_sided_changes_merge_themselves() {
        let tool = loaded("1\n2\n3\n", "1\n2\n3\n4\n", "one\n2\n3\n");
        assert_eq!(
            tool.chunks,
            [
                MergeChunk {
                    kind: ChunkKind::TakeB,
                    base: 0..1,
                    a: 0..1,
                    b: 0..1,
                },
                MergeChunk {
                    kind: ChunkKind::Stable,
                    base: 1..3,
                    a: 1..3,
                    b: 1..3,
                },
                MergeChunk {
                    kind: ChunkKind::TakeA,
                    base: 3..3,
                    a: 3..4,
                    b: 3..3,
                },
            ]
        );
        assert_eq!(tool.conflict_count(), 0);
        assert_eq!(tool.unresolved_count(), 0);
        assert!(!tool.side_fully_picked(None, Side::A));
        assert_eq!(tool.output_text(), "one\n2\n3\n4\n");
        assert!(
            tool.output()
                .iter()
                .all(|line| line.origin == Origin::Merged)
        );
        assert!(tool.revision.is_some());
    }

    #[test]
    fn the_same_change_on_both_sides_is_no_conflict() {
        let tool = loaded("a\nb\nc\n", "a\nB\nc\n", "a\nB\nc\n");
        assert_eq!(
            kinds(&tool),
            [ChunkKind::Stable, ChunkKind::TakeA, ChunkKind::Stable]
        );
        assert_eq!(tool.output_text(), "a\nB\nc\n");
        // Unchanged everywhere: one stable chunk; nothing at all: none.
        assert_eq!(
            kinds(&loaded("a\nb\n", "a\nb\n", "a\nb\n")),
            [ChunkKind::Stable]
        );
        assert!(loaded("", "", "").chunks.is_empty());
        assert_eq!(loaded("", "", "").output_text(), "");
    }

    #[test]
    fn a_conflict_is_resolved_by_picking_lines() {
        let mut tool = loaded(
            "a\nb\nc\nd\n",
            "a\nours 1\nours 2\nc\nd\nmore\n",
            "a\ntheirs\nc\nd\n",
        );
        assert_eq!(
            kinds(&tool),
            [
                ChunkKind::Stable,
                ChunkKind::Conflict,
                ChunkKind::Stable,
                ChunkKind::TakeA
            ]
        );
        assert_eq!(tool.conflicts, [1]);
        let chunk = tool.conflict_chunk(0).unwrap().clone();
        assert_eq!((chunk.base, chunk.a, chunk.b), (1..2, 1..3, 1..2));
        // The lines after the conflict are found on each side.
        assert_eq!(
            (tool.chunks[2].a.clone(), tool.chunks[2].b.clone()),
            (3..5, 2..4)
        );

        assert_eq!(tool.conflict_at(Side::A, 2), Some(0));
        assert_eq!(tool.conflict_at(Side::A, 3), None);
        assert_eq!(tool.conflict_at(Side::B, 1), Some(0));
        assert_eq!(tool.conflict_at(Side::B, 2), None);

        // Unresolved: the base's line stands in.
        assert_eq!(tool.unresolved_count(), 1);
        assert!(!tool.any_picked());
        assert_eq!(texts(&tool), ["a", "b", "c", "d", "more"]);
        assert_eq!(
            tool.output()[1].origin,
            Origin::Unresolved {
                conflict: 0,
                placeholder: false
            }
        );
        assert_eq!(tool.output()[1].conflict(), Some(0));
        assert_eq!(tool.output()[0].conflict(), None);

        // Picked lines come in the order they were picked.
        tool.toggle_line(0, Side::B, 1);
        tool.toggle_line(0, Side::A, 2);
        assert!(tool.is_picked(0, Side::B, 1));
        assert!(!tool.is_picked(0, Side::A, 1));
        assert!(tool.side_fully_picked(Some(0), Side::B));
        assert!(!tool.side_fully_picked(Some(0), Side::A));
        assert_eq!(tool.unresolved_count(), 0);
        assert_eq!(texts(&tool), ["a", "theirs", "ours 2", "c", "d", "more"]);
        assert_eq!(
            tool.output()[2].origin,
            Origin::Picked {
                side: Side::A,
                conflict: 0,
                line: 2
            }
        );
        tool.toggle_line(0, Side::B, 1);
        assert_eq!(texts(&tool), ["a", "ours 2", "c", "d", "more"]);

        // A whole side: all of it in file order, then none of it.
        tool.toggle_side(Some(0), Side::A);
        assert_eq!(tool.picks, [[(Side::A, 1), (Side::A, 2)]]);
        assert_eq!(tool.output_text(), "a\nours 1\nours 2\nc\nd\nmore\n");
        tool.toggle_side(Some(0), Side::A);
        assert!(!tool.any_picked());

        // Typing into the output replaces the picks, until a reset.
        tool.toggle_side(None, Side::B);
        tool.edited = Some("typed\r\ntext\n".to_owned());
        assert_eq!(tool.output_text(), "typed\ntext\n");
        tool.reset();
        assert_eq!(tool.edited, None);
        assert_eq!(tool.output_text(), "a\nb\nc\nd\nmore\n");

        // Indices that are not there are nobody's.
        tool.toggle_line(7, Side::A, 0);
        tool.toggle_side(Some(7), Side::A);
        assert!(!tool.is_picked(7, Side::A, 0));
        assert!(!tool.side_fully_picked(Some(7), Side::A));
        assert_eq!(tool.conflict_chunk(7), None);
    }

    #[test]
    fn a_conflict_without_base_lines_has_a_placeholder() {
        let mut tool = loaded("x\n", "x\nours\n", "x\ntheirs\n");
        assert_eq!(kinds(&tool), [ChunkKind::Stable, ChunkKind::Conflict]);
        let output = tool.output();
        assert_eq!(output.len(), 2);
        assert!(output[1].is_placeholder());
        assert_eq!(output[1].conflict(), Some(0));
        assert_eq!(tool.output_text(), "x\n"); // the placeholder is not written
        tool.toggle_side(None, Side::B);
        tool.toggle_side(None, Side::A);
        assert_eq!(tool.output_text(), "x\ntheirs\nours\n");
        assert!(tool.output().iter().all(|line| !line.is_placeholder()));
    }

    #[test]
    fn an_empty_side_counts_as_picked_with_its_conflict() {
        // Ours deletes the line theirs changes.
        let mut tool = loaded("a\nb\nc\n", "a\nc\n", "a\nB\nc\n");
        assert_eq!(tool.conflict_count(), 1);
        let chunk = tool.conflict_chunk(0).unwrap().clone();
        assert_eq!((chunk.a, chunk.b), (1..1, 1..2));
        assert!(!tool.side_fully_picked(Some(0), Side::A));
        tool.toggle_line(0, Side::B, 1);
        assert!(tool.side_fully_picked(Some(0), Side::A));
        assert!(tool.side_fully_picked(None, Side::B));
        // Toggling the empty side takes nothing and adds nothing.
        tool.toggle_side(Some(0), Side::A);
        assert_eq!(tool.picks, [[(Side::B, 1)]]);
    }

    #[test]
    fn every_conflict_at_once() {
        let mut tool = loaded("a\nb\nc\nd\ne\n", "A1\nb\nc\nd\nE1\n", "A2\nb\nc\nd\nE2\n");
        assert_eq!(tool.conflicts, [0, 2]);
        assert_eq!(tool.conflict_at(Side::B, 4), Some(1));
        tool.toggle_line(1, Side::A, 4);
        assert!(!tool.side_fully_picked(None, Side::A));
        assert_eq!(tool.unresolved_count(), 1);
        tool.toggle_side(None, Side::A);
        assert!(tool.side_fully_picked(None, Side::A));
        assert_eq!(tool.output_text(), "A1\nb\nc\nd\nE1\n");
        tool.toggle_side(None, Side::A);
        assert_eq!(tool.unresolved_count(), 2);
    }

    #[test]
    fn the_file_keeps_its_line_ending() {
        let tool = loaded("a\r\nb\r\n", "a\r\nb\r\nc", "A\r\nb\r\n");
        assert_eq!(tool.eol, Eol::CrLf);
        assert!(!tool.final_newline);
        assert_eq!(tool.ours, ["a", "b", "c"]);
        assert_eq!(tool.output_text(), "A\r\nb\r\nc");
        let mut tool = tool;
        tool.edited = Some("x\ny\n".to_owned());
        assert_eq!(tool.output_text(), "x\r\ny\r\n");

        // Ours is empty (deleted there): theirs decides.
        let tool = loaded("a\n", "", "a\r\nb\r\n");
        assert_eq!(tool.eol, Eol::CrLf);
        assert!(tool.final_newline);
        let tool = loaded("a\n", "a", "a\r\n");
        assert_eq!((tool.eol, tool.final_newline), (Eol::Lf, false));
    }

    #[test]
    fn touching_changes_group_into_one_chunk() {
        // By hand: A replaces base line 1, B inserts right after it.
        let mut tool = MergeTool::default();
        tool.set_texts("a\nb\nc\n", "a\nB\nc\n", "a\nb\nnew\nc\n");
        let a = LineHunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
        };
        let b = LineHunk {
            old_start: 2,
            old_count: 0,
            new_start: 2,
            new_count: 1,
        };
        tool.merge(&[a], &[b]);
        assert_eq!(
            tool.chunks,
            [
                MergeChunk {
                    kind: ChunkKind::Stable,
                    base: 0..1,
                    a: 0..1,
                    b: 0..1,
                },
                MergeChunk {
                    kind: ChunkKind::Conflict,
                    base: 1..2,
                    a: 1..2,
                    b: 1..3,
                },
                MergeChunk {
                    kind: ChunkKind::Stable,
                    base: 2..3,
                    a: 2..3,
                    b: 3..4,
                },
            ]
        );
        // The real diff of the same texts says the same.
        assert_eq!(
            loaded("a\nb\nc\n", "a\nB\nc\n", "a\nb\nnew\nc\n").chunks,
            tool.chunks
        );
    }
}
