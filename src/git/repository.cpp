#include "git/repository.hpp"
#include "git/git_cli.hpp"
#include <algorithm>
#include <chrono>
#include <cstring>
#include <filesystem>
#include <git2.h>
#include <map>
#include <mutex>
#include <unordered_set>
#include <utility>

namespace imgit::git
{
    std::string stash_summary(const std::string& message)
    {
        for (const char* prefix : {"WIP on ", "On "})
        {
            const std::size_t length = std::char_traits<char>::length(prefix);
            if (message.compare(0, length, prefix) == 0)
            {
                if (const std::size_t colon = message.find(": ", length); colon != std::string::npos)
                {
                    return message.substr(colon + 2);
                }
            }
        }
        return message;
    }

    namespace
    {
        // RAII for libgit2 handles.
        template<class T, void (*Free)(T*)>
        struct Handle
        {
            T* ptr = nullptr;

            Handle() = default;
            Handle(const Handle&) = delete;
            Handle& operator=(const Handle&) = delete;
            Handle(Handle&& other) noexcept: ptr(other.ptr)
            {
                other.ptr = nullptr;
            }
            Handle& operator=(Handle&& other) noexcept
            {
                if (this != &other)
                {
                    if (ptr != nullptr)
                    {
                        Free(ptr);
                    }
                    ptr = other.ptr;
                    other.ptr = nullptr;
                }
                return *this;
            }
            ~Handle()
            {
                if (ptr != nullptr)
                {
                    Free(ptr);
                }
            }
            [[nodiscard]] T** out() noexcept
            {
                return &ptr;
            }
            [[nodiscard]] operator T*() const noexcept
            {
                return ptr;
            }
            [[nodiscard]] T* get() const noexcept
            {
                return ptr;
            }
        };

        using CommitHandle = Handle<git_commit, git_commit_free>;
        using TreeHandle = Handle<git_tree, git_tree_free>;
        using ReferenceHandle = Handle<git_reference, git_reference_free>;
        using ObjectHandle = Handle<git_object, git_object_free>;
        using IndexHandle = Handle<git_index, git_index_free>;
        using DiffHandle = Handle<git_diff, git_diff_free>;
        using PatchHandle = Handle<git_patch, git_patch_free>;
        using SignatureHandle = Handle<git_signature, git_signature_free>;
        using RevwalkHandle = Handle<git_revwalk, git_revwalk_free>;
        using StatusListHandle = Handle<git_status_list, git_status_list_free>;
        using BranchIteratorHandle = Handle<git_branch_iterator, git_branch_iterator_free>;
        using RemoteHandle = Handle<git_remote, git_remote_free>;
        using AnnotatedCommitHandle = Handle<git_annotated_commit, git_annotated_commit_free>;
        using ConfigHandle = Handle<git_config, git_config_free>;
        using TreeEntryHandle = Handle<git_tree_entry, git_tree_entry_free>;

        std::unexpected<Error> last_error(const char* fallback)
        {
            const git_error* error = git_error_last();
            if (error != nullptr && error->message != nullptr && *error->message != '\0')
            {
                return std::unexpected(Error{error->message});
            }
            return std::unexpected(Error{fallback});
        }

#define IMGIT_TRY(expr, what)                                                                                          \
    if ((expr) < 0)                                                                                                    \
    {                                                                                                                  \
        return last_error(what);                                                                                       \
    }

        std::string oid_str(const git_oid* oid)
        {
            char buffer[GIT_OID_MAX_HEXSIZE + 1] = {};
            git_oid_tostr(buffer, sizeof(buffer), oid);
            return buffer;
        }

        Result<git_oid> parse_oid(const Sha& sha)
        {
            git_oid oid;
            if (git_oid_fromstr(&oid, sha.c_str()) < 0)
            {
                return last_error("invalid object id");
            }
            return oid;
        }

        Result<CommitHandle> lookup_commit(git_repository* repo, const Sha& sha)
        {
            auto oid = parse_oid(sha);
            if (!oid)
            {
                return std::unexpected(oid.error());
            }
            CommitHandle commit;
            IMGIT_TRY(git_commit_lookup(commit.out(), repo, &*oid), "commit not found");
            return commit;
        }

        std::vector<char*> c_strings(const std::vector<std::string>& strings)
        {
            std::vector<char*> result;
            result.reserve(strings.size());
            for (const std::string& s : strings)
            {
                result.push_back(const_cast<char*>(s.c_str()));
            }
            return result;
        }

        FileStatus delta_status(git_delta_t status)
        {
            switch (status)
            {
            case GIT_DELTA_ADDED:
                return FileStatus::ADDED;
            case GIT_DELTA_DELETED:
                return FileStatus::DELETED;
            case GIT_DELTA_MODIFIED:
                return FileStatus::MODIFIED;
            case GIT_DELTA_RENAMED:
                return FileStatus::RENAMED;
            case GIT_DELTA_TYPECHANGE:
                return FileStatus::TYPECHANGE;
            case GIT_DELTA_UNTRACKED:
                return FileStatus::UNTRACKED;
            case GIT_DELTA_CONFLICTED:
                return FileStatus::CONFLICTED;
            default:
                return FileStatus::UNKNOWN;
            }
        }

        FileChange change_from_delta(const git_diff_delta* delta)
        {
            FileChange change;
            change.status = delta_status(delta->status);
            change.path = delta->new_file.path != nullptr ? delta->new_file.path : delta->old_file.path;
            if (change.status == FileStatus::RENAMED && delta->old_file.path != nullptr)
            {
                change.old_path = delta->old_file.path;
            }
            return change;
        }

        Result<FileDiff> diff_to_file_diff(git_diff* diff, std::size_t delta_index, const std::string& path)
        {
            FileDiff result;
            result.path = path;
            if (delta_index >= git_diff_num_deltas(diff))
            {
                return result;
            }
            PatchHandle patch;
            IMGIT_TRY(git_patch_from_diff(patch.out(), diff, delta_index), "could not build patch");
            const git_diff_delta* delta = git_patch_get_delta(patch);
            if ((delta->flags & GIT_DIFF_FLAG_BINARY) != 0)
            {
                result.binary = true;
                return result;
            }
            std::size_t context = 0, additions = 0, deletions = 0;
            git_patch_line_stats(&context, &additions, &deletions, patch);
            result.additions = static_cast<int>(additions);
            result.deletions = static_cast<int>(deletions);

            const std::size_t hunk_count = git_patch_num_hunks(patch);
            for (std::size_t h = 0; h < hunk_count; ++h)
            {
                const git_diff_hunk* hunk = nullptr;
                std::size_t line_count = 0;
                IMGIT_TRY(git_patch_get_hunk(&hunk, &line_count, patch, h), "could not read hunk");
                DiffHunk out;
                out.header.assign(hunk->header, hunk->header_len);
                while (!out.header.empty() && (out.header.back() == '\n' || out.header.back() == '\r'))
                {
                    out.header.pop_back();
                }
                out.lines.reserve(line_count);
                for (std::size_t l = 0; l < line_count; ++l)
                {
                    const git_diff_line* line = nullptr;
                    IMGIT_TRY(git_patch_get_line_in_hunk(&line, patch, h, l), "could not read line");
                    if (line->origin != GIT_DIFF_LINE_CONTEXT && line->origin != GIT_DIFF_LINE_ADDITION
                        && line->origin != GIT_DIFF_LINE_DELETION)
                    {
                        continue;
                    }
                    DiffLine out_line;
                    out_line.origin = line->origin;
                    out_line.old_lineno = line->old_lineno;
                    out_line.new_lineno = line->new_lineno;
                    out_line.text.assign(line->content, line->content_len);
                    while (!out_line.text.empty() && (out_line.text.back() == '\n' || out_line.text.back() == '\r'))
                    {
                        out_line.text.pop_back();
                    }
                    out.lines.push_back(std::move(out_line));
                }
                result.hunks.push_back(std::move(out));
            }
            return result;
        }

        Result<TreeHandle> commit_tree(const git_commit* commit)
        {
            TreeHandle tree;
            IMGIT_TRY(git_commit_tree(tree.out(), commit), "could not read commit tree");
            return tree;
        }

        // HEAD's commit, or nullptr handle on an unborn branch.
        Result<CommitHandle> head_commit(git_repository* repo)
        {
            CommitHandle commit;
            const int rc = git_revparse_single(reinterpret_cast<git_object**>(commit.out()), repo, "HEAD");
            if (rc == GIT_ENOTFOUND || rc == GIT_EUNBORNBRANCH)
            {
                return CommitHandle{};
            }
            if (rc < 0)
            {
                return last_error("could not resolve HEAD");
            }
            return commit;
        }

        std::mutex g_profile_mutex;
        Author g_profile_identity;

        // True when the repository's own config (.git/config) sets user.name or user.email.
        bool has_local_identity(git_repository* repo)
        {
            git_config* config = nullptr;
            if (git_repository_config(&config, repo) != 0)
            {
                return false;
            }
            bool found = false;
            git_config* local = nullptr;
            if (git_config_open_level(&local, config, GIT_CONFIG_LEVEL_LOCAL) == 0)
            {
                for (const char* key : {"user.name", "user.email"})
                {
                    git_buf buffer = GIT_BUF_INIT;
                    if (git_config_get_string_buf(&buffer, local, key) == 0)
                    {
                        found = true;
                        git_buf_dispose(&buffer);
                    }
                }
                git_config_free(local);
            }
            git_config_free(config);
            return found;
        }

        Result<SignatureHandle> default_signature(git_repository* repo)
        {
            SignatureHandle sig;
            Author profile;
            {
                std::lock_guard lock(g_profile_mutex);
                profile = g_profile_identity;
            }
            if (!profile.name.empty() && !profile.email.empty() && !has_local_identity(repo)
                && git_signature_now(sig.out(), profile.name.c_str(), profile.email.c_str()) == 0)
            {
                return sig;
            }
            if (git_signature_default(sig.out(), repo) < 0)
            {
                // No user.name / user.email configured: fall back so commits still work.
                IMGIT_TRY(git_signature_now(sig.out(), "ImGit", "imgit@localhost"), "could not create signature");
            }
            return sig;
        }

        git_checkout_options safe_checkout_options()
        {
            git_checkout_options opts = GIT_CHECKOUT_OPTIONS_INIT;
            opts.checkout_strategy = GIT_CHECKOUT_SAFE | GIT_CHECKOUT_RECREATE_MISSING;
            return opts;
        }

        // Ends the operation a commit just completed, as git's own `commit` does. git_repository_state_cleanup would
        // also delete the rebase-merge / rebase-apply / sequencer directories, which destroys an in-progress rebase
        // or a multi-commit cherry-pick / revert: a rebase keeps its state (git rebase --continue goes on once the
        // index is clean), a sequence only loses the head file of the step just committed.
        void finish_operation_state(git_repository* repo)
        {
            const int state = git_repository_state(repo);
            switch (state)
            {
            case GIT_REPOSITORY_STATE_MERGE:
            case GIT_REPOSITORY_STATE_CHERRYPICK:
            case GIT_REPOSITORY_STATE_REVERT:
                git_repository_state_cleanup(repo);
                break;
            case GIT_REPOSITORY_STATE_CHERRYPICK_SEQUENCE:
            case GIT_REPOSITORY_STATE_REVERT_SEQUENCE:
            {
                const char* head_file =
                        state == GIT_REPOSITORY_STATE_CHERRYPICK_SEQUENCE ? "CHERRY_PICK_HEAD" : "REVERT_HEAD";
                std::error_code ec;
                std::filesystem::remove(std::filesystem::path(git_repository_path(repo)) / head_file, ec);
                break;
            }
            default:
                break;
            }
        }
    } // namespace

    Library::Library()
    {
        git_libgit2_init();
    }

    Library::~Library()
    {
        git_libgit2_shutdown();
    }

    void Repository::Deleter::operator()(git_repository* repo) const noexcept
    {
        git_repository_free(repo);
    }

    Repository::Repository(git_repository* repo): m_repo(repo)
    {
        const char* workdir = git_repository_workdir(repo);
        m_workdir = workdir != nullptr ? workdir : git_repository_path(repo);
        std::filesystem::path path(m_workdir);
        if (!path.has_filename())
        {
            path = path.parent_path();
        }
        m_name = path.filename().string();
    }

    Repository::~Repository() = default;

    Result<Repository> Repository::open(const std::string& path)
    {
        git_repository* repo = nullptr;
        if (git_repository_open_ext(&repo, path.c_str(), 0, nullptr) < 0)
        {
            return last_error("not a git repository");
        }
        return Repository(repo);
    }

    Result<Repository> Repository::init(const std::string& path)
    {
        git_repository* repo = nullptr;
        if (git_repository_init(&repo, path.c_str(), 0) < 0)
        {
            return last_error("could not initialize repository");
        }
        return Repository(repo);
    }

    RepoState Repository::state() const noexcept
    {
        switch (git_repository_state(m_repo.get()))
        {
        case GIT_REPOSITORY_STATE_NONE:
            return RepoState::NONE;
        case GIT_REPOSITORY_STATE_MERGE:
            return RepoState::MERGE;
        case GIT_REPOSITORY_STATE_REVERT:
        case GIT_REPOSITORY_STATE_REVERT_SEQUENCE:
            return RepoState::REVERT;
        case GIT_REPOSITORY_STATE_CHERRYPICK:
        case GIT_REPOSITORY_STATE_CHERRYPICK_SEQUENCE:
            return RepoState::CHERRYPICK;
        case GIT_REPOSITORY_STATE_REBASE:
        case GIT_REPOSITORY_STATE_REBASE_INTERACTIVE:
        case GIT_REPOSITORY_STATE_REBASE_MERGE:
            return RepoState::REBASE;
        default:
            return RepoState::OTHER;
        }
    }

    Result<History> Repository::load_history(const HistoryOptions& options) const
    {
        git_repository* repo = m_repo.get();
        History history;
        const std::size_t max_commits = options.max_commits;
        const auto walkable = [&](const std::string& full_name)
        {
            if (!options.solo_ref.empty())
            {
                return full_name == options.solo_ref;
            }
            return !options.hidden_refs.contains(full_name);
        };

        // HEAD
        {
            ReferenceHandle head;
            const int rc = git_repository_head(head.out(), repo);
            if (rc == GIT_EUNBORNBRANCH)
            {
                history.unborn = true;
            }
            else if (rc == GIT_ENOTFOUND)
            {
                history.unborn = true;
            }
            else if (rc < 0)
            {
                return last_error("could not read HEAD");
            }
            else
            {
                history.detached = git_repository_head_detached(repo) == 1;
                if (const git_oid* oid = git_reference_target(head); oid != nullptr)
                {
                    history.head_sha = oid_str(oid);
                }
                if (!history.detached)
                {
                    const char* name = nullptr;
                    if (git_branch_name(&name, head) == 0 && name != nullptr)
                    {
                        history.head_branch = name;
                    }
                }
            }
        }

        RevwalkHandle walk;
        IMGIT_TRY(git_revwalk_new(walk.out(), repo), "could not create revision walker");
        git_revwalk_sorting(walk, GIT_SORT_TOPOLOGICAL | GIT_SORT_TIME);

        std::unordered_set<std::string> pushed;
        auto push = [&](const git_oid* oid)
        {
            const std::string key = oid_str(oid);
            if (pushed.insert(key).second)
            {
                git_revwalk_push(walk, oid);
            }
        };
        if (!history.head_sha.empty())
        {
            git_oid oid;
            git_oid_fromstr(&oid, history.head_sha.c_str());
            push(&oid);
        }

        // Branches
        {
            BranchIteratorHandle it;
            IMGIT_TRY(git_branch_iterator_new(it.out(), repo, GIT_BRANCH_ALL), "could not list branches");
            git_reference* raw = nullptr;
            git_branch_t type;
            while (git_branch_next(&raw, &type, it) == 0)
            {
                ReferenceHandle ref;
                ref.ptr = raw;
                if (git_reference_type(ref) != GIT_REFERENCE_DIRECT)
                {
                    continue; // origin/HEAD
                }
                const char* name = nullptr;
                if (git_branch_name(&name, ref) < 0 || name == nullptr)
                {
                    continue;
                }
                Ref out;
                out.name = name;
                out.full_name = git_reference_name(ref);
                out.target = oid_str(git_reference_target(ref));
                if (type == GIT_BRANCH_REMOTE)
                {
                    out.type = RefType::REMOTE_BRANCH;
                    if (const auto slash = out.name.find('/'); slash != std::string::npos)
                    {
                        out.remote = out.name.substr(0, slash);
                    }
                }
                else
                {
                    out.type = RefType::LOCAL_BRANCH;
                    out.is_head = git_branch_is_head(ref) == 1;
                    ReferenceHandle upstream;
                    if (git_branch_upstream(upstream.out(), ref) == 0)
                    {
                        const char* upstream_name = nullptr;
                        if (git_branch_name(&upstream_name, upstream) == 0 && upstream_name != nullptr)
                        {
                            out.upstream = upstream_name;
                        }
                        if (const git_oid* up_oid = git_reference_target(upstream); up_oid != nullptr)
                        {
                            std::size_t ahead = 0, behind = 0;
                            if (git_graph_ahead_behind(&ahead, &behind, repo, git_reference_target(ref), up_oid) == 0)
                            {
                                out.ahead = static_cast<int>(ahead);
                                out.behind = static_cast<int>(behind);
                            }
                        }
                    }
                }
                out.hidden = !walkable(out.full_name);
                if (!out.hidden)
                {
                    push(git_reference_target(ref));
                }
                history.refs.push_back(std::move(out));
            }
        }

        // Tags
        {
            struct TagPayload
            {
                git_repository* repo;
                History* history;
                decltype(push)* push_tip;
                decltype(walkable)* is_walkable;
            } payload{repo, &history, &push, &walkable};
            auto callback = [](const char* name, git_oid* oid, void* data) -> int
            {
                auto* p = static_cast<TagPayload*>(data);
                Ref out;
                out.type = RefType::TAG;
                out.full_name = name;
                out.name = out.full_name.starts_with("refs/tags/") ? out.full_name.substr(10) : out.full_name;
                ObjectHandle object;
                if (git_object_lookup(object.out(), p->repo, oid, GIT_OBJECT_ANY) < 0)
                {
                    return 0;
                }
                if (git_object_type(object) == GIT_OBJECT_TAG)
                {
                    out.annotated = true;
                    auto* tag = reinterpret_cast<git_tag*>(object.get());
                    if (const char* message = git_tag_message(tag); message != nullptr)
                    {
                        out.message = message;
                    }
                    ObjectHandle target;
                    if (git_tag_peel(target.out(), tag) < 0 || git_object_type(target) != GIT_OBJECT_COMMIT)
                    {
                        return 0;
                    }
                    out.target = oid_str(git_object_id(target));
                }
                else if (git_object_type(object) == GIT_OBJECT_COMMIT)
                {
                    out.target = oid_str(oid);
                }
                else
                {
                    return 0;
                }
                out.hidden = !(*p->is_walkable)(out.full_name);
                if (!out.hidden)
                {
                    git_oid target_oid;
                    git_oid_fromstr(&target_oid, out.target.c_str());
                    (*p->push_tip)(&target_oid);
                }
                p->history->refs.push_back(std::move(out));
                return 0;
            };
            git_tag_foreach(repo, callback, &payload);
        }

        // Submodules and worktrees (left panel sections)
        {
            auto callback = [](git_submodule* submodule, const char* name, void* data) -> int
            {
                auto* out = static_cast<std::vector<Submodule>*>(data);
                const char* path = git_submodule_path(submodule);
                out->push_back(Submodule{name != nullptr ? name : "", path != nullptr ? path : ""});
                return 0;
            };
            git_submodule_foreach(repo, callback, &history.submodules);

            git_strarray names = {};
            if (git_worktree_list(&names, repo) == 0)
            {
                for (std::size_t i = 0; i < names.count; ++i)
                {
                    Worktree worktree;
                    worktree.name = names.strings[i];
                    git_worktree* raw = nullptr;
                    if (git_worktree_lookup(&raw, repo, names.strings[i]) == 0)
                    {
                        if (const char* path = git_worktree_path(raw); path != nullptr)
                        {
                            worktree.path = path;
                        }
                        git_worktree_free(raw);
                    }
                    history.worktrees.push_back(std::move(worktree));
                }
                git_strarray_dispose(&names);
            }
        }

        // Stashes (not pushed to the walk: they are inserted as rows next to their base commit below)
        std::vector<Ref> stashes;
        {
            auto callback = [](std::size_t index, const char* message, const git_oid* stash_id, void* data) -> int
            {
                auto* out = static_cast<std::vector<Ref>*>(data);
                Ref ref;
                ref.type = RefType::STASH;
                ref.stash_index = static_cast<int>(index);
                ref.name = "stash@{" + std::to_string(index) + "}";
                ref.full_name = "refs/stash";
                ref.target = oid_str(stash_id);
                ref.message = message != nullptr ? message : "";
                out->push_back(std::move(ref));
                return 0;
            };
            git_stash_foreach(repo, callback, &stashes);
        }

        // Remotes
        {
            git_strarray names = {};
            if (git_remote_list(&names, repo) == 0)
            {
                for (std::size_t i = 0; i < names.count; ++i)
                {
                    Remote remote;
                    remote.name = names.strings[i];
                    RemoteHandle handle;
                    if (git_remote_lookup(handle.out(), repo, names.strings[i]) == 0)
                    {
                        if (const char* url = git_remote_url(handle); url != nullptr)
                        {
                            remote.url = url;
                        }
                    }
                    history.remotes.push_back(std::move(remote));
                }
                git_strarray_dispose(&names);
            }
        }

        const auto read_commit = [](git_commit* commit, const Sha& sha)
        {
            Commit out;
            out.sha = sha;
            const unsigned parent_count = git_commit_parentcount(commit);
            out.parents.reserve(parent_count);
            for (unsigned i = 0; i < parent_count; ++i)
            {
                out.parents.push_back(oid_str(git_commit_parent_id(commit, i)));
            }
            const git_signature* author = git_commit_author(commit);
            out.author.name = author->name != nullptr ? author->name : "";
            out.author.email = author->email != nullptr ? author->email : "";
            out.time = static_cast<std::int64_t>(git_commit_time(commit));
            out.author_time = static_cast<std::int64_t>(author->when.time);
            out.summary = git_commit_summary(commit) != nullptr ? git_commit_summary(commit) : "";
            out.body = git_commit_body(commit) != nullptr ? git_commit_body(commit) : "";
            out.type = parent_count > 1 ? NodeType::MERGE : NodeType::COMMIT;
            return out;
        };

        // A stash commit's other parents (its index and untracked-files commits) are never rows.
        std::unordered_set<Sha> stash_internals;
        std::unordered_map<Sha, const Ref*> stash_by_sha;
        for (const Ref& stash : stashes)
        {
            CommitHandle commit;
            git_oid stash_oid;
            if (git_oid_fromstr(&stash_oid, stash.target.c_str()) < 0
                || git_commit_lookup(commit.out(), repo, &stash_oid) < 0)
            {
                continue;
            }
            stash_by_sha.emplace(stash.target, &stash);
            for (unsigned i = 1; i < git_commit_parentcount(commit); ++i)
            {
                stash_internals.insert(oid_str(git_commit_parent_id(commit, i)));
            }
        }

        // The row order: `git rev-list --date-order` from the visible refs in name
        // order, then the stashes; commits with the same date keep git's queue order, and stashes take their
        // place by date.
        std::vector<Sha> ordered;
        if (cli::available())
        {
            std::vector<const Ref*> tips;
            for (const Ref& ref : history.refs)
            {
                if (!ref.hidden)
                {
                    tips.push_back(&ref);
                }
            }
            std::stable_sort(tips.begin(), tips.end(),
                             [](const Ref* x, const Ref* y)
                             {
                                 return x->full_name < y->full_name;
                             });
            std::string input;
            for (const Ref* ref : tips)
            {
                input += ref->target + "\n";
            }
            // The checked-out commit is always a row, even when its branch is hidden or another ref is soloed.
            if (!history.head_sha.empty())
            {
                input += history.head_sha + "\n";
            }
            if (options.solo_ref.empty())
            {
                for (const Ref& stash : stashes)
                {
                    input += stash.target + "\n";
                }
            }
            if (!input.empty())
            {
                const cli::Output listed =
                        cli::run(m_workdir,
                                 {"rev-list", "--date-order", "--ignore-missing",
                                  "--max-count=" + std::to_string(max_commits + 1 + stash_internals.size()), "--stdin"},
                                 input);
                if (listed.exit_code == 0)
                {
                    std::size_t start = 0;
                    while (start < listed.text.size())
                    {
                        std::size_t end = listed.text.find('\n', start);
                        end = end == std::string::npos ? listed.text.size() : end;
                        std::string line = listed.text.substr(start, end - start);
                        if (!line.empty() && line.back() == '\r')
                        {
                            line.pop_back();
                        }
                        if (line.size() == 40 && line.find_first_not_of("0123456789abcdef") == std::string::npos)
                        {
                            ordered.push_back(std::move(line));
                        }
                        start = end + 1;
                    }
                }
            }
        }

        if (!ordered.empty())
        {
            history.commits.reserve(std::min<std::size_t>(max_commits, ordered.size()));
            for (const Sha& sha : ordered)
            {
                if (stash_internals.contains(sha))
                {
                    continue;
                }
                if (history.commits.size() >= max_commits)
                {
                    history.has_more = true;
                    break;
                }
                CommitHandle commit;
                git_oid oid;
                if (git_oid_fromstr(&oid, sha.c_str()) < 0 || git_commit_lookup(commit.out(), repo, &oid) < 0)
                {
                    continue;
                }
                Commit out = read_commit(commit, sha);
                if (const auto stash = stash_by_sha.find(sha); stash != stash_by_sha.end())
                {
                    // A stash row: a dashed node on its base commit.
                    out.parents.resize(std::min<std::size_t>(out.parents.size(), 1));
                    out.summary = stash_summary(stash->second->message);
                    out.body.clear();
                    out.type = NodeType::STASH;
                    history.refs.push_back(*stash->second);
                }
                history.commits.push_back(std::move(out));
            }
        }
        else
        {
            // Without git: libgit2's topological walk, stashes inserted right above their base commit.
            git_oid oid;
            std::size_t walked = 0;
            history.commits.reserve(std::min<std::size_t>(max_commits, 4096));
            while (git_revwalk_next(&oid, walk) == 0)
            {
                if (walked >= max_commits)
                {
                    history.has_more = true;
                    break;
                }
                ++walked;
                CommitHandle commit;
                if (git_commit_lookup(commit.out(), repo, &oid) < 0)
                {
                    continue;
                }
                history.commits.push_back(read_commit(commit, oid_str(&oid)));
            }
            if (!stashes.empty())
            {
                std::unordered_map<Sha, std::size_t> index_by_sha;
                for (std::size_t i = 0; i < history.commits.size(); ++i)
                {
                    index_by_sha.emplace(history.commits[i].sha, i);
                }
                std::vector<std::pair<std::size_t, Commit>> inserts;
                for (const Ref& stash : stashes)
                {
                    CommitHandle commit;
                    git_oid stash_oid;
                    if (git_oid_fromstr(&stash_oid, stash.target.c_str()) < 0
                        || git_commit_lookup(commit.out(), repo, &stash_oid) < 0 || git_commit_parentcount(commit) == 0)
                    {
                        continue;
                    }
                    const Sha base = oid_str(git_commit_parent_id(commit, 0));
                    const auto base_row = index_by_sha.find(base);
                    if (base_row == index_by_sha.end())
                    {
                        continue;
                    }
                    Commit out;
                    out.sha = stash.target;
                    out.parents = {base};
                    const git_signature* author = git_commit_author(commit);
                    out.author.name = author->name != nullptr ? author->name : "";
                    out.author.email = author->email != nullptr ? author->email : "";
                    out.time = static_cast<std::int64_t>(git_commit_time(commit));
                    out.author_time = static_cast<std::int64_t>(author->when.time);
                    out.summary = stash_summary(stash.message);
                    out.type = NodeType::STASH;
                    inserts.emplace_back(base_row->second, std::move(out));
                    history.refs.push_back(stash);
                }
                // Insert from the bottom up so earlier indices stay valid.
                std::stable_sort(inserts.begin(), inserts.end(),
                                 [](const auto& x, const auto& y)
                                 {
                                     return x.first > y.first;
                                 });
                for (auto& [row, commit] : inserts)
                {
                    history.commits.insert(history.commits.begin() + static_cast<std::ptrdiff_t>(row),
                                           std::move(commit));
                }
            }
        }

        for (std::size_t i = 0; i < history.refs.size(); ++i)
        {
            history.refs_by_sha[history.refs[i].target].push_back(i);
        }
        for (std::size_t i = 0; i < history.commits.size(); ++i)
        {
            history.row_by_sha.emplace(history.commits[i].sha, i);
        }
        return history;
    }

    Result<RepoSummary> Repository::summary() const
    {
        RepoSummary result;
        git_repository* repo = m_repo.get();
        ReferenceHandle head;
        if (git_repository_head(head.out(), repo) == 0 && git_reference_is_branch(head) != 0)
        {
            const char* name = nullptr;
            if (git_branch_name(&name, head) == 0 && name != nullptr)
            {
                result.branch = name;
            }
            ReferenceHandle upstream;
            if (git_branch_upstream(upstream.out(), head) == 0)
            {
                std::size_t ahead = 0, behind = 0;
                const git_oid* local = git_reference_target(head);
                const git_oid* remote = git_reference_target(upstream);
                if (local != nullptr && remote != nullptr
                    && git_graph_ahead_behind(&ahead, &behind, repo, local, remote) == 0)
                {
                    result.ahead = static_cast<int>(ahead);
                    result.behind = static_cast<int>(behind);
                }
            }
        }
        if (auto status = load_status())
        {
            std::map<std::string, FileStatus> files;
            const auto note = [&](const FileChange& change)
            {
                auto [it, inserted] = files.emplace(change.path, change.status);
                if (!inserted
                    && (change.status == FileStatus::ADDED || change.status == FileStatus::UNTRACKED
                        || change.status == FileStatus::DELETED))
                {
                    it->second = change.status;
                }
            };
            for (const FileChange& change : status->staged)
            {
                note(change);
            }
            for (const FileChange& change : status->unstaged)
            {
                note(change);
            }
            for (const FileChange& change : status->conflicted)
            {
                note(change);
            }
            for (const auto& [path, file_status] : files)
            {
                switch (file_status)
                {
                case FileStatus::ADDED:
                case FileStatus::UNTRACKED:
                    ++result.added;
                    break;
                case FileStatus::DELETED:
                    ++result.deleted;
                    break;
                default:
                    ++result.modified;
                    break;
                }
            }
        }
        git_remote* origin = nullptr;
        if (git_remote_lookup(&origin, repo, "origin") == 0)
        {
            const char* url = git_remote_url(origin);
            std::string text = url != nullptr ? url : "";
            git_remote_free(origin);
            // https://host/owner/repo(.git) or git@host:owner/repo(.git)
            std::size_t start = text.find("://");
            start = start == std::string::npos ? text.find(':') : text.find('/', start + 3);
            if (start != std::string::npos)
            {
                const std::size_t end = text.find('/', start + 1);
                if (end != std::string::npos)
                {
                    result.owner = text.substr(start + 1, end - start - 1);
                }
            }
        }
        return result;
    }

    Result<WorkDirStatus> Repository::load_status() const
    {
        git_status_options opts = GIT_STATUS_OPTIONS_INIT;
        opts.show = GIT_STATUS_SHOW_INDEX_AND_WORKDIR;
        opts.flags = GIT_STATUS_OPT_INCLUDE_UNTRACKED | GIT_STATUS_OPT_RECURSE_UNTRACKED_DIRS
                     | GIT_STATUS_OPT_RENAMES_HEAD_TO_INDEX | GIT_STATUS_OPT_RENAMES_INDEX_TO_WORKDIR
                     | GIT_STATUS_OPT_SORT_CASE_INSENSITIVELY;
        StatusListHandle list;
        IMGIT_TRY(git_status_list_new(list.out(), m_repo.get(), &opts), "could not read status");

        WorkDirStatus status;
        const std::size_t count = git_status_list_entrycount(list);
        for (std::size_t i = 0; i < count; ++i)
        {
            const git_status_entry* entry = git_status_byindex(list, i);
            if (entry->status == GIT_STATUS_CURRENT)
            {
                continue;
            }
            if ((entry->status & GIT_STATUS_CONFLICTED) != 0)
            {
                const git_diff_delta* delta =
                        entry->index_to_workdir != nullptr ? entry->index_to_workdir : entry->head_to_index;
                FileChange change = change_from_delta(delta);
                change.status = FileStatus::CONFLICTED;
                status.conflicted.push_back(std::move(change));
                continue;
            }
            if (entry->head_to_index != nullptr
                && (entry->status
                    & (GIT_STATUS_INDEX_NEW | GIT_STATUS_INDEX_MODIFIED | GIT_STATUS_INDEX_DELETED
                       | GIT_STATUS_INDEX_RENAMED | GIT_STATUS_INDEX_TYPECHANGE))
                           != 0)
            {
                status.staged.push_back(change_from_delta(entry->head_to_index));
            }
            if (entry->index_to_workdir != nullptr
                && (entry->status
                    & (GIT_STATUS_WT_NEW | GIT_STATUS_WT_MODIFIED | GIT_STATUS_WT_DELETED | GIT_STATUS_WT_RENAMED
                       | GIT_STATUS_WT_TYPECHANGE))
                           != 0)
            {
                FileChange change = change_from_delta(entry->index_to_workdir);
                if ((entry->status & GIT_STATUS_WT_NEW) != 0)
                {
                    change.status = FileStatus::UNTRACKED;
                }
                status.unstaged.push_back(std::move(change));
            }
        }
        return status;
    }

    Result<std::vector<FileChange>> Repository::commit_changes(const Sha& sha) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        auto tree = commit_tree(*commit);
        if (!tree)
        {
            return std::unexpected(tree.error());
        }
        TreeHandle parent_tree;
        if (git_commit_parentcount(*commit) > 0)
        {
            CommitHandle parent;
            IMGIT_TRY(git_commit_parent(parent.out(), *commit, 0), "could not read parent");
            IMGIT_TRY(git_commit_tree(parent_tree.out(), parent), "could not read parent tree");
        }
        git_diff_options opts = GIT_DIFF_OPTIONS_INIT;
        DiffHandle diff;
        IMGIT_TRY(git_diff_tree_to_tree(diff.out(), m_repo.get(), parent_tree, *tree, &opts), "could not diff commit");
        git_diff_find_similar(diff, nullptr);

        std::vector<FileChange> changes;
        const std::size_t count = git_diff_num_deltas(diff);
        changes.reserve(count);
        for (std::size_t i = 0; i < count; ++i)
        {
            changes.push_back(change_from_delta(git_diff_get_delta(diff, i)));
        }
        return changes;
    }

    Result<FileDiff> Repository::commit_file_diff(const Sha& sha, const std::string& path, int context_lines,
                                                  bool ignore_whitespace) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        auto tree = commit_tree(*commit);
        if (!tree)
        {
            return std::unexpected(tree.error());
        }
        TreeHandle parent_tree;
        if (git_commit_parentcount(*commit) > 0)
        {
            CommitHandle parent;
            IMGIT_TRY(git_commit_parent(parent.out(), *commit, 0), "could not read parent");
            IMGIT_TRY(git_commit_tree(parent_tree.out(), parent), "could not read parent tree");
        }
        std::vector<std::string> paths{path};
        auto c_paths = c_strings(paths);
        git_diff_options opts = GIT_DIFF_OPTIONS_INIT;
        opts.pathspec.strings = c_paths.data();
        opts.pathspec.count = c_paths.size();
        opts.flags |= GIT_DIFF_DISABLE_PATHSPEC_MATCH;
        if (ignore_whitespace)
        {
            opts.flags |= GIT_DIFF_IGNORE_WHITESPACE;
        }
        opts.context_lines = static_cast<std::uint32_t>(context_lines);
        DiffHandle diff;
        IMGIT_TRY(git_diff_tree_to_tree(diff.out(), m_repo.get(), parent_tree, *tree, &opts), "could not diff file");
        const std::size_t count = git_diff_num_deltas(diff);
        if (count == 1 && git_diff_get_delta(diff, 0)->status != GIT_DELTA_ADDED)
        {
            return diff_to_file_diff(diff, 0, path);
        }
        // An added file may really be the new side of a rename: re-run over the whole tree with similarity
        // detection and pick the delta that ends at `path`.
        git_diff_options all = GIT_DIFF_OPTIONS_INIT;
        if (ignore_whitespace)
        {
            all.flags |= GIT_DIFF_IGNORE_WHITESPACE;
        }
        all.context_lines = static_cast<std::uint32_t>(context_lines);
        DiffHandle full;
        IMGIT_TRY(git_diff_tree_to_tree(full.out(), m_repo.get(), parent_tree, *tree, &all), "could not diff file");
        git_diff_find_similar(full, nullptr);
        const std::size_t full_count = git_diff_num_deltas(full);
        for (std::size_t i = 0; i < full_count; ++i)
        {
            const git_diff_delta* delta = git_diff_get_delta(full, i);
            if (delta->new_file.path != nullptr && path == delta->new_file.path)
            {
                return diff_to_file_diff(full, i, path);
            }
        }
        return diff_to_file_diff(diff, 0, path);
    }

    Result<FileDiff> Repository::workdir_file_diff(const std::string& path, bool staged, int context_lines,
                                                   bool ignore_whitespace) const
    {
        git_repository* repo = m_repo.get();
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
        std::vector<std::string> paths{path};
        auto c_paths = c_strings(paths);
        git_diff_options opts = GIT_DIFF_OPTIONS_INIT;
        opts.pathspec.strings = c_paths.data();
        opts.pathspec.count = c_paths.size();
        opts.flags |= GIT_DIFF_DISABLE_PATHSPEC_MATCH | GIT_DIFF_INCLUDE_UNTRACKED | GIT_DIFF_SHOW_UNTRACKED_CONTENT
                      | GIT_DIFF_RECURSE_UNTRACKED_DIRS;
        if (ignore_whitespace)
        {
            opts.flags |= GIT_DIFF_IGNORE_WHITESPACE;
        }
        opts.context_lines = static_cast<std::uint32_t>(context_lines);
        DiffHandle diff;
        if (staged)
        {
            auto head = head_commit(repo);
            if (!head)
            {
                return std::unexpected(head.error());
            }
            TreeHandle head_tree;
            if (head->get() != nullptr)
            {
                IMGIT_TRY(git_commit_tree(head_tree.out(), *head), "could not read HEAD tree");
            }
            IMGIT_TRY(git_diff_tree_to_index(diff.out(), repo, head_tree, index, &opts), "could not diff index");
        }
        else
        {
            IMGIT_TRY(git_diff_index_to_workdir(diff.out(), repo, index, &opts), "could not diff working directory");
        }
        return diff_to_file_diff(diff, 0, path);
    }

    Result<Author> Repository::default_author() const
    {
        auto sig = default_signature(m_repo.get());
        if (!sig)
        {
            return std::unexpected(sig.error());
        }
        return Author{sig->get()->name, sig->get()->email};
    }

    Result<std::string> Repository::head_commit_message() const
    {
        auto head = head_commit(m_repo.get());
        if (!head)
        {
            return std::unexpected(head.error());
        }
        if (head->get() == nullptr)
        {
            return std::string{};
        }
        const char* message = git_commit_message(*head);
        return std::string(message != nullptr ? message : "");
    }

    Result<void> Repository::stage(const std::vector<std::string>& paths) const
    {
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), m_repo.get()), "could not read index");
        for (const std::string& path : paths)
        {
            const std::filesystem::path on_disk = std::filesystem::path(m_workdir) / path;
            // symlink_status: a dangling symlink is still a file to stage, and nothing throws.
            std::error_code ec;
            const std::filesystem::file_type type = std::filesystem::symlink_status(on_disk, ec).type();
            if (ec)
            {
                return std::unexpected(Error{path + ": " + ec.message()});
            }
            if (type != std::filesystem::file_type::not_found)
            {
                IMGIT_TRY(git_index_add_bypath(index, path.c_str()), "could not stage file");
            }
            else
            {
                IMGIT_TRY(git_index_remove_bypath(index, path.c_str()), "could not stage deletion");
            }
        }
        IMGIT_TRY(git_index_write(index), "could not write index");
        return {};
    }

    Result<void> Repository::stage_all() const
    {
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), m_repo.get()), "could not read index");
        IMGIT_TRY(git_index_add_all(index, nullptr, GIT_INDEX_ADD_DEFAULT, nullptr, nullptr), "could not stage files");
        IMGIT_TRY(git_index_update_all(index, nullptr, nullptr, nullptr), "could not stage deletions");
        IMGIT_TRY(git_index_write(index), "could not write index");
        return {};
    }

    Result<void> Repository::unstage(const std::vector<std::string>& paths) const
    {
        git_repository* repo = m_repo.get();
        auto head = head_commit(repo);
        if (!head)
        {
            return std::unexpected(head.error());
        }
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
        if (head->get() == nullptr)
        {
            for (const std::string& path : paths)
            {
                git_index_remove_bypath(index, path.c_str());
            }
            IMGIT_TRY(git_index_write(index), "could not write index");
            return {};
        }
        // Each path back to HEAD's entry, or out of the index when HEAD does not have it. By exact path:
        // git_reset_default reads the paths as wildmatch patterns ([ ] * ?).
        auto tree = commit_tree(*head);
        if (!tree)
        {
            return std::unexpected(tree.error());
        }
        for (const std::string& path : paths)
        {
            if (const int rc = git_index_conflict_remove(index, path.c_str()); rc < 0 && rc != GIT_ENOTFOUND)
            {
                return last_error("could not unstage");
            }
            TreeEntryHandle entry;
            const int found = git_tree_entry_bypath(entry.out(), *tree, path.c_str());
            if (found == GIT_ENOTFOUND)
            {
                if (const int rc = git_index_remove_bypath(index, path.c_str()); rc < 0 && rc != GIT_ENOTFOUND)
                {
                    return last_error("could not unstage");
                }
                continue;
            }
            IMGIT_TRY(found, "could not unstage");
            git_index_entry head_entry = {};
            head_entry.mode = static_cast<std::uint32_t>(git_tree_entry_filemode(entry));
            head_entry.id = *git_tree_entry_id(entry);
            head_entry.path = path.c_str();
            IMGIT_TRY(git_index_add(index, &head_entry), "could not unstage");
        }
        IMGIT_TRY(git_index_write(index), "could not write index");
        return {};
    }

    Result<void> Repository::unstage_all() const
    {
        auto status = load_status();
        if (!status)
        {
            return std::unexpected(status.error());
        }
        std::vector<std::string> paths;
        for (const FileChange& change : status->staged)
        {
            paths.push_back(change.path);
            if (!change.old_path.empty())
            {
                paths.push_back(change.old_path);
            }
        }
        if (paths.empty())
        {
            return {};
        }
        return unstage(paths);
    }

    Result<void> Repository::discard(const std::vector<FileChange>& changes) const
    {
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), m_repo.get()), "could not read index");
        std::vector<std::string> tracked;
        for (const FileChange& change : changes)
        {
            if (change.status == FileStatus::UNTRACKED)
            {
                std::error_code ec;
                std::filesystem::remove_all(std::filesystem::path(m_workdir) / change.path, ec);
            }
            else if (!change.old_path.empty() && change.old_path != change.path
                     && git_index_get_bypath(index, change.path.c_str(), 0) == nullptr)
            {
                // An unstaged rename: the new name is not in the index, so it goes and the old name comes back.
                std::error_code ec;
                std::filesystem::remove(std::filesystem::path(m_workdir) / change.path, ec);
                tracked.push_back(change.old_path);
            }
            else
            {
                tracked.push_back(change.path);
            }
        }
        if (tracked.empty())
        {
            return {};
        }
        auto c_paths = c_strings(tracked);
        git_checkout_options opts = GIT_CHECKOUT_OPTIONS_INIT;
        // The paths are literal, not patterns: [ ] * ? may be part of a file name.
        opts.checkout_strategy =
                GIT_CHECKOUT_FORCE | GIT_CHECKOUT_RECREATE_MISSING | GIT_CHECKOUT_DISABLE_PATHSPEC_MATCH;
        opts.paths.strings = c_paths.data();
        opts.paths.count = c_paths.size();
        IMGIT_TRY(git_checkout_index(m_repo.get(), nullptr, &opts), "could not discard changes");
        return {};
    }

    Result<void> Repository::discard_all() const
    {
        // Reset All: staged changes too, new files included (unstaged first, then discarded).
        if (auto unstaged = unstage_all(); !unstaged)
        {
            return unstaged;
        }
        auto status = load_status();
        if (!status)
        {
            return std::unexpected(status.error());
        }
        return discard(status->unstaged);
    }

    Result<Sha> Repository::commit_index(const std::string& message, const std::vector<Sha>& parents,
                                         const git_signature* author) const
    {
        git_repository* repo = m_repo.get();
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
        git_oid tree_oid;
        IMGIT_TRY(git_index_write_tree(&tree_oid, index), "could not write tree");
        TreeHandle tree;
        IMGIT_TRY(git_tree_lookup(tree.out(), repo, &tree_oid), "could not read tree");

        auto committer = default_signature(repo);
        if (!committer)
        {
            return std::unexpected(committer.error());
        }

        std::vector<CommitHandle> parent_handles(parents.size());
        std::vector<const git_commit*> parent_ptrs;
        for (std::size_t i = 0; i < parents.size(); ++i)
        {
            auto parent = lookup_commit(repo, parents[i]);
            if (!parent)
            {
                return std::unexpected(parent.error());
            }
            parent_handles[i].ptr = parent->ptr;
            parent->ptr = nullptr;
            parent_ptrs.push_back(parent_handles[i].get());
        }

        git_oid commit_oid;
        IMGIT_TRY(git_commit_create(&commit_oid, repo, "HEAD", author != nullptr ? author : committer->get(),
                                    committer->get(), nullptr, message.c_str(), tree, parent_ptrs.size(),
                                    parent_ptrs.data()),
                  "could not create commit");
        return oid_str(&commit_oid);
    }

    Result<Sha> Repository::commit(const std::string& message, bool amend) const
    {
        git_repository* repo = m_repo.get();
        auto head = head_commit(repo);
        if (!head)
        {
            return std::unexpected(head.error());
        }
        if (amend)
        {
            if (head->get() == nullptr)
            {
                return std::unexpected(Error{"nothing to amend"});
            }
            IndexHandle index;
            IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
            git_oid tree_oid;
            IMGIT_TRY(git_index_write_tree(&tree_oid, index), "could not write tree");
            TreeHandle tree;
            IMGIT_TRY(git_tree_lookup(tree.out(), repo, &tree_oid), "could not read tree");
            auto committer = default_signature(repo);
            if (!committer)
            {
                return std::unexpected(committer.error());
            }
            git_oid commit_oid;
            IMGIT_TRY(git_commit_amend(&commit_oid, *head, "HEAD", nullptr, committer->get(), nullptr, message.c_str(),
                                       tree),
                      "could not amend commit");
            return oid_str(&commit_oid);
        }
        std::vector<Sha> parents;
        if (head->get() != nullptr)
        {
            parents.push_back(oid_str(git_commit_id(*head)));
        }
        if (git_repository_state(repo) == GIT_REPOSITORY_STATE_MERGE)
        {
            // Add MERGE_HEAD as the second parent.
            auto callback = [](const git_oid* oid, void* data) -> int
            {
                static_cast<std::vector<Sha>*>(data)->push_back(oid_str(oid));
                return 0;
            };
            git_repository_mergehead_foreach(repo, callback, &parents);
        }
        auto sha = commit_index(message, parents, nullptr);
        if (sha)
        {
            finish_operation_state(repo);
        }
        return sha;
    }

    Result<void> Repository::create_branch(const std::string& name, const Sha& sha, bool checkout) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        ReferenceHandle ref;
        IMGIT_TRY(git_branch_create(ref.out(), m_repo.get(), name.c_str(), *commit, 0), "could not create branch");
        if (checkout)
        {
            return checkout_branch(name);
        }
        return {};
    }

    Result<void> Repository::checkout_branch(const std::string& name) const
    {
        git_repository* repo = m_repo.get();
        ReferenceHandle ref;
        IMGIT_TRY(git_branch_lookup(ref.out(), repo, name.c_str(), GIT_BRANCH_LOCAL), "branch not found");
        ObjectHandle target;
        IMGIT_TRY(git_reference_peel(target.out(), ref, GIT_OBJECT_COMMIT), "could not resolve branch");
        git_checkout_options opts = safe_checkout_options();
        IMGIT_TRY(git_checkout_tree(repo, target, &opts), "checkout failed");
        IMGIT_TRY(git_repository_set_head(repo, git_reference_name(ref)), "could not update HEAD");
        return {};
    }

    Result<void> Repository::checkout_remote_branch(const std::string& remote, const std::string& name) const
    {
        git_repository* repo = m_repo.get();
        const std::string remote_name = remote + "/" + name;
        ReferenceHandle remote_ref;
        IMGIT_TRY(git_branch_lookup(remote_ref.out(), repo, remote_name.c_str(), GIT_BRANCH_REMOTE),
                  "remote branch not found");
        ReferenceHandle local;
        if (git_branch_lookup(local.out(), repo, name.c_str(), GIT_BRANCH_LOCAL) < 0)
        {
            CommitHandle commit;
            IMGIT_TRY(git_reference_peel(reinterpret_cast<git_object**>(commit.out()), remote_ref, GIT_OBJECT_COMMIT),
                      "could not resolve remote branch");
            IMGIT_TRY(git_branch_create(local.out(), repo, name.c_str(), commit, 0), "could not create branch");
            IMGIT_TRY(git_branch_set_upstream(local, remote_name.c_str()), "could not set upstream");
        }
        return checkout_branch(name);
    }

    Result<void> Repository::checkout_commit(const Sha& sha) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        git_checkout_options opts = safe_checkout_options();
        IMGIT_TRY(git_checkout_tree(m_repo.get(), reinterpret_cast<git_object*>(commit->get()), &opts),
                  "checkout failed");
        IMGIT_TRY(git_repository_set_head_detached(m_repo.get(), git_commit_id(*commit)), "could not detach HEAD");
        return {};
    }

    Result<void> Repository::delete_branch(const std::string& name) const
    {
        ReferenceHandle ref;
        IMGIT_TRY(git_branch_lookup(ref.out(), m_repo.get(), name.c_str(), GIT_BRANCH_LOCAL), "branch not found");
        IMGIT_TRY(git_branch_delete(ref), "could not delete branch");
        return {};
    }

    Result<void> Repository::rename_branch(const std::string& name, const std::string& new_name) const
    {
        ReferenceHandle ref;
        IMGIT_TRY(git_branch_lookup(ref.out(), m_repo.get(), name.c_str(), GIT_BRANCH_LOCAL), "branch not found");
        ReferenceHandle renamed;
        IMGIT_TRY(git_branch_move(renamed.out(), ref, new_name.c_str(), 0), "could not rename branch");
        return {};
    }

    Result<void> Repository::create_tag(const std::string& name, const Sha& sha, const std::string& message) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        git_oid tag_oid;
        auto* target = reinterpret_cast<git_object*>(commit->get());
        if (message.empty())
        {
            IMGIT_TRY(git_tag_create_lightweight(&tag_oid, m_repo.get(), name.c_str(), target, 0),
                      "could not create tag");
        }
        else
        {
            auto sig = default_signature(m_repo.get());
            if (!sig)
            {
                return std::unexpected(sig.error());
            }
            IMGIT_TRY(git_tag_create(&tag_oid, m_repo.get(), name.c_str(), target, sig->get(), message.c_str(), 0),
                      "could not create tag");
        }
        return {};
    }

    Result<void> Repository::delete_tag(const std::string& name) const
    {
        IMGIT_TRY(git_tag_delete(m_repo.get(), name.c_str()), "could not delete tag");
        return {};
    }

    Result<void> Repository::reset(const Sha& sha, ResetMode mode) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        git_reset_t kind = GIT_RESET_MIXED;
        switch (mode)
        {
        case ResetMode::SOFT:
            kind = GIT_RESET_SOFT;
            break;
        case ResetMode::MIXED:
            kind = GIT_RESET_MIXED;
            break;
        case ResetMode::HARD:
            kind = GIT_RESET_HARD;
            break;
        }
        git_checkout_options opts = GIT_CHECKOUT_OPTIONS_INIT;
        opts.checkout_strategy = GIT_CHECKOUT_FORCE;
        IMGIT_TRY(git_reset(m_repo.get(), reinterpret_cast<git_object*>(commit->get()), kind, &opts), "reset failed");
        return {};
    }

    Result<bool> Repository::finish_sequencer_commit(const std::string& message, const git_signature* author) const
    {
        git_repository* repo = m_repo.get();
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
        if (git_index_has_conflicts(index))
        {
            return false;
        }
        auto head = head_commit(repo);
        if (!head)
        {
            return std::unexpected(head.error());
        }
        std::vector<Sha> parents;
        if (head->get() != nullptr)
        {
            parents.push_back(oid_str(git_commit_id(*head)));
        }
        auto sha = commit_index(message, parents, author);
        if (!sha)
        {
            return std::unexpected(sha.error());
        }
        finish_operation_state(repo);
        return true;
    }

    Result<bool> Repository::cherry_pick(const Sha& sha) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        git_cherrypick_options opts = GIT_CHERRYPICK_OPTIONS_INIT;
        opts.checkout_opts.checkout_strategy = GIT_CHECKOUT_SAFE | GIT_CHECKOUT_ALLOW_CONFLICTS;
        IMGIT_TRY(git_cherrypick(m_repo.get(), *commit, &opts), "cherry pick failed");
        // The author (with its date) belongs to `commit`, which outlives the call.
        const char* message = git_commit_message(*commit);
        return finish_sequencer_commit(message != nullptr ? message : "", git_commit_author(*commit));
    }

    Result<bool> Repository::revert(const Sha& sha) const
    {
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        git_revert_options opts = GIT_REVERT_OPTIONS_INIT;
        opts.checkout_opts.checkout_strategy = GIT_CHECKOUT_SAFE | GIT_CHECKOUT_ALLOW_CONFLICTS;
        IMGIT_TRY(git_revert(m_repo.get(), *commit, &opts), "revert failed");
        const char* summary = git_commit_summary(*commit);
        const std::string message = "Revert \"" + std::string(summary != nullptr ? summary : "")
                                    + "\"\n\nThis reverts commit " + sha + ".\n";
        return finish_sequencer_commit(message, nullptr);
    }

    bool Repository::is_ancestor(const Sha& ancestor, const Sha& descendant) const
    {
        git_oid a;
        git_oid d;
        if (git_oid_fromstr(&a, ancestor.c_str()) < 0 || git_oid_fromstr(&d, descendant.c_str()) < 0)
        {
            return false;
        }
        return git_graph_descendant_of(m_repo.get(), &d, &a) == 1;
    }

    Sha Repository::merge_base(const Sha& a, const Sha& b) const
    {
        git_oid one;
        git_oid two;
        git_oid base;
        if (git_oid_fromstr(&one, a.c_str()) < 0 || git_oid_fromstr(&two, b.c_str()) < 0
            || git_merge_base(&base, m_repo.get(), &one, &two) < 0)
        {
            return {};
        }
        return oid_str(&base);
    }

    Result<bool> Repository::merge(const std::string& branch_name, bool no_fast_forward) const
    {
        git_repository* repo = m_repo.get();
        ReferenceHandle ref;
        if (git_branch_lookup(ref.out(), repo, branch_name.c_str(), GIT_BRANCH_LOCAL) < 0)
        {
            IMGIT_TRY(git_branch_lookup(ref.out(), repo, branch_name.c_str(), GIT_BRANCH_REMOTE), "branch not found");
        }
        AnnotatedCommitHandle annotated;
        IMGIT_TRY(git_annotated_commit_from_ref(annotated.out(), repo, ref), "could not resolve branch");
        const git_annotated_commit* heads[] = {annotated.get()};

        git_merge_analysis_t analysis;
        git_merge_preference_t preference;
        IMGIT_TRY(git_merge_analysis(&analysis, &preference, repo, heads, 1), "merge analysis failed");
        if ((analysis & GIT_MERGE_ANALYSIS_UNBORN) != 0)
        {
            // An unborn HEAD (reported with FASTFORWARD): the tree is checked out, then HEAD's branch is created at
            // the incoming commit. git_repository_head would fail with EUNBORNBRANCH below.
            ReferenceHandle head;
            IMGIT_TRY(git_reference_lookup(head.out(), repo, "HEAD"), "could not read HEAD");
            const char* branch = git_reference_symbolic_target(head);
            if (branch == nullptr)
            {
                return std::unexpected(Error{"HEAD is not on a branch"});
            }
            const git_oid* target = git_annotated_commit_id(annotated);
            ObjectHandle object;
            IMGIT_TRY(git_object_lookup(object.out(), repo, target, GIT_OBJECT_COMMIT), "could not resolve commit");
            git_checkout_options opts = safe_checkout_options();
            IMGIT_TRY(git_checkout_tree(repo, object, &opts), "checkout failed");
            ReferenceHandle created;
            IMGIT_TRY(git_reference_create(created.out(), repo, branch, target, 1, "merge: fast-forward"),
                      "could not create branch");
            IMGIT_TRY(git_repository_set_head(repo, branch), "could not update HEAD");
            return true;
        }
        if ((analysis & GIT_MERGE_ANALYSIS_UP_TO_DATE) != 0)
        {
            return true;
        }
        if ((analysis & GIT_MERGE_ANALYSIS_FASTFORWARD) != 0 && (preference & GIT_MERGE_PREFERENCE_NO_FASTFORWARD) == 0
            && !no_fast_forward)
        {
            const git_oid* target = git_annotated_commit_id(annotated);
            ObjectHandle object;
            IMGIT_TRY(git_object_lookup(object.out(), repo, target, GIT_OBJECT_COMMIT), "could not resolve commit");
            git_checkout_options opts = safe_checkout_options();
            IMGIT_TRY(git_checkout_tree(repo, object, &opts), "checkout failed");
            ReferenceHandle head;
            IMGIT_TRY(git_repository_head(head.out(), repo), "could not read HEAD");
            ReferenceHandle updated;
            IMGIT_TRY(git_reference_set_target(updated.out(), head, target, "merge: fast-forward"),
                      "could not move branch");
            return true;
        }

        git_merge_options merge_opts = GIT_MERGE_OPTIONS_INIT;
        git_checkout_options checkout_opts = GIT_CHECKOUT_OPTIONS_INIT;
        checkout_opts.checkout_strategy = GIT_CHECKOUT_SAFE | GIT_CHECKOUT_ALLOW_CONFLICTS;
        IMGIT_TRY(git_merge(repo, heads, 1, &merge_opts, &checkout_opts), "merge failed");

        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
        if (git_index_has_conflicts(index))
        {
            return false;
        }
        auto head = head_commit(repo);
        if (!head)
        {
            return std::unexpected(head.error());
        }
        ReferenceHandle head_ref;
        std::string head_name = "HEAD";
        if (git_repository_head(head_ref.out(), repo) == 0)
        {
            const char* short_name = git_reference_shorthand(head_ref);
            head_name = short_name != nullptr ? short_name : "HEAD";
        }
        std::vector<Sha> parents{oid_str(git_commit_id(*head)), oid_str(git_annotated_commit_id(annotated))};
        auto sha = commit_index("Merge " + branch_name + " into " + head_name, parents, nullptr);
        if (!sha)
        {
            return std::unexpected(sha.error());
        }
        git_repository_state_cleanup(repo);
        return true;
    }

    Result<void> Repository::abort_operation() const
    {
        git_repository* repo = m_repo.get();
        // A rebase and a multi-commit cherry-pick / revert are git's to abort: resetting to HEAD and cleaning up
        // would leave HEAD detached (rebase) or the sequence half done.
        const char* verb = nullptr;
        switch (git_repository_state(repo))
        {
        case GIT_REPOSITORY_STATE_REBASE:
        case GIT_REPOSITORY_STATE_REBASE_INTERACTIVE:
        case GIT_REPOSITORY_STATE_REBASE_MERGE:
            verb = "rebase";
            break;
        case GIT_REPOSITORY_STATE_CHERRYPICK_SEQUENCE:
            verb = "cherry-pick";
            break;
        case GIT_REPOSITORY_STATE_REVERT_SEQUENCE:
            verb = "revert";
            break;
        default:
            break;
        }
        if (verb != nullptr)
        {
            if (!cli::available())
            {
                return std::unexpected(Error{std::string("git is needed to abort a ") + verb});
            }
            const cli::Output output = cli::run(m_workdir, {verb, "--abort"});
            if (output.exit_code != 0)
            {
                return std::unexpected(
                        Error{output.text.empty() ? std::string("could not abort the ") + verb : output.text});
            }
            return {};
        }
        auto head = head_commit(repo);
        if (!head)
        {
            return std::unexpected(head.error());
        }
        if (head->get() != nullptr)
        {
            git_checkout_options opts = GIT_CHECKOUT_OPTIONS_INIT;
            opts.checkout_strategy = GIT_CHECKOUT_FORCE;
            IMGIT_TRY(git_reset(repo, reinterpret_cast<git_object*>(head->get()), GIT_RESET_HARD, &opts),
                      "reset failed");
        }
        IMGIT_TRY(git_repository_state_cleanup(repo), "could not clean up state");
        return {};
    }

    Result<Repository::ConflictSides> Repository::conflict_sides(const std::string& path) const
    {
        IndexHandle index;
        IMGIT_TRY(git_repository_index(index.out(), m_repo.get()), "could not read index");
        const git_index_entry* ancestor = nullptr;
        const git_index_entry* ours = nullptr;
        const git_index_entry* theirs = nullptr;
        IMGIT_TRY(git_index_conflict_get(&ancestor, &ours, &theirs, index, path.c_str()), "the file has no conflict");
        ConflictSides sides;
        const auto read = [this](const git_index_entry* entry, std::string& text, bool& present) -> Result<void>
        {
            if (entry == nullptr)
            {
                return {};
            }
            git_blob* blob = nullptr;
            IMGIT_TRY(git_blob_lookup(&blob, m_repo.get(), &entry->id), "could not read a side of the conflict");
            text.assign(static_cast<const char*>(git_blob_rawcontent(blob)), static_cast<std::size_t>(git_blob_rawsize(blob)));
            git_blob_free(blob);
            present = true;
            return {};
        };
        if (auto r = read(ancestor, sides.base, sides.has_base); !r)
        {
            return std::unexpected(r.error());
        }
        if (auto r = read(ours, sides.ours, sides.has_ours); !r)
        {
            return std::unexpected(r.error());
        }
        if (auto r = read(theirs, sides.theirs, sides.has_theirs); !r)
        {
            return std::unexpected(r.error());
        }
        return sides;
    }

    Sha Repository::incoming_head() const
    {
        git_repository* repo = m_repo.get();
        Sha sha;
        git_repository_mergehead_foreach(
                repo,
                [](const git_oid* oid, void* data) -> int
                {
                    *static_cast<Sha*>(data) = oid_str(oid);
                    return 1; // the first one is enough
                },
                &sha);
        for (const char* name : {"CHERRY_PICK_HEAD", "REVERT_HEAD"})
        {
            git_oid oid;
            if (sha.empty() && git_reference_name_to_id(&oid, repo, name) == 0)
            {
                sha = oid_str(&oid);
            }
        }
        return sha;
    }

    std::vector<LineHunk> diff_lines(const std::string& old_text, const std::string& new_text)
    {
        git_diff_options options = GIT_DIFF_OPTIONS_INIT;
        options.context_lines = 0;
        options.interhunk_lines = 0;
        options.flags = GIT_DIFF_FORCE_TEXT;
        std::vector<LineHunk> hunks;
        git_diff_buffers(
                old_text.data(), old_text.size(), nullptr, new_text.data(), new_text.size(), nullptr, &options,
                nullptr, nullptr,
                [](const git_diff_delta*, const git_diff_hunk* hunk, void* payload) -> int
                {
                    // 1-based starts; a zero count names the line the change comes after.
                    LineHunk out;
                    out.old_count = hunk->old_lines;
                    out.new_count = hunk->new_lines;
                    out.old_start = hunk->old_lines == 0 ? hunk->old_start : hunk->old_start - 1;
                    out.new_start = hunk->new_lines == 0 ? hunk->new_start : hunk->new_start - 1;
                    static_cast<std::vector<LineHunk>*>(payload)->push_back(out);
                    return 0;
                },
                nullptr, &hunks);
        return hunks;
    }

    Result<Sha> Repository::stash_push(const std::string& message) const
    {
        auto sig = default_signature(m_repo.get());
        if (!sig)
        {
            return std::unexpected(sig.error());
        }
        git_oid oid;
        IMGIT_TRY(git_stash_save(&oid, m_repo.get(), sig->get(), message.empty() ? nullptr : message.c_str(),
                                 GIT_STASH_INCLUDE_UNTRACKED),
                  "could not stash changes");
        return oid_str(&oid);
    }

    int Repository::stash_index_of(const Sha& sha) const
    {
        struct Payload
        {
            const Sha* sha;
            int index;
        } payload{&sha, -1};
        git_stash_foreach(
                m_repo.get(),
                [](std::size_t index, const char*, const git_oid* stash_id, void* data) -> int
                {
                    auto* p = static_cast<Payload*>(data);
                    if (oid_str(stash_id) == *p->sha)
                    {
                        p->index = static_cast<int>(index);
                        return 1; // found: stop
                    }
                    return 0;
                },
                &payload);
        return payload.index;
    }

    namespace
    {
        // libgit2 refuses to apply a stash over staged changes; git applies it when they do not overlap (as in
        // ImGit), reinstating the stashed index when it can.
        Result<void> stash_apply_cli(const std::string& workdir, const char* verb, int index, const char* fallback)
        {
            if (!cli::available())
            {
                return std::unexpected(Error{fallback});
            }
            const std::string stash = "stash@{" + std::to_string(index) + "}";
            cli::Output output = cli::run(workdir, {"stash", verb, "--index", stash});
            if (output.exit_code != 0)
            {
                output = cli::run(workdir, {"stash", verb, stash});
            }
            if (output.exit_code != 0)
            {
                return std::unexpected(Error{output.text.empty() ? std::string(fallback) : output.text});
            }
            return {};
        }
    } // namespace

    Result<void> Repository::stash_apply(int index) const
    {
        git_stash_apply_options opts = GIT_STASH_APPLY_OPTIONS_INIT;
        opts.flags = GIT_STASH_APPLY_REINSTATE_INDEX;
        if (git_stash_apply(m_repo.get(), static_cast<std::size_t>(index), &opts) == 0)
        {
            return {};
        }
        return stash_apply_cli(m_workdir, "apply", index, "could not apply stash");
    }

    Result<void> Repository::stash_pop(int index) const
    {
        git_stash_apply_options opts = GIT_STASH_APPLY_OPTIONS_INIT;
        opts.flags = GIT_STASH_APPLY_REINSTATE_INDEX;
        if (git_stash_pop(m_repo.get(), static_cast<std::size_t>(index), &opts) == 0)
        {
            return {};
        }
        return stash_apply_cli(m_workdir, "pop", index, "could not pop stash");
    }

    Result<void> Repository::stash_drop(int index) const
    {
        IMGIT_TRY(git_stash_drop(m_repo.get(), static_cast<std::size_t>(index)), "could not drop stash");
        return {};
    }

    Result<void> Repository::fill_stats(std::vector<FileChange>& changes, StatsSource source, const Sha& sha) const
    {
        git_repository* repo = m_repo.get();
        git_diff_options opts = GIT_DIFF_OPTIONS_INIT;
        opts.flags = GIT_DIFF_INCLUDE_UNTRACKED | GIT_DIFF_SHOW_UNTRACKED_CONTENT | GIT_DIFF_RECURSE_UNTRACKED_DIRS;
        DiffHandle diff;
        TreeHandle tree;
        TreeHandle parent_tree;
        IndexHandle index;
        switch (source)
        {
        case StatsSource::COMMIT:
        {
            auto commit = lookup_commit(repo, sha);
            if (!commit)
            {
                return std::unexpected(commit.error());
            }
            IMGIT_TRY(git_commit_tree(tree.out(), *commit), "could not read commit tree");
            if (git_commit_parentcount(*commit) > 0)
            {
                CommitHandle parent;
                IMGIT_TRY(git_commit_parent(parent.out(), *commit, 0), "could not read parent");
                IMGIT_TRY(git_commit_tree(parent_tree.out(), parent), "could not read parent tree");
            }
            IMGIT_TRY(git_diff_tree_to_tree(diff.out(), repo, parent_tree, tree, &opts), "could not diff commit");
            git_diff_find_similar(diff, nullptr);
            break;
        }
        case StatsSource::STAGED:
        {
            auto head = head_commit(repo);
            if (!head)
            {
                return std::unexpected(head.error());
            }
            if (head->get() != nullptr)
            {
                IMGIT_TRY(git_commit_tree(tree.out(), *head), "could not read HEAD tree");
            }
            IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
            IMGIT_TRY(git_diff_tree_to_index(diff.out(), repo, tree, index, &opts), "could not diff index");
            git_diff_find_similar(diff, nullptr);
            break;
        }
        case StatsSource::UNSTAGED:
            IMGIT_TRY(git_repository_index(index.out(), repo), "could not read index");
            IMGIT_TRY(git_diff_index_to_workdir(diff.out(), repo, index, &opts), "could not diff working directory");
            break;
        }

        std::unordered_map<std::string, std::pair<int, int>> by_path;
        const std::size_t count = git_diff_num_deltas(diff);
        for (std::size_t i = 0; i < count; ++i)
        {
            const git_diff_delta* delta = git_diff_get_delta(diff, i);
            if ((delta->flags & GIT_DIFF_FLAG_BINARY) != 0)
            {
                continue;
            }
            PatchHandle patch;
            if (git_patch_from_diff(patch.out(), diff, i) < 0)
            {
                continue;
            }
            std::size_t context = 0, additions = 0, deletions = 0;
            git_patch_line_stats(&context, &additions, &deletions, patch);
            const char* path = delta->new_file.path != nullptr ? delta->new_file.path : delta->old_file.path;
            by_path[path] = {static_cast<int>(additions), static_cast<int>(deletions)};
        }
        for (FileChange& change : changes)
        {
            if (const auto found = by_path.find(change.path); found != by_path.end())
            {
                change.additions = found->second.first;
                change.deletions = found->second.second;
            }
            change.has_stats = true;
        }
        return {};
    }

    Result<Sha> Repository::reword_head(const std::string& message) const
    {
        git_repository* repo = m_repo.get();
        auto head = head_commit(repo);
        if (!head)
        {
            return std::unexpected(head.error());
        }
        if (head->get() == nullptr)
        {
            return std::unexpected(Error{"no commit to reword"});
        }
        auto committer = default_signature(repo);
        if (!committer)
        {
            return std::unexpected(committer.error());
        }
        git_oid oid;
        IMGIT_TRY(git_commit_amend(&oid, *head, "HEAD", nullptr, committer->get(), nullptr, message.c_str(), nullptr),
                  "could not reword commit");
        return oid_str(&oid);
    }

    Result<void> Repository::set_branch_target(const std::string& name, const Sha& sha) const
    {
        auto oid = parse_oid(sha);
        if (!oid)
        {
            return std::unexpected(oid.error());
        }
        ReferenceHandle ref;
        if (git_branch_lookup(ref.out(), m_repo.get(), name.c_str(), GIT_BRANCH_LOCAL) == 0)
        {
            ReferenceHandle updated;
            IMGIT_TRY(git_reference_set_target(updated.out(), ref, &*oid, "ImGit: move branch"),
                      "could not move branch");
            return {};
        }
        auto commit = lookup_commit(m_repo.get(), sha);
        if (!commit)
        {
            return std::unexpected(commit.error());
        }
        ReferenceHandle created;
        IMGIT_TRY(git_branch_create(created.out(), m_repo.get(), name.c_str(), *commit, 0), "could not create branch");
        return {};
    }

    Result<void> Repository::set_upstream(const std::string& name, const std::string& upstream) const
    {
        ReferenceHandle ref;
        IMGIT_TRY(git_branch_lookup(ref.out(), m_repo.get(), name.c_str(), GIT_BRANCH_LOCAL), "branch not found");
        IMGIT_TRY(git_branch_set_upstream(ref, upstream.empty() ? nullptr : upstream.c_str()),
                  "could not set upstream");
        return {};
    }

    Result<void> Repository::add_remote(const std::string& name, const std::string& url) const
    {
        RemoteHandle remote;
        IMGIT_TRY(git_remote_create(remote.out(), m_repo.get(), name.c_str(), url.c_str()), "could not add remote");
        return {};
    }

    Result<void> Repository::set_remote_url(const std::string& name, const std::string& url) const
    {
        IMGIT_TRY(git_remote_set_url(m_repo.get(), name.c_str(), url.c_str()), "could not update remote");
        return {};
    }

    Result<void> Repository::remove_remote(const std::string& name) const
    {
        IMGIT_TRY(git_remote_delete(m_repo.get(), name.c_str()), "could not remove remote");
        return {};
    }

    Result<void> Repository::rewrite_history(const Sha& base, const std::vector<RebaseStep>& steps) const
    {
        git_repository* repo = m_repo.get();
        auto status = load_status();
        if (!status)
        {
            return std::unexpected(status.error());
        }
        if (!status->empty())
        {
            return std::unexpected(Error{"The working directory must be clean. Commit or stash your changes first."});
        }
        auto original_head = head_commit(repo);
        if (!original_head)
        {
            return std::unexpected(original_head.error());
        }
        if (original_head->get() == nullptr)
        {
            return std::unexpected(Error{"nothing to rebase"});
        }
        const Sha original_sha = oid_str(git_commit_id(*original_head));

        const auto roll_back = [&](const std::string& why) -> std::unexpected<Error>
        {
            git_checkout_options opts = GIT_CHECKOUT_OPTIONS_INIT;
            opts.checkout_strategy = GIT_CHECKOUT_FORCE;
            if (auto original = lookup_commit(repo, original_sha))
            {
                git_reset(repo, reinterpret_cast<git_object*>(original->get()), GIT_RESET_HARD, &opts);
            }
            git_repository_state_cleanup(repo);
            return std::unexpected(Error{why + " The branch was restored."});
        };

        if (auto reset_result = reset(base, ResetMode::HARD); !reset_result)
        {
            return std::unexpected(reset_result.error());
        }

        bool have_previous = false;
        for (const RebaseStep& step : steps)
        {
            if (step.action == RebaseAction::DROP)
            {
                continue;
            }
            auto commit = lookup_commit(repo, step.sha);
            if (!commit)
            {
                return roll_back(commit.error().message + ".");
            }
            git_cherrypick_options opts = GIT_CHERRYPICK_OPTIONS_INIT;
            opts.checkout_opts.checkout_strategy = GIT_CHECKOUT_SAFE | GIT_CHECKOUT_ALLOW_CONFLICTS;
            if (git_cherrypick(repo, *commit, &opts) < 0)
            {
                return roll_back("Could not apply " + short_sha(step.sha) + ".");
            }
            IndexHandle index;
            if (git_repository_index(index.out(), repo) < 0 || git_index_has_conflicts(index))
            {
                return roll_back("Conflicts while applying " + short_sha(step.sha) + ".");
            }
            const char* original_message = git_commit_message(*commit);
            const std::string message = original_message != nullptr ? original_message : "";
            // `commit` outlives this step, and with it the author (its date kept on the rewritten commit).
            const git_signature* author = git_commit_author(*commit);

            auto head = head_commit(repo);
            if (!head || head->get() == nullptr)
            {
                return roll_back("Lost HEAD during the rebase.");
            }
            if (step.action == RebaseAction::SQUASH && have_previous)
            {
                git_oid tree_oid;
                if (git_index_write_tree(&tree_oid, index) < 0)
                {
                    return roll_back("Could not write the squashed tree.");
                }
                TreeHandle tree;
                if (git_tree_lookup(tree.out(), repo, &tree_oid) < 0)
                {
                    return roll_back("Could not read the squashed tree.");
                }
                std::string combined = step.message;
                if (combined.empty())
                {
                    const char* previous = git_commit_message(*head);
                    combined = std::string(previous != nullptr ? previous : "");
                    while (!combined.empty() && combined.back() == '\n')
                    {
                        combined.pop_back();
                    }
                    combined += "\n\n" + message;
                }
                auto committer = default_signature(repo);
                if (!committer)
                {
                    return roll_back(committer.error().message + ".");
                }
                git_oid new_oid;
                if (git_commit_amend(&new_oid, *head, "HEAD", nullptr, committer->get(), nullptr, combined.c_str(),
                                     tree)
                    < 0)
                {
                    return roll_back("Could not squash " + short_sha(step.sha) + ".");
                }
            }
            else
            {
                const std::string final_message =
                        step.action == RebaseAction::REWORD && !step.message.empty() ? step.message : message;
                auto created = commit_index(final_message, {oid_str(git_commit_id(*head))}, author);
                if (!created)
                {
                    return roll_back(created.error().message + ".");
                }
            }
            git_repository_state_cleanup(repo);
            have_previous = true;
        }
        return {};
    }

    namespace
    {
        std::string global_config_string(const char* key)
        {
            git_config* config = nullptr;
            if (git_config_open_default(&config) != 0)
            {
                return {};
            }
            std::string result;
            git_buf buffer = GIT_BUF_INIT;
            if (git_config_get_string_buf(&buffer, config, key) == 0)
            {
                result.assign(buffer.ptr, buffer.size);
                git_buf_dispose(&buffer);
            }
            git_config_free(config);
            return result;
        }
    } // namespace

    std::string global_user_email()
    {
        return global_config_string("user.email");
    }

    std::string global_user_name()
    {
        return global_config_string("user.name");
    }

    Result<void> set_global_identity(const std::string& name, const std::string& email)
    {
        git_config* config = nullptr;
        IMGIT_TRY(git_config_open_default(&config), "could not open the git configuration");
        git_config* global = nullptr;
        const int opened = git_config_open_level(&global, config, GIT_CONFIG_LEVEL_GLOBAL);
        git_config_free(config);
        IMGIT_TRY(opened, "could not open the global git configuration");
        int rc = 0;
        if (!name.empty())
        {
            rc = git_config_set_string(global, "user.name", name.c_str());
        }
        if (rc == 0 && !email.empty())
        {
            rc = git_config_set_string(global, "user.email", email.c_str());
        }
        git_config_free(global);
        IMGIT_TRY(rc, "could not update the global git configuration");
        return {};
    }

    void set_profile_identity(const std::string& name, const std::string& email)
    {
        std::lock_guard lock(g_profile_mutex);
        g_profile_identity = {name, email};
    }

    std::string short_sha(const Sha& sha)
    {
        return sha.substr(0, 7);
    }

    std::string status_letter(FileStatus status)
    {
        switch (status)
        {
        case FileStatus::ADDED:
        case FileStatus::UNTRACKED:
            return "A";
        case FileStatus::MODIFIED:
            return "M";
        case FileStatus::DELETED:
            return "D";
        case FileStatus::RENAMED:
            return "R";
        case FileStatus::TYPECHANGE:
            return "T";
        case FileStatus::CONFLICTED:
            return "!";
        default:
            return "?";
        }
    }
} // namespace imgit::git
