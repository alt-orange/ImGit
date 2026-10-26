#pragma once

#include "git/types.hpp"
#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <string_view>
#include <vector>

// Syntax highlighting for the diff and merge editors: a small line-based tokenizer. Each line is cut into spans of
// Monaco-style token classes; what a line leaves open (a block comment, a multi-line or raw string, a heredoc, a
// template literal, a markup tag, a code fence) is carried to the next line in a State. The common languages have
// their own scanners (tier 1); the long tail is driven by tables of keywords, comment markers and string delimiters
// alone (tier 2). Unknown files are plain text and cost nothing.
namespace imgit::syntax
{
    enum class Token : std::uint8_t
    {
        TEXT,
        COMMENT,
        STRING,
        NUMBER,
        KEYWORD,
        TYPE,
        FUNCTION,
        VARIABLE,
        OPERATOR,
        PUNCTUATION,
        TAG,
        ATTRIBUTE,
        REGEXP,
        CONSTANT,
    };

    // Bytes [begin, end) of a line. A line's spans cover it from start to end; blanks join the span before them.
    struct Span
    {
        std::uint16_t begin = 0;
        std::uint16_t end = 0;
        Token token = Token::TEXT;
    };

    struct Language;

    // What a line leaves open for the next one. Default-constructed: nothing (the start of a file or a hunk).
    struct State
    {
        std::uint8_t mode = 0;
        std::uint8_t delim = 0; // the open delimiter (an index into the language's table) or the open token
        std::uint8_t nesting = 0; // how deep the open delimiter is nested in itself: /* /* */ */
        std::uint8_t depth = 0; // CSS braces, a block scalar's indent, a tag's kind, an indented heredoc
        std::uint8_t outer = 0; // a Dockerfile instruction continued on the next line
        std::string close; // the text that ends the open construct
        // The template literals whose ${ } the code is in, innermost last: each one's count of open braces.
        std::string templates;
        const Language* embedded = nullptr; // the language of a <script> / <style> body or a code fence
        std::string end_marker; // what ends that embedded region; empty outside one
        // An embedded region inside that one: a <script> in a fence's HTML.
        const Language* inner_embedded = nullptr;
        std::string inner_end_marker;
        bool operator==(const State&) const = default;
    };

    // Longer lines, and files with more lines or bytes, are drawn as plain text.
    constexpr std::size_t MAX_LINE_LENGTH = 2000;
    constexpr std::size_t MAX_LINES = 5000;
    constexpr std::size_t MAX_BYTES = 1024 * 1024;

    // The language of `path`: by file name (Dockerfile), by extension, else by a shebang in `first_line`; null for
    // plain text.
    [[nodiscard]] const Language* language_for_path(std::string_view path, std::string_view first_line = {});
    // By id ("cpp", "python") or a common alias ("c++", "py", "sh"), as a code fence's info string names it.
    [[nodiscard]] const Language* language_by_name(std::string_view name);
    [[nodiscard]] std::string_view language_id(const Language& language) noexcept;
    // Whether the language has its own scanner (tier 1) rather than the generic table-driven one (tier 2).
    [[nodiscard]] bool has_own_scanner(const Language& language) noexcept;
    // Every language, for tests.
    [[nodiscard]] std::span<const Language* const> all_languages();

    // Appends the spans of `line` to `out` and advances `state` past it. A line over MAX_LINE_LENGTH gets no spans
    // (it is drawn plain) and resets the state.
    void tokenize_line(const Language& language, std::string_view line, State& state, std::vector<Span>& out);

    // The spans of many lines, stored flat.
    class LineSpans
    {
    public:
        void clear() noexcept;
        // Tokenizes `line` as the next line.
        void add(const Language& language, std::string_view line, State& state);
        // A line with no spans: drawn plain.
        void add_plain();
        [[nodiscard]] std::size_t size() const noexcept;
        [[nodiscard]] std::span<const Span> line(std::size_t index) const noexcept;

    private:
        std::vector<Span> m_spans;
        std::vector<std::uint32_t> m_starts; // per line, its first span; one past the end at the back
    };

    // A whole text, line by line from its start.
    [[nodiscard]] LineSpans tokenize_lines(const Language& language, std::span<const std::string> lines);

    // A diff's lines: each hunk from a fresh state, the old side (context and deletions) and the new side (context
    // and additions) each carried on its own, so a block comment opened by a deletion does not run into the
    // additions. Context lines are coloured by the new side.
    struct DiffSpans
    {
        LineSpans spans;
        std::vector<std::size_t> hunk_starts; // per hunk, the index of its first line in `spans`
        // The spans of `line`, a line of `diff.hunks[hunk]`; empty for a line outside it (drawn plain).
        [[nodiscard]] std::span<const Span> line(const git::FileDiff& diff, std::size_t hunk,
                                                 const git::DiffLine* line) const noexcept;
    };
    [[nodiscard]] DiffSpans tokenize_diff(const Language& language, const git::FileDiff& diff);

    // A cheap content hash of a diff / of lines, and whether they are within the size limits.
    [[nodiscard]] std::uint64_t fingerprint(const git::FileDiff& diff, bool& within_limits) noexcept;
    [[nodiscard]] std::uint64_t fingerprint(std::span<const std::string> lines, bool& within_limits) noexcept;

    // The highlighting of the diff a view shows. One per view; get() returns null for plain text (unknown language,
    // binary, over the limits). A loaded diff is looked at once per revision; one without a revision (0) is
    // fingerprinted each time and re-tokenized when its path or text changed.
    class DiffCache
    {
    public:
        [[nodiscard]] const DiffSpans* get(const git::FileDiff& diff);

    private:
        std::uint64_t m_revision = 0; // the diff last looked at
        bool m_highlighted = false; // whether get() returned m_spans for it
        bool m_tokenized = false; // m_spans holds the text with m_hash in m_language
        std::uint64_t m_hash = 0;
        const Language* m_language = nullptr;
        DiffSpans m_spans;
    };

    // The same for a whole text (a merge pane): `lines` of the file at `path`, at `revision` (0: fingerprinted).
    class TextCache
    {
    public:
        [[nodiscard]] const LineSpans* get(std::string_view path, std::span<const std::string> lines,
                                           std::uint64_t revision = 0);

    private:
        std::uint64_t m_revision = 0;
        std::string m_path;
        bool m_highlighted = false;
        bool m_tokenized = false;
        std::uint64_t m_hash = 0;
        const Language* m_language = nullptr;
        LineSpans m_spans;
    };
} // namespace imgit::syntax
