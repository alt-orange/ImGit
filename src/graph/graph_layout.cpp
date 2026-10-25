#include "graph/graph_layout.hpp"
#include <unordered_map>

namespace imgit::graph
{
    namespace
    {
        struct ReserverInfo
        {
            git::NodeType type = git::NodeType::COMMIT;
            std::optional<std::int64_t> newest_date;
            int column = 0;
        };

        // GraphContainer's per-pass bookkeeping: which lanes are taken, which parent has a lane reserved for
        // it, and which lanes a row frees when it is finally reached.
        struct Assigner
        {
            const std::unordered_set<git::Sha>& pinned;
            std::unordered_map<int, bool> columns_used;
            std::unordered_map<git::Sha, ReserverInfo> reserver_by_sha;
            std::unordered_map<git::Sha, std::vector<int>> columns_to_free_when_found;
            std::unordered_set<git::Sha> has_merge_node_child;

            int free_column()
            {
                int column = pinned.empty() ? 0 : 1;
                while (columns_used[column])
                {
                    ++column;
                }
                columns_used[column] = true;
                return column;
            }

            int assign(const git::Commit& row)
            {
                has_merge_node_child.erase(row.sha);
                const bool is_pinned = pinned.contains(row.sha);

                if (const auto to_free = columns_to_free_when_found.find(row.sha);
                    to_free != columns_to_free_when_found.end())
                {
                    for (const int column : to_free->second)
                    {
                        columns_used.erase(column);
                    }
                }

                std::optional<ReserverInfo> own_reservation;
                if (const auto it = reserver_by_sha.find(row.sha); it != reserver_by_sha.end())
                {
                    own_reservation = it->second;
                }

                int column;
                if (is_pinned)
                {
                    column = 0;
                    columns_used[0] = true;
                }
                else if (own_reservation)
                {
                    column = own_reservation->column;
                    reserver_by_sha.erase(row.sha);
                }
                else
                {
                    column = free_column();
                }

                for (std::size_t index = 0; index < row.parents.size(); ++index)
                {
                    const git::Sha& parent = row.parents[index];
                    if (row.type == git::NodeType::MERGE)
                    {
                        has_merge_node_child.insert(parent);
                    }
                    const auto reserved = reserver_by_sha.find(parent);
                    const bool parent_reserved = reserved != reserver_by_sha.end();

                    if (index == 0 && parent_reserved && reserved->second.column != column)
                    {
                        // The first parent already has a lane: either pull it into our lane (and free its old
                        // one when it is reached) or let our lane end at it.
                        std::vector<int>& to_free = columns_to_free_when_found[parent];
                        const ReserverInfo& theirs = reserved->second;
                        const bool stash_displaced = theirs.type == git::NodeType::STASH
                                                     && row.type != git::NodeType::STASH && own_reservation
                                                     && own_reservation->newest_date && theirs.newest_date
                                                     && *own_reservation->newest_date > *theirs.newest_date;
                        if ((theirs.column > column || stash_displaced) && !has_merge_node_child.contains(parent))
                        {
                            const int old_column = theirs.column;
                            reserver_by_sha[parent] = ReserverInfo{
                                    row.type, own_reservation ? own_reservation->newest_date : std::nullopt, column};
                            to_free.push_back(old_column);
                        }
                        else
                        {
                            to_free.push_back(column);
                        }
                    }
                    else if (!parent_reserved)
                    {
                        ReserverInfo info;
                        info.type = row.type;
                        info.newest_date = (own_reservation && own_reservation->column == column)
                                                   ? own_reservation->newest_date
                                                   : std::optional<std::int64_t>{row.time};
                        if (pinned.contains(parent))
                        {
                            info.column = 0;
                        }
                        else if (index == 0)
                        {
                            info.column = column;
                        }
                        else
                        {
                            info.column = free_column();
                        }
                        reserver_by_sha[parent] = info;
                    }
                }
                // A root commit ends its lane: nothing below carries it on, so the column is free again.
                if (row.parents.empty())
                {
                    columns_used.erase(column);
                }
                return column;
            }
        };
    } // namespace

    std::unordered_set<git::Sha> first_parent_chain(const std::vector<git::Commit>& commits, const git::Sha& tip_sha)
    {
        std::unordered_set<git::Sha> chain;
        if (tip_sha.empty())
        {
            return chain;
        }
        git::Sha next = tip_sha;
        for (const git::Commit& commit : commits)
        {
            if (commit.sha != next)
            {
                continue;
            }
            chain.insert(commit.sha);
            if (commit.parents.empty())
            {
                break;
            }
            next = commit.parents.front();
        }
        return chain;
    }

    Layout compute(const std::vector<git::Commit>& commits, const std::unordered_set<git::Sha>& pinned_shas)
    {
        Layout layout;
        layout.rows.resize(commits.size());

        std::unordered_map<git::Sha, std::size_t> row_by_sha;
        row_by_sha.reserve(commits.size());
        for (std::size_t i = 0; i < commits.size(); ++i)
        {
            row_by_sha.emplace(commits[i].sha, i);
        }

        // Pass 1: lanes.
        Assigner assigner{pinned_shas};
        for (std::size_t i = 0; i < commits.size(); ++i)
        {
            layout.rows[i].column = assigner.assign(commits[i]);
        }

        // Pass 2: edges, each row from the previous row's edges plus its own starting ones.
        for (std::size_t i = 0; i < commits.size(); ++i)
        {
            const git::Commit& commit = commits[i];
            RowLayout& row = layout.rows[i];

            if (i > 0)
            {
                for (const auto& [column, previous] : layout.rows[i - 1].edges)
                {
                    // A solid line beats a dashed one in the same lane, pass-throughs beat starts.
                    const Edge* continued = nullptr;
                    if (previous.pass_through && !is_dashed(previous.pass_through->type))
                    {
                        continued = &*previous.pass_through;
                    }
                    else if (previous.starting && !is_dashed(previous.starting->type))
                    {
                        continued = &*previous.starting;
                    }
                    else if (previous.pass_through)
                    {
                        continued = &*previous.pass_through;
                    }
                    else if (previous.starting)
                    {
                        continued = &*previous.starting;
                    }
                    if (continued == nullptr)
                    {
                        continue;
                    }
                    if (continued->parent_sha == commit.sha)
                    {
                        row.edges[column].ending = *continued;
                    }
                    else
                    {
                        row.edges[column].pass_through = *continued;
                    }
                }
            }

            if (!commit.parents.empty())
            {
                row.edges[row.column].starting = Edge{commit.parents.front(), commit.type};
                for (std::size_t p = 1; p < commit.parents.size(); ++p)
                {
                    const auto parent_row = row_by_sha.find(commit.parents[p]);
                    if (parent_row == row_by_sha.end())
                    {
                        continue;
                    }
                    row.edges[layout.rows[parent_row->second].column].starting = Edge{commit.parents[p], commit.type};
                }
            }

            row.edge_column_max = row.column;
            for (const auto& [column, edges] : row.edges)
            {
                row.edge_column_max = std::max(row.edge_column_max, column);
            }
            layout.max_columns = std::max(layout.max_columns, row.edge_column_max);
        }
        return layout;
    }
} // namespace imgit::graph
