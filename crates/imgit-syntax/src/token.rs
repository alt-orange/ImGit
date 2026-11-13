//! Token classes and the spans a line is cut into.

use std::ops::Range;

/// A Monaco-style token class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Token {
    #[default]
    Text,
    Comment,
    String,
    Number,
    Keyword,
    Type,
    Function,
    Variable,
    Operator,
    Punctuation,
    Tag,
    Attribute,
    Regexp,
    Constant,
}

/// Bytes `[start, end)` of a line. A line's spans cover it from start to end; blanks join the span before
/// them. Both ends fall on character boundaries of the line.
///
/// Offsets are stored in 16 bits: lines longer than [`MAX_LINE_LENGTH`](crate::MAX_LINE_LENGTH) get no spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub(crate) start: u16,
    pub(crate) end: u16,
    /// The class of the bytes.
    pub token: Token,
}

impl Span {
    /// The byte offset of the span's first byte in its line.
    #[must_use]
    pub fn start(&self) -> usize {
        usize::from(self.start)
    }

    /// The byte offset one past the span's last byte.
    #[must_use]
    pub fn end(&self) -> usize {
        usize::from(self.end)
    }

    /// The bytes of the line the span covers.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.start()..self.end()
    }
}
