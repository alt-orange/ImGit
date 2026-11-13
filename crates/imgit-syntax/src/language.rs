//! The tables behind a language (see [`crate::languages`]) that the scanners in [`crate::tokenizer`] read.

use crate::languages;
use crate::token::Token;
use std::collections::HashSet;
use std::fmt;
use std::ops::BitOr;

/// Which scanner a language uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Kind {
    /// The code scanner: delimiters, words, numbers, operators (plus the [`Hooks`]).
    #[default]
    Code,
    /// XML / HTML and the HTML template languages.
    Markup,
    Css,
    Json,
    Yaml,
    Ini,
    Markdown,
    Dockerfile,
}

/// A comment or string: from `open` to `close` (the end of the line when `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Delim {
    pub open: &'static str,
    pub close: Option<&'static str>,
    pub token: Token,
    /// May run on past the end of the line.
    pub multiline: bool,
    /// No escapes (a doubled close character stays inside).
    pub raw: bool,
    /// `open` inside opens one level deeper.
    pub nested: bool,
    /// Opens only at the line's first non-blank (`=begin`).
    pub opens_at_start: bool,
    /// Closes only at a line's first non-blank (`"@` here-strings, `=end`).
    pub closes_at_start: bool,
    /// A character literal: one character or one escape, else no string at all.
    pub char_literal: bool,
    /// `open` is a word (`REM`): case-folded per the language, not followed by a letter.
    pub word: bool,
}

impl Delim {
    /// A delimiter running from `open` to `close`.
    pub const fn new(open: &'static str, close: &'static str, token: Token) -> Self {
        Self {
            open,
            close: Some(close),
            token,
            multiline: false,
            raw: false,
            nested: false,
            opens_at_start: false,
            closes_at_start: false,
            char_literal: false,
            word: false,
        }
    }

    /// A delimiter running from `open` to the end of the line.
    pub const fn line(open: &'static str, token: Token) -> Self {
        Self {
            close: None,
            ..Self::new(open, "", token)
        }
    }

    pub const fn multiline(self) -> Self {
        Self {
            multiline: true,
            ..self
        }
    }

    pub const fn raw(self) -> Self {
        Self { raw: true, ..self }
    }

    pub const fn nested(self) -> Self {
        Self {
            nested: true,
            ..self
        }
    }

    pub const fn opens_at_start(self) -> Self {
        Self {
            opens_at_start: true,
            ..self
        }
    }

    pub const fn closes_at_start(self) -> Self {
        Self {
            closes_at_start: true,
            ..self
        }
    }

    /// Opens and closes only at a line's first non-blank.
    pub const fn at_start(self) -> Self {
        self.opens_at_start().closes_at_start()
    }

    pub const fn char_literal(self) -> Self {
        Self {
            char_literal: true,
            ..self
        }
    }

    pub const fn word(self) -> Self {
        Self { word: true, ..self }
    }
}

/// Hand-written scanners a tier-1 code language adds to the tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Hooks(u16);

impl Hooks {
    pub const NONE: Self = Self(0);
    /// `#` directives at the line start, `<header>` after `#include`.
    pub const PREPROCESSOR: Self = Self(1 << 0);
    /// `R"delim( ... )delim"`.
    pub const CPP_RAW: Self = Self(1 << 1);
    /// `1'000'000`.
    pub const DIGIT_QUOTES: Self = Self(1 << 2);
    /// `r#"..."#` raw strings, `'lifetimes`.
    pub const RUST: Self = Self(1 << 3);
    /// `` `template ${literals}` ``.
    pub const TEMPLATE: Self = Self(1 << 4);
    /// `/regex/flags` where an operand is expected.
    pub const REGEX: Self = Self(1 << 5);
    /// `<<EOF ... EOF` (`<<-` / `<<~` indented, `<<<` for PHP).
    pub const HEREDOC: Self = Self(1 << 6);
    /// The shell's `<< EOF` with a blank.
    pub const HEREDOC_SPACED: Self = Self(1 << 7);
    /// `[[ ]]`, `[==[ ]==]`, `--[[ ]]`.
    pub const LUA_LONG: Self = Self(1 << 8);
    /// `<?php ?>`.
    pub const PHP_TAGS: Self = Self(1 << 9);
    /// Batch: `%name%`, `%%i`, `%~dp0`, `!name!`, `:labels`.
    pub const PERCENT_VARS: Self = Self(1 << 10);

    pub const fn contains(self, hook: Self) -> bool {
        self.0 & hook.0 == hook.0
    }
}

impl BitOr for Hooks {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A whitespace-separated word list, looked up as whole identifiers (lower-cased first for a case-insensitive
/// language, whose lists are written in lower case).
#[derive(Debug, Clone, Default)]
pub(crate) struct WordSet {
    words: HashSet<Box<[u8]>>,
    fold_case: bool,
}

impl WordSet {
    /// The longest word a case-folding set looks up.
    const MAX_FOLDED: usize = 64;

    pub fn new(words: &str, fold_case: bool) -> Self {
        let words = words
            .split(' ')
            .filter(|word| !word.is_empty())
            .map(|word| {
                if fold_case {
                    word.to_ascii_lowercase().into_bytes().into_boxed_slice()
                } else {
                    word.as_bytes().into()
                }
            })
            .collect();
        Self { words, fold_case }
    }

    pub fn contains(&self, word: &[u8]) -> bool {
        if self.words.is_empty() || word.is_empty() {
            return false;
        }
        if !self.fold_case {
            return self.words.contains(word);
        }
        let mut buffer = [0u8; Self::MAX_FOLDED];
        let Some(folded) = buffer.get_mut(..word.len()) else {
            return false;
        };
        folded.copy_from_slice(word);
        folded.make_ascii_lowercase();
        self.words.contains(&*folded)
    }
}

/// What the code scanner reads; CSS, JSON and Dockerfile use its delimiters, sigils and word lists too.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CodeSpec {
    pub keywords: &'static str,
    pub types: &'static str,
    pub constants: &'static str,
    /// Drawn as functions.
    pub builtins: &'static str,
    /// Tried in order: longer opens come first.
    pub delims: &'static [Delim],
    /// Letters that may prefix a string (`r".."`, `b'..'`, `u8".."`).
    pub string_prefixes: &'static str,
    /// Characters an identifier may start with, besides letters and `_`.
    pub ident_start: &'static str,
    /// ... and continue with, besides letters, digits and `_`.
    pub ident_chars: &'static str,
    /// Variable prefixes: `$name`, `${name}`, `@ivar`.
    pub sigils: &'static str,
    /// `None`: no escapes (quotes are doubled).
    pub escape: Option<u8>,
    /// `@Annotation`, `@decorator`.
    pub annotation: Option<u8>,
    /// `:symbol`, `:keyword`.
    pub symbol: Option<u8>,
    pub fold_case: bool,
    /// `(` head position as a function, wide symbol characters.
    pub lisp: bool,
    /// The first word of a line is a tag (Pug).
    pub line_head_tag: bool,
    pub hooks: Hooks,
}

impl Default for CodeSpec {
    fn default() -> Self {
        Self {
            keywords: "",
            types: "",
            constants: "",
            builtins: "",
            delims: &[],
            string_prefixes: "",
            ident_start: "",
            ident_chars: "",
            sigils: "",
            escape: Some(b'\\'),
            annotation: None,
            symbol: None,
            fold_case: false,
            lisp: false,
            line_head_tag: false,
            hooks: Hooks::NONE,
        }
    }
}

/// Markup template tags: `{{ }}`, `{% %}`, `<% %>`, `@Model.Name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemplateTag {
    /// One comment from `open` to `close`.
    Comment {
        open: &'static str,
        close: &'static str,
    },
    /// `open` to `close`, the inside tokenized with the code language `language`.
    Code {
        open: &'static str,
        close: &'static str,
        language: &'static str,
    },
    /// `open` then one (dotted) identifier of `language` (Razor's `@Model.Name`).
    Identifier {
        open: &'static str,
        language: &'static str,
    },
}

impl TemplateTag {
    pub fn open(&self) -> &'static str {
        match *self {
            Self::Comment { open, .. }
            | Self::Code { open, .. }
            | Self::Identifier { open, .. } => open,
        }
    }

    pub fn close(&self) -> Option<&'static str> {
        match *self {
            Self::Comment { close, .. } | Self::Code { close, .. } => Some(close),
            Self::Identifier { .. } => None,
        }
    }

    pub fn is_comment(&self) -> bool {
        matches!(self, Self::Comment { .. })
    }

    /// The name of the language of the tag's inside.
    pub fn language(&self) -> Option<&'static str> {
        match *self {
            Self::Comment { .. } => None,
            Self::Code { language, .. } | Self::Identifier { language, .. } => Some(language),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MarkupSpec {
    /// `<script>` / `<style>` bodies as JavaScript / CSS, case-insensitive names.
    pub html: bool,
    pub templates: &'static [TemplateTag],
}

/// A language the highlighter knows: look one up with [`Language::for_path`] or [`Language::by_name`].
pub struct Language {
    pub(crate) id: &'static str,
    pub(crate) kind: Kind,
    pub(crate) own_scanner: bool,
    /// `Kind::Code`; and the delimiters / sigils of CSS, JSON and Dockerfile.
    pub(crate) code: CodeSpec,
    pub(crate) markup: MarkupSpec,
    // Built from `code`'s lists.
    pub(crate) keywords: WordSet,
    pub(crate) types: WordSet,
    pub(crate) constants: WordSet,
    pub(crate) builtins: WordSet,
    /// `markup.templates`' code languages, by index: their positions in [`Language::all`].
    pub(crate) template_languages: Vec<Option<usize>>,
}

impl Language {
    pub(crate) fn new(
        id: &'static str,
        kind: Kind,
        own_scanner: bool,
        code: CodeSpec,
        markup: MarkupSpec,
    ) -> Self {
        let fold = code.fold_case;
        Self {
            id,
            kind,
            own_scanner,
            code,
            markup,
            keywords: WordSet::new(code.keywords, fold),
            types: WordSet::new(code.types, fold),
            constants: WordSet::new(code.constants, fold),
            builtins: WordSet::new(code.builtins, fold),
            template_languages: Vec::new(),
        }
    }

    /// The language of `path`: by file name (`Dockerfile`), by extension, else by a shebang in `first_line`;
    /// `None` for plain text.
    #[must_use]
    pub fn for_path(path: &str, first_line: Option<&str>) -> Option<&'static Self> {
        languages::for_path(path, first_line)
    }

    /// By id (`"cpp"`, `"python"`) or a common alias (`"c++"`, `"py"`, `"sh"`), as a code fence's info string
    /// names it.
    #[must_use]
    pub fn by_name(name: &str) -> Option<&'static Self> {
        languages::by_name(name)
    }

    /// Every language.
    #[must_use]
    pub fn all() -> &'static [Self] {
        languages::all()
    }

    /// The language's canonical name: `"cpp"`, `"python"`.
    #[must_use]
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Whether the language has its own scanner (tier 1) rather than the generic table-driven one (tier 2).
    #[must_use]
    pub fn has_own_scanner(&self) -> bool {
        self.own_scanner
    }

    /// The code language of the inside of `markup.templates[index]`.
    pub(crate) fn template_language(&self, index: usize) -> Option<&'static Self> {
        let language = (*self.template_languages.get(index)?)?;
        Self::all().get(language)
    }
}

/// Languages are singletons named by their id.
impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Language {}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Language")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
