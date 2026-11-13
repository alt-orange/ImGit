//! What lets the highlighter read the Git layer's diffs: `imgit_syntax` knows nothing of `imgit_git`, so its
//! [`Diff`] / [`DiffLine`] traits are implemented here, on wrappers that borrow a loaded diff.
//!
//! ```
//! use imgit_app::syntax::SyntaxDiff;
//! use imgit_syntax::DiffCache;
//!
//! let diff = imgit_git::FileDiff::default();
//! let mut cache = DiffCache::default();
//! assert!(cache.get(&SyntaxDiff::new(&diff)).is_none()); // no hunks: plain
//! ```

use std::cell::OnceCell;

use imgit_git::{FileDiff, LineOrigin};
use imgit_syntax::{Diff, DiffLine, LineKind};

/// A diff line as the highlighter reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxLine<'a>(pub &'a imgit_git::DiffLine);

impl DiffLine for SyntaxLine<'_> {
    fn kind(&self) -> LineKind {
        match self.0.origin {
            LineOrigin::Context => LineKind::Context,
            LineOrigin::Added => LineKind::Added,
            LineOrigin::Deleted => LineKind::Removed,
        }
    }

    fn text(&self) -> &str {
        &self.0.text
    }

    fn old_lineno(&self) -> Option<u32> {
        self.0.old_lineno
    }

    fn new_lineno(&self) -> Option<u32> {
        self.0.new_lineno
    }
}

/// A file's diff as the highlighter reads it. Cheap to make every frame: the lines are only wrapped when the
/// highlighter actually reads them, which [`imgit_syntax::DiffCache`] does once per revision.
#[derive(Debug, Clone)]
pub struct SyntaxDiff<'a> {
    diff: &'a FileDiff,
    /// Per hunk, its lines.
    lines: OnceCell<Vec<Vec<SyntaxLine<'a>>>>,
}

impl<'a> SyntaxDiff<'a> {
    #[must_use]
    pub const fn new(diff: &'a FileDiff) -> Self {
        Self {
            diff,
            lines: OnceCell::new(),
        }
    }

    /// The diff this reads.
    #[must_use]
    pub const fn diff(&self) -> &'a FileDiff {
        self.diff
    }
}

impl<'a> From<&'a FileDiff> for SyntaxDiff<'a> {
    fn from(diff: &'a FileDiff) -> Self {
        Self::new(diff)
    }
}

impl<'a> Diff for SyntaxDiff<'a> {
    type Line = SyntaxLine<'a>;

    fn revision(&self) -> Option<u64> {
        self.diff.revision.map(imgit_git::Revision::get)
    }

    fn path(&self) -> &str {
        &self.diff.path
    }

    fn is_binary(&self) -> bool {
        self.diff.binary
    }

    fn hunk_count(&self) -> usize {
        self.diff.hunks.len()
    }

    fn hunk_lines(&self, hunk: usize) -> &[Self::Line] {
        let diff = self.diff;
        &self.lines.get_or_init(|| {
            diff.hunks
                .iter()
                .map(|hunk| hunk.lines.iter().map(SyntaxLine).collect())
                .collect()
        })[hunk]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imgit_git::{DiffHunk, Revision};
    use imgit_syntax::{DiffCache, Token};

    fn line(
        origin: LineOrigin,
        old: Option<u32>,
        new: Option<u32>,
        text: &str,
    ) -> imgit_git::DiffLine {
        imgit_git::DiffLine {
            origin,
            old_lineno: old,
            new_lineno: new,
            text: text.to_owned(),
        }
    }

    fn sample() -> FileDiff {
        FileDiff {
            revision: Some(Revision::next()),
            path: "src/main.rs".to_owned(),
            hunks: vec![
                DiffHunk {
                    header: "@@ -1,2 +1,2 @@".to_owned(),
                    lines: vec![
                        line(LineOrigin::Context, Some(1), Some(1), "fn main() {"),
                        line(LineOrigin::Deleted, Some(2), None, "    let a = 1;"),
                        line(LineOrigin::Added, None, Some(2), "    // gone"),
                    ],
                },
                DiffHunk {
                    header: "@@ -9 +9 @@".to_owned(),
                    lines: vec![line(LineOrigin::Context, Some(9), Some(9), "}")],
                },
            ],
            ..FileDiff::default()
        }
    }

    #[test]
    fn the_wrappers_read_the_diff() {
        let diff = sample();
        let wrapped = SyntaxDiff::new(&diff);
        assert_eq!(wrapped.revision(), diff.revision.map(Revision::get));
        assert_eq!(wrapped.path(), "src/main.rs");
        assert!(!wrapped.is_binary());
        assert_eq!(wrapped.hunk_count(), 2);
        let lines = wrapped.hunk_lines(0);
        assert_eq!(
            lines.iter().map(DiffLine::kind).collect::<Vec<_>>(),
            [LineKind::Context, LineKind::Removed, LineKind::Added]
        );
        assert_eq!(lines[1].text(), "    let a = 1;");
        assert_eq!(
            (lines[1].old_lineno(), lines[1].new_lineno()),
            (Some(2), None)
        );
        assert_eq!(
            (lines[2].old_lineno(), lines[2].new_lineno()),
            (None, Some(2))
        );
        assert_eq!(wrapped.hunk_lines(1).len(), 1);
    }

    #[test]
    fn the_highlighter_takes_a_wrapped_diff() {
        let diff = sample();
        let mut cache = DiffCache::default();
        let spans = cache.get(&SyntaxDiff::new(&diff)).expect("rust is known");
        assert_eq!(spans.line(0, 0)[0].token, Token::Keyword);
        assert_eq!(
            spans.line(0, 2).last().map(|span| span.token),
            Some(Token::Comment)
        );
        // The same revision again: answered without reading the lines.
        let again = SyntaxDiff::new(&diff);
        assert!(cache.get(&again).is_some());
        assert!(again.lines.get().is_none());
    }
}
