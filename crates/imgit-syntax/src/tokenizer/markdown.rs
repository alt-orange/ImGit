//! Markdown: block structure, inline markup and fenced code blocks in their own languages.

use super::{
    Mode, Out, Region, State, at, blank, contains, find, find_byte, is_alpha, is_blank, is_digit,
    skip_blanks, starts_at, tokenize_embedded, tokenize_piece,
};
use crate::language::Language;
use crate::token::Token;
use std::sync::LazyLock;

fn html() -> Option<&'static Language> {
    static LANGUAGE: LazyLock<Option<&'static Language>> =
        LazyLock::new(|| Language::by_name("html"));
    *LANGUAGE
}

/// The end of the run of `byte` from `i`.
fn run_end(s: &[u8], i: usize, byte: u8) -> usize {
    i + s[i..].iter().take_while(|&&c| c == byte).count()
}

/// Inline markup from `i`: `` `code` ``, `[links](url)`, `<html>`, escapes.
fn inline(s: &[u8], mut i: usize, out: &mut Out<'_>) {
    let n = s.len();
    while i < n {
        let ch = s[i];
        if ch == b'\\' {
            i += 2;
            continue;
        }
        if ch == b'`' {
            let run = run_end(s, i, b'`');
            let end = find(s, &s[i..run], run).map_or(n, |close| close + (run - i));
            out.add(i, end, Token::Variable);
            i = end;
            continue;
        }
        if ch == b'[' || (ch == b'!' && at(s, i + 1, b'[')) {
            let open = if ch == b'!' { i + 1 } else { i };
            if let Some(close) = find_byte(s, b']', open)
                && at(s, close + 1, b'(')
            {
                let end = find_byte(s, b')', close).map_or(n, |paren| paren + 1);
                out.add(i, close + 1, Token::String);
                out.add(close + 1, end, Token::Attribute);
                i = end;
                continue;
            }
        }
        if ch == b'<'
            && s.get(i + 1).is_some_and(|&c| is_alpha(c) || c == b'/')
            && let Some(close) = find_byte(s, b'>', i)
        {
            tokenize_piece(html(), s, i, close + 1, out, Token::Text);
            i = close + 1;
            continue;
        }
        if ch == b'|' {
            out.add(i, i + 1, Token::Punctuation);
        }
        i += 1;
    }
}

pub(super) fn tokenize(s: &[u8], st: &mut State, out: &mut Out<'_>) {
    let n = s.len();
    let first = skip_blanks(s, 0);
    if let Some(region) = &st.region {
        // Inside a fenced code block: its closing fence (a run of its character at least as long, alone on
        // the line), else the block's language.
        let fence = &region.end_marker;
        let run = fence.first().map_or(first, |&mark| run_end(s, first, mark));
        if run - first >= fence.len() && blank(&s[run..]) {
            out.add(first, n, Token::String);
            *st = State::default();
        } else if let Some(embedded) = region.language {
            tokenize_embedded(embedded, s, 0, n, st, out);
        } else {
            out.add(0, n, Token::Variable);
        }
        return;
    }
    if st.mode == Mode::MarkupComment {
        let Some(end) = find(s, b"-->", 0) else {
            out.add(0, n, Token::Comment);
            return;
        };
        out.add(0, end + 3, Token::Comment);
        st.mode = Mode::None;
        inline(s, end + 3, out);
        return;
    }
    if first == n {
        return;
    }
    if first <= 3 && (starts_at(s, first, b"```") || starts_at(s, first, b"~~~")) {
        let run = run_end(s, first, s[first]);
        let info = skip_blanks(s, run);
        let info_end = info
            + s[info..]
                .iter()
                .take_while(|&&c| !is_blank(c) && c != b'{' && c != b'`')
                .count();
        out.add(first, n, Token::String);
        st.region = Some(Region {
            language: std::str::from_utf8(&s[info..info_end])
                .ok()
                .and_then(Language::by_name),
            end_marker: s[first..run].to_vec(),
        });
        return;
    }
    if starts_at(s, first, b"<!--") {
        let Some(end) = find(s, b"-->", first + 4) else {
            out.add(first, n, Token::Comment);
            st.mode = Mode::MarkupComment;
            return;
        };
        out.add(first, end + 3, Token::Comment);
        inline(s, end + 3, out);
        return;
    }
    let mut i = first;
    if s[i] == b'#' {
        let level = run_end(s, i, b'#');
        if level - i <= 6 && s.get(level).is_none_or(|&c| is_blank(c)) {
            out.add(i, n, Token::Keyword);
            return;
        }
    }
    // Setext underlines and rules: ===, ---, ***, ___
    let mark = s[i];
    if contains("=-*_", mark) && n - i >= 3 && s[i..].iter().all(|&c| c == mark || is_blank(c)) {
        out.add(i, n, Token::Keyword);
        return;
    }
    while at(s, i, b'>') {
        out.add(i, i + 1, Token::Comment);
        i = skip_blanks(s, i + 1);
    }
    // List markers: - * + 1. 1)
    let marker = if s.get(i).is_some_and(|&c| contains("-*+", c)) {
        i + 1
    } else {
        let digits = i + s[i..].iter().take_while(|&&c| is_digit(c)).count();
        if digits > i && s.get(digits).is_some_and(|&c| matches!(c, b'.' | b')')) {
            digits + 1
        } else {
            i
        }
    };
    if marker > i && s.get(marker).is_none_or(|&c| is_blank(c)) {
        out.add(i, marker, Token::Keyword);
        i = marker;
    }
    inline(s, i, out);
}
