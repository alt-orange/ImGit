//! Syntax highlighting for the diff and merge editors: a small line-based tokenizer.
//!
//! Each line is cut into [`Span`]s of Monaco-style [`Token`] classes; what a line leaves open (a block
//! comment, a multi-line or raw string, a heredoc, a template literal, a markup tag, a code fence) is carried
//! to the next line in a [`State`]. The common languages have their own scanners (tier 1); the long tail is
//! driven by tables of keywords, comment markers and string delimiters alone (tier 2). Unknown files are
//! plain text and cost nothing.
//!
//! ```
//! use imgit_syntax::{Language, Token, tokenize_lines};
//!
//! let rust = Language::for_path("src/main.rs", None).expect("a known extension");
//! let spans = tokenize_lines(rust, &["fn main() {} // entry"]);
//! assert_eq!(spans.line(0)[0].token, Token::Keyword);
//! ```

mod diff;
mod language;
mod languages;
mod spans;
mod token;
mod tokenizer;

pub use diff::{Diff, DiffCache, DiffLine, DiffSpans, LineKind, fingerprint_diff, tokenize_diff};
pub use language::Language;
pub use spans::{LimitExceeded, LineSpans, TextCache, fingerprint_lines, tokenize_lines};
pub use token::{Span, Token};
pub use tokenizer::{State, tokenize_line};

/// Longer lines are drawn as plain text.
pub const MAX_LINE_LENGTH: usize = 2000;
/// Files with more lines are drawn as plain text.
pub const MAX_LINES: usize = 5000;
/// Files with more bytes are drawn as plain text.
pub const MAX_BYTES: usize = 1024 * 1024;
