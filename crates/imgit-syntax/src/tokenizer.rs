//! The line tokenizer: the state a line hands to the next one, the span sink and the scanners behind
//! [`tokenize_line`].

mod code;
mod css;
mod data;
mod dockerfile;
mod markdown;
mod markup;

use crate::MAX_LINE_LENGTH;
use crate::language::{Kind, Language};
use crate::token::{Span, Token};
use std::mem;

/// What a line leaves open for the next one. `State::default()`: nothing (the start of a file or a hunk).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    mode: Mode,
    /// How many CSS braces are open.
    braces: u8,
    /// A Dockerfile instruction continued by a backslash.
    continued: bool,
    /// The template literals whose `${ }` the code is in, innermost last: each one's count of open braces.
    templates: Vec<u8>,
    /// The embedded region the line ends in: a `<script>` / `<style>` body or a code fence.
    region: Option<Region>,
    /// An embedded region inside that one: a `<script>` in a fence's HTML.
    inner_region: Option<Region>,
}

/// The construct a line ends inside.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    None,
    /// Inside `code.delims[index]`, nested `nesting` deep in itself: `/* /* */ */`.
    Delim {
        index: usize,
        nesting: u8,
    },
    /// A string or comment ending at `close`, no escapes.
    Raw {
        token: Token,
        close: Vec<u8>,
    },
    /// Lines up to the one that is `close`, which may be indented when `indented`.
    Heredoc {
        close: Vec<u8>,
        indented: bool,
    },
    /// The text of a template literal.
    TemplateText,
    /// Inside a markup tag.
    Tag(TagKind),
    /// A tag's quoted attribute value, up to `quote`.
    AttrString {
        quote: u8,
        tag: TagKind,
    },
    MarkupComment,
    Cdata,
    /// Inside `markup.templates[index]`.
    TemplateTag(usize),
    /// A YAML `|` / `>` scalar: the lines indented past `indent`.
    BlockScalar {
        indent: u8,
    },
}

/// What a markup tag's body is, once the tag closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TagKind {
    #[default]
    Plain,
    /// `<script>`: JavaScript up to `</script`.
    Script,
    /// `<style>`: CSS up to `</style`.
    Style,
}

/// An embedded region: the language of a `<script>` / `<style>` body or a code fence (`None`: one the
/// highlighter does not know) and what ends it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Region {
    language: Option<&'static Language>,
    end_marker: Vec<u8>,
}

/// Appends the spans of `line` to `out` and advances `state` past it. A line over [`MAX_LINE_LENGTH`] gets no
/// spans (it is drawn plain) and resets the state.
pub fn tokenize_line(language: &Language, line: &str, state: &mut State, out: &mut Vec<Span>) {
    if line.len() > MAX_LINE_LENGTH {
        *state = State::default();
        return;
    }
    let first = out.len();
    let mut spans = Out {
        spans: out,
        first,
        line: line.as_bytes(),
        offset: 0,
    };
    tokenize_range(language, line.as_bytes(), state, &mut spans);
    spans.finish();
}

/// The spans of one line. Positions given to [`Out::add`] are relative to `offset` (where an embedded
/// language's part of the line starts); gaps between spans become text, or join the span before them when
/// blank.
struct Out<'a> {
    spans: &'a mut Vec<Span>,
    /// This line's first span.
    first: usize,
    line: &'a [u8],
    offset: usize,
}

impl Out<'_> {
    /// The same sink for a part of the line starting `offset` further in.
    fn at(&mut self, offset: usize) -> Out<'_> {
        Out {
            spans: self.spans,
            first: self.first,
            line: self.line,
            offset: self.offset + offset,
        }
    }

    fn add(&mut self, begin: usize, end: usize, token: Token) {
        let last = self.end();
        // A span never splits a character: its ends move on to the next boundary.
        let end = self.boundary((end + self.offset).min(self.line.len()));
        let begin = self.boundary((begin + self.offset).max(last));
        if end <= begin {
            return;
        }
        self.gap(last, begin);
        self.push(begin, end, token);
    }

    /// Covers the rest of the line.
    fn finish(&mut self) {
        self.gap(self.end(), self.line.len());
    }

    fn end(&self) -> usize {
        self.spans[self.first..].last().map_or(0, Span::end)
    }

    /// The first character boundary of the line at or after `index`.
    fn boundary(&self, mut index: usize) -> usize {
        while self
            .line
            .get(index)
            .is_some_and(|&byte| byte & 0xC0 == 0x80)
        {
            index += 1;
        }
        index
    }

    fn gap(&mut self, begin: usize, end: usize) {
        if end <= begin {
            return;
        }
        if blank(&self.line[begin..end])
            && let Some(last) = self.spans[self.first..].last_mut()
        {
            last.end = offset(end);
            return;
        }
        self.push(begin, end, Token::Text);
    }

    fn push(&mut self, begin: usize, end: usize, token: Token) {
        if let Some(last) = self.spans[self.first..].last_mut()
            && last.token == token
            && last.end() == begin
        {
            last.end = offset(end);
            return;
        }
        self.spans.push(Span {
            start: offset(begin),
            end: offset(end),
            token,
        });
    }
}

/// A byte offset into a line, which is at most [`MAX_LINE_LENGTH`] long.
fn offset(index: usize) -> u16 {
    u16::try_from(index).unwrap_or(u16::MAX)
}

fn tokenize_range(language: &Language, s: &[u8], state: &mut State, out: &mut Out<'_>) {
    match language.kind {
        Kind::Code => code::tokenize(language, s, state, out),
        Kind::Markup => markup::tokenize(language, s, state, out),
        Kind::Css => css::tokenize(language, s, state, out),
        Kind::Json => data::tokenize_json(language, s, state, out),
        Kind::Yaml => data::tokenize_yaml(language, s, state, out),
        Kind::Ini => data::tokenize_ini(language, s, out),
        Kind::Markdown => markdown::tokenize(s, state, out),
        Kind::Dockerfile => dockerfile::tokenize(language, s, state, out),
    }
}

/// Tokenizes `[begin, end)` of `s` with another language, from a fresh state: a template tag's code, a bit of
/// inline HTML. Without a language the piece is one `fallback` span.
fn tokenize_piece(
    language: Option<&Language>,
    s: &[u8],
    begin: usize,
    end: usize,
    out: &mut Out<'_>,
    fallback: Token,
) {
    if end <= begin {
        return;
    }
    match language {
        Some(language) => {
            let mut fresh = State::default();
            tokenize_range(language, &s[begin..end], &mut fresh, &mut out.at(begin));
        }
        None => out.add(begin, end, fallback),
    }
}

/// An embedded language's part of a line. The region's own fields of `state` are kept apart, so an embedded
/// HTML does not take over the fence it sits in; that HTML's own `<script>` / `<style>` region is the inner
/// one.
fn tokenize_embedded(
    language: &Language,
    s: &[u8],
    begin: usize,
    end: usize,
    state: &mut State,
    out: &mut Out<'_>,
) {
    let mut inner = State {
        mode: mem::take(&mut state.mode),
        braces: state.braces,
        continued: state.continued,
        templates: mem::take(&mut state.templates),
        region: state.inner_region.take(),
        inner_region: None,
    };
    tokenize_range(language, &s[begin..end], &mut inner, &mut out.at(begin));
    state.mode = inner.mode;
    state.braces = inner.braces;
    state.continued = inner.continued;
    state.templates = inner.templates;
    state.inner_region = inner.region;
}

// ---- byte helpers ----

fn is_blank(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | 0x0C | 0x0B)
}

/// A byte of a multi-byte UTF-8 character: part of a word, never split.
fn is_utf8(c: u8) -> bool {
    c >= 0x80
}

fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn contains(chars: &str, c: u8) -> bool {
    chars.as_bytes().contains(&c)
}

/// Whether `s` has `c` at `i`.
fn at(s: &[u8], i: usize, c: u8) -> bool {
    s.get(i) == Some(&c)
}

fn starts_at(s: &[u8], i: usize, prefix: &[u8]) -> bool {
    !prefix.is_empty() && s.get(i..).is_some_and(|rest| rest.starts_with(prefix))
}

fn starts_at_folded(s: &[u8], i: usize, prefix: &[u8]) -> bool {
    !prefix.is_empty()
        && s.get(i..)
            .and_then(|rest| rest.get(..prefix.len()))
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// The first `needle` in `s` at or after `from`.
fn find(s: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let rest = s.get(from..)?;
    if needle.is_empty() {
        return Some(from);
    }
    rest.windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

fn find_folded(s: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    (from..=s.len().checked_sub(needle.len())?).find(|&i| starts_at_folded(s, i, needle))
}

/// The first `byte` in `s` at or after `from`.
fn find_byte(s: &[u8], byte: u8, from: usize) -> Option<usize> {
    find_any(s, &[byte], from)
}

/// The first of `bytes` in `s` at or after `from`.
fn find_any(s: &[u8], bytes: &[u8], from: usize) -> Option<usize> {
    s.get(from..)?
        .iter()
        .position(|c| bytes.contains(c))
        .map(|position| position + from)
}

fn skip_blanks(s: &[u8], i: usize) -> usize {
    s.get(i..)
        .and_then(|rest| rest.iter().position(|&c| !is_blank(c)))
        .map_or(s.len().max(i), |position| position + i)
}

fn blank(s: &[u8]) -> bool {
    s.iter().copied().all(is_blank)
}

/// `end` moved back over the blanks before it, no further than `begin`.
fn trim_end(s: &[u8], begin: usize, end: usize) -> usize {
    let blanks = s[begin..end]
        .iter()
        .rev()
        .take_while(|&&c| is_blank(c))
        .count();
    end - blanks
}
