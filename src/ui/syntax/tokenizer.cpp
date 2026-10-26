#include "ui/syntax/language.hpp"
#include <algorithm>
#include <functional>

namespace imgit::syntax
{
    namespace
    {
        constexpr std::size_t npos = std::string_view::npos;

        // What State::mode holds.
        enum Mode : std::uint8_t
        {
            NONE,
            DELIM, // inside code.delims[state.delim], nested state.nesting deep
            RAW, // a string or comment ending at state.close, no escapes; delim: its Token
            HEREDOC_BODY, // lines up to the one that is state.close; depth 1: that line may be indented
            TEMPLATE_TEXT, // the text of a template literal
            TAG, // inside a markup tag; depth: 1 <script>, 2 <style>
            ATTR_STRING, // a tag's quoted attribute value, up to state.close
            MARKUP_COMMENT,
            CDATA,
            TEMPLATE_TAG, // inside markup.templates[state.delim]
            BLOCK_SCALAR, // a YAML | / > scalar: the lines indented past depth
        };

        // What State::outer holds.
        enum Outer : std::uint8_t
        {
            OUTER_NONE,
            OUTER_CONTINUED, // a Dockerfile instruction continued by a backslash
        };

        bool is_alpha(char c) noexcept
        {
            return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
        }

        bool is_digit(char c) noexcept
        {
            return c >= '0' && c <= '9';
        }

        bool is_alnum(char c) noexcept
        {
            return is_alpha(c) || is_digit(c);
        }

        bool is_blank(char c) noexcept
        {
            return c == ' ' || c == '\t' || c == '\r' || c == '\f' || c == '\v';
        }

        // A byte of a multi-byte UTF-8 character: part of a word, never split.
        bool is_utf8(char c) noexcept
        {
            return static_cast<unsigned char>(c) >= 0x80;
        }

        char to_lower(char c) noexcept
        {
            return c >= 'A' && c <= 'Z' ? static_cast<char>(c - 'A' + 'a') : c;
        }

        bool contains(std::string_view chars, char c) noexcept
        {
            return c != 0 && chars.find(c) != npos;
        }

        bool starts_at(std::string_view s, std::size_t i, std::string_view prefix) noexcept
        {
            return !prefix.empty() && s.size() - std::min(i, s.size()) >= prefix.size()
                   && s.compare(i, prefix.size(), prefix) == 0;
        }

        bool starts_at_folded(std::string_view s, std::size_t i, std::string_view prefix) noexcept
        {
            if (prefix.empty() || s.size() - std::min(i, s.size()) < prefix.size())
            {
                return false;
            }
            for (std::size_t k = 0; k < prefix.size(); ++k)
            {
                if (to_lower(s[i + k]) != to_lower(prefix[k]))
                {
                    return false;
                }
            }
            return true;
        }

        std::size_t find_folded(std::string_view s, std::string_view needle, std::size_t from) noexcept
        {
            for (std::size_t i = from; i + needle.size() <= s.size(); ++i)
            {
                if (starts_at_folded(s, i, needle))
                {
                    return i;
                }
            }
            return npos;
        }

        std::size_t skip_blanks(std::string_view s, std::size_t i) noexcept
        {
            while (i < s.size() && is_blank(s[i]))
            {
                ++i;
            }
            return i;
        }

        bool blank(std::string_view s) noexcept
        {
            return std::all_of(s.begin(), s.end(), is_blank);
        }

        // The spans of one line. Positions given to add() are relative to `offset` (where an embedded language's
        // part of the line starts); gaps between spans become TEXT, or join the span before them when blank.
        class Out
        {
        public:
            Out(std::vector<Span>& spans, std::size_t first, std::string_view line, std::size_t offset) noexcept
                : m_spans(spans), m_first(first), m_line(line), m_offset(offset)
            {
            }

            [[nodiscard]] Out at(std::size_t offset) const noexcept
            {
                return Out(m_spans, m_first, m_line, m_offset + offset);
            }

            void add(std::size_t begin, std::size_t end, Token token)
            {
                begin += m_offset;
                end = std::min(end + m_offset, m_line.size());
                const std::size_t last = this->end();
                begin = std::max(begin, last);
                if (end <= begin)
                {
                    return;
                }
                gap(last, begin);
                push(begin, end, token);
            }

            // Covers the rest of the line.
            void finish()
            {
                gap(end(), m_line.size());
            }

        private:
            [[nodiscard]] std::size_t end() const noexcept
            {
                return m_spans.size() > m_first ? m_spans.back().end : 0;
            }

            void gap(std::size_t begin, std::size_t end)
            {
                if (end <= begin)
                {
                    return;
                }
                if (m_spans.size() > m_first && blank(m_line.substr(begin, end - begin)))
                {
                    m_spans.back().end = static_cast<std::uint16_t>(end);
                    return;
                }
                push(begin, end, Token::TEXT);
            }

            void push(std::size_t begin, std::size_t end, Token token)
            {
                if (m_spans.size() > m_first && m_spans.back().token == token && m_spans.back().end == begin)
                {
                    m_spans.back().end = static_cast<std::uint16_t>(end);
                    return;
                }
                m_spans.push_back({static_cast<std::uint16_t>(begin), static_cast<std::uint16_t>(end), token});
            }

            std::vector<Span>& m_spans;
            std::size_t m_first; // this line's first span
            std::string_view m_line;
            std::size_t m_offset;
        };

        void tokenize_range(const Language& language, std::string_view s, State& state, Out out);

        // Tokenizes [begin, end) of `s` with another language, from a fresh state: a template tag's code, a bit of
        // inline HTML.
        void tokenize_piece(const Language* language, std::string_view s, std::size_t begin, std::size_t end, Out& out,
                            Token fallback = Token::TEXT)
        {
            if (end <= begin)
            {
                return;
            }
            if (language == nullptr)
            {
                out.add(begin, end, fallback);
                return;
            }
            State fresh;
            tokenize_range(*language, s.substr(begin, end - begin), fresh, out.at(begin));
        }

        // An embedded language's part of a line. The region's own fields of `state` are kept apart, so an embedded
        // HTML does not take over the fence it sits in; that HTML's own <script> / <style> region is the inner one.
        void tokenize_embedded(const Language& language, std::string_view s, std::size_t begin, std::size_t end,
                               State& state, Out& out)
        {
            State inner;
            inner.mode = state.mode;
            inner.delim = state.delim;
            inner.nesting = state.nesting;
            inner.depth = state.depth;
            inner.outer = state.outer;
            inner.close = std::move(state.close);
            inner.templates = std::move(state.templates);
            inner.embedded = state.inner_embedded;
            inner.end_marker = std::move(state.inner_end_marker);
            tokenize_range(language, s.substr(begin, end - begin), inner, out.at(begin));
            state.mode = inner.mode;
            state.delim = inner.delim;
            state.nesting = inner.nesting;
            state.depth = inner.depth;
            state.outer = inner.outer;
            state.close = std::move(inner.close);
            state.templates = std::move(inner.templates);
            state.inner_embedded = inner.embedded;
            state.inner_end_marker = std::move(inner.end_marker);
        }

        // The end of an embedded region: nothing of it is left open.
        void leave_embedded(State& state)
        {
            state = {};
        }

        // ---- the code scanner ----

        constexpr std::string_view DEF_WORDS[] = {"def",  "defp",      "fn",    "func",   "function", "fun",  "sub",
                                                  "proc", "procedure", "macro", "method", "defn",     "mixin"};
        constexpr std::string_view TYPE_DEF_WORDS[] = {
                "class",  "struct", "interface", "enum",       "trait", "type",      "union",  "record",
                "object", "new",    "extends",   "implements", "impl",  "namespace", "module", "contract"};

        bool one_of(std::string_view word, std::span<const std::string_view> words, bool fold) noexcept
        {
            return std::any_of(words.begin(), words.end(),
                               [&](std::string_view w)
                               {
                                   return w.size() == word.size() && starts_at_folded(word, 0, w)
                                          && (fold || word == w);
                               });
        }

        struct Code
        {
            Code(const Language& language, std::string_view line, State& state, Out& spans) noexcept
                : lang(language), c(language.code), s(line), st(state), out(spans), first(skip_blanks(line, 0))
            {
            }

            const Language& lang;
            const CodeSpec& c;
            std::string_view s;
            State& st;
            Out& out;
            std::size_t first; // the line's first non-blank
            // The last significant token, for regex literals and the names after def / class.
            Token prev = Token::TEXT;
            char prev_char = 0;
            std::string_view prev_word;
            std::string heredoc; // a heredoc opened on this line, starting on the next
            bool heredoc_indented = false;

            void emit(std::size_t begin, std::size_t end, Token token)
            {
                out.add(begin, end, token);
                if (end > begin)
                {
                    prev = token;
                    prev_char = s[end - 1];
                    prev_word = {};
                }
            }

            [[nodiscard]] bool ident_start(char ch) const noexcept
            {
                return is_alpha(ch) || ch == '_' || is_utf8(ch) || contains(c.ident_start, ch);
            }

            [[nodiscard]] bool ident_char(char ch) const noexcept
            {
                return is_alnum(ch) || ch == '_' || is_utf8(ch) || contains(c.ident_chars, ch);
            }

            // The end of the word starting at `i`, an identifier start (which may be a start-only character: `define).
            [[nodiscard]] std::size_t scan_ident(std::size_t i) const noexcept
            {
                ++i;
                while (i < s.size() && ident_char(s[i]))
                {
                    ++i;
                }
                return i;
            }
        };

        // The close of `d` from `j`, past nested opens and escapes: the index after it, npos when the line ends
        // first.
        std::size_t find_close(const Code& x, std::size_t j, const Delim& d, std::uint8_t& depth)
        {
            const std::string_view s = x.s;
            const bool raw = (d.flags & Delim::RAW) != 0 || x.c.escape == 0;
            const bool doubled = raw && d.close.size() == 1 && (d.close[0] == '\'' || d.close[0] == '"');
            if ((d.flags & Delim::CLOSES_AT_START) != 0)
            {
                const std::size_t k = skip_blanks(s, 0);
                return k >= j && starts_at(s, k, d.close) ? k + d.close.size() : npos;
            }
            while (j < s.size())
            {
                if (!raw && s[j] == x.c.escape)
                {
                    j += 2;
                    continue;
                }
                if ((d.flags & Delim::NESTED) != 0 && starts_at(s, j, d.open))
                {
                    depth = static_cast<std::uint8_t>(std::min(depth + 1, 255));
                    j += d.open.size();
                    continue;
                }
                if (starts_at(s, j, d.close))
                {
                    const std::size_t end = j + d.close.size();
                    if (doubled && end < s.size() && s[end] == d.close[0])
                    {
                        j = end + 1;
                        continue;
                    }
                    if (depth > 0)
                    {
                        --depth;
                        j = end;
                        continue;
                    }
                    return end;
                }
                ++j;
            }
            return npos;
        }

        // The delimiter that opens at `i`, or -1.
        int find_delim(const Code& x, std::size_t i)
        {
            const std::string_view s = x.s;
            for (std::size_t k = 0; k < x.c.delims.size(); ++k)
            {
                const Delim& d = x.c.delims[k];
                if ((d.flags & Delim::OPENS_AT_START) != 0 && i != x.first)
                {
                    continue;
                }
                if ((d.flags & Delim::WORD) != 0)
                {
                    if (!(x.c.fold_case ? starts_at_folded(s, i, d.open) : starts_at(s, i, d.open))
                        || (i + d.open.size() < s.size() && x.ident_char(s[i + d.open.size()])))
                    {
                        continue;
                    }
                    return static_cast<int>(k);
                }
                if (!starts_at(s, i, d.open))
                {
                    continue;
                }
                if ((d.flags & Delim::CHAR) != 0)
                {
                    // One character (a whole UTF-8 sequence) or one escape, then the close.
                    std::size_t j = i + d.open.size();
                    if (j >= s.size())
                    {
                        continue;
                    }
                    if (s[j] == x.c.escape && x.c.escape != 0)
                    {
                        const std::size_t end = s.find(d.close, j + 2);
                        if (end == npos || end > j + 12)
                        {
                            continue;
                        }
                        return static_cast<int>(k);
                    }
                    const auto lead = static_cast<unsigned char>(s[j]);
                    j += lead >= 0xF0 ? 4 : lead >= 0xE0 ? 3 : lead >= 0xC0 ? 2 : 1;
                    if (!starts_at(s, j, d.close))
                    {
                        continue;
                    }
                }
                return static_cast<int>(k);
            }
            return -1;
        }

        // Emits delimiter `index` opening at `i`: the index after its close, or npos when it runs to the end of the
        // line (and on to the next when it may).
        std::size_t take_delim(Code& x, std::size_t i, int index)
        {
            const Delim& d = x.c.delims[static_cast<std::size_t>(index)];
            const std::size_t n = x.s.size();
            if (d.close.empty())
            {
                x.emit(i, n, d.token);
                return npos;
            }
            std::uint8_t depth = 0;
            const std::size_t end =
                    (d.flags & Delim::CLOSES_AT_START) != 0 ? npos : find_close(x, i + d.open.size(), d, depth);
            if (end != npos)
            {
                x.emit(i, end, d.token);
                return end;
            }
            x.emit(i, n, d.token);
            const bool continued = (d.flags & Delim::MULTILINE) != 0
                                   || ((d.flags & Delim::RAW) == 0 && x.c.escape != 0 && x.s.back() == x.c.escape);
            if (continued)
            {
                x.st.mode = DELIM;
                x.st.delim = static_cast<std::uint8_t>(index);
                x.st.nesting = depth;
            }
            return npos;
        }

        // A raw string or comment from `i` (its text after the open) to `close`.
        std::size_t take_raw(Code& x, std::size_t begin, std::size_t i, std::string close, Token token)
        {
            const std::size_t end = x.s.find(close, i);
            if (end != npos)
            {
                x.emit(begin, end + close.size(), token);
                return end + close.size();
            }
            x.emit(begin, x.s.size(), token);
            x.st.mode = RAW;
            x.st.delim = static_cast<std::uint8_t>(token);
            x.st.close = std::move(close);
            return npos;
        }

        // A template literal's text from `i`: true when code follows (after its end or a ${), false when the line
        // ends inside it.
        bool scan_template(Code& x, std::size_t& i)
        {
            const std::string_view s = x.s;
            std::size_t j = i;
            while (j < s.size())
            {
                if (s[j] == '\\')
                {
                    j += 2;
                    continue;
                }
                if (s[j] == '`')
                {
                    x.emit(i, j + 1, Token::STRING);
                    x.st.mode = NONE;
                    i = j + 1;
                    return true;
                }
                if (s[j] == '$' && j + 1 < s.size() && s[j + 1] == '{')
                {
                    x.emit(i, j, Token::STRING);
                    x.emit(j, j + 2, Token::PUNCTUATION);
                    x.st.mode = NONE;
                    x.st.templates.push_back(1);
                    i = j + 2;
                    return true;
                }
                ++j;
            }
            x.emit(i, s.size(), Token::STRING);
            i = s.size();
            return false;
        }

        // Carries on what the line before left open: false when it takes the whole line.
        bool resume_code(Code& x, std::size_t& i)
        {
            State& st = x.st;
            const std::string_view s = x.s;
            switch (st.mode)
            {
            case DELIM:
            {
                if (st.delim >= x.c.delims.size())
                {
                    st = {};
                    return true;
                }
                const Delim& d = x.c.delims[st.delim];
                const std::size_t end = find_close(x, 0, d, st.nesting);
                if (end == npos)
                {
                    x.emit(0, s.size(), d.token);
                    const bool continued = (d.flags & Delim::MULTILINE) != 0
                                           || (x.c.escape != 0 && !s.empty() && s.back() == x.c.escape);
                    if (!continued)
                    {
                        st.mode = NONE;
                        st.nesting = 0;
                    }
                    return false;
                }
                x.emit(0, end, d.token);
                st.mode = NONE;
                st.nesting = 0;
                i = end;
                return true;
            }
            case RAW:
            {
                const auto token = static_cast<Token>(st.delim);
                const std::size_t end = s.find(st.close);
                if (end == npos)
                {
                    x.emit(0, s.size(), token);
                    return false;
                }
                i = end + st.close.size();
                x.emit(0, i, token);
                st.mode = NONE;
                st.close.clear();
                return true;
            }
            case HEREDOC_BODY:
            {
                const std::size_t k = st.depth != 0 ? x.first : 0;
                const std::size_t end = k + st.close.size();
                if (starts_at(s, k, st.close) && (end == s.size() || !x.ident_char(s[end])))
                {
                    x.emit(0, end, Token::STRING);
                    st.mode = NONE;
                    st.depth = 0;
                    st.close.clear();
                    i = end;
                    return true;
                }
                x.emit(0, s.size(), Token::STRING);
                return false;
            }
            case TEMPLATE_TEXT:
                return scan_template(x, i);
            default:
                return true;
            }
        }

        // <<EOF, <<-EOF, <<~'EOF', <<<"EOF": the index after the opener, or `i` when there is none.
        std::size_t open_heredoc(Code& x, std::size_t i)
        {
            const std::string_view s = x.s;
            const bool php = (x.c.hooks & PHP_TAGS) != 0;
            // Not the inside of a longer run (the shell's <<< here-string), nor a shift: $(( 1 << n )).
            if ((i > 0 && s[i - 1] == '<') || (!php && i + 2 < s.size() && s[i + 2] == '<'))
            {
                return i;
            }
            if ((x.c.hooks & HEREDOC_SPACED) != 0)
            {
                int arithmetic = 0;
                for (std::size_t k = 0; k + 1 < i; ++k)
                {
                    if (s[k] == s[k + 1] && (s[k] == '(' || s[k] == ')'))
                    {
                        arithmetic += s[k] == '(' ? 1 : -1;
                        ++k;
                    }
                }
                if (arithmetic > 0)
                {
                    return i;
                }
            }
            std::size_t j = i + 2;
            bool indented = false;
            if (php)
            {
                if (j >= s.size() || s[j] != '<')
                {
                    return i;
                }
                ++j;
                indented = true;
            }
            if (j < s.size() && (s[j] == '-' || s[j] == '~'))
            {
                indented = true;
                ++j;
            }
            if ((x.c.hooks & HEREDOC_SPACED) != 0)
            {
                j = skip_blanks(s, j);
            }
            const char quote = j < s.size() && (s[j] == '\'' || s[j] == '"') ? s[j] : 0;
            if (quote != 0)
            {
                ++j;
            }
            if (j >= s.size() || !(is_alpha(s[j]) || s[j] == '_'))
            {
                return i;
            }
            std::size_t k = j;
            while (k < s.size() && (is_alnum(s[k]) || s[k] == '_'))
            {
                ++k;
            }
            const std::string word(s.substr(j, k - j));
            if (quote != 0)
            {
                if (k >= s.size() || s[k] != quote)
                {
                    return i;
                }
                ++k;
            }
            x.emit(i, k, Token::STRING);
            x.heredoc = word;
            x.heredoc_indented = indented;
            return k;
        }

        std::size_t scan_number(const Code& x, std::size_t i)
        {
            const std::string_view s = x.s;
            const bool quotes = (x.c.hooks & DIGIT_QUOTES) != 0;
            std::size_t j = i;
            const bool radix = s[j] == '0' && j + 1 < s.size() && contains("xXbBoO", s[j + 1]);
            if (radix)
            {
                j += 2;
            }
            while (j < s.size())
            {
                const char ch = s[j];
                if (is_alnum(ch) || ch == '_')
                {
                    if (!radix && (ch == 'e' || ch == 'E') && j + 1 < s.size() && (s[j + 1] == '+' || s[j + 1] == '-'))
                    {
                        j += 2;
                        continue;
                    }
                    ++j;
                    continue;
                }
                if (ch == '.' && !radix && j + 1 < s.size() && is_digit(s[j + 1]))
                {
                    ++j;
                    continue;
                }
                if (quotes && ch == '\'' && j + 1 < s.size() && is_alnum(s[j + 1]))
                {
                    ++j;
                    continue;
                }
                break;
            }
            return j;
        }

        bool regex_allowed(const Code& x) noexcept
        {
            switch (x.prev)
            {
            case Token::TEXT:
                return x.prev_char == 0;
            case Token::OPERATOR:
            case Token::KEYWORD:
                return true;
            case Token::PUNCTUATION:
                return !contains(")]}", x.prev_char);
            default:
                return false;
            }
        }

        // /regex/flags from `i`: the index after it, or `i` when the line has no closing slash.
        std::size_t scan_regex(const Code& x, std::size_t i)
        {
            const std::string_view s = x.s;
            bool in_class = false;
            for (std::size_t j = i + 1; j < s.size(); ++j)
            {
                const char ch = s[j];
                if (ch == '\\')
                {
                    ++j;
                }
                else if (ch == '[')
                {
                    in_class = true;
                }
                else if (ch == ']')
                {
                    in_class = false;
                }
                else if (ch == '/' && !in_class)
                {
                    if (j == i + 1)
                    {
                        return i; // "//" is a comment, never an empty regex
                    }
                    ++j;
                    while (j < s.size() && is_alpha(s[j]))
                    {
                        ++j;
                    }
                    return j;
                }
            }
            return i;
        }

        // A sigil variable at `i` ($name, ${name}, @ivar, $1): the index after it, or `i`.
        std::size_t scan_variable(const Code& x, std::size_t i)
        {
            const std::string_view s = x.s;
            const char sigil = s[i];
            std::size_t j = i + 1;
            if (j >= s.size())
            {
                return i;
            }
            if (s[j] == sigil && sigil == '@')
            {
                ++j; // @@class_variable
            }
            if (j < s.size() && s[j] == '{')
            {
                const std::size_t close = s.find('}', j);
                return close == npos ? s.size() : close + 1;
            }
            if (sigil == '$' && j < s.size() && contains("#?@*!$-0123456789_", s[j]) && !x.ident_start(s[j]))
            {
                return j + 1;
            }
            if (j >= s.size() || !x.ident_start(s[j]))
            {
                return i;
            }
            j = x.scan_ident(j);
            // $env:PATH, $script:name
            if (j + 1 < s.size() && s[j] == ':' && s[j + 1] != ':' && x.ident_start(s[j + 1]))
            {
                j = x.scan_ident(j + 1);
            }
            return j;
        }

        // Batch variables: %name%, %%i, %~dp0, %1, !name!.
        std::size_t scan_percent_variable(const Code& x, std::size_t i)
        {
            const std::string_view s = x.s;
            const char ch = s[i];
            std::size_t j = i + 1;
            if (j >= s.size())
            {
                return i;
            }
            if (ch == '%' && s[j] == '%')
            {
                return j + 1 < s.size() && x.ident_start(s[j + 1]) ? j + 2 : i;
            }
            if (ch == '%' && (is_digit(s[j]) || s[j] == '~' || s[j] == '*'))
            {
                while (j < s.size() && (is_alpha(s[j]) || s[j] == '~'))
                {
                    ++j;
                }
                return j < s.size() && (is_digit(s[j]) || s[j] == '*') ? j + 1 : j;
            }
            const std::size_t close = s.find(ch, j);
            if (close == npos || close == j)
            {
                return i;
            }
            for (std::size_t k = j; k < close; ++k)
            {
                if (is_blank(s[k]))
                {
                    return i;
                }
            }
            return close + 1;
        }

        Token classify(Code& x, std::size_t begin, std::size_t end)
        {
            const std::string_view word = x.s.substr(begin, end - begin);
            const Language& l = x.lang;
            if (l.keywords.contains(word))
            {
                return Token::KEYWORD;
            }
            if (l.types.contains(word))
            {
                return Token::TYPE;
            }
            if (l.constants.contains(word))
            {
                return Token::CONSTANT;
            }
            if (l.builtins.contains(word))
            {
                return Token::FUNCTION;
            }
            if (x.prev == Token::KEYWORD && !x.prev_word.empty())
            {
                if (one_of(x.prev_word, DEF_WORDS, x.c.fold_case))
                {
                    return Token::FUNCTION;
                }
                if (one_of(x.prev_word, TYPE_DEF_WORDS, x.c.fold_case))
                {
                    return Token::TYPE;
                }
            }
            if (x.c.line_head_tag && begin == x.first)
            {
                return Token::TAG;
            }
            const std::size_t next = skip_blanks(x.s, end);
            if (next < x.s.size() && x.s[next] == '(')
            {
                return Token::FUNCTION;
            }
            // PowerShell's Verb-Noun cmdlets.
            const std::size_t dash = word.find('-');
            if (dash != npos && dash > 0 && dash + 1 < word.size() && word[0] >= 'A' && word[0] <= 'Z'
                && word[dash + 1] >= 'A' && word[dash + 1] <= 'Z')
            {
                return Token::FUNCTION;
            }
            return Token::TEXT;
        }

        bool lisp_symbol_char(char ch) noexcept
        {
            return !is_blank(ch) && !contains("()[]{}\";,'`", ch);
        }

        void tokenize_code(const Language& lang, std::string_view s, State& st, Out& out)
        {
            Code x(lang, s, st, out);
            const CodeSpec& c = lang.code;
            std::size_t i = 0;
            if (!resume_code(x, i))
            {
                return;
            }
            const std::size_t n = s.size();
            while (i < n)
            {
                const char ch = s[i];
                if (is_blank(ch))
                {
                    ++i;
                    continue;
                }
                // The braces of the innermost template literal's ${ }: its last } goes back to its text.
                if (!st.templates.empty() && (ch == '{' || ch == '}'))
                {
                    x.emit(i, i + 1, Token::PUNCTUATION);
                    ++i;
                    const int braces = static_cast<unsigned char>(st.templates.back());
                    if (ch == '{' || braces > 1)
                    {
                        st.templates.back() = static_cast<char>(ch == '{' ? std::min(braces + 1, 255) : braces - 1);
                        continue;
                    }
                    st.templates.pop_back();
                    st.mode = TEMPLATE_TEXT;
                    if (!scan_template(x, i))
                    {
                        return;
                    }
                    continue;
                }
                // Hooks whose openers the tables would take for something else.
                if ((c.hooks & LUA_LONG) != 0 && (ch == '[' || starts_at(s, i, "--[")))
                {
                    const bool comment = ch == '-';
                    std::size_t j = i + (comment ? 3 : 1);
                    std::size_t level = 0;
                    while (j < n && s[j] == '=')
                    {
                        ++level;
                        ++j;
                    }
                    if (j < n && s[j] == '[')
                    {
                        const std::size_t end = take_raw(x, i, j + 1, "]" + std::string(level, '=') + "]",
                                                         comment ? Token::COMMENT : Token::STRING);
                        if (end == npos)
                        {
                            return;
                        }
                        i = end;
                        continue;
                    }
                }
                if ((c.hooks & PHP_TAGS) != 0 && (starts_at(s, i, "<?") || starts_at(s, i, "?>")))
                {
                    std::size_t j = i + 2;
                    if (ch == '<')
                    {
                        if (starts_at_folded(s, j, "php"))
                        {
                            j += 3;
                        }
                        else if (j < n && s[j] == '=')
                        {
                            ++j;
                        }
                    }
                    x.emit(i, j, Token::KEYWORD);
                    i = j;
                    continue;
                }
                if ((c.hooks & PREPROCESSOR) != 0 && ch == '#' && i == x.first)
                {
                    std::size_t j = skip_blanks(s, i + 1);
                    const std::size_t word_end = j < n && x.ident_start(s[j]) ? x.scan_ident(j) : j;
                    const std::string_view directive = s.substr(j, word_end - j);
                    x.emit(i, word_end, Token::KEYWORD);
                    i = word_end;
                    if (directive == "include" || directive == "import")
                    {
                        j = skip_blanks(s, i);
                        if (j < n && s[j] == '<')
                        {
                            const std::size_t close = s.find('>', j);
                            const std::size_t end = close == npos ? n : close + 1;
                            x.emit(j, end, Token::STRING);
                            i = end;
                        }
                    }
                    continue;
                }
                if ((c.hooks & TEMPLATE) != 0 && ch == '`')
                {
                    ++i;
                    x.out.add(i - 1, i, Token::STRING);
                    st.mode = TEMPLATE_TEXT;
                    std::size_t j = i;
                    if (!scan_template(x, j))
                    {
                        return;
                    }
                    i = j;
                    continue;
                }
                if ((c.hooks & HEREDOC) != 0 && starts_at(s, i, "<<"))
                {
                    const std::size_t end = open_heredoc(x, i);
                    if (end != i)
                    {
                        i = end;
                        continue;
                    }
                }
                if ((c.hooks & PERCENT_VARS) != 0)
                {
                    if (ch == '%' || ch == '!')
                    {
                        const std::size_t end = scan_percent_variable(x, i);
                        if (end != i)
                        {
                            x.emit(i, end, Token::VARIABLE);
                            i = end;
                            continue;
                        }
                    }
                    if (ch == ':' && i == x.first && i + 1 < n && x.ident_start(s[i + 1]))
                    {
                        const std::size_t end = x.scan_ident(i + 1);
                        x.emit(i, end, Token::FUNCTION); // a label
                        i = end;
                        continue;
                    }
                }
                // Comments and strings from the tables.
                if (const int index = find_delim(x, i); index >= 0)
                {
                    const std::size_t end = take_delim(x, i, index);
                    if (end == npos)
                    {
                        break;
                    }
                    i = end;
                    continue;
                }
                if ((c.hooks & REGEX) != 0 && ch == '/' && regex_allowed(x))
                {
                    const std::size_t end = scan_regex(x, i);
                    if (end != i)
                    {
                        x.emit(i, end, Token::REGEXP);
                        i = end;
                        continue;
                    }
                }
                if ((c.hooks & RUST) != 0 && ch == '\'' && i + 1 < n && x.ident_start(s[i + 1]))
                {
                    const std::size_t end = x.scan_ident(i + 1); // a 'lifetime (a 'c' char was taken above)
                    x.emit(i, end, Token::TYPE);
                    i = end;
                    continue;
                }
                if (is_digit(ch)
                    || ((ch == '.' || (c.lisp && (ch == '-' || ch == '+'))) && i + 1 < n && is_digit(s[i + 1])
                        && (i == 0 || !x.ident_char(s[i - 1]))))
                {
                    const std::size_t end = scan_number(x, i + (is_digit(ch) ? 0 : 1));
                    x.emit(i, end, Token::NUMBER);
                    i = end;
                    continue;
                }
                if (contains(c.sigils, ch))
                {
                    const std::size_t end = scan_variable(x, i);
                    if (end != i)
                    {
                        const bool constant = lang.constants.contains(s.substr(i + 1, end - i - 1));
                        x.emit(i, end, constant ? Token::CONSTANT : Token::VARIABLE);
                        i = end;
                        continue;
                    }
                }
                if (ch == c.annotation && i + 1 < n && x.ident_start(s[i + 1]))
                {
                    std::size_t end = x.scan_ident(i + 1);
                    while (end + 1 < n && s[end] == '.' && x.ident_start(s[end + 1]))
                    {
                        end = x.scan_ident(end + 1);
                    }
                    x.emit(i, end, Token::FUNCTION);
                    i = end;
                    continue;
                }
                if (ch == c.symbol && !c.lisp && i + 1 < n && x.ident_start(s[i + 1])
                    && (i == 0 || (!x.ident_char(s[i - 1]) && s[i - 1] != ':')))
                {
                    const std::size_t end = x.scan_ident(i + 1);
                    x.emit(i, end, Token::CONSTANT);
                    i = end;
                    continue;
                }
                if (c.lisp && lisp_symbol_char(ch))
                {
                    std::size_t end = i;
                    while (end < n && lisp_symbol_char(s[end]))
                    {
                        ++end;
                    }
                    const std::string_view word = s.substr(i, end - i);
                    Token token = Token::TEXT;
                    if (word[0] == ':' && word.size() > 1)
                    {
                        token = Token::CONSTANT;
                    }
                    else if (lang.keywords.contains(word))
                    {
                        token = Token::KEYWORD;
                    }
                    else if (lang.constants.contains(word))
                    {
                        token = Token::CONSTANT;
                    }
                    else if (lang.types.contains(word))
                    {
                        token = Token::TYPE;
                    }
                    else if (lang.builtins.contains(word) || x.prev_char == '(')
                    {
                        token = Token::FUNCTION;
                    }
                    x.emit(i, end, token);
                    i = end;
                    continue;
                }
                if (x.ident_start(ch))
                {
                    const std::size_t end = x.scan_ident(i);
                    const std::string_view word = s.substr(i, end - i);
                    // A string prefix: r"..", b'..', f"..", u8"..", R"(..)", r#".."#.
                    if (end < n && !c.string_prefixes.empty() && word.size() <= 3
                        && std::all_of(word.begin(), word.end(),
                                       [&](char p)
                                       {
                                           return contains(c.string_prefixes, p);
                                       }))
                    {
                        if ((c.hooks & CPP_RAW) != 0 && word.back() == 'R' && s[end] == '"')
                        {
                            const std::size_t paren = s.find('(', end + 1);
                            if (paren != npos && paren - end - 1 <= 16)
                            {
                                const std::string close = ")" + std::string(s.substr(end + 1, paren - end - 1)) + "\"";
                                const std::size_t after = take_raw(x, i, paren + 1, close, Token::STRING);
                                if (after == npos)
                                {
                                    return;
                                }
                                i = after;
                                continue;
                            }
                        }
                        if ((c.hooks & RUST) != 0 && word.back() == 'r' && (s[end] == '"' || s[end] == '#'))
                        {
                            std::size_t j = end;
                            while (j < n && s[j] == '#')
                            {
                                ++j;
                            }
                            if (j < n && s[j] == '"')
                            {
                                const std::string close = "\"" + std::string(j - end, '#');
                                const std::size_t after = take_raw(x, i, j + 1, close, Token::STRING);
                                if (after == npos)
                                {
                                    return;
                                }
                                i = after;
                                continue;
                            }
                        }
                        if (const int index = find_delim(x, end);
                            index >= 0 && x.c.delims[static_cast<std::size_t>(index)].token == Token::STRING)
                        {
                            x.emit(i, end, Token::STRING);
                            const std::size_t after = take_delim(x, end, index);
                            if (after == npos)
                            {
                                break;
                            }
                            i = after;
                            continue;
                        }
                    }
                    const Token token = classify(x, i, end);
                    x.emit(i, end, token);
                    if (token == Token::KEYWORD)
                    {
                        x.prev_word = word;
                    }
                    i = end;
                    continue;
                }
                if (contains("()[]{},;.", ch))
                {
                    x.emit(i, i + 1, Token::PUNCTUATION);
                }
                else if (contains("+-*/%=<>!&|^~?:@#\\", ch))
                {
                    x.emit(i, i + 1, Token::OPERATOR);
                }
                else
                {
                    x.emit(i, i + 1, Token::TEXT);
                }
                ++i;
            }
            if (!x.heredoc.empty() && st.mode == NONE)
            {
                st.mode = HEREDOC_BODY;
                st.close = std::move(x.heredoc);
                st.depth = x.heredoc_indented ? 1 : 0;
            }
        }

        // ---- markup ----

        const Language* javascript()
        {
            static const Language* language = language_by_name("javascript");
            return language;
        }

        const Language* css()
        {
            static const Language* language = language_by_name("css");
            return language;
        }

        const Language* html()
        {
            static const Language* language = language_by_name("html");
            return language;
        }

        bool tag_name_start(char ch) noexcept
        {
            return is_alpha(ch) || ch == '_' || ch == ':' || ch == '#' || ch == '@' || is_utf8(ch);
        }

        std::size_t scan_tag_name(std::string_view s, std::size_t i) noexcept
        {
            while (i < s.size() && (is_alnum(s[i]) || is_utf8(s[i]) || contains("_:.-#@", s[i])))
            {
                ++i;
            }
            return i;
        }

        // A tag's attributes from `i`, up to its > (the index after it) or the end of the line.
        std::size_t tag_body(const Language& lang, std::string_view s, std::size_t i, State& st, Out& out)
        {
            bool value = false; // after an =
            while (i < s.size())
            {
                const char ch = s[i];
                if (is_blank(ch))
                {
                    ++i;
                    continue;
                }
                if (ch == '>' || ((ch == '/' || ch == '?' || ch == '%') && i + 1 < s.size() && s[i + 1] == '>'))
                {
                    const std::size_t end = i + (ch == '>' ? 1 : 2);
                    out.add(i, end, Token::PUNCTUATION);
                    if (lang.markup.html && ch == '>' && st.depth != 0)
                    {
                        st.embedded = st.depth == 1 ? javascript() : css();
                        st.end_marker = st.depth == 1 ? "</script" : "</style";
                    }
                    st.mode = NONE;
                    st.depth = 0;
                    return end;
                }
                if (ch == '=')
                {
                    out.add(i, i + 1, Token::PUNCTUATION);
                    value = true;
                    ++i;
                    continue;
                }
                if (ch == '"' || ch == '\'')
                {
                    const std::size_t close = s.find(ch, i + 1);
                    if (close == npos)
                    {
                        out.add(i, s.size(), Token::STRING);
                        st.mode = ATTR_STRING;
                        st.close = std::string(1, ch);
                        return s.size();
                    }
                    out.add(i, close + 1, Token::STRING);
                    value = false;
                    i = close + 1;
                    continue;
                }
                std::size_t end = i;
                while (end < s.size() && !is_blank(s[end]) && !contains("=\"'>", s[end])
                       && !((s[end] == '/' || s[end] == '?') && end + 1 < s.size() && s[end + 1] == '>'))
                {
                    ++end;
                }
                if (end == i)
                {
                    end = i + 1;
                }
                out.add(i, end, value ? Token::STRING : Token::ATTRIBUTE);
                value = false;
                i = end;
            }
            return i;
        }

        // A template tag opening at `i` (index `t`), or carried on from the line before (`i` after its open): the
        // index after its close, or the end of the line with the tag left open.
        std::size_t template_body(const Language& lang, std::string_view s, std::size_t i, std::size_t t, State& st,
                                  Out& out)
        {
            const TemplateTag& tag = lang.markup.templates[t];
            const std::size_t close = s.find(tag.close, i);
            const std::size_t stop = close == npos ? s.size() : close;
            if (tag.token == Token::COMMENT)
            {
                out.add(i, stop, Token::COMMENT);
            }
            else
            {
                tokenize_piece(lang.template_languages[t], s, i, stop, out);
            }
            if (close == npos)
            {
                st.mode = TEMPLATE_TAG;
                st.delim = static_cast<std::uint8_t>(t);
                return s.size();
            }
            out.add(close, close + tag.close.size(), tag.token == Token::COMMENT ? Token::COMMENT : Token::PUNCTUATION);
            st.mode = NONE;
            return close + tag.close.size();
        }

        // Text up to the next tag, comment or template tag, which it opens: the index after the opener.
        std::size_t markup_text(const Language& lang, std::string_view s, std::size_t i, State& st, Out& out)
        {
            const MarkupSpec& m = lang.markup;
            for (std::size_t j = i; j < s.size(); ++j)
            {
                for (std::size_t t = 0; t < m.templates.size(); ++t)
                {
                    const TemplateTag& tag = m.templates[t];
                    if (!starts_at(s, j, tag.open))
                    {
                        continue;
                    }
                    const std::size_t inside = j + tag.open.size();
                    if (tag.close.empty())
                    {
                        // Razor's @name.name: the code language's words.
                        std::size_t end = inside;
                        while (end < s.size() && (is_alnum(s[end]) || s[end] == '_' || s[end] == '.'))
                        {
                            ++end;
                        }
                        if (end == inside || !is_alpha(s[inside]))
                        {
                            continue;
                        }
                        out.add(j, inside, Token::PUNCTUATION);
                        tokenize_piece(lang.template_languages[t], s, inside, end, out, Token::VARIABLE);
                        return end;
                    }
                    out.add(j, inside, tag.token == Token::COMMENT ? Token::COMMENT : Token::PUNCTUATION);
                    return template_body(lang, s, inside, t, st, out);
                }
                const char ch = s[j];
                if (ch == '&')
                {
                    std::size_t k = j + 1;
                    while (k < s.size() && k < j + 12 && (is_alnum(s[k]) || s[k] == '#'))
                    {
                        ++k;
                    }
                    if (k > j + 1 && k < s.size() && s[k] == ';')
                    {
                        out.add(j, k + 1, Token::CONSTANT);
                        j = k;
                    }
                    continue;
                }
                if (ch != '<' || j + 1 >= s.size())
                {
                    continue;
                }
                if (starts_at(s, j, "<!--"))
                {
                    out.add(j, j + 4, Token::COMMENT);
                    st.mode = MARKUP_COMMENT;
                    return j + 4;
                }
                if (starts_at(s, j, "<![CDATA["))
                {
                    out.add(j, j + 9, Token::PUNCTUATION);
                    st.mode = CDATA;
                    return j + 9;
                }
                const char next = s[j + 1];
                const bool closing = next == '/';
                const bool special = next == '!' || next == '?';
                const std::size_t name = j + (closing || special ? 2 : 1);
                if (!(closing || special) && !tag_name_start(next))
                {
                    continue;
                }
                const std::size_t name_end = scan_tag_name(s, name);
                out.add(j, name, Token::PUNCTUATION);
                out.add(name, name_end, Token::TAG);
                st.mode = TAG;
                st.depth = 0;
                if (m.html && !closing && !special)
                {
                    const std::string_view tag = s.substr(name, name_end - name);
                    st.depth = starts_at_folded(tag, 0, "script") && tag.size() == 6  ? 1
                               : starts_at_folded(tag, 0, "style") && tag.size() == 5 ? 2
                                                                                      : 0;
                }
                return name_end;
            }
            return s.size();
        }

        void tokenize_markup(const Language& lang, std::string_view s, State& st, Out& out)
        {
            std::size_t i = 0;
            const std::size_t n = s.size();
            while (i < n)
            {
                if (!st.end_marker.empty())
                {
                    // A <script> / <style> body up to its closing tag.
                    const std::size_t end =
                            lang.markup.html ? find_folded(s, st.end_marker, i) : s.find(st.end_marker, i);
                    const std::size_t stop = end == npos ? n : end;
                    if (st.embedded != nullptr)
                    {
                        tokenize_embedded(*st.embedded, s, i, stop, st, out);
                    }
                    if (end == npos)
                    {
                        return;
                    }
                    leave_embedded(st);
                    i = end;
                    continue;
                }
                switch (st.mode)
                {
                case MARKUP_COMMENT:
                case CDATA:
                {
                    const std::string_view close = st.mode == CDATA ? "]]>" : "-->";
                    const Token token = st.mode == CDATA ? Token::STRING : Token::COMMENT;
                    const std::size_t end = s.find(close, i);
                    if (end == npos)
                    {
                        out.add(i, n, token);
                        return;
                    }
                    out.add(i, end, token);
                    out.add(end, end + close.size(), st.mode == CDATA ? Token::PUNCTUATION : Token::COMMENT);
                    st.mode = NONE;
                    i = end + close.size();
                    continue;
                }
                case TAG:
                    i = tag_body(lang, s, i, st, out);
                    continue;
                case ATTR_STRING:
                {
                    const std::size_t end = s.find(st.close, i);
                    if (end == npos)
                    {
                        out.add(i, n, Token::STRING);
                        return;
                    }
                    out.add(i, end + 1, Token::STRING);
                    st.mode = TAG;
                    st.close.clear();
                    i = end + 1;
                    continue;
                }
                case TEMPLATE_TAG:
                    if (st.delim >= lang.markup.templates.size())
                    {
                        st.mode = NONE;
                        continue;
                    }
                    i = template_body(lang, s, i, st.delim, st, out);
                    continue;
                default:
                    st.mode = NONE;
                    i = markup_text(lang, s, i, st, out);
                    continue;
                }
            }
        }

        // ---- CSS, SCSS, Less ----

        void tokenize_css(const Language& lang, std::string_view s, State& st, Out& out)
        {
            Code x(lang, s, st, out);
            std::size_t i = 0;
            if (!resume_code(x, i))
            {
                return;
            }
            const bool less = contains(lang.code.sigils, '@');
            const auto css_ident_start = [&](std::size_t k)
            {
                return k < s.size()
                       && (is_alpha(s[k]) || s[k] == '_' || is_utf8(s[k])
                           || (s[k] == '-' && k + 1 < s.size() && (is_alpha(s[k + 1]) || s[k + 1] == '-')));
            };
            const auto css_ident = [&](std::size_t k)
            {
                while (k < s.size() && (is_alnum(s[k]) || s[k] == '_' || s[k] == '-' || is_utf8(s[k])))
                {
                    ++k;
                }
                return k;
            };
            enum class Context
            {
                STATEMENT,
                SELECTOR,
                PROPERTY,
                VALUE,
                AT_RULE,
            };
            Context context = Context::STATEMENT;
            const std::size_t n = s.size();
            while (i < n)
            {
                const char ch = s[i];
                if (is_blank(ch))
                {
                    ++i;
                    continue;
                }
                if (const int index = find_delim(x, i); index >= 0)
                {
                    const std::size_t end = take_delim(x, i, index);
                    if (end == npos)
                    {
                        return;
                    }
                    i = end;
                    continue;
                }
                if (context == Context::STATEMENT)
                {
                    if (ch == '@' && css_ident_start(i + 1))
                    {
                        const std::size_t end = css_ident(i + 1);
                        const bool rule = !less || lang.keywords.contains(s.substr(i + 1, end - i - 1));
                        out.add(i, end, rule ? Token::KEYWORD : Token::VARIABLE);
                        context = rule ? Context::AT_RULE : Context::PROPERTY;
                        i = end;
                        continue;
                    }
                    const std::size_t stop = s.find_first_of("{;}", i);
                    context = stop != npos && s[stop] == '{'                     ? Context::SELECTOR
                              : st.depth > 0 || (stop != npos && s[stop] == ';') ? Context::PROPERTY
                                                                                 : Context::SELECTOR;
                }
                if (ch == '{' || ch == '}' || ch == ';')
                {
                    out.add(i, i + 1, Token::PUNCTUATION);
                    if (ch == '{')
                    {
                        st.depth = static_cast<std::uint8_t>(std::min(st.depth + 1, 255));
                    }
                    else if (ch == '}' && st.depth > 0)
                    {
                        --st.depth;
                    }
                    context = Context::STATEMENT;
                    ++i;
                    continue;
                }
                if (ch == '#' && i + 1 < n && s[i + 1] == '{')
                {
                    const std::size_t close = s.find('}', i);
                    const std::size_t end = close == npos ? n : close + 1;
                    out.add(i, end, Token::VARIABLE); // #{$interpolation}
                    i = end;
                    continue;
                }
                if (contains(lang.code.sigils, ch) && css_ident_start(i + 1))
                {
                    const std::size_t end = css_ident(i + 1);
                    out.add(i, end, Token::VARIABLE);
                    i = end;
                    continue;
                }
                const bool number_start = is_digit(ch)
                                          || ((ch == '.' || ch == '-' || ch == '+') && i + 1 < n && is_digit(s[i + 1])
                                              && context != Context::SELECTOR);
                if (number_start)
                {
                    std::size_t end = i + 1;
                    while (end < n && (is_alnum(s[end]) || s[end] == '.' || s[end] == '%'))
                    {
                        ++end;
                    }
                    out.add(i, end, Token::NUMBER);
                    i = end;
                    continue;
                }
                switch (context)
                {
                case Context::SELECTOR:
                {
                    std::size_t begin = i;
                    std::size_t k = i;
                    while (k < n && (s[k] == '.' || s[k] == '#' || s[k] == ':' || s[k] == '%'))
                    {
                        ++k;
                    }
                    if (css_ident_start(k))
                    {
                        const std::size_t end = css_ident(k);
                        out.add(begin, end, Token::TAG);
                        i = end;
                        continue;
                    }
                    if (ch == '*')
                    {
                        out.add(i, i + 1, Token::TAG);
                    }
                    else if (ch == '[')
                    {
                        // [attribute=value]
                        const std::size_t close = s.find(']', i);
                        const std::size_t end = close == npos ? n : close;
                        const std::size_t name_end = std::min(css_ident(i + 1), end);
                        out.add(i, i + 1, Token::PUNCTUATION);
                        out.add(i + 1, name_end, Token::ATTRIBUTE);
                        out.add(name_end, end, Token::STRING);
                        out.add(end, std::min(end + 1, n), Token::PUNCTUATION);
                        i = std::min(end + 1, n);
                        continue;
                    }
                    else
                    {
                        out.add(i, i + 1, contains(",()", ch) ? Token::PUNCTUATION : Token::OPERATOR);
                    }
                    ++i;
                    continue;
                }
                case Context::PROPERTY:
                    if (css_ident_start(i) || ch == '*')
                    {
                        const std::size_t end = css_ident(i + 1);
                        out.add(i, end, Token::ATTRIBUTE);
                        i = end;
                        continue;
                    }
                    out.add(i, i + 1, Token::PUNCTUATION);
                    if (ch == ':')
                    {
                        context = Context::VALUE;
                    }
                    ++i;
                    continue;
                case Context::VALUE:
                case Context::AT_RULE:
                    if (ch == '#' && i + 1 < n && is_alnum(s[i + 1]))
                    {
                        std::size_t end = i + 1;
                        while (end < n && is_alnum(s[end]))
                        {
                            ++end;
                        }
                        out.add(i, end, Token::NUMBER); // #rrggbb
                        i = end;
                        continue;
                    }
                    if (ch == '!' && i + 1 < n && is_alpha(s[i + 1]))
                    {
                        const std::size_t end = css_ident(i + 1);
                        out.add(i, end, Token::KEYWORD); // !important
                        i = end;
                        continue;
                    }
                    if (css_ident_start(i))
                    {
                        const std::size_t end = css_ident(i);
                        const std::string_view word = s.substr(i, end - i);
                        if (end < n && s[end] == '(')
                        {
                            out.add(i, end, Token::FUNCTION);
                            if (word == "url")
                            {
                                const std::size_t close = s.find(')', end);
                                const std::size_t stop = close == npos ? n : close;
                                out.add(end, end + 1, Token::PUNCTUATION);
                                out.add(end + 1, stop, Token::STRING);
                                i = stop;
                                continue;
                            }
                        }
                        else if (context == Context::AT_RULE)
                        {
                            const bool media = word == "and" || word == "not" || word == "only" || word == "or";
                            const std::size_t next = skip_blanks(s, end);
                            out.add(i, end,
                                    media                        ? Token::KEYWORD
                                    : next < n && s[next] == ':' ? Token::ATTRIBUTE
                                                                 : Token::TEXT);
                        }
                        else
                        {
                            out.add(i, end, Token::STRING);
                        }
                        i = end;
                        continue;
                    }
                    out.add(i, i + 1, contains(",()[]:", ch) ? Token::PUNCTUATION : Token::OPERATOR);
                    ++i;
                    continue;
                default:
                    ++i;
                    continue;
                }
            }
        }

        // ---- JSON ----

        void tokenize_json(const Language& lang, std::string_view s, State& st, Out& out)
        {
            Code x(lang, s, st, out);
            std::size_t i = 0;
            if (!resume_code(x, i))
            {
                return;
            }
            const std::size_t n = s.size();
            while (i < n)
            {
                const char ch = s[i];
                if (is_blank(ch))
                {
                    ++i;
                    continue;
                }
                if (ch == '"')
                {
                    std::size_t j = i + 1;
                    while (j < n && s[j] != '"')
                    {
                        j += s[j] == '\\' ? 2 : 1;
                    }
                    const std::size_t end = std::min(j + 1, n);
                    const std::size_t next = skip_blanks(s, end);
                    out.add(i, end, next < n && s[next] == ':' ? Token::ATTRIBUTE : Token::STRING);
                    i = end;
                    continue;
                }
                if (const int index = find_delim(x, i); index >= 0)
                {
                    const std::size_t end = take_delim(x, i, index);
                    if (end == npos)
                    {
                        return;
                    }
                    i = end;
                    continue;
                }
                if (is_digit(ch) || (ch == '-' && i + 1 < n && is_digit(s[i + 1])))
                {
                    const std::size_t end = scan_number(x, i + (ch == '-' ? 1 : 0));
                    out.add(i, end, Token::NUMBER);
                    i = end;
                    continue;
                }
                if (is_alpha(ch))
                {
                    std::size_t end = i;
                    while (end < n && is_alnum(s[end]))
                    {
                        ++end;
                    }
                    out.add(i, end, lang.constants.contains(s.substr(i, end - i)) ? Token::CONSTANT : Token::TEXT);
                    i = end;
                    continue;
                }
                out.add(i, i + 1, contains("{}[],:", ch) ? Token::PUNCTUATION : Token::TEXT);
                ++i;
            }
        }

        // ---- YAML ----

        // A scalar from `i` to the end of the line (or a comment): a number, a constant or a string.
        void yaml_scalar(const Language& lang, std::string_view s, std::size_t i, Out& out)
        {
            std::size_t end = s.size();
            for (std::size_t k = i; k < s.size(); ++k)
            {
                if (s[k] == '#' && k > i && is_blank(s[k - 1]))
                {
                    end = k;
                    break;
                }
            }
            std::size_t last = end;
            while (last > i && is_blank(s[last - 1]))
            {
                --last;
            }
            const std::string_view value = s.substr(i, last - i);
            const bool number = !value.empty()
                                && std::all_of(value.begin(), value.end(),
                                               [](char c)
                                               {
                                                   return is_alnum(c) || c == '.' || c == '-' || c == '+' || c == '_';
                                               })
                                && (is_digit(value[0])
                                    || (value.size() > 1 && (value[0] == '-' || value[0] == '+' || value[0] == '.')
                                        && is_digit(value[1])));
            const bool constant = value == "~" || lang.constants.contains(value);
            out.add(i, last, number ? Token::NUMBER : constant ? Token::CONSTANT : Token::STRING);
            if (end < s.size())
            {
                out.add(end, s.size(), Token::COMMENT);
            }
        }

        void tokenize_yaml(const Language& lang, std::string_view s, State& st, Out& out)
        {
            const std::size_t n = s.size();
            const std::size_t indent = skip_blanks(s, 0);
            // A block scalar's lines are those indented past its key (past the dash of a "- |" item).
            std::size_t parent = indent;
            std::size_t i = 0;
            if (st.mode == BLOCK_SCALAR)
            {
                if (indent == n || indent > st.depth)
                {
                    out.add(0, n, Token::STRING);
                    return;
                }
                st.mode = NONE;
            }
            if (st.mode == RAW)
            {
                const std::size_t end = s.find(st.close);
                if (end == npos)
                {
                    out.add(0, n, Token::STRING);
                    return;
                }
                out.add(0, end + 1, Token::STRING);
                st.mode = NONE;
                st.close.clear();
                i = end + 1;
            }
            else
            {
                i = indent;
                if (i < n && s[i] == '#')
                {
                    out.add(i, n, Token::COMMENT);
                    return;
                }
                if (i == 0 && (starts_at(s, 0, "---") || starts_at(s, 0, "...") || starts_at(s, 0, "%")))
                {
                    const std::size_t end = s[0] == '%' ? n : 3;
                    out.add(0, end, s[0] == '%' ? Token::KEYWORD : Token::PUNCTUATION);
                    i = end;
                }
                // List markers and complex keys.
                while (i < n && (s[i] == '-' || s[i] == '?') && (i + 1 == n || is_blank(s[i + 1])))
                {
                    out.add(i, i + 1, Token::OPERATOR);
                    i = skip_blanks(s, i + 1);
                }
                // key:
                if (i < n && s[i] != '#' && s[i] != '[' && s[i] != '{')
                {
                    std::size_t colon = npos;
                    char quote = 0;
                    for (std::size_t k = i; k < n; ++k)
                    {
                        if (quote != 0)
                        {
                            quote = s[k] == quote ? 0 : quote;
                            continue;
                        }
                        if ((s[k] == '"' || s[k] == '\'') && k == i)
                        {
                            quote = s[k];
                            continue;
                        }
                        if (s[k] == '#' && k > i && is_blank(s[k - 1]))
                        {
                            break;
                        }
                        if (s[k] == ':' && (k + 1 == n || is_blank(s[k + 1])))
                        {
                            colon = k;
                            break;
                        }
                    }
                    if (colon != npos)
                    {
                        parent = i;
                        out.add(i, colon, Token::TYPE);
                        out.add(colon, colon + 1, Token::OPERATOR);
                        i = colon + 1;
                    }
                }
            }
            // The value.
            while (i < n)
            {
                const char ch = s[i];
                if (is_blank(ch))
                {
                    ++i;
                    continue;
                }
                if (ch == '#')
                {
                    out.add(i, n, Token::COMMENT);
                    return;
                }
                if ((ch == '|' || ch == '>') && (i == 0 || is_blank(s[i - 1])))
                {
                    std::size_t end = i + 1;
                    while (end < n && (is_digit(s[end]) || s[end] == '-' || s[end] == '+'))
                    {
                        ++end;
                    }
                    const std::size_t rest = skip_blanks(s, end);
                    if (rest == n || s[rest] == '#')
                    {
                        out.add(i, end, Token::OPERATOR);
                        if (rest < n)
                        {
                            out.add(rest, n, Token::COMMENT);
                        }
                        st.mode = BLOCK_SCALAR;
                        st.depth = static_cast<std::uint8_t>(std::min<std::size_t>(parent, 255));
                        return;
                    }
                }
                if (ch == '&' || ch == '*' || ch == '!')
                {
                    std::size_t end = i + 1;
                    while (end < n && !is_blank(s[end]) && !contains(",[]{}", s[end]))
                    {
                        ++end;
                    }
                    out.add(i, end, ch == '!' ? Token::TAG : Token::VARIABLE);
                    i = end;
                    continue;
                }
                if (ch == '"' || ch == '\'')
                {
                    std::size_t j = i + 1;
                    while (j < n && s[j] != ch)
                    {
                        j += ch == '"' && s[j] == '\\' ? 2 : 1;
                    }
                    if (j >= n)
                    {
                        out.add(i, n, Token::STRING);
                        st.mode = RAW;
                        st.close = std::string(1, ch);
                        return;
                    }
                    out.add(i, j + 1, Token::STRING);
                    i = j + 1;
                    continue;
                }
                if (contains("[]{},", ch))
                {
                    out.add(i, i + 1, Token::PUNCTUATION);
                    ++i;
                    continue;
                }
                // A plain scalar, up to a flow delimiter.
                std::size_t end = i;
                while (end < n && !contains("[]{},", s[end]) && !(s[end] == '#' && is_blank(s[end - 1])))
                {
                    ++end;
                }
                if (end == n || s[end] == '#')
                {
                    yaml_scalar(lang, s, i, out);
                    return;
                }
                yaml_scalar(lang, s.substr(0, end), i, out);
                i = end;
            }
        }

        // ---- INI, properties, .gitconfig ----

        void tokenize_ini(const Language& lang, std::string_view s, Out& out)
        {
            const std::size_t n = s.size();
            std::size_t i = skip_blanks(s, 0);
            if (i == n)
            {
                return;
            }
            if (s[i] == ';' || s[i] == '#')
            {
                out.add(i, n, Token::COMMENT);
                return;
            }
            if (s[i] == '[')
            {
                const std::size_t close = s.find(']', i);
                const std::size_t end = close == npos ? n : close + 1;
                out.add(i, end, Token::TAG);
                i = end;
            }
            else
            {
                const std::size_t eq = s.find_first_of("=:", i);
                if (eq == npos)
                {
                    out.add(i, n, Token::ATTRIBUTE);
                    return;
                }
                out.add(i, eq, Token::ATTRIBUTE);
                out.add(eq, eq + 1, Token::OPERATOR);
                i = skip_blanks(s, eq + 1);
                if (i < n && (s[i] == '"' || s[i] == '\''))
                {
                    const std::size_t close = s.find(s[i], i + 1);
                    const std::size_t end = close == npos ? n : close + 1;
                    out.add(i, end, Token::STRING);
                    i = end;
                }
                else if (i < n)
                {
                    std::size_t end = n;
                    for (std::size_t k = i; k < n; ++k)
                    {
                        if ((s[k] == ';' || s[k] == '#') && is_blank(s[k - 1]))
                        {
                            end = k;
                            break;
                        }
                    }
                    std::size_t last = end;
                    while (last > i && is_blank(s[last - 1]))
                    {
                        --last;
                    }
                    const std::string_view value = s.substr(i, last - i);
                    const bool number = !value.empty()
                                        && std::all_of(value.begin(), value.end(),
                                                       [](char c)
                                                       {
                                                           return is_digit(c) || c == '.';
                                                       });
                    out.add(i, last,
                            number                           ? Token::NUMBER
                            : lang.constants.contains(value) ? Token::CONSTANT
                                                             : Token::TEXT);
                    i = end;
                }
            }
            i = skip_blanks(s, i);
            if (i < n && (s[i] == ';' || s[i] == '#'))
            {
                out.add(i, n, Token::COMMENT);
            }
        }

        // ---- Markdown ----

        // Inline markup from `i`: `code`, [links](url), <html>, escapes.
        void markdown_inline(std::string_view s, std::size_t i, Out& out)
        {
            const std::size_t n = s.size();
            while (i < n)
            {
                const char ch = s[i];
                if (ch == '\\')
                {
                    i += 2;
                    continue;
                }
                if (ch == '`')
                {
                    std::size_t run = i;
                    while (run < n && s[run] == '`')
                    {
                        ++run;
                    }
                    const std::string_view fence = s.substr(i, run - i);
                    const std::size_t close = s.find(fence, run);
                    const std::size_t end = close == npos ? n : close + fence.size();
                    out.add(i, end, Token::VARIABLE);
                    i = end;
                    continue;
                }
                if (ch == '[' || (ch == '!' && i + 1 < n && s[i + 1] == '['))
                {
                    const std::size_t open = ch == '!' ? i + 1 : i;
                    const std::size_t close = s.find(']', open);
                    if (close != npos && close + 1 < n && s[close + 1] == '(')
                    {
                        const std::size_t paren = s.find(')', close);
                        const std::size_t end = paren == npos ? n : paren + 1;
                        out.add(i, close + 1, Token::STRING);
                        out.add(close + 1, end, Token::ATTRIBUTE);
                        i = end;
                        continue;
                    }
                }
                if (ch == '<' && i + 1 < n && (is_alpha(s[i + 1]) || s[i + 1] == '/'))
                {
                    const std::size_t close = s.find('>', i);
                    if (close != npos)
                    {
                        tokenize_piece(html(), s, i, close + 1, out);
                        i = close + 1;
                        continue;
                    }
                }
                if (ch == '|')
                {
                    out.add(i, i + 1, Token::PUNCTUATION);
                }
                ++i;
            }
        }

        void tokenize_markdown(std::string_view s, State& st, Out& out)
        {
            const std::size_t n = s.size();
            const std::size_t first = skip_blanks(s, 0);
            if (!st.end_marker.empty())
            {
                // Inside a fenced code block: its closing fence (a run of its character at least as long, alone on
                // the line), else the block's language.
                std::size_t run = first;
                while (run < n && s[run] == st.end_marker[0])
                {
                    ++run;
                }
                if (run - first >= st.end_marker.size() && blank(s.substr(run)))
                {
                    out.add(first, n, Token::STRING);
                    leave_embedded(st);
                    return;
                }
                if (st.embedded != nullptr)
                {
                    tokenize_embedded(*st.embedded, s, 0, n, st, out);
                }
                else
                {
                    out.add(0, n, Token::VARIABLE);
                }
                return;
            }
            if (st.mode == MARKUP_COMMENT)
            {
                const std::size_t end = s.find("-->");
                if (end == npos)
                {
                    out.add(0, n, Token::COMMENT);
                    return;
                }
                out.add(0, end + 3, Token::COMMENT);
                st.mode = NONE;
                markdown_inline(s, end + 3, out);
                return;
            }
            if (first == n)
            {
                return;
            }
            if (first <= 3 && (starts_at(s, first, "```") || starts_at(s, first, "~~~")))
            {
                std::size_t run = first;
                while (run < n && s[run] == s[first])
                {
                    ++run;
                }
                const std::size_t info = skip_blanks(s, run);
                std::size_t info_end = info;
                while (info_end < n && !is_blank(s[info_end]) && s[info_end] != '{' && s[info_end] != '`')
                {
                    ++info_end;
                }
                out.add(first, n, Token::STRING);
                st.end_marker = std::string(s.substr(first, run - first));
                st.embedded = language_by_name(s.substr(info, info_end - info));
                return;
            }
            if (starts_at(s, first, "<!--"))
            {
                const std::size_t end = s.find("-->", first + 4);
                if (end == npos)
                {
                    out.add(first, n, Token::COMMENT);
                    st.mode = MARKUP_COMMENT;
                    return;
                }
                out.add(first, end + 3, Token::COMMENT);
                markdown_inline(s, end + 3, out);
                return;
            }
            std::size_t i = first;
            if (s[i] == '#')
            {
                std::size_t level = i;
                while (level < n && s[level] == '#')
                {
                    ++level;
                }
                if (level - i <= 6 && (level == n || is_blank(s[level])))
                {
                    out.add(i, n, Token::KEYWORD);
                    return;
                }
            }
            // Setext underlines and rules: ===, ---, ***, ___
            if (contains("=-*_", s[i]))
            {
                const char mark = s[i];
                const std::string_view rest = s.substr(i);
                if (rest.size() >= 3
                    && std::all_of(rest.begin(), rest.end(),
                                   [mark](char c)
                                   {
                                       return c == mark || is_blank(c);
                                   }))
                {
                    out.add(i, n, Token::KEYWORD);
                    return;
                }
            }
            while (i < n && s[i] == '>')
            {
                out.add(i, i + 1, Token::COMMENT);
                i = skip_blanks(s, i + 1);
            }
            // List markers: - * + 1. 1)
            std::size_t marker = i;
            if (marker < n && contains("-*+", s[marker]))
            {
                ++marker;
            }
            else
            {
                while (marker < n && is_digit(s[marker]))
                {
                    ++marker;
                }
                marker = marker > i && marker < n && (s[marker] == '.' || s[marker] == ')') ? marker + 1 : i;
            }
            if (marker > i && (marker == n || is_blank(s[marker])))
            {
                out.add(i, marker, Token::KEYWORD);
                i = marker;
            }
            markdown_inline(s, i, out);
        }

        // ---- Dockerfile ----

        void tokenize_dockerfile(const Language& lang, std::string_view s, State& st, Out& out)
        {
            static const Language* shell = language_by_name("shell");
            const std::size_t n = s.size();
            std::size_t i = skip_blanks(s, 0);
            const bool continued = st.outer == OUTER_CONTINUED;
            st.outer = OUTER_NONE;
            if (!continued && st.mode == NONE)
            {
                if (i < n && s[i] == '#')
                {
                    out.add(i, n, Token::COMMENT);
                    return;
                }
                std::size_t end = i;
                while (end < n && is_alpha(s[end]))
                {
                    ++end;
                }
                if (lang.keywords.contains(s.substr(i, end - i)))
                {
                    out.add(i, end, Token::KEYWORD);
                    i = end;
                }
            }
            else
            {
                i = 0;
                if (s.substr(skip_blanks(s, 0)).starts_with("#"))
                {
                    out.add(0, n, Token::COMMENT); // a comment inside a continued RUN
                    st.outer = OUTER_CONTINUED;
                    return;
                }
            }
            if (shell != nullptr && i < n)
            {
                tokenize_range(*shell, s.substr(i), st, out.at(i));
            }
            std::size_t last = n;
            while (last > 0 && is_blank(s[last - 1]))
            {
                --last;
            }
            if (last > 0 && s[last - 1] == '\\')
            {
                st.outer = OUTER_CONTINUED;
            }
        }

        void tokenize_range(const Language& language, std::string_view s, State& state, Out out)
        {
            switch (language.kind)
            {
            case Kind::CODE:
                tokenize_code(language, s, state, out);
                break;
            case Kind::MARKUP:
                tokenize_markup(language, s, state, out);
                break;
            case Kind::CSS:
                tokenize_css(language, s, state, out);
                break;
            case Kind::JSON:
                tokenize_json(language, s, state, out);
                break;
            case Kind::YAML:
                tokenize_yaml(language, s, state, out);
                break;
            case Kind::INI:
                tokenize_ini(language, s, out);
                break;
            case Kind::MARKDOWN:
                tokenize_markdown(s, state, out);
                break;
            case Kind::DOCKERFILE:
                tokenize_dockerfile(language, s, state, out);
                break;
            }
        }
    } // namespace

    void tokenize_line(const Language& language, std::string_view line, State& state, std::vector<Span>& out)
    {
        if (line.size() > MAX_LINE_LENGTH)
        {
            state = {};
            return;
        }
        Out spans(out, out.size(), line, 0);
        tokenize_range(language, line, state, spans);
        spans.finish();
    }
} // namespace imgit::syntax
