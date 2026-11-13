//! The code scanner: comments and strings from the language's tables, words, numbers and operators, plus
//! the hooks a tier-1 language adds.

use super::{
    Mode, Out, State, at, contains, find, find_byte, is_alnum, is_alpha, is_blank, is_digit,
    is_utf8, skip_blanks, starts_at, starts_at_folded,
};
use crate::language::{CodeSpec, Delim, Hooks, Language};
use crate::token::Token;
use std::mem;

/// Keywords whose next word names a function.
const DEF_WORDS: &[&str] = &[
    "def",
    "defp",
    "fn",
    "func",
    "function",
    "fun",
    "sub",
    "proc",
    "procedure",
    "macro",
    "method",
    "defn",
    "mixin",
];

/// Keywords whose next word names a type.
const TYPE_DEF_WORDS: &[&str] = &[
    "class",
    "struct",
    "interface",
    "enum",
    "trait",
    "type",
    "union",
    "record",
    "object",
    "new",
    "extends",
    "implements",
    "impl",
    "namespace",
    "module",
    "contract",
];

fn one_of(word: &[u8], words: &[&str], fold: bool) -> bool {
    words.iter().any(|candidate| {
        if fold {
            word.eq_ignore_ascii_case(candidate.as_bytes())
        } else {
            word == candidate.as_bytes()
        }
    })
}

fn lisp_symbol_char(c: u8) -> bool {
    !is_blank(c) && !contains("()[]{}\";,'`", c)
}

/// One line being scanned with a language's tables. The scanners of CSS and JSON borrow its delimiters and
/// numbers.
pub(super) struct Code<'a, 'o> {
    lang: &'a Language,
    c: &'a CodeSpec,
    s: &'a [u8],
    pub(super) st: &'a mut State,
    pub(super) out: &'a mut Out<'o>,
    /// The line's first non-blank.
    first: usize,
    // The last significant token, for regex literals and the names after def / class.
    prev: Token,
    prev_char: Option<u8>,
    /// The keyword just before, when the last token was one.
    prev_word: Option<&'a [u8]>,
    /// A heredoc opened on this line, starting on the next: its closing word and whether that may be indented.
    heredoc: Option<(Vec<u8>, bool)>,
}

impl<'a, 'o> Code<'a, 'o> {
    pub(super) fn new(
        lang: &'a Language,
        s: &'a [u8],
        st: &'a mut State,
        out: &'a mut Out<'o>,
    ) -> Self {
        Self {
            lang,
            c: &lang.code,
            s,
            st,
            out,
            first: skip_blanks(s, 0),
            prev: Token::Text,
            prev_char: None,
            prev_word: None,
            heredoc: None,
        }
    }

    fn emit(&mut self, begin: usize, end: usize, token: Token) {
        self.out.add(begin, end, token);
        if end > begin {
            self.prev = token;
            self.prev_char = self.s.get(end - 1).copied();
            self.prev_word = None;
        }
    }

    fn ident_start(&self, c: u8) -> bool {
        is_alpha(c) || c == b'_' || is_utf8(c) || contains(self.c.ident_start, c)
    }

    fn ident_char(&self, c: u8) -> bool {
        is_alnum(c) || c == b'_' || is_utf8(c) || contains(self.c.ident_chars, c)
    }

    /// Whether an identifier starts at `i`.
    fn ident_starts_at(&self, i: usize) -> bool {
        self.s.get(i).is_some_and(|&c| self.ident_start(c))
    }

    /// The end of the word starting at `i`, an identifier start (which may be a start-only character:
    /// `` `define ``).
    fn scan_ident(&self, i: usize) -> usize {
        let rest = self.s.get(i + 1..).unwrap_or_default();
        let length = rest.iter().take_while(|&&c| self.ident_char(c)).count();
        i + 1 + length
    }

    /// The close of `d` from `j`, past nested opens and escapes: the index after it, `None` when the line
    /// ends first.
    fn find_close(&self, mut j: usize, d: &Delim, depth: &mut u8) -> Option<usize> {
        let s = self.s;
        let close = d.close?.as_bytes();
        let open = d.open.as_bytes();
        let raw = d.raw || self.c.escape.is_none();
        let doubled = raw && matches!(close, b"'" | b"\"");
        if d.closes_at_start {
            let k = skip_blanks(s, 0);
            return (k >= j && starts_at(s, k, close)).then(|| k + close.len());
        }
        while j < s.len() {
            if !raw && Some(s[j]) == self.c.escape {
                j += 2;
                continue;
            }
            if d.nested && starts_at(s, j, open) {
                *depth = depth.saturating_add(1);
                j += open.len();
                continue;
            }
            if starts_at(s, j, close) {
                let end = j + close.len();
                if doubled && at(s, end, close[0]) {
                    j = end + 1;
                } else if *depth > 0 {
                    *depth -= 1;
                    j = end;
                } else {
                    return Some(end);
                }
                continue;
            }
            j += 1;
        }
        None
    }

    /// The delimiter that opens at `i`: its index in the language's table.
    pub(super) fn find_delim(&self, i: usize) -> Option<usize> {
        let s = self.s;
        self.c.delims.iter().position(|d| {
            let open = d.open.as_bytes();
            if d.opens_at_start && i != self.first {
                return false;
            }
            if d.word {
                let opens = if self.c.fold_case {
                    starts_at_folded(s, i, open)
                } else {
                    starts_at(s, i, open)
                };
                return opens && !s.get(i + open.len()).is_some_and(|&c| self.ident_char(c));
            }
            if !starts_at(s, i, open) {
                return false;
            }
            if !d.char_literal {
                return true;
            }
            // One character (a whole UTF-8 sequence) or one escape, then the close.
            let close = d.close.unwrap_or_default().as_bytes();
            let j = i + open.len();
            let Some(&lead) = s.get(j) else {
                return false;
            };
            if Some(lead) == self.c.escape {
                return find(s, close, j + 2).is_some_and(|end| end <= j + 12);
            }
            let length = match lead {
                0xF0.. => 4,
                0xE0.. => 3,
                0xC0.. => 2,
                _ => 1,
            };
            starts_at(s, j + length, close)
        })
    }

    /// Emits delimiter `index` opening at `i`: the index after its close, or `None` when it runs to the end
    /// of the line (and on to the next when it may).
    pub(super) fn take_delim(&mut self, i: usize, index: usize) -> Option<usize> {
        let d = self.c.delims[index];
        let n = self.s.len();
        if d.close.is_none() {
            self.emit(i, n, d.token);
            return None;
        }
        let mut depth = 0;
        let end = if d.closes_at_start {
            None
        } else {
            self.find_close(i + d.open.len(), &d, &mut depth)
        };
        if let Some(end) = end {
            self.emit(i, end, d.token);
            return Some(end);
        }
        self.emit(i, n, d.token);
        let continued = d.multiline
            || (!d.raw && self.c.escape.is_some() && self.s.last().copied() == self.c.escape);
        if continued {
            self.st.mode = Mode::Delim {
                index,
                nesting: depth,
            };
        }
        None
    }

    /// A raw string or comment opened at `begin`, from `i` (its text after the open) to `close`.
    fn take_raw(&mut self, begin: usize, i: usize, close: Vec<u8>, token: Token) -> Option<usize> {
        if let Some(end) = find(self.s, &close, i) {
            let end = end + close.len();
            self.emit(begin, end, token);
            return Some(end);
        }
        self.emit(begin, self.s.len(), token);
        self.st.mode = Mode::Raw { token, close };
        None
    }

    /// A template literal's text from `i`: the index where code follows (after its end or a `${`), `None`
    /// when the line ends inside it.
    fn scan_template(&mut self, i: usize) -> Option<usize> {
        let s = self.s;
        let mut j = i;
        while j < s.len() {
            match s[j] {
                b'\\' => {
                    j += 2;
                    continue;
                }
                b'`' => {
                    self.emit(i, j + 1, Token::String);
                    self.st.mode = Mode::None;
                    return Some(j + 1);
                }
                b'$' if at(s, j + 1, b'{') => {
                    self.emit(i, j, Token::String);
                    self.emit(j, j + 2, Token::Punctuation);
                    self.st.mode = Mode::None;
                    self.st.templates.push(1);
                    return Some(j + 2);
                }
                _ => j += 1,
            }
        }
        self.emit(i, s.len(), Token::String);
        None
    }

    /// Carries on what the line before left open: the index its code starts at, `None` when that takes the
    /// whole line.
    pub(super) fn resume(&mut self) -> Option<usize> {
        let s = self.s;
        match mem::take(&mut self.st.mode) {
            Mode::Delim { index, mut nesting } => {
                let Some(&d) = self.c.delims.get(index) else {
                    *self.st = State::default();
                    return Some(0);
                };
                if let Some(end) = self.find_close(0, &d, &mut nesting) {
                    self.emit(0, end, d.token);
                    return Some(end);
                }
                self.emit(0, s.len(), d.token);
                let continued =
                    d.multiline || (self.c.escape.is_some() && s.last().copied() == self.c.escape);
                if continued {
                    self.st.mode = Mode::Delim { index, nesting };
                }
                None
            }
            Mode::Raw { token, close } => {
                if let Some(end) = find(s, &close, 0) {
                    let end = end + close.len();
                    self.emit(0, end, token);
                    return Some(end);
                }
                self.emit(0, s.len(), token);
                self.st.mode = Mode::Raw { token, close };
                None
            }
            Mode::Heredoc { close, indented } => {
                let k = if indented { self.first } else { 0 };
                let end = k + close.len();
                if starts_at(s, k, &close) && !s.get(end).is_some_and(|&c| self.ident_char(c)) {
                    self.emit(0, end, Token::String);
                    return Some(end);
                }
                self.emit(0, s.len(), Token::String);
                self.st.mode = Mode::Heredoc { close, indented };
                None
            }
            Mode::TemplateText => {
                self.st.mode = Mode::TemplateText;
                self.scan_template(0)
            }
            other => {
                self.st.mode = other;
                Some(0)
            }
        }
    }

    /// `<<EOF`, `<<-EOF`, `<<~'EOF'`, `<<<"EOF"` at `i`: the index after the opener, `None` when there is
    /// none.
    fn open_heredoc(&mut self, i: usize) -> Option<usize> {
        let s = self.s;
        let php = self.c.hooks.contains(Hooks::PHP_TAGS);
        let spaced = self.c.hooks.contains(Hooks::HEREDOC_SPACED);
        // Not the inside of a longer run (the shell's <<< here-string), nor a shift: $(( 1 << n )).
        if (i > 0 && s[i - 1] == b'<') || (!php && at(s, i + 2, b'<')) {
            return None;
        }
        if spaced {
            let mut arithmetic = 0i32;
            let mut k = 0;
            while k + 1 < i {
                if s[k] == s[k + 1] && matches!(s[k], b'(' | b')') {
                    arithmetic += if s[k] == b'(' { 1 } else { -1 };
                    k += 1;
                }
                k += 1;
            }
            if arithmetic > 0 {
                return None;
            }
        }
        let mut j = i + 2;
        let mut indented = false;
        if php {
            if !at(s, j, b'<') {
                return None;
            }
            j += 1;
            indented = true;
        }
        if at(s, j, b'-') || at(s, j, b'~') {
            indented = true;
            j += 1;
        }
        if spaced {
            j = skip_blanks(s, j);
        }
        let quote = s.get(j).copied().filter(|c| matches!(c, b'\'' | b'"'));
        if quote.is_some() {
            j += 1;
        }
        if !s.get(j).is_some_and(|&c| is_alpha(c) || c == b'_') {
            return None;
        }
        let mut k = j + s[j..]
            .iter()
            .take_while(|&&c| is_alnum(c) || c == b'_')
            .count();
        let word = s[j..k].to_vec();
        if let Some(quote) = quote {
            if !at(s, k, quote) {
                return None;
            }
            k += 1;
        }
        self.emit(i, k, Token::String);
        self.heredoc = Some((word, indented));
        Some(k)
    }

    /// The end of the number whose digits start at `i`.
    pub(super) fn scan_number(&self, i: usize) -> usize {
        let s = self.s;
        let quotes = self.c.hooks.contains(Hooks::DIGIT_QUOTES);
        let mut j = i;
        let radix = s[j] == b'0' && s.get(j + 1).is_some_and(|&c| contains("xXbBoO", c));
        if radix {
            j += 2;
        }
        while j < s.len() {
            let ch = s[j];
            let next = s.get(j + 1).copied();
            if is_alnum(ch) || ch == b'_' {
                let exponent =
                    !radix && matches!(ch, b'e' | b'E') && matches!(next, Some(b'+' | b'-'));
                j += if exponent { 2 } else { 1 };
            } else if (ch == b'.' && !radix && next.is_some_and(is_digit))
                || (quotes && ch == b'\'' && next.is_some_and(is_alnum))
            {
                j += 1;
            } else {
                break;
            }
        }
        j
    }

    fn regex_allowed(&self) -> bool {
        match self.prev {
            Token::Text => self.prev_char.is_none(),
            Token::Operator | Token::Keyword => true,
            Token::Punctuation => !self.prev_char.is_some_and(|c| contains(")]}", c)),
            _ => false,
        }
    }

    /// `/regex/flags` from `i`: the index after it, `None` when the line has no closing slash.
    fn scan_regex(&self, i: usize) -> Option<usize> {
        let s = self.s;
        let mut in_class = false;
        let mut j = i + 1;
        while j < s.len() {
            match s[j] {
                b'\\' => j += 1,
                b'[' => in_class = true,
                b']' => in_class = false,
                b'/' if !in_class => {
                    if j == i + 1 {
                        return None; // "//" is a comment, never an empty regex
                    }
                    let flags = s[j + 1..].iter().take_while(|&&c| is_alpha(c)).count();
                    return Some(j + 1 + flags);
                }
                _ => {}
            }
            j += 1;
        }
        None
    }

    /// A sigil variable at `i` (`$name`, `${name}`, `@ivar`, `$1`): the index after it.
    fn scan_variable(&self, i: usize) -> Option<usize> {
        let s = self.s;
        let sigil = s[i];
        let mut j = i + 1;
        if j >= s.len() {
            return None;
        }
        if s[j] == sigil && sigil == b'@' {
            j += 1; // @@class_variable
        }
        let next = *s.get(j)?;
        if next == b'{' {
            return Some(find_byte(s, b'}', j).map_or(s.len(), |close| close + 1));
        }
        if sigil == b'$' && contains("#?@*!$-0123456789_", next) && !self.ident_start(next) {
            return Some(j + 1);
        }
        if !self.ident_start(next) {
            return None;
        }
        j = self.scan_ident(j);
        // $env:PATH, $script:name
        if at(s, j, b':') && !at(s, j + 1, b':') && self.ident_starts_at(j + 1) {
            j = self.scan_ident(j + 1);
        }
        Some(j)
    }

    /// Batch variables at `i`: `%name%`, `%%i`, `%~dp0`, `%1`, `!name!`.
    fn scan_percent_variable(&self, i: usize) -> Option<usize> {
        let s = self.s;
        let ch = s[i];
        let mut j = i + 1;
        let next = *s.get(j)?;
        if ch == b'%' && next == b'%' {
            return self.ident_starts_at(j + 1).then_some(j + 2);
        }
        if ch == b'%' && (is_digit(next) || next == b'~' || next == b'*') {
            j += s[j..]
                .iter()
                .take_while(|&&c| is_alpha(c) || c == b'~')
                .count();
            let argument = s.get(j).is_some_and(|&c| is_digit(c) || c == b'*');
            return Some(if argument { j + 1 } else { j });
        }
        let close = find_byte(s, ch, j).filter(|&close| close != j)?;
        (!s[j..close].iter().copied().any(is_blank)).then_some(close + 1)
    }

    fn classify(&self, begin: usize, end: usize) -> Token {
        let s = self.s;
        let word = &s[begin..end];
        let lang = self.lang;
        if lang.keywords.contains(word) {
            return Token::Keyword;
        }
        if lang.types.contains(word) {
            return Token::Type;
        }
        if lang.constants.contains(word) {
            return Token::Constant;
        }
        if lang.builtins.contains(word) {
            return Token::Function;
        }
        if self.prev == Token::Keyword
            && let Some(prev_word) = self.prev_word
        {
            if one_of(prev_word, DEF_WORDS, self.c.fold_case) {
                return Token::Function;
            }
            if one_of(prev_word, TYPE_DEF_WORDS, self.c.fold_case) {
                return Token::Type;
            }
        }
        if self.c.line_head_tag && begin == self.first {
            return Token::Tag;
        }
        if at(s, skip_blanks(s, end), b'(') {
            return Token::Function;
        }
        // PowerShell's Verb-Noun cmdlets.
        if let Some(dash) = word.iter().position(|&c| c == b'-')
            && dash > 0
            && word[0].is_ascii_uppercase()
            && word.get(dash + 1).is_some_and(u8::is_ascii_uppercase)
        {
            return Token::Function;
        }
        Token::Text
    }

    /// A string prefix (`r".."`, `b'..'`, `f".."`, `u8".."`, `R"(..)"`, `r#".."#`) from `i` to `end`, with the
    /// string it prefixes: where the scan goes on.
    fn take_prefixed_string(&mut self, i: usize, end: usize) -> Option<Flow> {
        let s = self.s;
        let n = s.len();
        let word = &s[i..end];
        let hooks = self.c.hooks;
        let last = *word.last()?;
        let next = *s.get(end)?;
        if self.c.string_prefixes.is_empty()
            || word.len() > 3
            || !word.iter().all(|&p| contains(self.c.string_prefixes, p))
        {
            return None;
        }
        if hooks.contains(Hooks::CPP_RAW)
            && last == b'R'
            && next == b'"'
            && let Some(paren) = find_byte(s, b'(', end + 1)
            && paren - end - 1 <= 16
        {
            let close = [b")", &s[end + 1..paren], b"\""].concat();
            return Some(Flow::after(
                self.take_raw(i, paren + 1, close, Token::String),
                Flow::Stop,
            ));
        }
        if hooks.contains(Hooks::RUST) && last == b'r' && matches!(next, b'"' | b'#') {
            let hashes = s[end..].iter().take_while(|&&c| c == b'#').count();
            let j = end + hashes;
            if j < n && s[j] == b'"' {
                let mut close = vec![b'"'];
                close.resize(1 + hashes, b'#');
                return Some(Flow::after(
                    self.take_raw(i, j + 1, close, Token::String),
                    Flow::Stop,
                ));
            }
        }
        let index = self
            .find_delim(end)
            .filter(|&index| self.c.delims[index].token == Token::String)?;
        self.emit(i, end, Token::String);
        Some(Flow::after(self.take_delim(end, index), Flow::EndLine))
    }

    /// Lua's long brackets at `i`: `[[ ]]`, `[==[ ]==]`, `--[[ ]]`.
    fn take_lua_long(&mut self, i: usize) -> Option<Flow> {
        let s = self.s;
        let comment = s[i] == b'-';
        let start = i + if comment { 3 } else { 1 };
        let level = s.get(start..)?.iter().take_while(|&&c| c == b'=').count();
        let j = start + level;
        if !at(s, j, b'[') {
            return None;
        }
        let mut close = vec![b']'];
        close.resize(1 + level, b'=');
        close.push(b']');
        let token = if comment {
            Token::Comment
        } else {
            Token::String
        };
        Some(Flow::after(
            self.take_raw(i, j + 1, close, token),
            Flow::Stop,
        ))
    }

    /// A `#` directive at `i`, the line's first non-blank: the index after it and its `<header>`.
    fn take_directive(&mut self, i: usize) -> usize {
        let s = self.s;
        let j = skip_blanks(s, i + 1);
        let word_end = if self.ident_starts_at(j) {
            self.scan_ident(j)
        } else {
            j
        };
        self.emit(i, word_end, Token::Keyword);
        if matches!(&s[j..word_end], b"include" | b"import") {
            let j = skip_blanks(s, word_end);
            if at(s, j, b'<') {
                let end = find_byte(s, b'>', j).map_or(s.len(), |close| close + 1);
                self.emit(j, end, Token::String);
                return end;
            }
        }
        word_end
    }

    /// A lisp symbol at `i`: the index after it.
    fn take_lisp_symbol(&mut self, i: usize) -> usize {
        let s = self.s;
        let end = i + s[i..].iter().take_while(|&&c| lisp_symbol_char(c)).count();
        let word = &s[i..end];
        let lang = self.lang;
        let token = if word[0] == b':' && word.len() > 1 {
            Token::Constant
        } else if lang.keywords.contains(word) {
            Token::Keyword
        } else if lang.constants.contains(word) {
            Token::Constant
        } else if lang.types.contains(word) {
            Token::Type
        } else if lang.builtins.contains(word) || self.prev_char == Some(b'(') {
            Token::Function
        } else {
            Token::Text
        };
        self.emit(i, end, token);
        end
    }
}

/// Where the scan of a line goes on after a construct that may run past its end.
enum Flow {
    /// At this index.
    At(usize),
    /// Nowhere: the line is done.
    EndLine,
    /// Nowhere, and the line's state is settled: a heredoc opened on it does not start.
    Stop,
}

impl Flow {
    /// At `next`, or `otherwise` when the construct took the rest of the line.
    fn after(next: Option<usize>, otherwise: Self) -> Self {
        next.map_or(otherwise, Self::At)
    }
}

pub(super) fn tokenize(lang: &Language, s: &[u8], st: &mut State, out: &mut Out<'_>) {
    let mut x = Code::new(lang, s, st, out);
    let Some(start) = x.resume() else {
        return;
    };
    if !scan(&mut x, start) {
        return;
    }
    if x.st.mode == Mode::None
        && let Some((close, indented)) = x.heredoc.take()
    {
        x.st.mode = Mode::Heredoc { close, indented };
    }
}

/// Scans the line's code from `i`: false when a construct that settles the line's state took its rest.
fn scan<'a>(x: &mut Code<'a, '_>, mut i: usize) -> bool {
    let s = x.s;
    let c = x.c;
    let hooks = c.hooks;
    let n = s.len();
    while i < n {
        let ch = s[i];
        if is_blank(ch) {
            i += 1;
            continue;
        }
        // The braces of the innermost template literal's ${ }: its last } goes back to its text.
        if matches!(ch, b'{' | b'}')
            && let Some(&braces) = x.st.templates.last()
        {
            x.emit(i, i + 1, Token::Punctuation);
            i += 1;
            if ch == b'{' || braces > 1 {
                let braces = if ch == b'{' {
                    braces.saturating_add(1)
                } else {
                    braces - 1
                };
                if let Some(last) = x.st.templates.last_mut() {
                    *last = braces;
                }
                continue;
            }
            x.st.templates.pop();
            x.st.mode = Mode::TemplateText;
            match x.scan_template(i) {
                Some(next) => i = next,
                None => return false,
            }
            continue;
        }
        // Hooks whose openers the tables would take for something else.
        if hooks.contains(Hooks::LUA_LONG)
            && (ch == b'[' || starts_at(s, i, b"--["))
            && let Some(flow) = x.take_lua_long(i)
        {
            match flow {
                Flow::At(next) => i = next,
                Flow::EndLine => break,
                Flow::Stop => return false,
            }
            continue;
        }
        if hooks.contains(Hooks::PHP_TAGS) && (starts_at(s, i, b"<?") || starts_at(s, i, b"?>")) {
            let mut j = i + 2;
            if ch == b'<' {
                if starts_at_folded(s, j, b"php") {
                    j += 3;
                } else if at(s, j, b'=') {
                    j += 1;
                }
            }
            x.emit(i, j, Token::Keyword);
            i = j;
            continue;
        }
        if hooks.contains(Hooks::PREPROCESSOR) && ch == b'#' && i == x.first {
            i = x.take_directive(i);
            continue;
        }
        if hooks.contains(Hooks::TEMPLATE) && ch == b'`' {
            x.out.add(i, i + 1, Token::String);
            x.st.mode = Mode::TemplateText;
            match x.scan_template(i + 1) {
                Some(next) => i = next,
                None => return false,
            }
            continue;
        }
        if hooks.contains(Hooks::HEREDOC)
            && starts_at(s, i, b"<<")
            && let Some(end) = x.open_heredoc(i)
        {
            i = end;
            continue;
        }
        if hooks.contains(Hooks::PERCENT_VARS) {
            if matches!(ch, b'%' | b'!')
                && let Some(end) = x.scan_percent_variable(i)
            {
                x.emit(i, end, Token::Variable);
                i = end;
                continue;
            }
            if ch == b':' && i == x.first && x.ident_starts_at(i + 1) {
                let end = x.scan_ident(i + 1);
                x.emit(i, end, Token::Function); // a label
                i = end;
                continue;
            }
        }
        // Comments and strings from the tables.
        if let Some(index) = x.find_delim(i) {
            match x.take_delim(i, index) {
                Some(end) => i = end,
                None => break,
            }
            continue;
        }
        if hooks.contains(Hooks::REGEX)
            && ch == b'/'
            && x.regex_allowed()
            && let Some(end) = x.scan_regex(i)
        {
            x.emit(i, end, Token::Regexp);
            i = end;
            continue;
        }
        if hooks.contains(Hooks::RUST) && ch == b'\'' && x.ident_starts_at(i + 1) {
            let end = x.scan_ident(i + 1); // a 'lifetime (a 'c' char was taken above)
            x.emit(i, end, Token::Type);
            i = end;
            continue;
        }
        let signed = ch == b'.' || (c.lisp && matches!(ch, b'-' | b'+'));
        if is_digit(ch)
            || (signed
                && s.get(i + 1).copied().is_some_and(is_digit)
                && (i == 0 || !x.ident_char(s[i - 1])))
        {
            let end = x.scan_number(if is_digit(ch) { i } else { i + 1 });
            x.emit(i, end, Token::Number);
            i = end;
            continue;
        }
        if contains(c.sigils, ch)
            && let Some(end) = x.scan_variable(i)
        {
            let constant = x.lang.constants.contains(&s[i + 1..end]);
            x.emit(
                i,
                end,
                if constant {
                    Token::Constant
                } else {
                    Token::Variable
                },
            );
            i = end;
            continue;
        }
        if Some(ch) == c.annotation && x.ident_starts_at(i + 1) {
            let mut end = x.scan_ident(i + 1);
            while at(s, end, b'.') && x.ident_starts_at(end + 1) {
                end = x.scan_ident(end + 1);
            }
            x.emit(i, end, Token::Function);
            i = end;
            continue;
        }
        if Some(ch) == c.symbol
            && !c.lisp
            && x.ident_starts_at(i + 1)
            && (i == 0 || (!x.ident_char(s[i - 1]) && s[i - 1] != b':'))
        {
            let end = x.scan_ident(i + 1);
            x.emit(i, end, Token::Constant);
            i = end;
            continue;
        }
        if c.lisp && lisp_symbol_char(ch) {
            i = x.take_lisp_symbol(i);
            continue;
        }
        if x.ident_start(ch) {
            let end = x.scan_ident(i);
            if let Some(flow) = x.take_prefixed_string(i, end) {
                match flow {
                    Flow::At(next) => i = next,
                    Flow::EndLine => break,
                    Flow::Stop => return false,
                }
                continue;
            }
            let token = x.classify(i, end);
            x.emit(i, end, token);
            if token == Token::Keyword {
                x.prev_word = Some(&s[i..end]);
            }
            i = end;
            continue;
        }
        let token = if contains("()[]{},;.", ch) {
            Token::Punctuation
        } else if contains("+-*/%=<>!&|^~?:@#\\", ch) {
            Token::Operator
        } else {
            Token::Text
        };
        x.emit(i, i + 1, token);
        i += 1;
    }
    true
}
