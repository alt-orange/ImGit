//! Dockerfile: an instruction keyword, then the shell's scanner.

use super::{Mode, Out, State, at, is_alpha, skip_blanks, tokenize_range, trim_end};
use crate::language::Language;
use crate::token::Token;
use std::mem;
use std::sync::LazyLock;

fn shell() -> Option<&'static Language> {
    static LANGUAGE: LazyLock<Option<&'static Language>> =
        LazyLock::new(|| Language::by_name("shell"));
    *LANGUAGE
}

pub(super) fn tokenize(lang: &Language, s: &[u8], st: &mut State, out: &mut Out<'_>) {
    let n = s.len();
    let mut i = skip_blanks(s, 0);
    let continued = mem::take(&mut st.continued);
    if !continued && st.mode == Mode::None {
        if at(s, i, b'#') {
            out.add(i, n, Token::Comment);
            return;
        }
        let end = i + s[i..].iter().take_while(|&&c| is_alpha(c)).count();
        if lang.keywords.contains(&s[i..end]) {
            out.add(i, end, Token::Keyword);
            i = end;
        }
    } else {
        if at(s, i, b'#') {
            out.add(0, n, Token::Comment); // a comment inside a continued RUN
            st.continued = true;
            return;
        }
        i = 0;
    }
    if i < n
        && let Some(shell) = shell()
    {
        tokenize_range(shell, &s[i..], st, &mut out.at(i));
    }
    let last = trim_end(s, 0, n);
    if last > 0 && s[last - 1] == b'\\' {
        st.continued = true;
    }
}
