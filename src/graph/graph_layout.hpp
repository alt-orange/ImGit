#pragma once

#include "git/types.hpp"
#include <map>
#include <optional>
#include <unordered_set>
#include <vector>

// The graph's column / edge assignment: lane reservation per row, then the starting, ending and
// pass-through edges of each column.
namespace imgit::graph
{
    struct Edge
    {
        git::Sha parent_sha;
        git::NodeType type = git::NodeType::COMMIT; // of the child row that started the edge
    };

    // The edges crossing one lane (column) of one row.
    struct ColumnEdges
    {
        std::optional<Edge> starting; // leaves this row's node downward (own column, or a merge parent's)
        std::optional<Edge> ending; // arrives from above into this row's node
        std::optional<Edge> pass_through; // crosses the whole row
    };

    struct RowLayout
    {
        int column = 0;
        std::map<int, ColumnEdges> edges; // by column
        int edge_column_max = 0;
    };

    struct Layout
    {
        std::vector<RowLayout> rows; // parallel to the input commits
        int max_columns = 0; // highest column touched by any edge
    };

    // `pinned_shas`: the first-parent chain of the pinned branch, forced into column 0; pass an empty set
    // for none.
    [[nodiscard]] Layout compute(const std::vector<git::Commit>& commits,
                                 const std::unordered_set<git::Sha>& pinned_shas);

    // The first-parent chain of `tip_sha` through `commits`, for `compute`'s pinned set.
    [[nodiscard]] std::unordered_set<git::Sha> first_parent_chain(const std::vector<git::Commit>& commits,
                                                                  const git::Sha& tip_sha);

    // Dashed lines: edges that belong to the working directory or a stash.
    [[nodiscard]] constexpr bool is_dashed(git::NodeType type) noexcept
    {
        return type == git::NodeType::WORK_DIR || type == git::NodeType::STASH;
    }
} // namespace imgit::graph
