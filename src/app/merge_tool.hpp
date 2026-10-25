#pragma once

#include "git/types.hpp"
#include <string>
#include <utility>
#include <vector>

// The merge tool's model for one conflicted file: its base, ours (A) and theirs (B) texts split into chunks by a
// three-way line merge, the lines picked from A and B for each conflict, and the output they make.
namespace imgit::app
{
    enum class Side
    {
        A,
        B,
    };

    // A run of lines the three texts agree on how to merge: `base_*`, `a_*`, `b_*` are line ranges (start,
    // count). A stable chunk is unchanged on both sides, a one-sided one takes that side's lines, a conflict
    // needs a choice.
    struct MergeChunk
    {
        enum class Kind
        {
            STABLE,
            TAKE_A,
            TAKE_B,
            CONFLICT,
        };
        Kind kind = Kind::STABLE;
        int base_start = 0, base_count = 0;
        int a_start = 0, a_count = 0;
        int b_start = 0, b_count = 0;
    };

    // One line of the output: its text and where it came from.
    struct OutputLine
    {
        enum class Origin
        {
            MERGED, // agreed on (or a one-sided change)
            PICKED_A,
            PICKED_B,
            UNRESOLVED, // a conflict nothing was picked for: the base's lines stand in
        };
        std::string text;
        Origin origin = Origin::MERGED;
        int conflict = -1; // the conflict it belongs to
        int source_line = -1; // PICKED_*: the line in A / B
        bool placeholder = false; // marks where an unresolved conflict with no base lines sits; not written
    };

    struct MergeTool
    {
        std::string path;
        std::string ours_label, theirs_label; // "Commit <sha6> on <branch>" parts: short sha and branch
        std::string ours_sha, theirs_sha;
        std::vector<std::string> base, ours, theirs;
        std::vector<MergeChunk> chunks;
        std::vector<int> conflicts; // indices into chunks
        // Per conflict, the picked lines in the order they were picked.
        std::vector<std::vector<std::pair<Side, int>>> picks;
        std::string eol = "\n"; // the line ending written back (CRLF when ours uses it)
        bool final_newline = true;
        bool edited = false; // the output was typed into: `edited_text` replaces the picks
        std::string edited_text;
        int current = 0; // the conflict the navigator is on
        int scroll_request = -1; // a conflict the panes should scroll to

        // Splits the three texts and merges them.
        void load(const std::string& base_text, const std::string& ours_text, const std::string& theirs_text);

        [[nodiscard]] int conflict_count() const noexcept
        {
            return static_cast<int>(conflicts.size());
        }
        // The conflict whose `side` range holds `line`, else -1.
        [[nodiscard]] int conflict_at(Side side, int line) const;
        [[nodiscard]] bool is_picked(int conflict, Side side, int line) const;
        void toggle_line(int conflict, Side side, int line);
        // Every line of `side` in `conflict` (or, with -1, in every conflict): picked when not all are, else none.
        void toggle_side(int conflict, Side side);
        [[nodiscard]] bool side_fully_picked(int conflict, Side side) const;
        [[nodiscard]] bool any_picked() const;
        [[nodiscard]] int unresolved_count() const;
        void reset();

        [[nodiscard]] std::vector<OutputLine> output() const;
        // The text written to the file: the edited text, or the output's lines.
        [[nodiscard]] std::string output_text() const;
    };
} // namespace imgit::app
