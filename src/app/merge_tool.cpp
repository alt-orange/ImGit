#include "app/merge_tool.hpp"
#include "git/repository.hpp"
#include <algorithm>

namespace imgit::app
{
    namespace
    {
        // Lines split on '\n' with a trailing '\r' dropped; a final line break does not start another line.
        std::vector<std::string> split_lines(const std::string& text)
        {
            std::vector<std::string> lines;
            std::size_t start = 0;
            while (start < text.size())
            {
                std::size_t end = text.find('\n', start);
                if (end == std::string::npos)
                {
                    end = text.size();
                }
                std::string line = text.substr(start, end - start);
                if (!line.empty() && line.back() == '\r')
                {
                    line.pop_back();
                }
                lines.push_back(std::move(line));
                start = end + 1;
            }
            return lines;
        }

        struct SideHunk
        {
            Side side;
            git::LineHunk hunk;
        };
    } // namespace

    void MergeTool::load(const std::string& base_text, const std::string& ours_text, const std::string& theirs_text)
    {
        base = split_lines(base_text);
        ours = split_lines(ours_text);
        theirs = split_lines(theirs_text);
        const std::string& reference = ours_text.empty() ? theirs_text : ours_text;
        eol = reference.find("\r\n") != std::string::npos ? "\r\n" : "\n";
        final_newline = reference.empty() || reference.back() == '\n';

        // Both sides' changes against the base, in base order.
        std::vector<SideHunk> hunks;
        for (const git::LineHunk& h : git::diff_lines(base_text, ours_text))
        {
            hunks.push_back({Side::A, h});
        }
        for (const git::LineHunk& h : git::diff_lines(base_text, theirs_text))
        {
            hunks.push_back({Side::B, h});
        }
        std::stable_sort(hunks.begin(), hunks.end(),
                         [](const SideHunk& x, const SideHunk& y)
                         {
                             return x.hunk.old_start < y.hunk.old_start;
                         });

        chunks.clear();
        int cursor = 0; // in the base
        int delta_a = 0, delta_b = 0; // side line - base line, outside the changes
        const auto stable = [&](int until)
        {
            if (until > cursor)
            {
                chunks.push_back({MergeChunk::Kind::STABLE, cursor, until - cursor, cursor + delta_a, until - cursor,
                                  cursor + delta_b, until - cursor});
            }
        };
        std::size_t i = 0;
        while (i < hunks.size())
        {
            // A group: changes whose base ranges overlap or touch.
            const int lo = hunks[i].hunk.old_start;
            int hi = lo + hunks[i].hunk.old_count;
            int grow_a = 0, grow_b = 0;
            bool has_a = false, has_b = false;
            std::size_t j = i;
            while (j < hunks.size() && hunks[j].hunk.old_start <= hi)
            {
                const git::LineHunk& h = hunks[j].hunk;
                hi = std::max(hi, h.old_start + h.old_count);
                (hunks[j].side == Side::A ? grow_a : grow_b) += h.new_count - h.old_count;
                (hunks[j].side == Side::A ? has_a : has_b) = true;
                ++j;
            }
            stable(lo);
            MergeChunk chunk;
            chunk.base_start = lo;
            chunk.base_count = hi - lo;
            chunk.a_start = lo + delta_a;
            chunk.a_count = hi - lo + grow_a;
            chunk.b_start = lo + delta_b;
            chunk.b_count = hi - lo + grow_b;
            if (has_a && has_b)
            {
                // Both sides made the same change: not a conflict.
                const bool same = chunk.a_count == chunk.b_count
                                  && std::equal(ours.begin() + chunk.a_start,
                                                ours.begin() + chunk.a_start + chunk.a_count,
                                                theirs.begin() + chunk.b_start);
                chunk.kind = same ? MergeChunk::Kind::TAKE_A : MergeChunk::Kind::CONFLICT;
            }
            else
            {
                chunk.kind = has_a ? MergeChunk::Kind::TAKE_A : MergeChunk::Kind::TAKE_B;
            }
            chunks.push_back(chunk);
            delta_a += grow_a;
            delta_b += grow_b;
            cursor = hi;
            i = j;
        }
        stable(static_cast<int>(base.size()));

        conflicts.clear();
        for (std::size_t c = 0; c < chunks.size(); ++c)
        {
            if (chunks[c].kind == MergeChunk::Kind::CONFLICT)
            {
                conflicts.push_back(static_cast<int>(c));
            }
        }
        reset();
    }

    int MergeTool::conflict_at(Side side, int line) const
    {
        for (int c = 0; c < conflict_count(); ++c)
        {
            const MergeChunk& chunk = chunks[static_cast<std::size_t>(conflicts[static_cast<std::size_t>(c)])];
            const int start = side == Side::A ? chunk.a_start : chunk.b_start;
            const int count = side == Side::A ? chunk.a_count : chunk.b_count;
            if (line >= start && line < start + count)
            {
                return c;
            }
        }
        return -1;
    }

    bool MergeTool::is_picked(int conflict, Side side, int line) const
    {
        const auto& list = picks[static_cast<std::size_t>(conflict)];
        return std::find(list.begin(), list.end(), std::pair{side, line}) != list.end();
    }

    void MergeTool::toggle_line(int conflict, Side side, int line)
    {
        auto& list = picks[static_cast<std::size_t>(conflict)];
        const auto found = std::find(list.begin(), list.end(), std::pair{side, line});
        if (found != list.end())
        {
            list.erase(found);
        }
        else
        {
            list.emplace_back(side, line);
        }
    }

    bool MergeTool::side_fully_picked(int conflict, Side side) const
    {
        const auto check = [&](int c)
        {
            const MergeChunk& chunk = chunks[static_cast<std::size_t>(conflicts[static_cast<std::size_t>(c)])];
            const int start = side == Side::A ? chunk.a_start : chunk.b_start;
            const int count = side == Side::A ? chunk.a_count : chunk.b_count;
            if (count == 0)
            {
                // An empty side counts as picked once anything of the conflict was chosen.
                return !picks[static_cast<std::size_t>(c)].empty();
            }
            for (int line = start; line < start + count; ++line)
            {
                if (!is_picked(c, side, line))
                {
                    return false;
                }
            }
            return true;
        };
        if (conflict >= 0)
        {
            return check(conflict);
        }
        if (conflicts.empty())
        {
            return false;
        }
        for (int c = 0; c < conflict_count(); ++c)
        {
            if (!check(c))
            {
                return false;
            }
        }
        return true;
    }

    void MergeTool::toggle_side(int conflict, Side side)
    {
        const bool select = !side_fully_picked(conflict, side);
        const int first = conflict >= 0 ? conflict : 0;
        const int last = conflict >= 0 ? conflict : conflict_count() - 1;
        for (int c = first; c <= last; ++c)
        {
            const MergeChunk& chunk = chunks[static_cast<std::size_t>(conflicts[static_cast<std::size_t>(c)])];
            const int start = side == Side::A ? chunk.a_start : chunk.b_start;
            const int count = side == Side::A ? chunk.a_count : chunk.b_count;
            auto& list = picks[static_cast<std::size_t>(c)];
            std::erase_if(list,
                          [side](const std::pair<Side, int>& pick)
                          {
                              return pick.first == side;
                          });
            if (select)
            {
                for (int line = start; line < start + count; ++line)
                {
                    list.emplace_back(side, line);
                }
            }
        }
    }

    bool MergeTool::any_picked() const
    {
        return std::any_of(picks.begin(), picks.end(),
                           [](const auto& list)
                           {
                               return !list.empty();
                           });
    }

    int MergeTool::unresolved_count() const
    {
        return static_cast<int>(std::count_if(picks.begin(), picks.end(),
                                              [](const auto& list)
                                              {
                                                  return list.empty();
                                              }));
    }

    void MergeTool::reset()
    {
        picks.assign(conflicts.size(), {});
        edited = false;
        edited_text.clear();
    }

    std::vector<OutputLine> MergeTool::output() const
    {
        std::vector<OutputLine> out;
        int conflict = 0;
        for (const MergeChunk& chunk : chunks)
        {
            switch (chunk.kind)
            {
            case MergeChunk::Kind::STABLE:
                for (int l = 0; l < chunk.base_count; ++l)
                {
                    out.push_back({base[static_cast<std::size_t>(chunk.base_start + l)]});
                }
                break;
            case MergeChunk::Kind::TAKE_A:
                for (int l = 0; l < chunk.a_count; ++l)
                {
                    out.push_back({ours[static_cast<std::size_t>(chunk.a_start + l)]});
                }
                break;
            case MergeChunk::Kind::TAKE_B:
                for (int l = 0; l < chunk.b_count; ++l)
                {
                    out.push_back({theirs[static_cast<std::size_t>(chunk.b_start + l)]});
                }
                break;
            case MergeChunk::Kind::CONFLICT:
            {
                const auto& list = picks[static_cast<std::size_t>(conflict)];
                if (list.empty() && chunk.base_count == 0)
                {
                    OutputLine marker{{}, OutputLine::Origin::UNRESOLVED, conflict};
                    marker.placeholder = true;
                    out.push_back(marker);
                }
                if (list.empty())
                {
                    for (int l = 0; l < chunk.base_count; ++l)
                    {
                        out.push_back({base[static_cast<std::size_t>(chunk.base_start + l)],
                                       OutputLine::Origin::UNRESOLVED, conflict});
                    }
                }
                for (const auto& [side, line] : list)
                {
                    const std::string& text = side == Side::A ? ours[static_cast<std::size_t>(line)]
                                                              : theirs[static_cast<std::size_t>(line)];
                    out.push_back({text, side == Side::A ? OutputLine::Origin::PICKED_A : OutputLine::Origin::PICKED_B,
                                   conflict, line});
                }
                ++conflict;
                break;
            }
            }
        }
        return out;
    }

    std::string MergeTool::output_text() const
    {
        std::string text;
        if (edited)
        {
            // The editor's '\n' line breaks, in the file's own line ending.
            for (const char c : edited_text)
            {
                if (c == '\n')
                {
                    text += eol;
                }
                else if (c != '\r')
                {
                    text += c;
                }
            }
            return text;
        }
        std::vector<OutputLine> lines = output();
        std::erase_if(lines,
                      [](const OutputLine& line)
                      {
                          return line.placeholder;
                      });
        for (std::size_t i = 0; i < lines.size(); ++i)
        {
            text += lines[i].text;
            if (i + 1 < lines.size() || final_newline)
            {
                text += eol;
            }
        }
        return text;
    }
} // namespace imgit::app
