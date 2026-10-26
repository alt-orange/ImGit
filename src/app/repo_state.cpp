#include "app/repo_state.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <chrono>
#include <future>
#include <thread>
#include <unordered_map>

namespace imgit::app
{
    namespace
    {
        constexpr std::size_t MAX_FILES_WITH_STATS = 600;
        constexpr std::size_t MAX_UNDO = 50;

        // Status plus each file's line counts. Slow on a big tree or a network share (WSL's /mnt/c takes seconds),
        // so the periodic poll runs it off the UI thread.
        // `with_submodules`: also the submodules (each one opened to read its HEAD, as the status read does).
        RepoState::StatusSnapshot read_status(const git::Repository& repo, bool with_submodules = false)
        {
            RepoState::StatusSnapshot snapshot{repo.load_status(), repo.state()};
            if (snapshot.status && snapshot.status->count() <= MAX_FILES_WITH_STATS)
            {
                (void)repo.fill_stats(snapshot.status->staged, git::StatsSource::STAGED, {});
                (void)repo.fill_stats(snapshot.status->unstaged, git::StatsSource::UNSTAGED, {});
            }
            if (auto signature = repo.refs_signature())
            {
                snapshot.refs_signature = std::move(*signature);
            }
            if (with_submodules)
            {
                snapshot.submodules = repo.submodules();
            }
            return snapshot;
        }

        // Whether two working directory listings show the same files in the same state.
        bool same_changes(const std::vector<git::FileChange>& a, const std::vector<git::FileChange>& b)
        {
            return std::equal(a.begin(), a.end(), b.begin(), b.end(),
                              [](const git::FileChange& x, const git::FileChange& y)
                              {
                                  return x.path == y.path && x.old_path == y.old_path && x.status == y.status
                                         && x.additions == y.additions && x.deletions == y.deletions
                                         && x.has_stats == y.has_stats;
                              });
        }

        // A std::async future blocks its destructor until the task ends: a detached thread waits for it instead, so
        // closing a tab during a slow status read or git command never stalls the UI thread.
        template<class T>
        void park(std::future<T>&& future)
        {
            if (!future.valid() || future.wait_for(std::chrono::seconds(0)) == std::future_status::ready)
            {
                return;
            }
            std::thread(
                    [job = std::move(future)]() mutable
                    {
                        job.wait();
                    })
                    .detach();
        }
    } // namespace

    RepoState::RepoState(git::Repository repository): repo(std::move(repository))
    {
        refresh();
    }

    RepoState::~RepoState()
    {
        park(std::move(m_status_job));
        if (file_history)
        {
            park(std::move(file_history->history_job));
            park(std::move(file_history->blame_job));
        }
        if (async)
        {
            park(std::move(async->future));
        }
    }

    void RepoState::refresh()
    {
        // The status (with the refs signature) is read before the history: a ref that moves in between then shows
        // as a change on the next poll instead of hiding behind a signature newer than the rows.
        ++m_status_generation;
        rebuild(read_status(repo));
        last_status_poll = 0.0;
    }

    void RepoState::rebuild(StatusSnapshot snapshot)
    {
        load_error.clear();
        m_refs_signature = snapshot.refs_signature;
        git::HistoryOptions options;
        options.max_commits = commit_limit;
        options.hidden_refs = hidden_refs;
        options.solo_ref = solo_ref;
        auto loaded = repo.load_history(options);
        if (!loaded)
        {
            load_error = loaded.error().message;
            history = {};
            layout = {};
            m_refs_signature.reset(); // the next poll tries again
            return;
        }
        history = std::move(*loaded);
        if (snapshot.status)
        {
            status = std::move(*snapshot.status);
        }
        else
        {
            status = {};
            load_error = snapshot.status.error().message;
        }
        operation_state = snapshot.operation_state;
        insert_wip_row();
        rebuild_layout();

        // Drop stale selection, keep the WIP row selected when it still exists.
        std::erase_if(selected,
                      [this](const git::Sha& sha)
                      {
                          return !history.row_by_sha.contains(sha);
                      });
        m_selected_set = {selected.begin(), selected.end()};
        if (!focused.empty() && !history.row_by_sha.contains(focused))
        {
            focused.clear();
        }
        if (!files_for_sha.empty())
        {
            const git::Sha reload = files_for_sha;
            files_for_sha.clear();
            load_commit_files(reload);
        }
        reload_diff();
        run_search();
    }

    void RepoState::refresh_status()
    {
        ++m_status_generation;
        apply_status(read_status(repo));
    }

    void RepoState::poll_status(const double now, const double interval)
    {
        if (m_status_job.valid())
        {
            if (m_status_job.wait_for(std::chrono::seconds(0)) != std::future_status::ready)
            {
                return;
            }
            StatusSnapshot snapshot = m_status_job.get();
            last_status_poll = now; // the interval counts from the end of a read, so slow reads never queue up
            // A refresh on the UI thread since the job started already holds newer state.
            if (m_status_job_generation == m_status_generation)
            {
                apply_status(std::move(snapshot));
            }
            return;
        }
        if (now - last_status_poll <= interval)
        {
            return;
        }
        last_status_poll = now;
        m_status_job_generation = m_status_generation;
        m_status_job = std::async(std::launch::async,
                                  [path = repo.workdir()]
                                  {
                                      // A libgit2 repository must not be shared across threads: open another.
                                      auto worker = git::Repository::open(path);
                                      if (!worker)
                                      {
                                          return StatusSnapshot{std::unexpected(worker.error())};
                                      }
                                      return read_status(*worker, true);
                                  });
    }

    void RepoState::apply_status(StatusSnapshot snapshot)
    {
        if (!snapshot.status)
        {
            return;
        }
        const bool had_wip = history.row_by_sha.contains(WIP_SHA);
        const bool has_wip = !snapshot.status->empty();
        // A ref moved, appeared or disappeared outside ImGit (a commit, checkout, fetch...): the rows are stale.
        const bool refs_changed = snapshot.refs_signature && snapshot.refs_signature != m_refs_signature;
        if (had_wip != has_wip || refs_changed)
        {
            rebuild(std::move(snapshot)); // no second status read: this one already predates the history load
            return;
        }
        // A submodule moved (a checkout or commit inside it), was added or removed: only the list is stale, not the
        // rows.
        if (snapshot.submodules && *snapshot.submodules != history.submodules)
        {
            history.submodules = std::move(*snapshot.submodules);
        }
        const bool changed = !same_changes(snapshot.status->staged, status.staged)
                             || !same_changes(snapshot.status->unstaged, status.unstaged)
                             || !same_changes(snapshot.status->conflicted, status.conflicted);
        status = std::move(*snapshot.status);
        operation_state = snapshot.operation_state;
        if (changed && diff_target && diff_target->source != DiffTarget::Source::COMMIT)
        {
            reload_diff();
        }
    }

    void RepoState::insert_wip_row()
    {
        if (status.empty())
        {
            return;
        }
        git::Commit wip;
        wip.sha = WIP_SHA;
        wip.type = git::NodeType::WORK_DIR;
        wip.summary = "// WIP";
        wip.time = std::chrono::duration_cast<std::chrono::seconds>(std::chrono::system_clock::now().time_since_epoch())
                           .count();
        if (auto author = repo.default_author())
        {
            wip.author = *author;
        }
        // Always the first row: its lane reservation also keeps the checked-out branch leftmost.
        if (!history.head_sha.empty())
        {
            wip.parents.push_back(history.head_sha);
        }
        history.commits.insert(history.commits.begin(), std::move(wip));
        history.row_by_sha.clear();
        for (std::size_t i = 0; i < history.commits.size(); ++i)
        {
            history.row_by_sha.emplace(history.commits[i].sha, i);
        }
    }

    void RepoState::rebuild_layout()
    {
        std::unordered_set<git::Sha> pinned;
        if (!pinned_branch.empty())
        {
            if (const git::Ref* ref = find_ref(pinned_branch); ref != nullptr)
            {
                pinned = graph::first_parent_chain(history.commits, ref->target);
            }
        }
        layout = graph::compute(history.commits, pinned);

        // Inherited refs: walking the rows top-down, a commit inherits the refs of the child it is the
        // lowest-index parent of (the earliest such child on ties); a child with branches passes its own on, one
        // without (tags only, or no refs) what it inherited.
        struct Pending
        {
            int source = -1;
            std::size_t parent_index = 0;
            std::size_t row = 0;
        };
        std::unordered_map<git::Sha, std::vector<Pending>> pending;
        child_ref_rows.assign(history.commits.size(), -1);
        ghost_ref_rows.assign(history.commits.size(), -1);
        for (std::size_t row = 0; row < history.commits.size(); ++row)
        {
            const git::Commit& commit = history.commits[row];
            int inherited = -1;
            if (const auto found = pending.find(commit.sha); found != pending.end())
            {
                const auto best = std::min_element(found->second.begin(), found->second.end(),
                                                   [](const Pending& a, const Pending& b)
                                                   {
                                                       return a.parent_index != b.parent_index
                                                                      ? a.parent_index < b.parent_index
                                                                      : a.row < b.row;
                                                   });
                inherited = best->source;
                pending.erase(found);
            }
            const std::vector<const git::Ref*> refs = refs_at(commit.sha);
            const bool has_refs = std::any_of(refs.begin(), refs.end(),
                                              [](const git::Ref* ref)
                                              {
                                                  return ref->type != git::RefType::STASH;
                                              });
            const bool has_branches = std::any_of(refs.begin(), refs.end(),
                                                  [](const git::Ref* ref)
                                                  {
                                                      return ref->type == git::RefType::LOCAL_BRANCH
                                                             || ref->type == git::RefType::REMOTE_BRANCH;
                                                  });
            child_ref_rows[row] = inherited;
            if (!has_refs)
            {
                ghost_ref_rows[row] = inherited;
            }
            const int source = has_branches ? static_cast<int>(row) : inherited;
            if (source < 0)
            {
                continue;
            }
            for (std::size_t p = 0; p < commit.parents.size(); ++p)
            {
                pending[commit.parents[p]].push_back(Pending{source, p, row});
            }
        }
    }

    void RepoState::load_commit_files(const git::Sha& sha)
    {
        if (files_for_sha == sha)
        {
            return;
        }
        files_for_sha = sha;
        commit_files.clear();
        commit_files_error.clear();
        if (sha == WIP_SHA)
        {
            return;
        }
        auto files = repo.commit_changes(sha);
        if (!files)
        {
            commit_files_error = files.error().message;
            return;
        }
        commit_files = std::move(*files);
        if (commit_files.size() <= MAX_FILES_WITH_STATS)
        {
            (void)repo.fill_stats(commit_files, git::StatsSource::COMMIT, sha);
        }
    }

    bool RepoState::open_merge_tool(const std::string& path, std::string& error)
    {
        auto sides = repo.conflict_sides(path);
        if (!sides)
        {
            error = sides.error().message;
            return false;
        }
        MergeTool tool;
        tool.path = path;
        tool.load(sides->base, sides->ours, sides->theirs);
        // A: HEAD on its branch; B: the commit coming in, on the local (else remote) branch pointing at it.
        tool.ours_sha = history.head_sha;
        tool.ours_label = history.head_branch;
        tool.theirs_sha = repo.incoming_head();
        for (const git::RefType type : {git::RefType::LOCAL_BRANCH, git::RefType::REMOTE_BRANCH})
        {
            for (const git::Ref* ref : refs_at(tool.theirs_sha))
            {
                if (tool.theirs_label.empty() && ref->type == type)
                {
                    tool.theirs_label = ref->name;
                }
            }
        }
        close_diff();
        merge_tool = std::move(tool);
        return true;
    }

    void RepoState::open_diff(DiffTarget target)
    {
        diff_target = std::move(target);
        reload_diff();
    }

    void RepoState::close_diff()
    {
        left_open_in_diff = false;
        diff_target.reset();
        diff.reset();
        diff_error.clear();
    }

    void RepoState::reload_diff()
    {
        if (!diff_target)
        {
            return;
        }
        diff_error.clear();
        // The inline / split / file views show the whole file; the hunk view 3 lines of context.
        const int context = diff_mode == DiffMode::HUNK && !file_view ? 3 : 1 << 24;
        git::Result<git::FileDiff> loaded = std::unexpected(git::Error{});
        const bool ignore_whitespace = !diff_whitespace;
        switch (diff_target->source)
        {
        case DiffTarget::Source::COMMIT:
            loaded = repo.commit_file_diff(diff_target->sha, diff_target->path, context, ignore_whitespace);
            break;
        case DiffTarget::Source::STAGED:
            loaded = repo.workdir_file_diff(diff_target->path, true, context, ignore_whitespace);
            break;
        case DiffTarget::Source::UNSTAGED:
            loaded = repo.workdir_file_diff(diff_target->path, false, context, ignore_whitespace);
            break;
        }
        if (!loaded)
        {
            diff.reset();
            diff_error = loaded.error().message;
            return;
        }
        diff = std::move(*loaded);
    }

    void RepoState::open_file_history(std::string path, git::Sha from, git::Sha select, bool blame)
    {
        if (file_history)
        {
            park(std::move(file_history->history_job));
            park(std::move(file_history->blame_job));
        }
        file_history.emplace();
        FileHistoryView& view = *file_history;
        view.path = std::move(path);
        view.from = std::move(from);
        view.selected_sha = std::move(select);
        view.blame = blame;
        view.history_job = std::async(std::launch::async,
                                      [workdir = repo.workdir(), path = view.path, from = view.from]
                                      {
                                          // A libgit2 repository must not be shared across threads: open another.
                                          auto worker = git::Repository::open(workdir);
                                          if (!worker)
                                          {
                                              return git::Result<git::FileHistory>(std::unexpected(worker.error()));
                                          }
                                          return worker->file_history(path, from);
                                      });
    }

    void RepoState::close_file_history()
    {
        if (!file_history)
        {
            return;
        }
        park(std::move(file_history->history_job));
        park(std::move(file_history->blame_job));
        file_history.reset();
    }

    void RepoState::select_file_history_entry(int index)
    {
        if (!file_history)
        {
            return;
        }
        FileHistoryView& view = *file_history;
        if (index < 0 || index >= static_cast<int>(view.history.entries.size()))
        {
            view.selected = -1;
            view.diff.reset();
            view.diff_error.clear();
            return;
        }
        view.selected = index;
        const git::FileHistoryEntry& entry = view.history.entries[static_cast<std::size_t>(index)];
        view.selected_sha = entry.commit.sha;
        reload_file_history_diff();
        // The blame of the file as this commit left it (a deleted file has none).
        park(std::move(view.blame_job));
        view.blame_data.reset();
        view.blame_error.clear();
        view.blame_sha = entry.commit.sha;
        view.blame_loading = entry.status != git::FileStatus::DELETED;
        if (!view.blame_loading)
        {
            return;
        }
        view.blame_job = std::async(std::launch::async,
                                    [workdir = repo.workdir(), path = entry.path, sha = entry.commit.sha]
                                    {
                                        auto worker = git::Repository::open(workdir);
                                        if (!worker)
                                        {
                                            return git::Result<git::Blame>(std::unexpected(worker.error()));
                                        }
                                        return worker->blame_file(path, sha);
                                    });
    }

    void RepoState::reload_file_history_diff()
    {
        if (!file_history || file_history->selected < 0)
        {
            return;
        }
        FileHistoryView& view = *file_history;
        const git::FileHistoryEntry& entry = view.history.entries[static_cast<std::size_t>(view.selected)];
        view.diff.reset();
        view.diff_error.clear();
        const int context = diff_mode == DiffMode::HUNK && !view.file_view ? 3 : 1 << 24;
        auto loaded = repo.commit_file_diff(entry.commit.sha, entry.path, context, !diff_whitespace);
        if (!loaded)
        {
            view.diff_error = loaded.error().message;
            return;
        }
        view.diff = std::move(*loaded);
    }

    void RepoState::poll_file_history()
    {
        if (!file_history)
        {
            return;
        }
        FileHistoryView& view = *file_history;
        if (view.history_job.valid() && view.history_job.wait_for(std::chrono::seconds(0)) == std::future_status::ready)
        {
            auto loaded = view.history_job.get();
            view.history_loading = false;
            if (!loaded)
            {
                view.history_error = loaded.error().message;
            }
            else
            {
                view.history = std::move(*loaded);
                int index = view.history.entries.empty() ? -1 : 0;
                for (std::size_t i = 0; i < view.history.entries.size() && !view.selected_sha.empty(); ++i)
                {
                    if (view.history.entries[i].commit.sha == view.selected_sha)
                    {
                        index = static_cast<int>(i);
                        break;
                    }
                }
                select_file_history_entry(index);
                if (index >= 0)
                {
                    view.scroll_to = static_cast<std::size_t>(index);
                }
            }
        }
        if (view.blame_job.valid() && view.blame_job.wait_for(std::chrono::seconds(0)) == std::future_status::ready)
        {
            auto loaded = view.blame_job.get();
            view.blame_loading = false;
            if (!loaded)
            {
                view.blame_error = loaded.error().message;
            }
            else
            {
                view.blame_data = std::move(*loaded);
            }
        }
    }

    void RepoState::run_search()
    {
        search_matches.clear();
        if (!search_open || search_text.empty())
        {
            search_index = -1;
            search_match_sha.clear();
            return;
        }
        for (std::size_t i = 0; i < history.commits.size(); ++i)
        {
            const git::Commit& commit = history.commits[i];
            if (ui::contains_ci(commit.summary, search_text) || ui::contains_ci(commit.body, search_text)
                || ui::contains_ci(commit.author.name, search_text) || ui::contains_ci(commit.author.email, search_text)
                || ui::contains_ci(commit.sha, search_text))
            {
                search_matches.push_back(i);
            }
        }
        // The match the user is on survives a refresh (or a narrowing of the text): only a new match set moves
        // the selection, to its first match.
        search_index = -1;
        if (!search_match_sha.empty())
        {
            for (std::size_t i = 0; i < search_matches.size(); ++i)
            {
                if (history.commits[search_matches[i]].sha == search_match_sha)
                {
                    search_index = static_cast<int>(i);
                    break;
                }
            }
        }
        if (search_index < 0)
        {
            search_match_sha.clear();
            if (!search_matches.empty())
            {
                search_index = 0;
                search_match_sha = history.commits[search_matches[0]].sha;
                select_only(search_match_sha);
                scroll_to_row = search_matches[0];
            }
        }
    }

    void RepoState::select_search_match(int direction)
    {
        if (search_matches.empty())
        {
            return;
        }
        const int count = static_cast<int>(search_matches.size());
        search_index = ((search_index + direction) % count + count) % count;
        const std::size_t row = search_matches[static_cast<std::size_t>(search_index)];
        search_match_sha = history.commits[row].sha;
        select_only(search_match_sha);
        scroll_to_row = row;
    }

    void RepoState::push_undo(UndoEntry entry)
    {
        undo_stack.push_back(std::move(entry));
        if (undo_stack.size() > MAX_UNDO)
        {
            undo_stack.erase(undo_stack.begin());
        }
        redo_stack.clear();
    }

    void RepoState::select(const git::Sha& sha, bool toggle, bool range)
    {
        if (range && !focused.empty() && history.row_by_sha.contains(focused) && history.row_by_sha.contains(sha))
        {
            const std::size_t a = history.row_by_sha.at(focused);
            const std::size_t b = history.row_by_sha.at(sha);
            const std::size_t lo = std::min(a, b);
            const std::size_t hi = std::max(a, b);
            if (!toggle)
            {
                selected.clear();
                m_selected_set.clear();
            }
            for (std::size_t i = lo; i <= hi; ++i)
            {
                const git::Sha& row_sha = history.commits[i].sha;
                if (m_selected_set.insert(row_sha).second)
                {
                    selected.push_back(row_sha);
                }
            }
            return;
        }
        if (toggle)
        {
            if (m_selected_set.erase(sha) > 0)
            {
                std::erase(selected, sha);
            }
            else
            {
                m_selected_set.insert(sha);
                selected.push_back(sha);
            }
            focused = sha;
            range_cursor.clear();
            return;
        }
        select_only(sha);
    }

    void RepoState::select_only(const git::Sha& sha)
    {
        selected.assign(1, sha);
        m_selected_set = {sha};
        focused = sha;
        range_cursor.clear();
    }

    bool RepoState::is_selected(const git::Sha& sha) const
    {
        return m_selected_set.contains(sha);
    }

    const git::Commit* RepoState::commit_for(const git::Sha& sha) const
    {
        const auto row = history.row_by_sha.find(sha);
        return row == history.row_by_sha.end() ? nullptr : &history.commits[row->second];
    }

    const git::Commit* RepoState::single_selected() const
    {
        return selected.size() == 1 ? commit_for(selected.front()) : nullptr;
    }

    const git::Ref* RepoState::head_ref() const
    {
        for (const git::Ref& ref : history.refs)
        {
            if (ref.is_head)
            {
                return &ref;
            }
        }
        return nullptr;
    }

    const git::Ref* RepoState::find_ref(const std::string& full_name) const
    {
        for (const git::Ref& ref : history.refs)
        {
            if (ref.full_name == full_name)
            {
                return &ref;
            }
        }
        return nullptr;
    }

    std::vector<const git::Ref*> RepoState::refs_at(const git::Sha& sha) const
    {
        std::vector<const git::Ref*> result;
        const auto found = history.refs_by_sha.find(sha);
        if (found == history.refs_by_sha.end())
        {
            return result;
        }
        for (const std::size_t index : found->second)
        {
            if (!history.refs[index].hidden)
            {
                result.push_back(&history.refs[index]);
            }
        }
        // Checked-out branch first, then local, remote, tags, stashes.
        std::stable_sort(result.begin(), result.end(),
                         [](const git::Ref* a, const git::Ref* b)
                         {
                             if (a->is_head != b->is_head)
                             {
                                 return a->is_head;
                             }
                             return static_cast<int>(a->type) < static_cast<int>(b->type);
                         });
        return result;
    }

    std::vector<const git::Commit*> RepoState::commits_above(const git::Sha& base_sha, const git::Sha& tip) const
    {
        std::vector<const git::Commit*> result;
        git::Sha next = tip.empty() ? history.head_sha : tip;
        while (!next.empty() && next != base_sha)
        {
            const git::Commit* commit = commit_for(next);
            if (commit == nullptr)
            {
                return {};
            }
            if (commit->type == git::NodeType::MERGE)
            {
                return {}; // ImGit refuses to rewrite across merge commits as well
            }
            result.push_back(commit);
            next = commit->parents.empty() ? git::Sha{} : commit->parents.front();
        }
        if (next != base_sha)
        {
            return {};
        }
        std::reverse(result.begin(), result.end());
        return result;
    }

    void RepoState::start_rebase_plan(const git::Sha& base_sha, const std::string& base_label,
                                      const git::Sha& fork_point, const git::Sha& tip)
    {
        rebase = {};
        rebase.base = base_sha;
        rebase.base_label = base_label;
        // The branch being rebased: the local branch at `tip` (HEAD's when there is none).
        rebase.head_label = history.head_branch.empty() ? git::short_sha(history.head_sha) : history.head_branch;
        if (!tip.empty())
        {
            for (const git::Ref* ref : refs_at(tip))
            {
                if (ref->type == git::RefType::LOCAL_BRANCH)
                {
                    rebase.head_label = ref->name;
                    break;
                }
            }
        }
        for (const git::Commit* commit : commits_above(fork_point.empty() ? base_sha : fork_point, tip))
        {
            rebase.steps.push_back(git::RebaseStep{commit->sha, git::RebaseAction::PICK, {}});
            rebase.summaries.push_back(commit->summary);
            rebase.authors.push_back(commit->author);
        }
        rebase.original_steps = rebase.steps;
        rebase.original_summaries = rebase.summaries;
        rebase.original_authors = rebase.authors;
        rebase.active = !rebase.steps.empty();
    }
} // namespace imgit::app
