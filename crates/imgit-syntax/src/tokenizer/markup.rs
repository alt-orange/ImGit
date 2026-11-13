//! Markup: XML, HTML (with its `<script>` / `<style>` bodies) and the HTML template languages.

use super::{
    Mode, Out, Region, State, TagKind, at, contains, find, find_byte, find_folded, is_alnum,
    is_alpha, is_blank, is_utf8, starts_at, tokenize_embedded, tokenize_piece,
};
use crate::language::{Language, TemplateTag};
use crate::token::Token;
use std::sync::LazyLock;

fn javascript() -> Option<&'static Language> {
    static LANGUAGE: LazyLock<Option<&'static Language>> =
        LazyLock::new(|| Language::by_name("javascript"));
    *LANGUAGE
}

fn css() -> Option<&'static Language> {
    static LANGUAGE: LazyLock<Option<&'static Language>> =
        LazyLock::new(|| Language::by_name("css"));
    *LANGUAGE
}

fn tag_name_start(c: u8) -> bool {
    is_alpha(c) || matches!(c, b'_' | b':' | b'#' | b'@') || is_utf8(c)
}

/// The end of the tag name starting at `i`.
fn scan_tag_name(s: &[u8], i: usize) -> usize {
    i + s[i..]
        .iter()
        .take_while(|&&c| is_alnum(c) || is_utf8(c) || contains("_:.-#@", c))
        .count()
}

/// Whether a tag's `>`, `/>` or `?>` is at `i` (`%>` too when `percent`).
fn tag_end_at(s: &[u8], i: usize, percent: bool) -> bool {
    match s[i] {
        b'>' => true,
        b'/' | b'?' => at(s, i + 1, b'>'),
        b'%' => percent && at(s, i + 1, b'>'),
        _ => false,
    }
}

/// A tag's attributes from `i`, up to its `>` (the index after it) or the end of the line.
fn tag_body(lang: &Language, s: &[u8], mut i: usize, st: &mut State, out: &mut Out<'_>) -> usize {
    let kind = match st.mode {
        Mode::Tag(kind) => kind,
        _ => TagKind::Plain,
    };
    let n = s.len();
    let mut value = false; // after an =
    while i < n {
        let ch = s[i];
        if is_blank(ch) {
            i += 1;
            continue;
        }
        if tag_end_at(s, i, true) {
            let end = i + if ch == b'>' { 1 } else { 2 };
            out.add(i, end, Token::Punctuation);
            st.mode = Mode::None;
            if lang.markup.html && ch == b'>' {
                st.region = match kind {
                    TagKind::Plain => None,
                    TagKind::Script => Some(Region {
                        language: javascript(),
                        end_marker: b"</script".to_vec(),
                    }),
                    TagKind::Style => Some(Region {
                        language: css(),
                        end_marker: b"</style".to_vec(),
                    }),
                };
            }
            return end;
        }
        if ch == b'=' {
            out.add(i, i + 1, Token::Punctuation);
            value = true;
            i += 1;
            continue;
        }
        if matches!(ch, b'"' | b'\'') {
            let Some(close) = find_byte(s, ch, i + 1) else {
                out.add(i, n, Token::String);
                st.mode = Mode::AttrString {
                    quote: ch,
                    tag: kind,
                };
                return n;
            };
            out.add(i, close + 1, Token::String);
            value = false;
            i = close + 1;
            continue;
        }
        let name = s[i..]
            .iter()
            .enumerate()
            .take_while(|&(k, &c)| {
                !is_blank(c) && !contains("=\"'>", c) && !tag_end_at(s, i + k, false)
            })
            .count();
        let end = i + name.max(1);
        out.add(
            i,
            end,
            if value {
                Token::String
            } else {
                Token::Attribute
            },
        );
        value = false;
        i = end;
    }
    i
}

/// Template tag `t` from `i`, after its open (on this line or the one before): the index after its close, or
/// the end of the line with the tag left open.
fn template_body(
    lang: &Language,
    s: &[u8],
    i: usize,
    t: usize,
    st: &mut State,
    out: &mut Out<'_>,
) -> usize {
    let tag = &lang.markup.templates[t];
    let Some(close_text) = tag.close() else {
        st.mode = Mode::None;
        return i;
    };
    let close = find(s, close_text.as_bytes(), i);
    let stop = close.unwrap_or(s.len());
    if tag.is_comment() {
        out.add(i, stop, Token::Comment);
    } else {
        tokenize_piece(lang.template_language(t), s, i, stop, out, Token::Text);
    }
    let Some(close) = close else {
        st.mode = Mode::TemplateTag(t);
        return s.len();
    };
    let end = close + close_text.len();
    out.add(
        close,
        end,
        if tag.is_comment() {
            Token::Comment
        } else {
            Token::Punctuation
        },
    );
    st.mode = Mode::None;
    end
}

/// Text up to the next tag, comment or template tag, which it opens: the index after the opener.
fn markup_text(lang: &Language, s: &[u8], i: usize, st: &mut State, out: &mut Out<'_>) -> usize {
    let m = &lang.markup;
    let n = s.len();
    let mut j = i;
    while j < n {
        for (t, tag) in m.templates.iter().enumerate() {
            if !starts_at(s, j, tag.open().as_bytes()) {
                continue;
            }
            let inside = j + tag.open().len();
            if let TemplateTag::Identifier { .. } = tag {
                // Razor's @name.name: the code language's words.
                let end = inside
                    + s[inside..]
                        .iter()
                        .take_while(|&&c| is_alnum(c) || c == b'_' || c == b'.')
                        .count();
                if end == inside || !is_alpha(s[inside]) {
                    continue;
                }
                out.add(j, inside, Token::Punctuation);
                tokenize_piece(
                    lang.template_language(t),
                    s,
                    inside,
                    end,
                    out,
                    Token::Variable,
                );
                return end;
            }
            out.add(
                j,
                inside,
                if tag.is_comment() {
                    Token::Comment
                } else {
                    Token::Punctuation
                },
            );
            return template_body(lang, s, inside, t, st, out);
        }
        let ch = s[j];
        if ch == b'&' {
            let limit = n.min(j + 12);
            let k = j
                + 1
                + s[j + 1..limit]
                    .iter()
                    .take_while(|&&c| is_alnum(c) || c == b'#')
                    .count();
            if k > j + 1 && at(s, k, b';') {
                out.add(j, k + 1, Token::Constant);
                j = k;
            }
            j += 1;
            continue;
        }
        if ch != b'<' || j + 1 >= n {
            j += 1;
            continue;
        }
        if starts_at(s, j, b"<!--") {
            out.add(j, j + 4, Token::Comment);
            st.mode = Mode::MarkupComment;
            return j + 4;
        }
        if starts_at(s, j, b"<![CDATA[") {
            out.add(j, j + 9, Token::Punctuation);
            st.mode = Mode::Cdata;
            return j + 9;
        }
        let next = s[j + 1];
        let closing = next == b'/';
        let special = matches!(next, b'!' | b'?');
        if !(closing || special || tag_name_start(next)) {
            j += 1;
            continue;
        }
        let name = j + if closing || special { 2 } else { 1 };
        let name_end = scan_tag_name(s, name);
        out.add(j, name, Token::Punctuation);
        out.add(name, name_end, Token::Tag);
        let tag = &s[name..name_end];
        st.mode = Mode::Tag(if !m.html || closing || special {
            TagKind::Plain
        } else if tag.eq_ignore_ascii_case(b"script") {
            TagKind::Script
        } else if tag.eq_ignore_ascii_case(b"style") {
            TagKind::Style
        } else {
            TagKind::Plain
        });
        return name_end;
    }
    n
}

pub(super) fn tokenize(lang: &Language, s: &[u8], st: &mut State, out: &mut Out<'_>) {
    let n = s.len();
    let mut i = 0;
    while i < n {
        if let Some(region) = &st.region {
            // A <script> / <style> body up to its closing tag.
            let end = if lang.markup.html {
                find_folded(s, &region.end_marker, i)
            } else {
                find(s, &region.end_marker, i)
            };
            if let Some(embedded) = region.language {
                tokenize_embedded(embedded, s, i, end.unwrap_or(n), st, out);
            }
            let Some(end) = end else {
                return;
            };
            // The end of the region: nothing of it is left open.
            *st = State::default();
            i = end;
            continue;
        }
        match st.mode {
            Mode::MarkupComment | Mode::Cdata => {
                let cdata = st.mode == Mode::Cdata;
                let close: &[u8] = if cdata { b"]]>" } else { b"-->" };
                let token = if cdata { Token::String } else { Token::Comment };
                let Some(end) = find(s, close, i) else {
                    out.add(i, n, token);
                    return;
                };
                out.add(i, end, token);
                i = end + close.len();
                out.add(
                    end,
                    i,
                    if cdata {
                        Token::Punctuation
                    } else {
                        Token::Comment
                    },
                );
                st.mode = Mode::None;
            }
            Mode::Tag(_) => i = tag_body(lang, s, i, st, out),
            Mode::AttrString { quote, tag } => {
                let Some(end) = find_byte(s, quote, i) else {
                    out.add(i, n, Token::String);
                    return;
                };
                out.add(i, end + 1, Token::String);
                st.mode = Mode::Tag(tag);
                i = end + 1;
            }
            Mode::TemplateTag(t) if t < lang.markup.templates.len() => {
                i = template_body(lang, s, i, t, st, out);
            }
            _ => {
                st.mode = Mode::None;
                i = markup_text(lang, s, i, st, out);
            }
        }
    }
}
