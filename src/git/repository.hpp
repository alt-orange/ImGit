#pragma once

#include "git/types.hpp"
#include <expected>
#include <memory>
#include <string>
#include <vector>

struct git_repository;
struct git_signature;

namespace imgit::git
{
    // A stash's message as ImGit lists it: git's "On <branch>: " / "WIP on <branch>: " prefix dropped.
    [[nodiscard]] std::string stash_summary(const std::string& message);

    struct Error
    {
        std::string message;
    };

    template<class T>
    using Result = std::expected<T, Error>;

    // Process-wide libgit2 init/shutdown.
    struct Library
    {
        Library();
        ~Library();
        Library(const Library&) = delete;
        Library& operator=(const Library&) = delete;
    };

    // A libgit2-backed repository. Every operation is synchronous and reports libgit2's error text on failure.
    class Repository
    {
    public:
        [[nodiscard]] static Result<Repository> open(const std::string& path);
        [[nodiscard]] static Result<Repository> init(const std::string& path);

        Repository(Repository&&) noexcept = default;
        Repository& operator=(Repository&&) noexcept = default;
        ~Repository();

        [[nodiscard]] const std::string& workdir() const noexcept
        {
            return m_workdir;
        }
        [[nodiscard]] const std::string& name() const noexcept
        {
            return m_name;
        }
        [[nodiscard]] git_repository* handle() const noexcept
        {
            return m_repo.get();
        }

        // Reading
        [[nodiscard]] Result<History> load_history(const HistoryOptions& options) const;
        [[nodiscard]] Result<WorkDirStatus> load_status() const;
        [[nodiscard]] Result<RepoSummary> summary() const;
        [[nodiscard]] RepoState state() const noexcept;
        [[nodiscard]] Result<std::vector<FileChange>> commit_changes(const Sha& sha) const;
        // Fills additions / deletions of `changes` (matched by path) from the given diff source.
        [[nodiscard]] Result<void> fill_stats(std::vector<FileChange>& changes, StatsSource source,
                                              const Sha& sha) const;
        // `context_lines` around each change; a huge value yields the whole file as one hunk. `ignore_whitespace`
        // leaves out changes in whitespace only (GIT_DIFF_IGNORE_WHITESPACE).
        [[nodiscard]] Result<FileDiff> commit_file_diff(const Sha& sha, const std::string& path,
                                                        int context_lines = 3, bool ignore_whitespace = false) const;
        [[nodiscard]] Result<FileDiff> workdir_file_diff(const std::string& path, bool staged,
                                                         int context_lines = 3, bool ignore_whitespace = false) const;
        [[nodiscard]] Result<Author> default_author() const;
        [[nodiscard]] Result<std::string> head_commit_message() const;

        // Index
        [[nodiscard]] Result<void> stage(const std::vector<std::string>& paths) const;
        [[nodiscard]] Result<void> stage_all() const;
        [[nodiscard]] Result<void> unstage(const std::vector<std::string>& paths) const;
        [[nodiscard]] Result<void> unstage_all() const;
        [[nodiscard]] Result<void> discard(const std::vector<FileChange>& changes) const;
        [[nodiscard]] Result<void> discard_all() const;
        [[nodiscard]] Result<Sha> commit(const std::string& message, bool amend) const;
        // Changes only HEAD's message, keeping its tree.
        [[nodiscard]] Result<Sha> reword_head(const std::string& message) const;

        // Refs
        [[nodiscard]] Result<void> create_branch(const std::string& name, const Sha& sha, bool checkout) const;
        // Points (or re-creates) a local branch at `sha` without touching the working directory.
        [[nodiscard]] Result<void> set_branch_target(const std::string& name, const Sha& sha) const;
        [[nodiscard]] Result<void> checkout_branch(const std::string& name) const;
        // "origin/feature" -> local "feature" tracking it, checked out.
        [[nodiscard]] Result<void> checkout_remote_branch(const std::string& remote, const std::string& name) const;
        [[nodiscard]] Result<void> checkout_commit(const Sha& sha) const;
        [[nodiscard]] Result<void> delete_branch(const std::string& name) const;
        [[nodiscard]] Result<void> rename_branch(const std::string& name, const std::string& new_name) const;
        [[nodiscard]] Result<void> set_upstream(const std::string& name, const std::string& upstream) const;
        [[nodiscard]] Result<void> create_tag(const std::string& name, const Sha& sha,
                                              const std::string& message) const;
        [[nodiscard]] Result<void> delete_tag(const std::string& name) const;
        [[nodiscard]] Result<void> add_remote(const std::string& name, const std::string& url) const;
        [[nodiscard]] Result<void> set_remote_url(const std::string& name, const std::string& url) const;
        [[nodiscard]] Result<void> remove_remote(const std::string& name) const;

        // History rewriting
        [[nodiscard]] Result<void> reset(const Sha& sha, ResetMode mode) const;
        // Returns false when the operation stopped on conflicts (the repository stays in that state).
        [[nodiscard]] Result<bool> cherry_pick(const Sha& sha) const;
        [[nodiscard]] Result<bool> revert(const Sha& sha) const;
        // Merges a branch into HEAD; `no_fast_forward` records a merge commit even when HEAD could fast-forward.
        [[nodiscard]] Result<bool> merge(const std::string& branch_name, bool no_fast_forward = false) const;
        // Whether `ancestor` is reachable from `descendant` (false for the same commit).
        [[nodiscard]] bool is_ancestor(const Sha& ancestor, const Sha& descendant) const;
        // The best common ancestor of two commits (empty when they share no history).
        [[nodiscard]] Sha merge_base(const Sha& a, const Sha& b) const;
        [[nodiscard]] Result<void> abort_operation() const;

        // The three versions of a conflicted file the index holds (stages 1 to 3). A side that does not have the
        // file (added on one side, deleted on the other) is empty and not `has_*`.
        struct ConflictSides
        {
            std::string base, ours, theirs;
            bool has_base = false, has_ours = false, has_theirs = false;
        };
        [[nodiscard]] Result<ConflictSides> conflict_sides(const std::string& path) const;
        // The commit coming in: MERGE_HEAD, else CHERRY_PICK_HEAD or REVERT_HEAD; empty when there is none.
        [[nodiscard]] Sha incoming_head() const;
        // Rebuilds the current branch on top of `base` from `steps` (oldest first): the interactive
        // rebase, squash and drop. Needs a clean working directory; conflicts roll everything back.
        [[nodiscard]] Result<void> rewrite_history(const Sha& base, const std::vector<RebaseStep>& steps) const;

        // Stashes
        // Returns the new stash's commit sha (stash@{0}).
        [[nodiscard]] Result<Sha> stash_push(const std::string& message) const;
        // The current index of the stash with that commit sha, -1 when it is gone.
        [[nodiscard]] int stash_index_of(const Sha& sha) const;
        [[nodiscard]] Result<void> stash_apply(int index) const;
        [[nodiscard]] Result<void> stash_pop(int index) const;
        [[nodiscard]] Result<void> stash_drop(int index) const;

    private:
        struct Deleter
        {
            void operator()(git_repository* repo) const noexcept;
        };

        explicit Repository(git_repository* repo);

        // `author` (name, email and date) when it is not the committer: the original author of a commit replayed.
        [[nodiscard]] Result<Sha> commit_index(const std::string& message, const std::vector<Sha>& parents,
                                               const git_signature* author) const;
        [[nodiscard]] Result<bool> finish_sequencer_commit(const std::string& message,
                                                           const git_signature* author) const;

        std::unique_ptr<git_repository, Deleter> m_repo;
        std::string m_workdir;
        std::string m_name;
    };

    [[nodiscard]] std::string short_sha(const Sha& sha);
    // user.email from the global / system git configuration, empty when unset.
    [[nodiscard]] std::string global_user_email();
    // user.name from the global / system git configuration, empty when unset.
    [[nodiscard]] std::string global_user_name();
    // Writes user.name / user.email to the global .gitconfig (the profile's "Keep my .gitconfig updated").
    [[nodiscard]] Result<void> set_global_identity(const std::string& name, const std::string& email);
    // The author / committer of commits in repositories without their own user.name / user.email: the current
    // profile's. Empty values leave it to the git configuration.
    void set_profile_identity(const std::string& name, const std::string& email);

    // A changed range between two texts, in 0-based lines: `old_count` lines at `old_start` became `new_count`
    // lines at `new_start` (a count of 0 is an insertion / deletion point).
    struct LineHunk
    {
        int old_start = 0, old_count = 0, new_start = 0, new_count = 0;
    };
    // The line hunks (no context) turning `old_text` into `new_text`, in order.
    [[nodiscard]] std::vector<LineHunk> diff_lines(const std::string& old_text, const std::string& new_text);
    [[nodiscard]] std::string status_letter(FileStatus status);
} // namespace imgit::git
