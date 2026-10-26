#pragma once

#include "ui/syntax/syntax.hpp"
#include <cstdint>
#include <memory>
#include <span>
#include <string_view>
#include <unordered_set>
#include <vector>

// The tables behind a language (ui/syntax/languages.cpp) and the scanners that read them (ui/syntax/tokenizer.cpp).
namespace imgit::syntax
{
    // Which scanner a language uses.
    enum class Kind : std::uint8_t
    {
        CODE, // the code scanner: delimiters, words, numbers, operators (plus the hooks below)
        MARKUP, // XML / HTML and the HTML template languages
        CSS,
        JSON,
        YAML,
        INI,
        MARKDOWN,
        DOCKERFILE,
    };

    // A comment or string: from `open` to `close` (the end of the line when empty).
    struct Delim
    {
        enum Flags : std::uint8_t
        {
            MULTILINE = 1 << 0, // may run on past the end of the line
            RAW = 1 << 1, // no escapes (a doubled close character stays inside)
            NESTED = 1 << 2, // `open` inside opens one level deeper
            OPENS_AT_START = 1 << 3, // opens only at the line's first non-blank (=begin)
            CLOSES_AT_START = 1 << 4, // closes only at a line's first non-blank ("@ here-strings, =end)
            CHAR = 1 << 5, // a character literal: one character or one escape, else no string at all
            WORD = 1 << 6, // `open` is a word (REM): case-folded per the language, not followed by a letter
        };
        std::string_view open;
        std::string_view close;
        Token token = Token::STRING;
        std::uint8_t flags = 0;
    };

    // Hand-written scanners a tier-1 code language adds to the tables.
    enum Hook : std::uint32_t
    {
        PREPROCESSOR = 1 << 0, // # directives at the line start, <header> after #include
        CPP_RAW = 1 << 1, // R"delim( ... )delim"
        DIGIT_QUOTES = 1 << 2, // 1'000'000
        RUST = 1 << 3, // r#"..."# raw strings, 'lifetimes
        TEMPLATE = 1 << 4, // `template ${literals}`
        REGEX = 1 << 5, // /regex/flags where an operand is expected
        HEREDOC = 1 << 6, // <<EOF ... EOF (<<- / <<~ indented, <<< for PHP)
        HEREDOC_SPACED = 1 << 7, // the shell's << EOF with a blank
        LUA_LONG = 1 << 8, // [[ ]], [==[ ]==], --[[ ]]
        PHP_TAGS = 1 << 9, // <?php ?>
        PERCENT_VARS = 1 << 10, // batch: %name%, %%i, %~dp0, !name!, :labels
    };

    // A whitespace-separated word list, looked up as whole identifiers (lower-cased first for a case-insensitive
    // language, whose lists are written in lower case).
    class WordSet
    {
    public:
        WordSet(std::string_view words, bool fold_case);
        [[nodiscard]] bool contains(std::string_view word) const;

    private:
        std::unique_ptr<char[]> m_storage; // the words (lower-cased when folding); m_words views into it
        std::unordered_set<std::string_view> m_words;
        bool m_fold_case;
    };

    struct CodeSpec
    {
        std::string_view keywords;
        std::string_view types;
        std::string_view constants;
        std::string_view builtins; // drawn as functions
        std::span<const Delim> delims; // tried in order: put longer opens first
        std::string_view string_prefixes; // letters that may prefix a string (r"..", b'..', u8"..")
        std::string_view ident_start; // characters an identifier may start with, besides letters and _
        std::string_view ident_chars; // ... and continue with, besides letters, digits and _
        std::string_view sigils; // variable prefixes: $name, ${name}, @ivar
        char escape = '\\'; // 0: none (quotes are doubled)
        char annotation = 0; // @Annotation, @decorator
        char symbol = 0; // :symbol, :keyword
        bool fold_case = false;
        bool lisp = false; // ( head position as a function, wide symbol characters
        bool line_head_tag = false; // the first word of a line is a tag (Pug)
        std::uint32_t hooks = 0;
    };

    // Markup template tags: {{ }}, {% %}, <% %>, <?php ?>. A COMMENT tag is one comment; any other has its inside
    // tokenized with the code language `code`.
    struct TemplateTag
    {
        std::string_view open;
        std::string_view close; // empty: `open` then one (dotted) identifier (Razor's @Model.Name)
        Token token = Token::PUNCTUATION;
        std::string_view code;
    };

    struct MarkupSpec
    {
        bool html = false; // <script> / <style> bodies as JavaScript / CSS, case-insensitive names
        std::span<const TemplateTag> templates;
    };

    struct Language
    {
        std::string_view id;
        Kind kind = Kind::CODE;
        bool own_scanner = false;
        CodeSpec code; // CODE; and the delimiters / sigils of CSS, JSON and Dockerfile
        MarkupSpec markup;
        // Built from `code`'s lists.
        WordSet keywords{{}, false};
        WordSet types{{}, false};
        WordSet constants{{}, false};
        WordSet builtins{{}, false};
        std::vector<const Language*> template_languages; // markup.templates' code languages, by index
    };
} // namespace imgit::syntax
