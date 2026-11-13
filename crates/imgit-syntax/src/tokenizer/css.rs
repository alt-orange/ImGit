//! CSS, SCSS and Less.

use super::code::Code;
use super::{
    Out, State, at, contains, find_any, find_byte, is_alnum, is_alpha, is_blank, is_digit, is_utf8,
    skip_blanks,
};
use crate::language::Language;
use crate::token::Token;

/// What the scanner is reading.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    /// The start of a statement: not yet known to be a selector or a declaration.
    Statement,
    Selector,
    Property,
    Value,
    AtRule,
}

fn ident_start(s: &[u8], k: usize) -> bool {
    s.get(k).is_some_and(|&c| {
        is_alpha(c)
            || c == b'_'
            || is_utf8(c)
            || (c == b'-'
                && s.get(k + 1)
                    .is_some_and(|&next| is_alpha(next) || next == b'-'))
    })
}

/// The end of the identifier characters from `k`.
fn ident(s: &[u8], k: usize) -> usize {
    run(s, k, |c| {
        is_alnum(c) || c == b'_' || c == b'-' || is_utf8(c)
    })
}

/// The end of the run of bytes matching `matches` from `k`.
fn run(s: &[u8], k: usize, matches: impl Fn(u8) -> bool) -> usize {
    let rest = s.get(k..).unwrap_or_default();
    k + rest.iter().take_while(|&&c| matches(c)).count()
}

pub(super) fn tokenize(lang: &Language, s: &[u8], st: &mut State, out: &mut Out<'_>) {
    let mut x = Code::new(lang, s, st, out);
    let Some(mut i) = x.resume() else {
        return;
    };
    let sigils = lang.code.sigils;
    let less = contains(sigils, b'@');
    let mut context = Context::Statement;
    let n = s.len();
    while i < n {
        let ch = s[i];
        if is_blank(ch) {
            i += 1;
            continue;
        }
        if let Some(index) = x.find_delim(i) {
            match x.take_delim(i, index) {
                Some(end) => i = end,
                None => return,
            }
            continue;
        }
        if context == Context::Statement {
            if ch == b'@' && ident_start(s, i + 1) {
                let end = ident(s, i + 1);
                let rule = !less || lang.keywords.contains(&s[i + 1..end]);
                x.out.add(
                    i,
                    end,
                    if rule {
                        Token::Keyword
                    } else {
                        Token::Variable
                    },
                );
                context = if rule {
                    Context::AtRule
                } else {
                    Context::Property
                };
                i = end;
                continue;
            }
            context = match find_any(s, b"{;}", i).map(|stop| s[stop]) {
                Some(b'{') => Context::Selector,
                Some(b';') => Context::Property,
                _ if x.st.braces > 0 => Context::Property,
                _ => Context::Selector,
            };
        }
        if matches!(ch, b'{' | b'}' | b';') {
            x.out.add(i, i + 1, Token::Punctuation);
            if ch == b'{' {
                x.st.braces = x.st.braces.saturating_add(1);
            } else if ch == b'}' {
                x.st.braces = x.st.braces.saturating_sub(1);
            }
            context = Context::Statement;
            i += 1;
            continue;
        }
        if ch == b'#' && at(s, i + 1, b'{') {
            let end = find_byte(s, b'}', i).map_or(n, |close| close + 1);
            x.out.add(i, end, Token::Variable); // #{$interpolation}
            i = end;
            continue;
        }
        if contains(sigils, ch) && ident_start(s, i + 1) {
            let end = ident(s, i + 1);
            x.out.add(i, end, Token::Variable);
            i = end;
            continue;
        }
        let next_is_digit = s.get(i + 1).copied().is_some_and(is_digit);
        if is_digit(ch)
            || (matches!(ch, b'.' | b'-' | b'+') && next_is_digit && context != Context::Selector)
        {
            let end = run(s, i + 1, |c| is_alnum(c) || c == b'.' || c == b'%');
            x.out.add(i, end, Token::Number);
            i = end;
            continue;
        }
        i = match context {
            Context::Selector => selector(s, i, x.out),
            Context::Property => {
                if ident_start(s, i) || ch == b'*' {
                    let end = ident(s, i + 1);
                    x.out.add(i, end, Token::Attribute);
                    end
                } else {
                    x.out.add(i, i + 1, Token::Punctuation);
                    if ch == b':' {
                        context = Context::Value;
                    }
                    i + 1
                }
            }
            Context::Value | Context::AtRule => value(s, i, context == Context::AtRule, x.out),
            Context::Statement => i + 1,
        };
    }
}

/// One piece of a selector at `i`: the index after it.
fn selector(s: &[u8], i: usize, out: &mut Out<'_>) -> usize {
    let n = s.len();
    let ch = s[i];
    let k = run(s, i, |c| matches!(c, b'.' | b'#' | b':' | b'%'));
    if ident_start(s, k) {
        let end = ident(s, k);
        out.add(i, end, Token::Tag);
        return end;
    }
    match ch {
        b'*' => out.add(i, i + 1, Token::Tag),
        b'[' => {
            // [attribute=value]
            let end = find_byte(s, b']', i).unwrap_or(n);
            let name_end = ident(s, i + 1).min(end);
            let after = (end + 1).min(n);
            out.add(i, i + 1, Token::Punctuation);
            out.add(i + 1, name_end, Token::Attribute);
            out.add(name_end, end, Token::String);
            out.add(end, after, Token::Punctuation);
            return after;
        }
        _ if contains(",()", ch) => out.add(i, i + 1, Token::Punctuation),
        _ => out.add(i, i + 1, Token::Operator),
    }
    i + 1
}

/// One piece of a declaration's value, or of an at-rule's prelude, at `i`: the index after it.
fn value(s: &[u8], i: usize, at_rule: bool, out: &mut Out<'_>) -> usize {
    let n = s.len();
    let ch = s[i];
    let next = s.get(i + 1).copied();
    if ch == b'#' && next.is_some_and(is_alnum) {
        let end = run(s, i + 1, is_alnum);
        out.add(i, end, Token::Number); // #rrggbb
        return end;
    }
    if ch == b'!' && next.is_some_and(is_alpha) {
        let end = ident(s, i + 1);
        out.add(i, end, Token::Keyword); // !important
        return end;
    }
    if !ident_start(s, i) {
        let token = if contains(",()[]:", ch) {
            Token::Punctuation
        } else {
            Token::Operator
        };
        out.add(i, i + 1, token);
        return i + 1;
    }
    let end = ident(s, i);
    let word = &s[i..end];
    if at(s, end, b'(') {
        out.add(i, end, Token::Function);
        if word == b"url" {
            let stop = find_byte(s, b')', end).unwrap_or(n);
            out.add(end, end + 1, Token::Punctuation);
            out.add(end + 1, stop, Token::String);
            return stop;
        }
    } else if at_rule {
        let media = matches!(word, b"and" | b"not" | b"only" | b"or");
        let token = if media {
            Token::Keyword
        } else if at(s, skip_blanks(s, end), b':') {
            Token::Attribute
        } else {
            Token::Text
        };
        out.add(i, end, token);
    } else {
        out.add(i, end, Token::String);
    }
    end
}
