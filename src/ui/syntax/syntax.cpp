#include "ui/syntax/syntax.hpp"
#include <functional>

namespace imgit::syntax
{
    namespace
    {
        // FNV-1a.
        constexpr std::uint64_t FNV_OFFSET = 14695981039346656037ull;
        constexpr std::uint64_t FNV_PRIME = 1099511628211ull;

        void hash_bytes(std::uint64_t& hash, std::string_view bytes) noexcept
        {
            for (const char c : bytes)
            {
                hash = (hash ^ static_cast<unsigned char>(c)) * FNV_PRIME;
            }
        }

        // The first line of the file, when the diff starts there: for a shebang.
        std::string_view first_line(const git::FileDiff& diff) noexcept
        {
            if (diff.hunks.empty() || diff.hunks[0].lines.empty())
            {
                return {};
            }
            const git::DiffLine& line = diff.hunks[0].lines[0];
            return line.new_lineno == 1 || line.old_lineno == 1 ? std::string_view(line.text) : std::string_view();
        }
    } // namespace

    void LineSpans::clear() noexcept
    {
        m_spans.clear();
        m_starts.clear();
    }

    void LineSpans::add(const Language& language, std::string_view line, State& state)
    {
        if (m_starts.empty())
        {
            m_starts.push_back(0);
        }
        tokenize_line(language, line, state, m_spans);
        m_starts.push_back(static_cast<std::uint32_t>(m_spans.size()));
    }

    void LineSpans::add_plain()
    {
        if (m_starts.empty())
        {
            m_starts.push_back(0);
        }
        m_starts.push_back(static_cast<std::uint32_t>(m_spans.size()));
    }

    std::size_t LineSpans::size() const noexcept
    {
        return m_starts.empty() ? 0 : m_starts.size() - 1;
    }

    std::span<const Span> LineSpans::line(std::size_t index) const noexcept
    {
        if (index + 1 >= m_starts.size())
        {
            return {};
        }
        return {m_spans.data() + m_starts[index], m_starts[index + 1] - m_starts[index]};
    }

    LineSpans tokenize_lines(const Language& language, std::span<const std::string> lines)
    {
        LineSpans spans;
        State state;
        for (const std::string& line : lines)
        {
            spans.add(language, line, state);
        }
        return spans;
    }

    std::span<const Span> DiffSpans::line(const git::FileDiff& diff, std::size_t hunk,
                                          const git::DiffLine* line) const noexcept
    {
        if (hunk >= diff.hunks.size() || hunk >= hunk_starts.size())
        {
            return {};
        }
        const std::vector<git::DiffLine>& lines = diff.hunks[hunk].lines;
        const std::less<const git::DiffLine*> before;
        if (lines.empty() || before(line, lines.data()) || !before(line, lines.data() + lines.size()))
        {
            return {};
        }
        return spans.line(hunk_starts[hunk] + static_cast<std::size_t>(line - lines.data()));
    }

    DiffSpans tokenize_diff(const Language& language, const git::FileDiff& diff)
    {
        DiffSpans result;
        std::vector<Span> scratch;
        for (const git::DiffHunk& hunk : diff.hunks)
        {
            result.hunk_starts.push_back(result.spans.size());
            State old_side, new_side;
            for (const git::DiffLine& line : hunk.lines)
            {
                if (line.origin == '-')
                {
                    result.spans.add(language, line.text, old_side);
                }
                else if (line.origin == '+')
                {
                    result.spans.add(language, line.text, new_side);
                }
                else
                {
                    // A context line moves both sides on; tokenize it twice only when they disagree.
                    const bool same = old_side == new_side;
                    result.spans.add(language, line.text, new_side);
                    if (same)
                    {
                        old_side = new_side;
                    }
                    else
                    {
                        scratch.clear();
                        tokenize_line(language, line.text, old_side, scratch);
                    }
                }
            }
        }
        return result;
    }

    std::uint64_t fingerprint(const git::FileDiff& diff, bool& within_limits) noexcept
    {
        std::uint64_t hash = FNV_OFFSET;
        std::size_t lines = 0, bytes = 0;
        hash_bytes(hash, diff.path);
        for (const git::DiffHunk& hunk : diff.hunks)
        {
            hash_bytes(hash, "\n@@");
            lines += hunk.lines.size();
            for (const git::DiffLine& line : hunk.lines)
            {
                bytes += line.text.size();
                hash = (hash ^ static_cast<unsigned char>(line.origin)) * FNV_PRIME;
                hash_bytes(hash, line.text);
                hash_bytes(hash, "\n");
            }
            if (lines > MAX_LINES || bytes > MAX_BYTES)
            {
                within_limits = false;
                return 0;
            }
        }
        within_limits = true;
        return hash;
    }

    std::uint64_t fingerprint(std::span<const std::string> lines, bool& within_limits) noexcept
    {
        within_limits = false;
        if (lines.size() > MAX_LINES)
        {
            return 0;
        }
        std::uint64_t hash = FNV_OFFSET;
        std::size_t bytes = 0;
        for (const std::string& line : lines)
        {
            bytes += line.size();
            if (bytes > MAX_BYTES)
            {
                return 0;
            }
            hash_bytes(hash, line);
            hash_bytes(hash, "\n");
        }
        within_limits = true;
        return hash;
    }

    const DiffSpans* DiffCache::get(const git::FileDiff& diff)
    {
        if (diff.binary || diff.hunks.empty())
        {
            return nullptr;
        }
        const Language* language = language_for_path(diff.path, first_line(diff));
        if (language == nullptr)
        {
            return nullptr;
        }
        bool within_limits = false;
        const std::uint64_t hash = fingerprint(diff, within_limits);
        if (!within_limits)
        {
            return nullptr;
        }
        if (!m_valid || hash != m_hash || language != m_language || diff.path != m_path)
        {
            m_spans = tokenize_diff(*language, diff);
            m_valid = true;
            m_hash = hash;
            m_language = language;
            m_path = diff.path;
        }
        return &m_spans;
    }

    const LineSpans* TextCache::get(std::string_view path, std::span<const std::string> lines)
    {
        const Language* language = language_for_path(path, lines.empty() ? std::string_view() : lines[0]);
        if (language == nullptr)
        {
            return nullptr;
        }
        bool within_limits = false;
        const std::uint64_t hash = fingerprint(lines, within_limits);
        if (!within_limits)
        {
            return nullptr;
        }
        if (!m_valid || hash != m_hash || language != m_language || path != m_path)
        {
            m_spans = tokenize_lines(*language, lines);
            m_valid = true;
            m_hash = hash;
            m_language = language;
            m_path = path;
        }
        return &m_spans;
    }
} // namespace imgit::syntax
