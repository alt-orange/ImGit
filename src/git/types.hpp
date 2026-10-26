#pragma once

#include <algorithm>
#include <atomic>
#include <cstdint>
#include <iterator>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace imgit::git
{
    using Sha = std::string;

    // Mirrors the graph node kinds: a plain commit, a merge (2+ parents), a stash entry and the
    // uncommitted working directory (drawn as the "// WIP" row).
    enum class NodeType
    {
        COMMIT,
        MERGE,
        STASH,
        WORK_DIR,
    };

    enum class RefType
    {
        LOCAL_BRANCH,
        REMOTE_BRANCH,
        TAG,
        STASH,
    };

    struct Author
    {
        std::string name;
        std::string email;
    };

    struct Ref
    {
        RefType type{};
        std::string name; // "main", "origin/main", "v1.0", "stash@{0}"
        std::string full_name; // "refs/heads/main"
        std::string remote; // remote branches only: "origin"
        Sha target;
        bool is_head = false; // the checked-out branch
        std::string upstream; // local branches: "origin/main" when tracking
        int ahead = 0;
        int behind = 0;
        bool annotated = false; // tags
        int stash_index = -1; // stashes
        std::string message; // stash / annotated tag message
        bool hidden = false; // not pushed to the graph walk
    };

    struct Commit
    {
        Sha sha;
        std::vector<Sha> parents;
        Author author;
        std::int64_t time = 0; // unix seconds, the committer's
        std::int64_t author_time = 0; // the author's (kept by an amend or rebase)
        std::string summary;
        std::string body;
        NodeType type = NodeType::COMMIT;
    };

    struct Remote
    {
        std::string name;
        std::string url;
    };

    struct Submodule
    {
        std::string name;
        std::string path;
    };

    struct Worktree
    {
        std::string name;
        std::string path;
    };

    struct History
    {
        std::vector<Commit> commits; // topological, newest first; may contain a WORK_DIR row and STASH rows
        std::vector<Ref> refs;
        std::unordered_map<Sha, std::vector<std::size_t>> refs_by_sha; // indices into refs
        std::unordered_map<Sha, std::size_t> row_by_sha; // indices into commits
        std::vector<Remote> remotes;
        std::vector<Submodule> submodules;
        std::vector<Worktree> worktrees;
        std::string head_branch; // "" when detached or unborn
        Sha head_sha;
        bool detached = false;
        bool unborn = false;
        bool has_more = false; // the commit limit cut the walk short
    };

    // What to walk: everything but the hidden refs, or only the soloed one.
    struct HistoryOptions
    {
        std::size_t max_commits = 2000;
        std::unordered_set<std::string> hidden_refs; // full names
        std::string solo_ref; // full name, "" for none
    };

    enum class FileStatus
    {
        ADDED,
        MODIFIED,
        DELETED,
        RENAMED,
        TYPECHANGE,
        UNTRACKED,
        CONFLICTED,
        UNKNOWN,
    };

    struct FileChange
    {
        std::string path;
        std::string old_path; // renames
        FileStatus status = FileStatus::UNKNOWN;
        int additions = 0;
        int deletions = 0;
        bool has_stats = false;
    };

    // One repository as the Repository Management lists it.
    struct RepoSummary
    {
        std::string branch; // "" when HEAD is detached or unborn
        int ahead = 0;
        int behind = 0;
        int modified = 0; // working directory files, staged or not
        int added = 0;
        int deleted = 0;
        std::string owner; // origin's owner on its hosting service ("github.com/<owner>/...")
    };

    struct WorkDirStatus
    {
        std::vector<FileChange> staged;
        std::vector<FileChange> unstaged;
        std::vector<FileChange> conflicted;

        [[nodiscard]] bool empty() const noexcept
        {
            return staged.empty() && unstaged.empty() && conflicted.empty();
        }
        [[nodiscard]] std::size_t count() const noexcept
        {
            return staged.size() + unstaged.size() + conflicted.size();
        }
    };

    enum class StatsSource
    {
        COMMIT,
        STAGED,
        UNSTAGED,
    };

    struct DiffLine
    {
        char origin = ' '; // ' ' context, '+' added, '-' deleted
        int old_lineno = -1;
        int new_lineno = -1;
        std::string text;
    };

    struct DiffHunk
    {
        std::string header; // "@@ -1,4 +1,6 @@ ..."
        std::vector<DiffLine> lines;
    };

    // A number no text loaded before has had (never 0): the views key the work they cache on it.
    [[nodiscard]] inline std::uint64_t new_revision() noexcept
    {
        static std::atomic<std::uint64_t> next{0};
        return ++next;
    }

    struct FileDiff
    {
        std::uint64_t revision = 0; // new_revision() when loaded; 0 for one built by hand
        std::string path;
        std::vector<DiffHunk> hunks;
        bool binary = false;
        int additions = 0;
        int deletions = 0;
    };

    // One commit of a file's history and what it did to the file.
    struct FileHistoryEntry
    {
        Commit commit;
        FileStatus status = FileStatus::MODIFIED; // ADDED / MODIFIED / DELETED / RENAMED
        std::string path; // the file's path at this commit
        std::string old_path; // RENAMED: its path in the parent
    };

    struct FileHistory
    {
        std::vector<FileHistoryEntry> entries; // newest first
        bool has_more = false; // the walk stopped at its limit
    };

    // One run of lines a blame attributes to the same commit.
    struct BlameHunk
    {
        int start_line = 1; // 1-based line in the blamed revision
        int line_count = 0;
        Sha sha;
        Author author;
        std::int64_t time = 0; // the author's
        std::string summary;
        std::string orig_path; // the file's path in that commit
        int orig_start_line = 1;
    };

    struct Blame
    {
        std::vector<BlameHunk> hunks; // in line order
        // The hunk holding `line` (1-based), nullptr past the end.
        [[nodiscard]] const BlameHunk* hunk_at(int line) const noexcept
        {
            const auto after = std::partition_point(hunks.begin(), hunks.end(),
                                                    [line](const BlameHunk& hunk)
                                                    {
                                                        return hunk.start_line <= line;
                                                    });
            if (after == hunks.begin())
            {
                return nullptr;
            }
            const BlameHunk& hunk = *std::prev(after);
            return line < hunk.start_line + hunk.line_count ? &hunk : nullptr;
        }
    };

    enum class ResetMode
    {
        SOFT,
        MIXED,
        HARD,
    };

    enum class RepoState
    {
        NONE,
        MERGE,
        REVERT,
        CHERRYPICK,
        REBASE,
        OTHER,
    };

    // One row of an interactive rebase plan.
    enum class RebaseAction
    {
        PICK,
        REWORD,
        SQUASH,
        DROP,
    };

    struct RebaseStep
    {
        Sha sha;
        RebaseAction action = RebaseAction::PICK;
        std::string message; // REWORD / SQUASH: the new message ("" keeps the original)
    };
} // namespace imgit::git
