//! The spans of whole texts, and the cache a merge pane keeps of them.

use crate::language::Language;
use crate::token::Span;
use crate::tokenizer::{State, tokenize_line};
use crate::{MAX_BYTES, MAX_LINES};
use std::error::Error;
use std::fmt;

/// The spans of many lines, stored flat.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineSpans {
    spans: Vec<Span>,
    /// Per line, one past its last span.
    ends: Vec<usize>,
}

impl LineSpans {
    /// Forgets every line.
    pub fn clear(&mut self) {
        self.spans.clear();
        self.ends.clear();
    }

    /// Tokenizes `line` as the next line.
    pub fn push(&mut self, language: &Language, line: &str, state: &mut State) {
        tokenize_line(language, line, state, &mut self.spans);
        self.ends.push(self.spans.len());
    }

    /// Adds a line with no spans: drawn plain.
    pub fn push_plain(&mut self) {
        self.ends.push(self.spans.len());
    }

    /// How many lines there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The spans of line `index`; none for a plain line or one past the end.
    #[must_use]
    pub fn line(&self, index: usize) -> &[Span] {
        let Some(&end) = self.ends.get(index) else {
            return &[];
        };
        let start = index.checked_sub(1).map_or(0, |before| self.ends[before]);
        &self.spans[start..end]
    }

    /// Every line's spans, in order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &[Span]> {
        (0..self.len()).map(|index| self.line(index))
    }
}

/// A whole text, line by line from its start.
#[must_use]
pub fn tokenize_lines<S: AsRef<str>>(language: &Language, lines: &[S]) -> LineSpans {
    let mut spans = LineSpans::default();
    let mut state = State::default();
    for line in lines {
        spans.push(language, line.as_ref(), &mut state);
    }
    spans
}

/// A text too large to highlight: it is drawn plain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitExceeded {
    /// More than [`MAX_LINES`] lines.
    Lines,
    /// More than [`MAX_BYTES`] bytes.
    Bytes,
}

impl fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lines => write!(f, "more than {MAX_LINES} lines"),
            Self::Bytes => write!(f, "more than {MAX_BYTES} bytes"),
        }
    }
}

impl Error for LimitExceeded {}

/// FNV-1a.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;

    pub fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(Self::PRIME);
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_byte(byte);
        }
    }

    pub fn finish(self) -> u64 {
        self.0
    }
}

/// A cheap content hash of `lines`.
///
/// # Errors
///
/// [`LimitExceeded`] when the text is over the size limits.
pub fn fingerprint_lines<S: AsRef<str>>(lines: &[S]) -> Result<u64, LimitExceeded> {
    if lines.len() > MAX_LINES {
        return Err(LimitExceeded::Lines);
    }
    let mut hash = Fnv::new();
    let mut bytes = 0;
    for line in lines {
        let line = line.as_ref().as_bytes();
        bytes += line.len();
        if bytes > MAX_BYTES {
            return Err(LimitExceeded::Bytes);
        }
        hash.write(line);
        hash.write_byte(b'\n');
    }
    Ok(hash.finish())
}

/// The highlighting of a whole text (a merge pane). One per view; [`TextCache::get`] returns `None` for plain
/// text (unknown language, over the limits). A text with a revision is looked at once per revision and path;
/// one without is fingerprinted each time and re-tokenized when its path or text changed.
#[derive(Debug, Default)]
pub struct TextCache {
    /// The text last looked at.
    revision: Option<u64>,
    path: String,
    /// Whether `get` returned `spans` for it.
    highlighted: bool,
    /// The hash and language of the text `spans` holds.
    tokenized: Option<(u64, &'static Language)>,
    spans: LineSpans,
}

impl TextCache {
    /// The spans of `lines`, the text of the file at `path`, at `revision` (`None`: fingerprinted).
    pub fn get<S: AsRef<str>>(
        &mut self,
        path: &str,
        lines: &[S],
        revision: Option<u64>,
    ) -> Option<&LineSpans> {
        if revision.is_some() && revision == self.revision && path == self.path {
            return self.highlighted.then_some(&self.spans);
        }
        self.revision = revision;
        self.path.clear();
        self.path.push_str(path);
        self.highlighted = false;
        let language = Language::for_path(path, lines.first().map(AsRef::as_ref))?;
        let hash = fingerprint_lines(lines).ok()?;
        if self.tokenized != Some((hash, language)) {
            self.spans = tokenize_lines(language, lines);
            self.tokenized = Some((hash, language));
        }
        self.highlighted = true;
        Some(&self.spans)
    }
}
