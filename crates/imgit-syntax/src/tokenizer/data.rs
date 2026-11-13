//! The data languages: JSON, YAML and INI (properties, `.gitconfig`).

use super::code::Code;
use super::{
    Mode, Out, State, at, contains, find, find_any, find_byte, is_alnum, is_alpha, is_blank,
    is_digit, skip_blanks, trim_end,
};
use crate::language::Language;
use crate::token::Token;

// ---- JSON ----

pub(super) fn tokenize_json(lang: &Language, s: &[u8], st: &mut State, out: &mut Out<'_>) {
    let mut x = Code::new(lang, s, st, out);
    let Some(mut i) = x.resume() else {
        return;
    };
    let n = s.len();
    while i < n {
        let ch = s[i];
        if is_blank(ch) {
            i += 1;
            continue;
        }
        if ch == b'"' {
            let mut j = i + 1;
            while j < n && s[j] != b'"' {
                j += if s[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(n);
            let key = at(s, skip_blanks(s, end), b':');
            x.out
                .add(i, end, if key { Token::Attribute } else { Token::String });
            i = end;
            continue;
        }
        if let Some(index) = x.find_delim(i) {
            match x.take_delim(i, index) {
                Some(end) => i = end,
                None => return,
            }
            continue;
        }
        let next_is_digit = s.get(i + 1).copied().is_some_and(is_digit);
        if is_digit(ch) || (ch == b'-' && next_is_digit) {
            let end = x.scan_number(if ch == b'-' { i + 1 } else { i });
            x.out.add(i, end, Token::Number);
            i = end;
            continue;
        }
        if is_alpha(ch) {
            let end = i + s[i..].iter().take_while(|&&c| is_alnum(c)).count();
            let constant = lang.constants.contains(&s[i..end]);
            x.out.add(
                i,
                end,
                if constant {
                    Token::Constant
                } else {
                    Token::Text
                },
            );
            i = end;
            continue;
        }
        let token = if contains("{}[],:", ch) {
            Token::Punctuation
        } else {
            Token::Text
        };
        x.out.add(i, i + 1, token);
        i += 1;
    }
}

// ---- YAML ----

/// Where a ` #` comment starts in `s[i..]`: a `#` after a blank.
fn comment_start(s: &[u8], i: usize) -> Option<usize> {
    (i + 1..s.len()).find(|&k| s[k] == b'#' && is_blank(s[k - 1]))
}

/// A scalar from `i` to the end of the line (or a comment): a number, a constant or a string.
fn yaml_scalar(lang: &Language, s: &[u8], i: usize, out: &mut Out<'_>) {
    let end = comment_start(s, i).unwrap_or(s.len());
    let last = trim_end(s, i, end);
    let value = &s[i..last];
    let number = value
        .iter()
        .all(|&c| is_alnum(c) || matches!(c, b'.' | b'-' | b'+' | b'_'))
        && match *value {
            [first, ..] if is_digit(first) => true,
            [b'-' | b'+' | b'.', second, ..] => is_digit(second),
            _ => false,
        };
    let token = if number {
        Token::Number
    } else if value == b"~" || lang.constants.contains(value) {
        Token::Constant
    } else {
        Token::String
    };
    out.add(i, last, token);
    if end < s.len() {
        out.add(end, s.len(), Token::Comment);
    }
}

/// The colon ending the key that starts at `i`: one followed by a blank or the end of the line, outside
/// the key's quotes and before any comment.
fn yaml_key_end(s: &[u8], i: usize) -> Option<usize> {
    let n = s.len();
    let mut quote = None;
    for k in i..n {
        let ch = s[k];
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
        } else if k == i && matches!(ch, b'"' | b'\'') {
            quote = Some(ch);
        } else if ch == b'#' && k > i && is_blank(s[k - 1]) {
            return None;
        } else if ch == b':' && s.get(k + 1).is_none_or(|&next| is_blank(next)) {
            return Some(k);
        }
    }
    None
}

pub(super) fn tokenize_yaml(lang: &Language, s: &[u8], st: &mut State, out: &mut Out<'_>) {
    let n = s.len();
    let indent = skip_blanks(s, 0);
    // A block scalar's lines are those indented past its key (past the dash of a "- |" item).
    let mut parent = indent;
    let mut i;
    if let Mode::BlockScalar { indent: key } = st.mode {
        if indent == n || indent > usize::from(key) {
            out.add(0, n, Token::String);
            return;
        }
        st.mode = Mode::None;
    }
    if let Mode::Raw { close, .. } = &st.mode {
        let Some(end) = find(s, close, 0) else {
            out.add(0, n, Token::String);
            return;
        };
        out.add(0, end + 1, Token::String);
        st.mode = Mode::None;
        i = end + 1;
    } else {
        i = indent;
        if at(s, i, b'#') {
            out.add(i, n, Token::Comment);
            return;
        }
        if i == 0 && (s.starts_with(b"---") || s.starts_with(b"...") || s.starts_with(b"%")) {
            let directive = s[0] == b'%';
            let end = if directive { n } else { 3 };
            out.add(
                0,
                end,
                if directive {
                    Token::Keyword
                } else {
                    Token::Punctuation
                },
            );
            i = end;
        }
        // List markers and complex keys.
        while i < n && matches!(s[i], b'-' | b'?') && s.get(i + 1).is_none_or(|&c| is_blank(c)) {
            out.add(i, i + 1, Token::Operator);
            i = skip_blanks(s, i + 1);
        }
        // key:
        if i < n
            && !matches!(s[i], b'#' | b'[' | b'{')
            && let Some(colon) = yaml_key_end(s, i)
        {
            parent = i;
            out.add(i, colon, Token::Type);
            out.add(colon, colon + 1, Token::Operator);
            i = colon + 1;
        }
    }
    // The value.
    while i < n {
        let ch = s[i];
        if is_blank(ch) {
            i += 1;
            continue;
        }
        if ch == b'#' {
            out.add(i, n, Token::Comment);
            return;
        }
        if matches!(ch, b'|' | b'>') && (i == 0 || is_blank(s[i - 1])) {
            let end = i
                + 1
                + s[i + 1..]
                    .iter()
                    .take_while(|&&c| is_digit(c) || c == b'-' || c == b'+')
                    .count();
            let rest = skip_blanks(s, end);
            if rest == n || s[rest] == b'#' {
                out.add(i, end, Token::Operator);
                out.add(rest, n, Token::Comment);
                st.mode = Mode::BlockScalar {
                    indent: u8::try_from(parent).unwrap_or(u8::MAX),
                };
                return;
            }
        }
        if matches!(ch, b'&' | b'*' | b'!') {
            let end = i
                + 1
                + s[i + 1..]
                    .iter()
                    .take_while(|&&c| !is_blank(c) && !contains(",[]{}", c))
                    .count();
            out.add(
                i,
                end,
                if ch == b'!' {
                    Token::Tag
                } else {
                    Token::Variable
                },
            );
            i = end;
            continue;
        }
        if matches!(ch, b'"' | b'\'') {
            let mut j = i + 1;
            while j < n && s[j] != ch {
                j += if ch == b'"' && s[j] == b'\\' { 2 } else { 1 };
            }
            if j >= n {
                out.add(i, n, Token::String);
                st.mode = Mode::Raw {
                    token: Token::String,
                    close: vec![ch],
                };
                return;
            }
            out.add(i, j + 1, Token::String);
            i = j + 1;
            continue;
        }
        if contains("[]{},", ch) {
            out.add(i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }
        // A plain scalar, up to a flow delimiter.
        let end = (i..n)
            .find(|&k| contains("[]{},", s[k]) || (s[k] == b'#' && k > 0 && is_blank(s[k - 1])))
            .unwrap_or(n);
        if end == n || s[end] == b'#' {
            yaml_scalar(lang, s, i, out);
            return;
        }
        yaml_scalar(lang, &s[..end], i, out);
        i = end;
    }
}

// ---- INI, properties, .gitconfig ----

pub(super) fn tokenize_ini(lang: &Language, s: &[u8], out: &mut Out<'_>) {
    let n = s.len();
    let mut i = skip_blanks(s, 0);
    if i == n {
        return;
    }
    if matches!(s[i], b';' | b'#') {
        out.add(i, n, Token::Comment);
        return;
    }
    if s[i] == b'[' {
        let end = find_byte(s, b']', i).map_or(n, |close| close + 1);
        out.add(i, end, Token::Tag);
        i = end;
    } else {
        let Some(eq) = find_any(s, b"=:", i) else {
            out.add(i, n, Token::Attribute);
            return;
        };
        out.add(i, eq, Token::Attribute);
        out.add(eq, eq + 1, Token::Operator);
        i = skip_blanks(s, eq + 1);
        if i < n && matches!(s[i], b'"' | b'\'') {
            let end = find_byte(s, s[i], i + 1).map_or(n, |close| close + 1);
            out.add(i, end, Token::String);
            i = end;
        } else if i < n {
            let end = (i..n)
                .find(|&k| matches!(s[k], b';' | b'#') && is_blank(s[k - 1]))
                .unwrap_or(n);
            let last = trim_end(s, i, end);
            let value = &s[i..last];
            let token = if !value.is_empty() && value.iter().all(|&c| is_digit(c) || c == b'.') {
                Token::Number
            } else if lang.constants.contains(value) {
                Token::Constant
            } else {
                Token::Text
            };
            out.add(i, last, token);
            i = end;
        }
    }
    i = skip_blanks(s, i);
    if i < n && matches!(s[i], b';' | b'#') {
        out.add(i, n, Token::Comment);
    }
}
