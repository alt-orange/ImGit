#pragma once

#include "app/merge_tool.hpp"
#include "git/git_cli.hpp"
#include "git/repository.hpp"
#include "graph/graph_layout.hpp"
#include <cstdint>
#include <functional>
#include <future>
#include <optional>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

namespace imgit::app
{
    // The sha ImGit gives the uncommitted-changes row.
    inline const git::Sha WIP_SHA = "WIP";

    struct DiffTarget
    {
        enum class Source
        {
            COMMIT,
            STAGED,
            UNSTAGED,
        };
        Source source = Source::COMMIT;
        git::Sha sha; // COMMIT
        std::string path;
        git::FileStatus status = git::FileStatus::UNKNOWN;
    };

    // A long-running `git` CLI command (fetch / pull / push / rebase) in flight.
    struct AsyncOperation
    {
        std::string title;
        std::future<git::cli::Output> future;
        bool silent = false; // auto-fetch: no toast on success
        std::vector<std::string> args; // run again after the credentials prompt
        std::optional<git::cli::Credential> attempt; // the credential entered for this run, kept when it works
        // Commands run one after another once this one succeeds (push a tag to every remote); the toast comes
        // with the last.
        std::vector<std::vector<std::string>> queued{};
    };

    // The Undo / Redo: each entry knows how to reverse (and, when cheap, repeat) one action.
    struct UndoEntry
    {
        std::string label;
        std::function<git::Result<void>()> undo;
        // May be empty: undo then derives it (back to the branch / commit it left, with `redo_mode`).
        std::function<git::Result<void>()> redo;
        git::ResetMode redo_mode = git::ResetMode::HARD;
    };

    // A pending interactive rebase.
    struct RebasePlan
    {
        bool active = false;
        git::Sha base;
        std::string base_label;
        std::string head_label; // the branch being rebased
        std::vector<git::RebaseStep> steps; // oldest first
        std::vector<std::string> summaries;
        std::vector<git::Author> authors;
        int selected = -1; // step the keyboard acts on
        int editing = -1; // step whose message is being edited (reword)
        // The plan as it started, for Reset.
        std::vector<git::RebaseStep> original_steps;
        std::vector<std::string> original_summaries;
        std::vector<git::Author> original_authors;

        // Swaps steps `a` and `b` with their summaries and authors.
        void swap_steps(int a, int b)
        {
            std::swap(steps[static_cast<std::size_t>(a)], steps[static_cast<std::size_t>(b)]);
            std::swap(summaries[static_cast<std::size_t>(a)], summaries[static_cast<std::size_t>(b)]);
            std::swap(authors[static_cast<std::size_t>(a)], authors[static_cast<std::size_t>(b)]);
        }
        void reset()
        {
            steps = original_steps;
            summaries = original_summaries;
            authors = original_authors;
            selected = -1;
            editing = -1;
        }
    };

    // Everything one repository tab holds: the loaded history, its graph layout, selection and panel state.
    class RepoState
    {
    public:
        explicit RepoState(git::Repository repository);
        // Hands any background job still running (status read, git command) to a detached waiter, so closing the
        // tab never blocks the UI thread on it.
        ~RepoState();
        RepoState(const RepoState&) = delete;
        RepoState& operator=(const RepoState&) = delete;

        git::Repository repo;
        git::History history;
        git::WorkDirStatus status;
        graph::Layout layout;
        // Per row: the row whose branches it inherits, -1 for none.
        std::vector<int> child_ref_rows;
        // Per row without refs: the row whose refs it shows as ghost refs (its childRefs), -1 for none.
        std::vector<int> ghost_ref_rows;
        git::RepoState operation_state = git::RepoState::NONE;
        std::string load_error;
        std::string pinned_branch; // full ref name forced into column 0, "" for none

        // Graph walk options
        std::size_t commit_limit = 2000;
        std::unordered_set<std::string> hidden_refs; // full names
        std::string solo_ref;

        // Graph selection (ordered by click) and keyboard focus.
        std::vector<git::Sha> selected;
        git::Sha focused; // the anchor of a Shift range
        git::Sha range_cursor; // the moving end of a Shift+arrow range, "" when none
        std::optional<std::size_t> scroll_to_row;
        git::Sha hovered_sha; // row under the mouse this frame

        // Commit search (Ctrl+F)
        bool search_open = false;
        std::string search_text;
        std::vector<std::size_t> search_matches;
        int search_index = -1;
        git::Sha search_match_sha; // the match selected, kept across refreshes
        bool search_focus_requested = false;

        // Detail panel caches
        git::Sha files_for_sha;
        std::vector<git::FileChange> commit_files;
        std::string commit_files_error;
        std::optional<DiffTarget> diff_target;
        std::optional<MergeTool> merge_tool; // a conflicted file being resolved
        std::optional<git::FileDiff> diff;
        // The diff editor modes: the hunk list, the whole file inline (default) or side by side; the
        // file view shows the file itself.
        enum class DiffMode
        {
            HUNK,
            INLINE,
            SPLIT,
        };
        DiffMode diff_mode = DiffMode::INLINE;
        bool file_view = false;
        bool diff_whitespace = true; // show changes in whitespace only (off: GIT_DIFF_IGNORE_WHITESPACE)
        bool left_open_in_diff = false; // the left panel collapses while a file is open
        std::string diff_error;
        bool tree_view = true; // Path | Tree toggle
        std::unordered_set<std::string> expanded_folders; // tree folders start collapsed
        std::unordered_set<std::string> collapsed_sections; // "Unstaged Files" / "Staged Files" headers

        // Commit form (kept across refreshes)
        std::string commit_summary;
        std::string commit_description;
        bool amend = false;
        bool push_after_commit = false;
        bool commit_options_open = false;
        // The composer's Stash tab (GitKraken's): its own message, stashing every change.
        bool composer_stash = false;
        std::string stash_summary;
        std::string stash_description;
        // The commit view's message editor (clicking HEAD's message): its own copy of the message.
        bool amending_message = false;
        bool amend_focus_requested = false;
        std::string amend_summary;
        std::string amend_description;

        // Left panel
        std::string left_filter;
        std::unordered_set<std::string> collapsed_nodes; // collapsed folders / remotes
        std::unordered_set<std::string> expanded_left_sections; // none expanded by default
        std::unordered_map<std::string, float> left_section_ys; // dragged section ends (sectionYsByKey)
        std::string left_selected; // the ref clicked in the left panel

        // The inline "enter branch name" / "enter tag name" input in the graph's ref zone.
        struct InlineRef
        {
            git::Sha sha;
            bool tag = false;
            bool checkout = false;
            std::string name;
            bool focus = true;
        };
        std::optional<InlineRef> inline_ref;

        // Undo / redo
        std::vector<UndoEntry> undo_stack;
        std::vector<UndoEntry> redo_stack;

        RebasePlan rebase;

        std::optional<AsyncOperation> async;
        double last_status_poll = 0.0;
        double last_auto_fetch = 0.0;

        // The working directory as one status read sees it.
        struct StatusSnapshot
        {
            git::Result<git::WorkDirStatus> status;
            git::RepoState operation_state = git::RepoState::NONE;
            std::optional<std::uint64_t> refs_signature{}; // Repository::refs_signature; none: it could not be read
        };

        void refresh(); // history + status + layout
        // Cheap: working directory only; rebuilds the rows when the WIP row appears / disappears or a ref changed.
        void refresh_status();
        // The periodic status poll: re-reads the working directory and the refs on a worker thread once `interval`
        // seconds have passed since the last read finished, and applies the result on a later frame. A commit,
        // checkout, fetch or stash made outside ImGit thus shows up within one interval.
        void poll_status(double now, double interval);
        void load_commit_files(const git::Sha& sha);
        void open_diff(DiffTarget target);
        void close_diff();
        // Opens the merge tool on a conflicted file; false (with `error`) when its sides cannot be read.
        bool open_merge_tool(const std::string& path, std::string& error);
        void reload_diff();
        void run_search();
        void select_search_match(int direction);
        void push_undo(UndoEntry entry);

        void select(const git::Sha& sha, bool toggle, bool range);
        void select_only(const git::Sha& sha);
        [[nodiscard]] bool is_selected(const git::Sha& sha) const;
        [[nodiscard]] const git::Commit* commit_for(const git::Sha& sha) const;
        [[nodiscard]] const git::Commit* single_selected() const; // one selected row, else nullptr
        [[nodiscard]] bool busy() const noexcept
        {
            return async.has_value();
        }
        [[nodiscard]] const git::Ref* head_ref() const;
        [[nodiscard]] std::vector<const git::Ref*> refs_at(const git::Sha& sha) const;
        [[nodiscard]] const git::Ref* find_ref(const std::string& full_name) const;
        // The commits on HEAD's first-parent chain above `base_sha` (oldest first) for a rebase plan.
        // First-parent commits from `tip` (HEAD when empty) down to `base_sha`, oldest first; empty across merges.
        [[nodiscard]] std::vector<const git::Commit*> commits_above(const git::Sha& base_sha,
                                                                    const git::Sha& tip = {}) const;
        // Opens the interactive rebase of the commits between `fork_point` (`base_sha` when empty) and `tip`
        // onto `base_sha`.
        void start_rebase_plan(const git::Sha& base_sha, const std::string& base_label, const git::Sha& fork_point = {},
                               const git::Sha& tip = {});

    private:
        void apply_status(StatusSnapshot snapshot);
        // refresh() from a status already read (one that predates the history load, as refresh()'s own does).
        void rebuild(StatusSnapshot snapshot);
        void insert_wip_row();
        void rebuild_layout();
        std::unordered_set<git::Sha> m_selected_set;
        std::future<StatusSnapshot> m_status_job;
        // Bumped by every status read on the UI thread; a poll started before one is out of date.
        std::uint64_t m_status_generation = 0;
        std::uint64_t m_status_job_generation = 0;
        std::optional<std::uint64_t> m_refs_signature; // the refs the rows were built from
    };
} // namespace imgit::app
