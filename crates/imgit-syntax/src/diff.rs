//! The spans of a diff's lines, and the cache a diff view keeps of them.

use crate::language::Language;
use crate::spans::{Fnv, LimitExceeded, LineSpans};
use crate::token::Span;
use crate::tokenizer::{State, tokenize_line};
use crate::{MAX_BYTES, MAX_LINES};

/// Which side of a diff a line is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineKind {
    /// On both sides.
    #[default]
    Context,
    /// On the new side only.
    Added,
    /// On the old side only.
    Removed,
}

/// A line of a diff, as the highlighter reads it.
pub trait DiffLine {
    fn kind(&self) -> LineKind;
    /// The line's text, without its origin marker or line ending.
    fn text(&self) -> &str;
    /// 1-based; `None` for an added line.
    fn old_lineno(&self) -> Option<u32>;
    /// 1-based; `None` for a removed line.
    fn new_lineno(&self) -> Option<u32>;
}

/// A file's diff, as the highlighter reads it.
pub trait Diff {
    type Line: DiffLine;

    /// What identifies this load of the diff, if anything does: a diff with a revision never changes.
    fn revision(&self) -> Option<u64>;
    /// The file's path, which picks the language.
    fn path(&self) -> &str;
    fn is_binary(&self) -> bool;
    fn hunk_count(&self) -> usize;
    /// The lines of hunk `hunk`, which is below [`Diff::hunk_count`].
    fn hunk_lines(&self, hunk: usize) -> &[Self::Line];
}

/// The hunks of `diff`, each as its lines.
fn hunks<D: Diff + ?Sized>(diff: &D) -> impl Iterator<Item = &[D::Line]> {
    (0..diff.hunk_count()).map(|hunk| diff.hunk_lines(hunk))
}

/// The first line of the file, when the diff starts there: for a shebang.
fn first_line<D: Diff + ?Sized>(diff: &D) -> Option<&str> {
    let line = hunks(diff).next()?.first()?;
    (line.new_lineno() == Some(1) || line.old_lineno() == Some(1)).then(|| line.text())
}

/// A diff's lines: each hunk from a fresh state, the old side (context and deletions) and the new side
/// (context and additions) each carried on its own, so a block comment opened by a deletion does not run into
/// the additions. Context lines are coloured by the new side.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffSpans {
    spans: LineSpans,
    /// Per hunk, the index of its first line in `spans`.
    hunk_starts: Vec<usize>,
}

impl DiffSpans {
    /// The spans of line `line` of hunk `hunk`; none for a line outside the diff (drawn plain).
    #[must_use]
    pub fn line(&self, hunk: usize, line: usize) -> &[Span] {
        let Some(&start) = self.hunk_starts.get(hunk) else {
            return &[];
        };
        let end = self
            .hunk_starts
            .get(hunk + 1)
            .copied()
            .unwrap_or(self.spans.len());
        if line < end - start {
            self.spans.line(start + line)
        } else {
            &[]
        }
    }

    /// The spans of every line, hunk after hunk.
    #[must_use]
    pub fn lines(&self) -> &LineSpans {
        &self.spans
    }
}

/// Tokenizes the lines of `diff`.
#[must_use]
pub fn tokenize_diff<D: Diff + ?Sized>(language: &Language, diff: &D) -> DiffSpans {
    let mut result = DiffSpans::default();
    let mut scratch = Vec::new();
    for lines in hunks(diff) {
        result.hunk_starts.push(result.spans.len());
        let mut old_side = State::default();
        let mut new_side = State::default();
        for line in lines {
            let text = line.text();
            match line.kind() {
                LineKind::Removed => result.spans.push(language, text, &mut old_side),
                LineKind::Added => result.spans.push(language, text, &mut new_side),
                LineKind::Context => {
                    // A context line moves both sides on; tokenize it twice only when they disagree.
                    let same = old_side == new_side;
                    result.spans.push(language, text, &mut new_side);
                    if same {
                        old_side.clone_from(&new_side);
                    } else {
                        scratch.clear();
                        tokenize_line(language, text, &mut old_side, &mut scratch);
                    }
                }
            }
        }
    }
    result
}

/// A cheap content hash of `diff`.
///
/// # Errors
///
/// [`LimitExceeded`] when the diff is over the size limits.
pub fn fingerprint_diff<D: Diff + ?Sized>(diff: &D) -> Result<u64, LimitExceeded> {
    let mut hash = Fnv::new();
    let mut line_count = 0;
    let mut bytes = 0;
    hash.write(diff.path().as_bytes());
    for lines in hunks(diff) {
        hash.write(b"\n@@");
        line_count += lines.len();
        for line in lines {
            let text = line.text().as_bytes();
            bytes += text.len();
            hash.write_byte(match line.kind() {
                LineKind::Context => b' ',
                LineKind::Added => b'+',
                LineKind::Removed => b'-',
            });
            hash.write(text);
            hash.write_byte(b'\n');
        }
        if line_count > MAX_LINES {
            return Err(LimitExceeded::Lines);
        }
        if bytes > MAX_BYTES {
            return Err(LimitExceeded::Bytes);
        }
    }
    Ok(hash.finish())
}

/// The highlighting of the diff a view shows. One per view; [`DiffCache::get`] returns `None` for plain text
/// (unknown language, binary, over the limits). A loaded diff is looked at once per revision; one without a
/// revision is fingerprinted each time and re-tokenized when its path or text changed.
#[derive(Debug, Default)]
pub struct DiffCache {
    /// The diff last looked at.
    revision: Option<u64>,
    /// Whether `get` returned `spans` for it.
    highlighted: bool,
    /// The hash and language of the text `spans` holds.
    tokenized: Option<(u64, &'static Language)>,
    spans: DiffSpans,
}

impl DiffCache {
    /// The spans of `diff`'s lines.
    pub fn get<D: Diff + ?Sized>(&mut self, diff: &D) -> Option<&DiffSpans> {
        let revision = diff.revision();
        if revision.is_some() && revision == self.revision {
            return self.highlighted.then_some(&self.spans);
        }
        self.revision = revision;
        self.highlighted = false;
        if diff.is_binary() || diff.hunk_count() == 0 {
            return None;
        }
        let language = Language::for_path(diff.path(), first_line(diff))?;
        let hash = fingerprint_diff(diff).ok()?;
        if self.tokenized != Some((hash, language)) {
            self.spans = tokenize_diff(language, diff);
            self.tokenized = Some((hash, language));
        }
        self.highlighted = true;
        Some(&self.spans)
    }
}
