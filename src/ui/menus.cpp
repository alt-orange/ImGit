#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/shell.hpp"
#include "ui/panels.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <imgui.h>
#include <imgui_internal.h>
#include <map>
#include <string>

// The context menus: groups joined by separators, each item offered under its own conditions. Items copy what
// they need: actions reload the history and invalidate references into it. Features ImGit does not implement yet
// are placeholders that say so.
namespace imgit::ui
{
    namespace
    {
        using app::Application;
        using app::RepoState;

        // "{0}" / "{1}" placeholders.
        std::string fmt(std::string pattern, std::initializer_list<std::string> args)
        {
            std::size_t index = 0;
            for (const std::string& arg : args)
            {
                const std::string key = "{" + std::to_string(index++) + "}";
                for (std::size_t at = pattern.find(key); at != std::string::npos;
                     at = pattern.find(key, at + arg.size()))
                {
                    pattern.replace(at, key.size(), arg);
                }
            }
            return pattern;
        }

        MenuItem item(std::string label, std::function<void()> click, bool enabled = true)
        {
            return menu_item(std::move(label), std::move(click), enabled);
        }

        std::function<void()> placeholder(Application& app, std::string what, std::string why)
        {
            return [&app, what = std::move(what), why = std::move(why)]
            {
                app.toasts.push(app::ToastKind::INFO, what, why);
            };
        }

        // Undo for anything that moved HEAD: go back to the branch (or detached commit) we were on.
        app::UndoEntry head_restore_entry(RepoState& state, const std::string& label,
                                          std::function<git::Result<void>()> redo)
        {
            git::Repository* repo = &state.repo;
            const std::string branch = state.history.detached ? "" : state.history.head_branch;
            const git::Sha sha = state.history.head_sha;
            return {label,
                    [repo, branch, sha]() -> git::Result<void>
                    {
                        if (!branch.empty())
                        {
                            return repo->checkout_branch(branch);
                        }
                        if (!sha.empty())
                        {
                            return repo->checkout_commit(sha);
                        }
                        return {};
                    },
                    std::move(redo)};
        }

        // Undo for history rewrites on the current branch: a hard reset to the previous tip. Uncommitted
        // changes present at the time are stashed around the reset so they survive.
        app::UndoEntry tip_restore_entry(RepoState& state, const std::string& label, git::ResetMode mode)
        {
            git::Repository* repo = &state.repo;
            const git::Sha sha = state.history.head_sha;
            return {label,
                    [repo, sha, mode]() -> git::Result<void>
                    {
                        if (sha.empty())
                        {
                            return std::unexpected(git::Error{"nothing to restore"});
                        }
                        bool stashed = false;
                        if (repo->load_status().value_or(git::WorkDirStatus{}).count() > 0)
                        {
                            if (auto pushed = repo->stash_push("ImGit: auto stash before undo"); pushed)
                            {
                                stashed = true;
                            }
                        }
                        auto result = repo->reset(sha, mode);
                        if (stashed)
                        {
                            (void)repo->stash_pop(0);
                        }
                        return result;
                    },
                    {},
                    mode};
        }

        void report_sequencer(Application& app, RepoState& state, const char* what, const git::Result<bool>& result,
                              const std::string& success)
        {
            if (!result)
            {
                app.toasts.push(app::ToastKind::ERROR, std::string(what) + " failed", result.error().message);
            }
            else if (!*result)
            {
                app.toasts.push(app::ToastKind::ERROR, std::string(what) + " stopped on conflicts",
                                "Resolve them in the working directory, then commit.");
            }
            else if (!success.empty())
            {
                app.toasts.push(app::ToastKind::SUCCESS, success);
            }
            state.refresh();
        }

        void merge_into_head(Application& app, RepoState& state, const std::string& name, bool no_fast_forward = false)
        {
            const std::string head = state.history.head_branch;
            app::UndoEntry undo = tip_restore_entry(state, "merge " + name, git::ResetMode::HARD);
            auto result = state.repo.merge(name, no_fast_forward);
            if (result && *result)
            {
                state.push_undo(std::move(undo));
            }
            report_sequencer(app, state, "Merge", result, "Merged " + name + " into " + head);
        }

        void rewrite(Application& app, RepoState& state, const git::Sha& base,
                     const std::vector<git::RebaseStep>& steps, const std::string& label, const std::string& success)
        {
            app::UndoEntry undo = tip_restore_entry(state, label, git::ResetMode::HARD);
            auto result = state.repo.rewrite_history(base, steps);
            if (!result)
            {
                app.toasts.push(app::ToastKind::ERROR, label + " failed", result.error().message);
            }
            else
            {
                state.push_undo(std::move(undo));
                app.toasts.push(app::ToastKind::SUCCESS, success);
            }
            state.refresh();
        }

        void confirm(Application& app, std::string title, std::string message, std::string confirm_label, bool danger,
                     std::function<void()> action)
        {
            app.dialog.open(app::Dialog::Kind::CONFIRM);
            app.dialog.title = std::move(title);
            app.dialog.message = std::move(message);
            app.dialog.confirm_label = std::move(confirm_label);
            app.dialog.danger = danger;
            app.dialog.on_confirm = std::move(action);
        }

        std::string full_path(const RepoState& state, const std::string& path)
        {
            std::string workdir = state.repo.workdir();
            if (!workdir.empty() && workdir.back() != '/' && workdir.back() != '\\')
            {
                workdir += '/';
            }
            return workdir + path;
        }

        // `git <args>` written to a file the user picks.
        void save_git_output(Application& app, RepoState& state, const std::string& default_name,
                             std::vector<std::string> args)
        {
            const auto destination = platform::shell::save_file_dialog(default_name, "patch");
            if (!destination)
            {
                return;
            }
            const git::cli::Output output = git::cli::run(state.repo.workdir(), args);
            if (output.exit_code != 0)
            {
                app.toasts.push(app::ToastKind::ERROR, "Could not create patch", output.text);
                return;
            }
            std::ofstream(*destination, std::ios::binary) << output.text;
            app.toasts.push(app::ToastKind::SUCCESS, "Patch saved", *destination);
        }

        // ---------------------------------------------------------------------------------------------------
        // Commit facts the selectors provide.

        // getDistinctCommitsFromHeadToFirstMergeCommit: HEAD's first-parent chain down to (excluding) the first
        // merge commit; the root commit is included.
        std::vector<const git::Commit*> head_chain(const RepoState& state)
        {
            std::vector<const git::Commit*> chain;
            const git::Commit* commit = state.commit_for(state.history.head_sha);
            while (commit != nullptr && commit->parents.size() <= 1)
            {
                chain.push_back(commit);
                if (commit->parents.empty())
                {
                    break;
                }
                commit = state.commit_for(commit->parents.front());
            }
            return chain;
        }

        int chain_index(const std::vector<const git::Commit*>& chain, const git::Sha& sha)
        {
            for (std::size_t i = 0; i < chain.size(); ++i)
            {
                if (chain[i]->sha == sha)
                {
                    return static_cast<int>(i);
                }
            }
            return -1;
        }

        std::vector<git::Sha> selected_commits(const RepoState& state)
        {
            std::vector<git::Sha> selected = state.selected;
            std::erase(selected, app::WIP_SHA);
            return selected;
        }

        // Selected shas sorted youngest (top row) to oldest.
        std::vector<git::Sha> selected_youngest_first(const RepoState& state)
        {
            std::vector<git::Sha> shas = selected_commits(state);
            std::sort(shas.begin(), shas.end(),
                      [&](const git::Sha& a, const git::Sha& b)
                      {
                          return state.history.row_by_sha.at(a) < state.history.row_by_sha.at(b);
                      });
            return shas;
        }

        // Whether every selected commit is in the chain and they are adjacent in it.
        bool selection_contiguous_in_chain(const std::vector<const git::Commit*>& chain,
                                           const std::vector<git::Sha>& shas)
        {
            if (shas.empty())
            {
                return false;
            }
            std::vector<int> indices;
            for (const git::Sha& sha : shas)
            {
                const int index = chain_index(chain, sha);
                if (index < 0 || chain[static_cast<std::size_t>(index)]->parents.size() != 1)
                {
                    return false;
                }
                indices.push_back(index);
            }
            std::sort(indices.begin(), indices.end());
            return indices.back() - indices.front() + 1 == static_cast<int>(indices.size());
        }

        // getMergeState(a, b): FAST_FORWARD when a can fast-forward to b, NORMAL when they diverged, else
        // UP_TO_DATE.
        enum class MergeState
        {
            FAST_FORWARD,
            NORMAL,
            UP_TO_DATE,
        };

        MergeState merge_state(const RepoState& state, const git::Sha& a, const git::Sha& b)
        {
            if (a == b || state.repo.is_ancestor(b, a))
            {
                return MergeState::UP_TO_DATE;
            }
            if (state.repo.is_ancestor(a, b))
            {
                return MergeState::FAST_FORWARD;
            }
            return MergeState::NORMAL;
        }

        std::string branch_group_name(const git::Ref& ref)
        {
            if (ref.type == git::RefType::REMOTE_BRANCH && ref.name.size() > ref.remote.size())
            {
                return ref.name.substr(ref.remote.size() + 1);
            }
            return ref.name;
        }

        // A clicked pill / row: the branches (local first, then remote) sharing one name at one commit.
        struct RefGroup
        {
            std::string name;
            std::vector<git::Ref> refs;

            [[nodiscard]] const git::Ref* local() const
            {
                for (const git::Ref& ref : refs)
                {
                    if (ref.type == git::RefType::LOCAL_BRANCH)
                    {
                        return &ref;
                    }
                }
                return nullptr;
            }
        };

        std::vector<RefGroup> branch_groups_at(const RepoState& state, const git::Sha& sha)
        {
            std::map<std::string, RefGroup> groups;
            std::vector<std::string> order;
            for (const git::Ref* ref : state.refs_at(sha))
            {
                if (ref->type != git::RefType::LOCAL_BRANCH && ref->type != git::RefType::REMOTE_BRANCH)
                {
                    continue;
                }
                const std::string name = branch_group_name(*ref);
                auto [it, inserted] = groups.try_emplace(name);
                if (inserted)
                {
                    it->second.name = name;
                    order.push_back(name);
                }
                if (ref->type == git::RefType::LOCAL_BRANCH)
                {
                    it->second.refs.insert(it->second.refs.begin(), *ref);
                }
                else
                {
                    it->second.refs.push_back(*ref);
                }
            }
            std::vector<RefGroup> result;
            for (const std::string& name : order)
            {
                result.push_back(std::move(groups[name]));
            }
            return result;
        }

        RefGroup group_for_ref(const RepoState& state, const git::Ref& ref)
        {
            for (RefGroup& group : branch_groups_at(state, ref.target))
            {
                if (group.name == branch_group_name(ref))
                {
                    return group;
                }
            }
            return {branch_group_name(ref), {ref}};
        }

        // ---------------------------------------------------------------------------------------------------
        // Item builders.

        MenuItem checkout_commit_item(Application& app, RepoState& state, const git::Sha& sha, bool in_submenu)
        {
            return item(in_submenu ? "this commit" : "Checkout this commit",
                        [&app, &state, sha]
                        {
                            git::Repository* repo = &state.repo;
                            app::UndoEntry undo = head_restore_entry(state, "checkout " + git::short_sha(sha),
                                                                     [repo, sha]
                                                                     {
                                                                         return repo->checkout_commit(sha);
                                                                     });
                            if (app.run(
                                        state, "Checkout",
                                        [&]
                                        {
                                            return state.repo.checkout_commit(sha);
                                        },
                                        "Checked out " + git::short_sha(sha) + " (detached HEAD)"))
                            {
                                state.push_undo(std::move(undo));
                            }
                        });
        }

        MenuItem worktree_commit_item(Application& app, bool in_submenu)
        {
            return item(in_submenu ? "this commit" : "Create worktree from this commit",
                        placeholder(app, "Create worktree", "Worktree creation is not available in ImGit yet."));
        }

        MenuItem create_branch_item(Application& app, const git::Sha& sha)
        {
            return item("Create branch here",
                        [&app, sha]
                        {
                            app.dialog.open(app::Dialog::Kind::CREATE_BRANCH);
                            app.dialog.sha = sha;
                            app.dialog.flag = true;
                        });
        }

        MenuItem cherry_pick_item(Application& app, RepoState& state, const git::Sha& sha)
        {
            return item("Cherry pick commit",
                        [&app, &state, sha]
                        {
                            app::UndoEntry undo = tip_restore_entry(state, "cherry pick " + git::short_sha(sha),
                                                                    git::ResetMode::HARD);
                            auto result = state.repo.cherry_pick(sha);
                            if (result && *result)
                            {
                                state.push_undo(std::move(undo));
                            }
                            report_sequencer(app, state, "Cherry pick", result, "Cherry picked " + git::short_sha(sha));
                        });
        }

        std::optional<MenuItem> cherry_picks_item(Application& app, RepoState& state)
        {
            const std::vector<git::Sha> shas = selected_youngest_first(state);
            for (const git::Sha& sha : shas)
            {
                const git::Commit* commit = state.commit_for(sha);
                if (commit == nullptr || commit->parents.size() > 1 || commit->type == git::NodeType::STASH)
                {
                    return std::nullopt;
                }
            }
            return item(fmt("Cherry pick {0} commits", {std::to_string(shas.size())}),
                        [&app, &state, shas]
                        {
                            app::UndoEntry undo = tip_restore_entry(state, "cherry pick", git::ResetMode::HARD);
                            for (auto it = shas.rbegin(); it != shas.rend(); ++it)
                            {
                                auto result = state.repo.cherry_pick(*it);
                                if (!result || !*result)
                                {
                                    report_sequencer(app, state, "Cherry pick", result, "");
                                    return;
                                }
                            }
                            state.push_undo(std::move(undo));
                            app.toasts.push(app::ToastKind::SUCCESS,
                                            "Cherry picked " + std::to_string(shas.size()) + " commits");
                            state.refresh();
                        });
        }

        MenuItems reset_items(Application& app, RepoState& state, const git::Sha& sha)
        {
            if (state.history.head_sha.empty())
            {
                return {};
            }
            const std::string head = state.history.detached ? "HEAD" : state.history.head_branch;
            const auto reset = [&app, sha, head](git::ResetMode mode)
            {
                return [&app, sha, head, mode]
                {
                    app.dialog.open(app::Dialog::Kind::RESET);
                    app.dialog.sha = sha;
                    app.dialog.ref_name = head;
                    app.dialog.reset_mode = mode;
                };
            };
            return {menu_submenu(fmt("Reset {0} to this commit", {head}),
                                 {item("Soft - keep all changes", reset(git::ResetMode::SOFT)),
                                  item("Mixed - keep working copy but reset index", reset(git::ResetMode::MIXED)),
                                  item("Hard - discard all changes", reset(git::ResetMode::HARD))})};
        }

        MenuItem revert_item(Application& app, RepoState& state, const git::Sha& sha)
        {
            return item("Revert commit",
                        [&app, &state, sha]
                        {
                            app::UndoEntry undo =
                                    tip_restore_entry(state, "revert " + git::short_sha(sha), git::ResetMode::HARD);
                            auto result = state.repo.revert(sha);
                            if (result && *result)
                            {
                                state.push_undo(std::move(undo));
                            }
                            report_sequencer(app, state, "Revert", result, "Reverted " + git::short_sha(sha));
                        });
        }

        MenuItem reword_item(Application& app, RepoState& state, const git::Sha& sha)
        {
            return item("Edit commit message",
                        [&app, &state, sha]
                        {
                            const git::Commit* commit = state.commit_for(sha);
                            if (commit == nullptr)
                            {
                                return;
                            }
                            app.dialog.open(app::Dialog::Kind::REWORD_COMMIT);
                            app.dialog.sha = sha;
                            app.dialog.text_a =
                                    commit->body.empty() ? commit->summary : commit->summary + "\n\n" + commit->body;
                        });
        }

        MenuItems rebase_onto_commit_items(Application& app, RepoState& state, const git::Sha& sha)
        {
            if (state.history.detached || state.history.head_branch.empty() || sha == state.history.head_sha
                || state.repo.state() == git::RepoState::REBASE)
            {
                return {};
            }
            const std::string head = state.history.head_branch;
            return {item(fmt("Rebase {0} onto this commit", {head}),
                         [&app, &state, sha]
                         {
                             app.run_git_cli(state, "Rebase", {"rebase", sha});
                         })};
        }

        // getInteractiveRebaseShortcuts + the AI / interactive rebase entries (the "gs" group).
        MenuItems rebase_group(Application& app, RepoState& state, const git::Sha& sha, bool multi)
        {
            MenuItems items;
            if (state.history.head_sha.empty() || state.repo.state() == git::RepoState::REBASE)
            {
                return items;
            }
            const std::vector<const git::Commit*> chain = head_chain(state);
            const int index = chain_index(chain, sha);
            if (index < 0)
            {
                return items;
            }
            const git::Commit* commit = chain[static_cast<std::size_t>(index)];
            const std::vector<git::Sha> selected = selected_youngest_first(state);
            const bool sha_selected = std::find(selected.begin(), selected.end(), sha) != selected.end();
            const bool contiguous = selection_contiguous_in_chain(chain, selected);
            if (index > 0)
            {
                const std::string children = std::to_string(index);
                const std::string short_sha = sha.substr(0, 6);
                items.push_back(item(fmt("Interactive Rebase {0} children of {1}", {children, short_sha}),
                                     [&state, sha]
                                     {
                                         state.start_rebase_plan(sha, git::short_sha(sha));
                                     }));
            }

            const bool selection_on_chain = multi && sha_selected && contiguous;
            const auto movable = [&](int i)
            {
                return i >= 0 && chain[static_cast<std::size_t>(i)]->parents.size() == 1;
            };
            // Squash: a contiguous multi-selection on the chain.
            if (selection_on_chain)
            {
                items.push_back(item(fmt("Squash {0} commits", {std::to_string(selected.size())}),
                                     [&app, &state, selected]
                                     {
                                         app.dialog.open(app::Dialog::Kind::SQUASH_COMMITS);
                                         std::vector<git::Sha> oldest_first(selected.rbegin(), selected.rend());
                                         app.dialog.shas = oldest_first;
                                         std::string message;
                                         for (const git::Sha& s : oldest_first)
                                         {
                                             if (const git::Commit* c = state.commit_for(s); c != nullptr)
                                             {
                                                 message += (message.empty() ? "" : "\n\n") + c->summary;
                                                 if (!c->body.empty())
                                                 {
                                                     message += "\n\n" + c->body;
                                                 }
                                             }
                                         }
                                         app.dialog.text_a = message;
                                     }));
            }
            if (!multi && index > 0 && commit->parents.size() == 1)
            {
                items.push_back(reword_item(app, state, sha));
            }

            // Drop / move: rebuild the chain above the lowest affected commit's parent.
            const auto rewrite_with = [&app, &state](std::vector<git::Sha> affected, int direction, std::string label,
                                                     std::string success)
            {
                return [&app, &state, affected = std::move(affected), direction, label = std::move(label),
                        success = std::move(success)]
                {
                    const std::vector<const git::Commit*> chain = head_chain(state);
                    int oldest = -1;
                    for (const git::Sha& s : affected)
                    {
                        oldest = std::max(oldest, chain_index(chain, s));
                    }
                    // Moving down also rewrites the commit below the block.
                    const int start = oldest + (direction < 0 ? 1 : 0);
                    if (oldest < 0 || start >= static_cast<int>(chain.size())
                        || chain[static_cast<std::size_t>(start)]->parents.size() != 1)
                    {
                        return;
                    }
                    const git::Sha base = chain[static_cast<std::size_t>(start)]->parents.front();
                    std::vector<git::RebaseStep> steps; // oldest first
                    for (int i = start; i >= 0; --i)
                    {
                        steps.push_back({chain[static_cast<std::size_t>(i)]->sha, git::RebaseAction::PICK, {}});
                    }
                    const auto is_affected = [&](const git::Sha& s)
                    {
                        return std::find(affected.begin(), affected.end(), s) != affected.end();
                    };
                    if (direction == 0)
                    {
                        for (git::RebaseStep& step : steps)
                        {
                            if (is_affected(step.sha))
                            {
                                step.action = git::RebaseAction::DROP;
                            }
                        }
                    }
                    else
                    {
                        // Steps are oldest first: "up" moves the block one step later (younger), "down" earlier.
                        std::vector<git::RebaseStep> block;
                        std::vector<git::RebaseStep> rest;
                        int first = -1;
                        for (std::size_t i = 0; i < steps.size(); ++i)
                        {
                            if (is_affected(steps[i].sha))
                            {
                                if (first < 0)
                                {
                                    first = static_cast<int>(rest.size());
                                }
                                block.push_back(steps[i]);
                            }
                            else
                            {
                                rest.push_back(steps[i]);
                            }
                        }
                        const int insert =
                                std::clamp(first + (direction > 0 ? 1 : -1), 0, static_cast<int>(rest.size()));
                        rest.insert(rest.begin() + insert, block.begin(), block.end());
                        steps = std::move(rest);
                    }
                    rewrite(app, state, base, steps, label, success);
                };
            };
            const auto n = std::to_string(selected.size());
            if (selection_on_chain)
            {
                items.push_back(item(fmt("Drop {0} commits", {n}),
                                     rewrite_with(selected, 0, "drop commits", "Dropped " + n + " commits")));
            }
            else if (!multi && movable(index))
            {
                items.push_back(
                        item("Drop commit", rewrite_with({sha}, 0, "drop commit", "Dropped " + git::short_sha(sha))));
            }
            int top = index;
            int bottom = index;
            if (selection_on_chain)
            {
                top = chain_index(chain, selected.front());
                bottom = chain_index(chain, selected.back());
            }
            const bool can_up = top > 0 && movable(top);
            const bool can_down =
                    movable(bottom) && movable(bottom + 1 < static_cast<int>(chain.size()) ? bottom + 1 : -1);
            if (selection_on_chain && can_up)
            {
                items.push_back(item(fmt("Move {0} commits up", {n}),
                                     rewrite_with(selected, 1, "move commits", "Moved " + n + " commits up")));
            }
            else if (!multi && can_up)
            {
                items.push_back(item("Move commit up", rewrite_with({sha}, 1, "move commit", "Moved commit up")));
            }
            if (selection_on_chain && can_down)
            {
                items.push_back(item(fmt("Move {0} commits down", {n}),
                                     rewrite_with(selected, -1, "move commits", "Moved " + n + " commits down")));
            }
            else if (!multi && can_down)
            {
                items.push_back(item("Move commit down", rewrite_with({sha}, -1, "move commit", "Moved commit down")));
            }
            return items;
        }

        MenuItem copy_sha_item(Application& app, const git::Sha& sha)
        {
            return item("Copy commit sha",
                        [&app, sha]
                        {
                            app.copy_to_clipboard(sha);
                        });
        }

        MenuItems patch_items(Application& app, RepoState& state, const git::Sha& sha, bool multi)
        {
            std::vector<git::Sha> shas = multi ? selected_youngest_first(state) : std::vector<git::Sha>{sha};
            MenuItems items;
            items.push_back(item(shas.size() > 1 ? "Create patch from commits" : "Create patch from commit",
                                 [&app, &state, shas]
                                 {
                                     std::vector<std::string> args{"format-patch", "--stdout"};
                                     if (shas.size() == 1)
                                     {
                                         args.push_back("-1");
                                         args.push_back(shas.front());
                                     }
                                     else
                                     {
                                         args.push_back(shas.back() + "^.." + shas.front());
                                     }
                                     save_git_output(app, state, git::short_sha(shas.front()) + ".patch", args);
                                 }));
            return items;
        }

        MenuItems compare_items(Application& app, RepoState& state, const git::Sha& sha)
        {
            if (state.status.empty())
            {
                return {};
            }
            return {item("Compare commit against working directory",
                         [&app, &state, sha]
                         {
                             state.selected = {app::WIP_SHA, sha};
                             app.toasts.push(app::ToastKind::INFO, "Compare against working directory",
                                             "Comparing two revisions is not available in ImGit yet.");
                         })};
        }

        MenuItems create_tag_items(Application& app, const git::Sha& sha)
        {
            return {item("Create tag here",
                         [&app, sha]
                         {
                             app.dialog.open(app::Dialog::Kind::CREATE_TAG);
                             app.dialog.sha = sha;
                         }),
                    item("Create annotated tag here",
                         [&app, sha]
                         {
                             app.dialog.open(app::Dialog::Kind::CREATE_TAG);
                             app.dialog.sha = sha;
                             app.dialog.flag = true;
                         })};
        }

        void checkout_any(Application& app, RepoState& state, const git::Ref& ref)
        {
            checkout_ref(app, state, ref);
        }

        // getMergeItems + getInteractiveRebaseSteps between HEAD (ours) and `theirs`.
        MenuItems merge_and_rebase_items(Application& app, RepoState& state, const git::Ref& theirs)
        {
            MenuItems items;
            const git::Ref* head = state.head_ref();
            if (head == nullptr)
            {
                return items;
            }
            const std::string ours = head->name;
            const std::string their_short = theirs.name;
            const MergeState their_state = merge_state(state, theirs.target, head->target);
            const MergeState our_state = merge_state(state, head->target, theirs.target);
            const git::Ref their_ref = theirs;
            const auto merge_theirs_into_head = [&app, &state, their_ref]
            {
                merge_into_head(app, state, their_ref.name);
            };
            const auto fast_forward_theirs = [&app, &state, their_ref, head_sha = head->target]
            {
                // Fast-forward their branch to HEAD (it is behind).
                git::Repository* repo = &state.repo;
                app::UndoEntry undo{"fast-forward " + their_ref.name,
                                    [repo, name = their_ref.name, sha = their_ref.target]
                                    {
                                        return repo->set_branch_target(name, sha);
                                    },
                                    [repo, name = their_ref.name, sha = head_sha]
                                    {
                                        return repo->set_branch_target(name, sha);
                                    }};
                if (app.run(
                            state, "Fast-forward",
                            [&]
                            {
                                return state.repo.set_branch_target(their_ref.name, head_sha);
                            },
                            "Fast-forwarded " + their_ref.name))
                {
                    state.push_undo(std::move(undo));
                }
            };
            switch (their_state)
            {
            case MergeState::FAST_FORWARD:
                if (theirs.type != git::RefType::REMOTE_BRANCH)
                {
                    if (theirs.type == git::RefType::TAG)
                    {
                        items.push_back(item(fmt("Fast-forward {0} to {1}", {their_short, ours}),
                                             placeholder(app, "Fast-forward", "Tags cannot be fast-forwarded.")));
                    }
                    else
                    {
                        items.push_back(item(fmt("Fast-forward {0} to {1}", {their_short, ours}), fast_forward_theirs));
                    }
                    items.push_back(item(fmt("Merge {0} into {1}", {ours, their_short}),
                                         [&app, &state, their_ref, ours]
                                         {
                                             if (their_ref.type != git::RefType::LOCAL_BRANCH)
                                             {
                                                 app.toasts.push(app::ToastKind::INFO, "Merge",
                                                                 "Only branches can be merged into.");
                                                 return;
                                             }
                                             checkout_ref(app, state, their_ref);
                                             merge_into_head(app, state, ours);
                                         }));
                }
                break;
            case MergeState::NORMAL:
                items.push_back(item(fmt("Merge {0} into {1}", {their_short, ours}), merge_theirs_into_head));
                break;
            case MergeState::UP_TO_DATE:
                if (our_state == MergeState::FAST_FORWARD)
                {
                    items.push_back(item(fmt("Fast-forward {0} to {1}", {ours, their_short}), merge_theirs_into_head));
                    items.push_back(item(fmt("Merge {0} into {1}", {their_short, ours}), merge_theirs_into_head));
                }
                break;
            }
            items.push_back(item(fmt("Rebase {0} onto {1}", {ours, their_short}),
                                 [&app, &state, their_short]
                                 {
                                     app.run_git_cli(state, "Rebase", {"rebase", their_short});
                                 }));
            const std::vector<const git::Commit*> above = state.commits_above(theirs.target);
            const bool linear = std::none_of(above.begin(), above.end(),
                                             [](const git::Commit* c)
                                             {
                                                 return c->parents.size() > 1;
                                             });
            if (state.repo.is_ancestor(theirs.target, head->target) && !above.empty() && linear)
            {
                items.push_back(item(fmt("Interactive Rebase {0} onto {1}", {ours, their_short}),
                                     [&state, target = theirs.target, their_short]
                                     {
                                         state.start_rebase_plan(target, their_short);
                                     }));
            }
            return items;
        }

        MenuItem toggle_hide_item(RepoState& state, const std::string& full_name, bool enabled)
        {
            return menu_checkbox(
                    "Hide", state.hidden_refs.contains(full_name),
                    [&state, full_name]
                    {
                        if (!state.hidden_refs.erase(full_name))
                        {
                            state.hidden_refs.insert(full_name);
                        }
                        state.solo_ref.clear();
                        state.refresh();
                    },
                    enabled);
        }

        MenuItem toggle_solo_item(RepoState& state, const std::string& full_name, std::string label = "Solo")
        {
            return menu_checkbox(std::move(label), state.solo_ref == full_name,
                                 [&state, full_name]
                                 {
                                     state.solo_ref = state.solo_ref == full_name ? "" : full_name;
                                     state.refresh();
                                 });
        }

        MenuItem pin_item(RepoState& state, const std::string& full_name, bool short_label)
        {
            const bool pinned = state.pinned_branch == full_name;
            std::string label = short_label ? full_name.substr(full_name.find('/', 5) + 1)
                                            : (pinned ? "Unpin from Left" : "Pin to Left");
            return item(std::move(label),
                        [&state, full_name, pinned]
                        {
                            state.pinned_branch = pinned ? "" : full_name;
                            state.refresh();
                        });
        }

        MenuItems delete_items(Application& app, RepoState& state, const std::vector<git::Ref>& refs)
        {
            MenuItems items;
            for (const git::Ref& ref : refs)
            {
                items.push_back(item(fmt("Delete {0}", {ref.name}),
                                     [&app, ref]
                                     {
                                         app.dialog.open(app::Dialog::Kind::DELETE_BRANCH);
                                         if (ref.type == git::RefType::REMOTE_BRANCH)
                                         {
                                             app.dialog.ref_name = ref.name.substr(ref.remote.size() + 1);
                                             app.dialog.ref_remote = ref.remote;
                                         }
                                         else
                                         {
                                             app.dialog.ref_name = ref.name;
                                             app.dialog.text_a = ref.upstream;
                                             app.dialog.sha = ref.target;
                                         }
                                     }));
            }
            if (refs.size() == 2)
            {
                const git::Ref local = refs[0];
                const git::Ref remote = refs[1];
                items.push_back(item(fmt("Delete {0} and {1}", {local.name, remote.name}),
                                     [&app, local, remote]
                                     {
                                         app.dialog.open(app::Dialog::Kind::DELETE_BRANCH);
                                         app.dialog.ref_name = local.name;
                                         app.dialog.text_a = remote.name;
                                         app.dialog.sha = local.target;
                                         app.dialog.flag = true;
                                     }));
            }
            else if (refs.size() > 2)
            {
                items.push_back(item(fmt("Delete all \"{0}\" branches (local && remote)", {refs[0].name}),
                                     placeholder(app, "Delete branches", "Delete them one at a time.")));
            }
            (void)state;
            return items;
        }

        // getRefGroupMenuItems (45051): a single branch group, from a pill, a left panel row or a commit with
        // one branch.
        MenuItems ref_group_items(Application& app, RepoState& state, const RefGroup& group, const git::Sha& sha,
                                  bool in_left_panel, bool is_submenu)
        {
            const git::Ref* head = state.head_ref();
            const git::Ref* local = group.local();
            const std::vector<git::Ref>& refs = group.refs;
            const bool multi = selected_commits(state).size() > 1;
            const bool head_at_group = head != nullptr && !refs.empty() && head->target == refs.front().target;

            MenuItems remote_ops;
            if (local != nullptr && !state.history.remotes.empty())
            {
                const git::Ref branch = *local;
                remote_ops.push_back(item("Pull (fast-forward if possible)",
                                          [&app, &state, branch]
                                          {
                                              // "origin/main"; an upstream without a remote part tracks a local
                                              // branch, which counts as none here.
                                              const auto slash = branch.upstream.find('/');
                                              if (branch.is_head)
                                              {
                                                  app.run_git_cli(state, "Pull", {"pull", "--ff"});
                                              }
                                              else if (slash != std::string::npos)
                                              {
                                                  app.run_git_cli(
                                                          state, "Pull",
                                                          {"fetch", branch.upstream.substr(0, slash),
                                                           branch.upstream.substr(slash + 1) + ":" + branch.name});
                                              }
                                              else
                                              {
                                                  app.toasts.push(app::ToastKind::ERROR, "Pull failed",
                                                                  branch.name + " has no upstream.");
                                              }
                                          }));
                remote_ops.push_back(item("Push",
                                          [&app, &state, branch]
                                          {
                                              const auto slash = branch.upstream.find('/');
                                              if (slash == std::string::npos)
                                              {
                                                  app.dialog.open(app::Dialog::Kind::PUSH_NEW_BRANCH);
                                                  app.dialog.ref_name = branch.name;
                                                  app.dialog.text_a = state.history.remotes.front().name;
                                                  return;
                                              }
                                              app.run_git_cli(state, "Push",
                                                              {"push", branch.upstream.substr(0, slash),
                                                               branch.name + ":" + branch.upstream.substr(slash + 1)});
                                          }));
                remote_ops.push_back(item("Set Upstream",
                                          [&app, branch]
                                          {
                                              app.dialog.open(app::Dialog::Kind::SET_UPSTREAM);
                                              app.dialog.ref_name = branch.name;
                                              app.dialog.text_a = branch.upstream;
                                          }));
            }

            MenuItems merge_items;
            if (head != nullptr && !refs.empty() && !head_at_group && refs.front().full_name != head->full_name)
            {
                merge_items = merge_and_rebase_items(app, state, refs.front());
            }

            // Checkout: other refs of the group (not HEAD's) plus "this commit".
            MenuItems checkout;
            MenuItems checkout_sub;
            for (const git::Ref& ref : refs)
            {
                if (head != nullptr && head->full_name == ref.full_name)
                {
                    continue;
                }
                checkout_sub.push_back(item(ref.name,
                                            [&app, &state, ref]
                                            {
                                                checkout_any(app, state, ref);
                                            }));
            }
            const int extra = (is_submenu || in_left_panel) ? 0 : 1;
            if (static_cast<int>(checkout_sub.size()) + extra > 1)
            {
                MenuItems sub = std::move(checkout_sub);
                if (extra != 0)
                {
                    sub.push_back(checkout_commit_item(app, state, sha, true));
                }
                checkout.push_back(menu_submenu("Checkout", std::move(sub)));
            }
            else
            {
                for (MenuItem& entry : checkout_sub)
                {
                    entry.label = "Checkout " + entry.label;
                    checkout.push_back(std::move(entry));
                }
                if (extra != 0)
                {
                    checkout.push_back(checkout_commit_item(app, state, sha, false));
                }
            }

            // Create worktree from: refs without a worktree (the checked-out branch has the main one).
            MenuItems worktree;
            MenuItems worktree_sub;
            for (const git::Ref& ref : refs)
            {
                if (ref.is_head)
                {
                    continue;
                }
                worktree_sub.push_back(item(ref.name, placeholder(app, "Create worktree",
                                                                  "Worktree creation is not available in ImGit yet.")));
            }
            if (extra != 0)
            {
                worktree_sub.push_back(worktree_commit_item(app, true));
            }
            if (worktree_sub.size() > 1)
            {
                worktree.push_back(menu_submenu("Create worktree from", std::move(worktree_sub)));
            }
            else
            {
                for (MenuItem& entry : worktree_sub)
                {
                    if (entry.label != "this commit")
                    {
                        entry.label = "Create worktree from " + entry.label;
                    }
                    else
                    {
                        entry.label = "Create worktree from this commit";
                    }
                    worktree.push_back(std::move(entry));
                }
            }

            MenuItems commit_ops;
            MenuItems rebase_ops;
            if (!is_submenu)
            {
                commit_ops.push_back(create_branch_item(app, sha));
                if (!head_at_group && !multi)
                {
                    commit_ops.push_back(cherry_pick_item(app, state, sha));
                }
                if (multi && state.is_selected(sha) && state.repo.state() != git::RepoState::REBASE)
                {
                    if (auto picks = cherry_picks_item(app, state))
                    {
                        commit_ops.push_back(std::move(*picks));
                    }
                }
                for (MenuItem& entry : reset_items(app, state, sha))
                {
                    commit_ops.push_back(std::move(entry));
                }
                if (sha == state.history.head_sha)
                {
                    commit_ops.push_back(reword_item(app, state, sha));
                }
                commit_ops.push_back(revert_item(app, state, sha));
                rebase_ops = rebase_group(app, state, sha, multi);
            }

            MenuItems branch_ops;
            if (!is_submenu && local != nullptr)
            {
                if (sha == state.history.head_sha)
                {
                    branch_ops.push_back(item("Apply patch",
                                              [&app, &state]
                                              {
                                                  const auto file = platform::shell::open_file_dialog("patch");
                                                  if (file)
                                                  {
                                                      app.run_git_cli(state, "Apply patch", {"apply", *file});
                                                  }
                                              }));
                }
                const std::string name = local->name;
                branch_ops.push_back(item(fmt("Rename {0}", {name}),
                                          [&app, name]
                                          {
                                              app.dialog.open(app::Dialog::Kind::RENAME_BRANCH);
                                              app.dialog.ref_name = name;
                                              app.dialog.text_a = name;
                                          }));
            }
            for (MenuItem& entry : delete_items(app, state, refs))
            {
                branch_ops.push_back(std::move(entry));
            }

            MenuItems copy_ops;
            const std::string copy_name = refs.empty() ? group.name : branch_group_name(refs.front());
            copy_ops.push_back(item("Copy branch name",
                                    [&app, copy_name]
                                    {
                                        app.copy_to_clipboard(copy_name);
                                    }));
            copy_ops.push_back(copy_sha_item(app, sha));
            if (!in_left_panel && (!multi || state.is_selected(sha)))
            {
                for (MenuItem& entry : patch_items(app, state, sha, multi))
                {
                    copy_ops.push_back(std::move(entry));
                }
            }

            MenuItems view_ops;
            if (refs.size() == 1)
            {
                const std::string full_name = refs.front().full_name;
                if (in_left_panel)
                {
                    view_ops.push_back(toggle_hide_item(state, full_name, state.solo_ref.empty()));
                }
                view_ops.push_back(pin_item(state, full_name, false));
                view_ops.push_back(toggle_solo_item(state, full_name));
            }
            else if (!in_left_panel && !refs.empty())
            {
                const auto pinned = std::find_if(refs.begin(), refs.end(),
                                                 [&](const git::Ref& r)
                                                 {
                                                     return r.full_name == state.pinned_branch;
                                                 });
                if (pinned != refs.end())
                {
                    view_ops.push_back(pin_item(state, pinned->full_name, false));
                }
                else
                {
                    MenuItems pins;
                    for (const git::Ref& ref : refs)
                    {
                        pins.push_back(pin_item(state, ref.full_name, true));
                    }
                    view_ops.push_back(menu_submenu("Pin to Left", std::move(pins)));
                }
                MenuItems solos;
                for (const git::Ref& ref : refs)
                {
                    solos.push_back(toggle_solo_item(state, ref.full_name, ref.name));
                }
                view_ops.push_back(menu_submenu("Solo", std::move(solos)));
            }

            return separated_menu({std::move(remote_ops), std::move(merge_items), std::move(checkout),
                                   std::move(worktree), std::move(commit_ops), std::move(rebase_ops),
                                   std::move(branch_ops), std::move(copy_ops),
                                   std::move(view_ops), compare_items(app, state, sha)});
        }

        // The menu for a commit row or a branch pill.
        MenuItems commit_or_branch_menu(Application& app, RepoState& state, const git::Sha& sha,
                                        std::optional<RefGroup> clicked, bool in_left_panel)
        {
            std::vector<RefGroup> groups;
            int hidden = 0;
            if (clicked)
            {
                groups.push_back(std::move(*clicked));
            }
            else
            {
                groups = branch_groups_at(state, sha);
                if (groups.size() > 20)
                {
                    hidden = static_cast<int>(groups.size()) - 20;
                    groups.resize(20);
                }
            }
            const bool multi = selected_commits(state).size() > 1;
            if (groups.size() == 1)
            {
                return separated_menu({ref_group_items(app, state, groups.front(), sha, in_left_panel, false),
                                       create_tag_items(app, sha)});
            }
            MenuItems checkout;
            MenuItems worktree;
            MenuItems compare;
            if (!in_left_panel)
            {
                checkout.push_back(checkout_commit_item(app, state, sha, false));
                worktree.push_back(worktree_commit_item(app, false));
                compare = compare_items(app, state, sha);
            }
            MenuItems commit_ops;
            commit_ops.push_back(create_branch_item(app, sha));
            if (sha != state.history.head_sha && !multi)
            {
                commit_ops.push_back(cherry_pick_item(app, state, sha));
            }
            if (multi && state.is_selected(sha) && state.repo.state() != git::RepoState::REBASE)
            {
                if (auto picks = cherry_picks_item(app, state))
                {
                    commit_ops.push_back(std::move(*picks));
                }
            }
            if (groups.empty())
            {
                for (MenuItem& entry : rebase_onto_commit_items(app, state, sha))
                {
                    commit_ops.push_back(std::move(entry));
                }
            }
            for (MenuItem& entry : reset_items(app, state, sha))
            {
                commit_ops.push_back(std::move(entry));
            }
            commit_ops.push_back(revert_item(app, state, sha));
            if (sha == state.history.head_sha)
            {
                commit_ops.push_back(reword_item(app, state, sha));
            }
            MenuItems rebase_ops = rebase_group(app, state, sha, multi);

            MenuItems group_menus;
            MenuItems copy_ops;
            if (groups.size() > 1)
            {
                for (const RefGroup& group : groups)
                {
                    group_menus.push_back(
                            menu_submenu(group.name, ref_group_items(app, state, group, sha, in_left_panel, true)));
                }
                if (hidden > 0)
                {
                    group_menus.push_back(
                            item(fmt("{0} more refs not displayed", {std::to_string(hidden)}), {}, false));
                }
            }
            else
            {
                copy_ops.push_back(copy_sha_item(app, sha));
                if (!multi || state.is_selected(sha))
                {
                    for (MenuItem& entry : patch_items(app, state, sha, multi))
                    {
                        copy_ops.push_back(std::move(entry));
                    }
                }
            }
            return separated_menu({std::move(checkout), std::move(worktree), std::move(commit_ops),
                                   std::move(rebase_ops), std::move(group_menus), std::move(copy_ops),
                                   std::move(compare), create_tag_items(app, sha)});
        }

        MenuItems stash_menu(Application& app, RepoState& state, const git::Ref& stash)
        {
            const int index = stash.stash_index;
            const std::string full_name = stash.full_name;
            return separated_menu({
                    {item("Apply Stash",
                          [&app, &state, index]
                          {
                              app.run(
                                      state, "Apply stash",
                                      [&]
                                      {
                                          return state.repo.stash_apply(index);
                                      },
                                      "Stash applied");
                          }),
                     item("Pop Stash",
                          [&app, &state, index]
                          {
                              app.run(
                                      state, "Pop stash",
                                      [&]
                                      {
                                          return state.repo.stash_pop(index);
                                      },
                                      "Stash popped");
                          }),
                     item("Delete Stash",
                          [&app, index, sha = stash.target, name = stash.name, stash_message = stash.message]
                          {
                              app.dialog.open(app::Dialog::Kind::DELETE_STASH);
                              app.dialog.stash_index = index;
                              app.dialog.sha = sha; // found again at confirm: a stash pushed since shifts the indexes
                              app.dialog.ref_name = name;
                              app.dialog.text_b = git::stash_summary(stash_message);
                          })},
                    {item("Edit stash message", placeholder(app, "Edit stash message",
                                                            "Editing stash messages is not available in ImGit yet."))},
                    {toggle_hide_item(state, full_name, true)},
            });
        }

        MenuItems tag_menu(Application& app, RepoState& state, const git::Ref& tag, bool in_left_panel)
        {
            const git::Sha sha = tag.target;
            const std::string name = tag.name;
            const bool multi = selected_commits(state).size() > 1;
            MenuItems push;
            if (state.history.remotes.size() == 1)
            {
                const std::string remote = state.history.remotes.front().name;
                push.push_back(item(fmt("Push {0} to {1}", {name, remote}),
                                    [&app, &state, remote, name]
                                    {
                                        app.run_git_cli(state, "Push tag", {"push", remote, "refs/tags/" + name});
                                    }));
            }
            else if (state.history.remotes.size() > 1)
            {
                MenuItems remotes;
                for (const git::Remote& remote : state.history.remotes)
                {
                    remotes.push_back(item(fmt("Push {0} to {1}", {name, remote.name}),
                                           [&app, &state, remote = remote.name, name]
                                           {
                                               app.run_git_cli(state, "Push tag",
                                                               {"push", remote, "refs/tags/" + name});
                                           }));
                }
                push.push_back(menu_submenu(
                        "Push tag to remote",
                        separated_menu({std::move(remotes),
                                        {item(fmt("Push {0} to all remotes", {name}),
                                              [&app, &state, name]
                                              {
                                                  std::vector<std::vector<std::string>> runs;
                                                  for (const git::Remote& remote : state.history.remotes)
                                                  {
                                                      runs.push_back({"push", remote.name, "refs/tags/" + name});
                                                  }
                                                  app.run_git_cli_sequence(state, "Push tag", std::move(runs));
                                              })}})));
            }
            MenuItems merge_items = merge_and_rebase_items(app, state, tag);
            MenuItems checkout_tag;
            MenuItems checkout;
            MenuItems worktree;
            if (in_left_panel)
            {
                checkout_tag.push_back(item(fmt("Checkout the commit at {0}", {name}),
                                            [&app, &state, tag]
                                            {
                                                checkout_ref(app, state, tag);
                                            }));
            }
            else
            {
                checkout.push_back(checkout_commit_item(app, state, sha, false));
                worktree.push_back(worktree_commit_item(app, false));
            }
            MenuItems commit_ops;
            commit_ops.push_back(create_branch_item(app, sha));
            if (sha != state.history.head_sha && !multi)
            {
                commit_ops.push_back(cherry_pick_item(app, state, sha));
            }
            for (MenuItem& entry : reset_items(app, state, sha))
            {
                commit_ops.push_back(std::move(entry));
            }
            commit_ops.push_back(revert_item(app, state, sha));
            MenuItems view;
            if (in_left_panel)
            {
                view.push_back(toggle_hide_item(state, tag.full_name, state.solo_ref.empty()));
            }
            view.push_back(toggle_solo_item(state, tag.full_name));
            return separated_menu({
                    std::move(push),
                    std::move(merge_items),
                    std::move(checkout_tag),
                    std::move(checkout),
                    std::move(worktree),
                    std::move(commit_ops),
                    {item(fmt("Delete {0} locally", {name}),
                          [&app, name, sha]
                          {
                              app.dialog.open(app::Dialog::Kind::DELETE_TAG);
                              app.dialog.ref_name = name;
                              app.dialog.sha = sha;
                          })},
                    {item("Copy tag name",
                          [&app, name]
                          {
                              app.copy_to_clipboard(name);
                          })},
                    std::move(view),
                    {item(fmt("Annotate {0}", {name}),
                          [&app, name, sha]
                          {
                              app.dialog.open(app::Dialog::Kind::CREATE_TAG);
                              app.dialog.sha = sha;
                              app.dialog.text_a = name;
                              app.dialog.flag = true;
                          })},
            });
        }

        // ---------------------------------------------------------------------------------------------------
        // Files.

        MenuItem ignore_submenu(Application& app, RepoState& state, const std::string& path)
        {
            const std::filesystem::path p(path);
            MenuItems items;
            const auto append = [&app, &state](std::string pattern)
            {
                return [&app, &state, pattern = std::move(pattern)]
                {
                    std::ofstream(full_path(state, ".gitignore"), std::ios::app) << pattern << '\n';
                    app.toasts.push(app::ToastKind::SUCCESS, "Added to .gitignore", pattern);
                    state.refresh_status();
                };
            };
            items.push_back(item(fmt("Ignore '{0}'", {p.filename().string()}), append(path)));
            if (p.has_extension())
            {
                const std::string extension = p.extension().string();
                items.push_back(item(fmt("All files with the extension '{0}'", {extension}), append("*" + extension)));
            }
            if (p.has_parent_path())
            {
                const std::string folder = p.parent_path().generic_string();
                items.push_back(
                        item(fmt("All files in '{0}/'", {p.parent_path().filename().string()}), append(folder + "/")));
            }
            return menu_submenu("Ignore", std::move(items));
        }

        MenuItems open_file_items(RepoState& state, const std::string& path)
        {
            const std::string full = full_path(state, path);
            if (!std::filesystem::is_regular_file(full))
            {
                return {};
            }
            return {item("Open in external editor",
                         [full]
                         {
                             platform::shell::open_in_editor(full);
                         }),
                    item("Open file in default program",
                         [full]
                         {
                             platform::shell::open_path(full);
                         }),
                    item("Show in folder",
                         [full]
                         {
                             platform::shell::show_in_folder(full);
                         })};
        }

        MenuItems edit_delete_items(Application& app, RepoState& state, const std::string& path)
        {
            const std::string full = full_path(state, path);
            if (!std::filesystem::is_regular_file(full))
            {
                return {};
            }
            return {item("Edit file",
                         placeholder(app, "Edit file", "The in-app file editor is not available in ImGit yet.")),
                    item("Delete file",
                         [&app, &state, full, path]
                         {
                             confirm(app, "Delete File", "Are you sure you want to delete " + path + "?", "Delete",
                                     true,
                                     [&app, &state, full, path]
                                     {
                                         std::error_code error;
                                         std::filesystem::remove(full, error);
                                         if (error)
                                         {
                                             app.toasts.push(app::ToastKind::ERROR, "Could not delete " + path,
                                                             error.message());
                                         }
                                         state.refresh_status();
                                     });
                         })};
        }

        // File History / File Blame: the File History view of `path` at `sha` (HEAD when empty: the working
        // directory's files).
        MenuItems history_items(app::RepoState& state, const std::string& path, const git::Sha& sha)
        {
            const git::Sha from = sha == app::WIP_SHA ? git::Sha{} : sha;
            return {item("File History",
                         [&state, path, from]
                         {
                             state.open_file_history(path, from, from, false);
                         }),
                    item("File Blame",
                         [&state, path, from]
                         {
                             state.open_file_history(path, from, from, true);
                         })};
        }
    } // namespace

    // -------------------------------------------------------------------------------------------------------
    // Public menus

    void jump_to_commit(app::RepoState& state, const char* sha)
    {
        const auto row = state.history.row_by_sha.find(sha);
        if (row == state.history.row_by_sha.end())
        {
            return;
        }
        state.select_only(sha);
        state.scroll_to_row = row->second;
        state.close_diff();
    }

    bool checkout_ref(app::Application& app, app::RepoState& state, const git::Ref& ref_in)
    {
        const git::Ref ref = ref_in;
        git::Repository* repo = &state.repo;
        bool ok = true;
        switch (ref.type)
        {
        case git::RefType::LOCAL_BRANCH:
            if (!ref.is_head)
            {
                app::UndoEntry undo = head_restore_entry(state, "checkout " + ref.name,
                                                         [repo, name = ref.name]
                                                         {
                                                             return repo->checkout_branch(name);
                                                         });
                ok = app.run(
                        state, "Checkout",
                        [&]
                        {
                            return state.repo.checkout_branch(ref.name);
                        },
                        "Checked out " + ref.name);
                if (ok)
                {
                    state.push_undo(std::move(undo));
                }
            }
            break;
        case git::RefType::REMOTE_BRANCH:
        {
            const std::string local = ref.name.substr(ref.remote.size() + 1);
            app::UndoEntry undo = head_restore_entry(state, "checkout " + local,
                                                     [repo, local]
                                                     {
                                                         return repo->checkout_branch(local);
                                                     });
            ok = app.run(
                    state, "Checkout",
                    [&]
                    {
                        return state.repo.checkout_remote_branch(ref.remote, local);
                    },
                    "Checked out " + local);
            if (ok)
            {
                state.push_undo(std::move(undo));
            }
            break;
        }
        case git::RefType::TAG:
        {
            app::UndoEntry undo = head_restore_entry(state, "checkout " + ref.name,
                                                     [repo, sha = ref.target]
                                                     {
                                                         return repo->checkout_commit(sha);
                                                     });
            ok = app.run(
                    state, "Checkout",
                    [&]
                    {
                        return state.repo.checkout_commit(ref.target);
                    },
                    "Checked out " + ref.name + " (detached HEAD)");
            if (ok)
            {
                state.push_undo(std::move(undo));
            }
            break;
        }
        case git::RefType::STASH:
            break;
        }
        return ok;
    }

    bool can_drop_ref(const app::RepoState& state, const git::Ref& source, const git::Ref& target)
    {
        // Whether a drop is allowed: another commit, with a local branch on either side (or a
        // hosting service both sides are connected to, which ImGit has none of).
        if (source.target == target.target || source.type == git::RefType::STASH || target.type == git::RefType::STASH)
        {
            return false;
        }
        return group_for_ref(state, source).local() != nullptr || group_for_ref(state, target).local() != nullptr;
    }

    MenuItems ref_drop_menu(app::Application& app, app::RepoState& state, const git::Ref& source_in,
                            const git::Ref& target_in)
    {
        // The drop menu: the dragged group D dropped on the target group T.
        const git::Ref source = source_in;
        const git::Ref target = target_in;
        if (source.target == target.target)
        {
            return {};
        }
        const RefGroup dragged = group_for_ref(state, source);
        const RefGroup dropped = group_for_ref(state, target);
        const git::Ref* dragged_local = dragged.local();
        const git::Ref* target_local = dropped.local();
        // A group's primary ref: its local branch, else its first ref (a remote branch or the tag).
        const git::Ref dragged_primary = dragged_local != nullptr ? *dragged_local : dragged.refs.front();
        const git::Ref target_primary = target_local != nullptr ? *target_local : dropped.refs.front();
        std::vector<git::Ref> target_remotes;
        for (const git::Ref& ref : dropped.refs)
        {
            if (ref.type == git::RefType::REMOTE_BRANCH)
            {
                target_remotes.push_back(ref);
            }
        }

        // Merges check the receiving branch out first; a fast-forward of another branch only moves it.
        const auto merge = [&app, &state](const git::Ref& into, const git::Ref& from, bool no_fast_forward)
        {
            return [&app, &state, into, from, no_fast_forward]
            {
                if (!into.is_head && !checkout_ref(app, state, into))
                {
                    return;
                }
                merge_into_head(app, state, from.name, no_fast_forward);
            };
        };
        const auto fast_forward = [&app, &state](const git::Ref& branch, const git::Ref& to)
        {
            return [&app, &state, branch, to]
            {
                if (branch.is_head)
                {
                    merge_into_head(app, state, to.name);
                }
                else
                {
                    app.run_git_cli(state, "Fast-forward", {"branch", "-f", branch.name, to.target});
                }
            };
        };

        MenuItems merges;
        if (target_local != nullptr)
        {
            // getMergeItems: ours = T's local branch, theirs = D's primary ref.
            const git::Ref ours = *target_local;
            const MergeState their_state = merge_state(state, dragged_primary.target, ours.target);
            const MergeState our_state = merge_state(state, ours.target, dragged_primary.target);
            if (their_state == MergeState::FAST_FORWARD)
            {
                if (dragged_local != nullptr)
                {
                    merges.push_back(item(fmt("Fast-forward {0} to {1}", {dragged_local->name, ours.name}),
                                          fast_forward(*dragged_local, ours)));
                    merges.push_back(item(fmt("Merge {0} into {1}", {ours.name, dragged_local->name}),
                                          merge(*dragged_local, ours, true)));
                }
            }
            else if (their_state == MergeState::NORMAL)
            {
                merges.push_back(item(fmt("Merge {0} into {1}", {dragged_primary.name, ours.name}),
                                      merge(ours, dragged_primary, true)));
            }
            else if (our_state == MergeState::FAST_FORWARD)
            {
                merges.push_back(item(fmt("Fast-forward {0} to {1}", {ours.name, dragged_primary.name}),
                                      fast_forward(ours, dragged_primary)));
                merges.push_back(item(fmt("Merge {0} into {1}", {dragged_primary.name, ours.name}),
                                      merge(ours, dragged_primary, true)));
            }
        }
        // getInteractiveRebaseSteps: D's local branch onto T, when T has a local or remote branch.
        if (dragged_local != nullptr && (target_local != nullptr || !target_remotes.empty()))
        {
            const git::Ref ours = *dragged_local;
            const git::Ref onto = target_primary;
            merges.push_back(item(fmt("Rebase {0} onto {1}", {ours.name, onto.name}),
                                  [&app, &state, ours, onto]
                                  {
                                      if (!ours.is_head && !checkout_ref(app, state, ours))
                                      {
                                          return;
                                      }
                                      app.run_git_cli(state, "Rebase", {"rebase", onto.name});
                                  }));
            // Offered when D has commits T lacks.
            if (!state.repo.is_ancestor(ours.target, onto.target))
            {
                merges.push_back(item(fmt("Interactive Rebase {0} onto {1}", {ours.name, onto.name}),
                                      [&app, &state, ours, onto]
                                      {
                                          const git::Sha fork_point = state.repo.merge_base(ours.target, onto.target);
                                          if (state.commits_above(fork_point, ours.target).empty())
                                          {
                                              app.toasts.push(app::ToastKind::ERROR, "Interactive Rebase",
                                                              "The commits to rebase include a merge commit.");
                                              return;
                                          }
                                          if (!ours.is_head && !checkout_ref(app, state, ours))
                                          {
                                              return;
                                          }
                                          state.start_rebase_plan(onto.target, onto.name, fork_point, ours.target);
                                      }));
            }
        }
        MenuItems push_reset;
        if (dragged_local != nullptr)
        {
            const git::Ref local = *dragged_local;
            const auto push_to = [&app, &state, local](const git::Ref& remote)
            {
                return [&app, &state, local, remote]
                {
                    app.run_git_cli(
                            state, "Push",
                            {"push", remote.remote, local.name + ":" + remote.name.substr(remote.remote.size() + 1)});
                };
            };
            if (target_remotes.size() == 1)
            {
                push_reset.push_back(item(fmt("Push {0} to {1}", {local.name, target_remotes.front().name}),
                                          push_to(target_remotes.front())));
            }
            else if (target_remotes.size() > 1)
            {
                MenuItems sub;
                for (const git::Ref& remote : target_remotes)
                {
                    sub.push_back(item(remote.name, push_to(remote)));
                }
                push_reset.push_back(menu_submenu(fmt("Push {0} to...", {local.name}), std::move(sub)));
            }
            if (local.is_head)
            {
                for (MenuItem& entry : reset_items(app, state, target.target))
                {
                    push_reset.push_back(std::move(entry));
                }
            }
        }
        // Pull request items need a connected hosting service; ImGit leaves them out without one.
        return separated_menu({std::move(merges), std::move(push_reset)});
    }

    MenuItems commit_menu(app::Application& app, app::RepoState& state, const git::Sha& sha)
    {
        const git::Commit* commit = state.commit_for(sha);
        if (commit == nullptr)
        {
            return {};
        }
        if (commit->type == git::NodeType::WORK_DIR)
        {
            return {};
        }
        if (commit->type == git::NodeType::STASH)
        {
            for (const git::Ref* ref : state.refs_at(sha))
            {
                if (ref->type == git::RefType::STASH)
                {
                    return stash_menu(app, state, *ref);
                }
            }
            return {};
        }
        return commit_or_branch_menu(app, state, sha, std::nullopt, false);
    }

    MenuItems ref_menu(app::Application& app, app::RepoState& state, const git::Ref& ref, bool in_left_panel)
    {
        switch (ref.type)
        {
        case git::RefType::TAG:
            return tag_menu(app, state, ref, in_left_panel);
        case git::RefType::STASH:
            return stash_menu(app, state, ref);
        case git::RefType::LOCAL_BRANCH:
        case git::RefType::REMOTE_BRANCH:
            break;
        }
        RefGroup group = in_left_panel ? RefGroup{branch_group_name(ref), {ref}} : group_for_ref(state, ref);
        return commit_or_branch_menu(app, state, ref.target, std::move(group), in_left_panel);
    }

    MenuItems workdir_file_menu(app::Application& app, app::RepoState& state, const git::FileChange& change,
                                bool staged)
    {
        const std::string path = change.path;
        const git::FileChange copy = change;
        const bool added = change.status == git::FileStatus::ADDED || change.status == git::FileStatus::UNTRACKED;
        const bool conflicted = change.status == git::FileStatus::CONFLICTED;
        MenuItems ops;
        if (conflicted)
        {
            ops.push_back(
                    item("Open in merge tool", placeholder(app, "Merge tool", "No external merge tool is set up.")));
            ops.push_back(item("Mark resolved",
                               [&app, &state, path]
                               {
                                   app.run(state, "Mark resolved",
                                           [&]
                                           {
                                               return state.repo.stage({path});
                                           });
                               }));
        }
        else if (staged)
        {
            ops.push_back(item("Unstage",
                               [&app, &state, copy]
                               {
                                   std::vector<std::string> paths{copy.path};
                                   if (!copy.old_path.empty())
                                   {
                                       paths.push_back(copy.old_path);
                                   }
                                   app.run(state, "Unstage",
                                           [&]
                                           {
                                               return state.repo.unstage(paths);
                                           });
                               }));
        }
        else
        {
            ops.push_back(item("Stage",
                               [&app, &state, path]
                               {
                                   app.run(state, "Stage",
                                           [&]
                                           {
                                               return state.repo.stage({path});
                                           });
                               }));
        }
        const auto discard = [&app, &state, copy, staged]
        {
            if (staged)
            {
                std::vector<std::string> paths{copy.path};
                if (!copy.old_path.empty())
                {
                    paths.push_back(copy.old_path);
                }
                if (!state.repo.unstage(paths))
                {
                    return;
                }
            }
            app.run(
                    state, "Discard",
                    [&]
                    {
                        return state.repo.discard({copy});
                    },
                    "Discarded changes to " + file_name(copy.path));
        };
        if (!conflicted)
        {
            if (added && staged)
            {
                ops.push_back(item("Unstage and delete file", discard));
            }
            else if (!added)
            {
                ops.push_back(item("Discard changes", discard));
            }
        }
        if (file_name(path) != ".gitattributes" && file_name(path) != ".gitignore")
        {
            ops.push_back(ignore_submenu(app, state, path));
        }
        if (!conflicted && state.repo.state() != git::RepoState::MERGE)
        {
            ops.push_back(item("Stash file",
                               [&app, &state, path]
                               {
                                   app.run_git_cli(state, "Stash file", {"stash", "push", "--", path});
                               }));
        }
        MenuItems history = added ? MenuItems{} : history_items(state, path, {});
        MenuItems open;
        open.push_back(item("Open in external diff tool",
                            [&app, &state, path, staged]
                            {
                                std::vector<std::string> args{"difftool", "-y"};
                                if (staged)
                                {
                                    args.push_back("--cached");
                                }
                                args.push_back("--");
                                args.push_back(path);
                                app.run_git_cli(state, "Open in diff tool", args, true);
                            }));
        for (MenuItem& entry : open_file_items(state, path))
        {
            open.push_back(std::move(entry));
        }
        return separated_menu({std::move(ops),
                               std::move(history),
                               std::move(open),
                               {item("Copy file path",
                                     [&app, &state, path]
                                     {
                                         app.copy_to_clipboard(full_path(state, path));
                                     }),
                                item("Create patch from file changes",
                                     [&app, &state, path, staged]
                                     {
                                         std::vector<std::string> args{"diff"};
                                         if (staged)
                                         {
                                             args.push_back("--cached");
                                         }
                                         args.push_back("--");
                                         args.push_back(path);
                                         save_git_output(app, state, file_name(path) + ".patch", args);
                                     })},
                               edit_delete_items(app, state, path)});
    }

    MenuItems commit_file_menu(app::Application& app, app::RepoState& state, const git::Sha& sha,
                               const git::FileChange& change)
    {
        const std::string path = change.path;
        const bool stash = state.commit_for(sha) != nullptr && state.commit_for(sha)->type == git::NodeType::STASH;
        MenuItems apply;
        if (stash)
        {
            apply.push_back(item("Apply stashed file",
                                 [&app, &state, sha, path]
                                 {
                                     app.run_git_cli(state, "Apply stashed file", {"checkout", sha, "--", path});
                                 }));
        }
        MenuItems restore;
        if (!stash && selected_commits(state).size() == 1)
        {
            restore.push_back(item("Restore file from this commit",
                                   [&app, &state, sha, path]
                                   {
                                       app.run_git_cli(state, "Restore file", {"checkout", sha, "--", path});
                                   }));
        }
        MenuItems open;
        open.push_back(item("Open in external diff tool",
                            [&app, &state, sha, path]
                            {
                                app.run_git_cli(state, "Open in diff tool", {"difftool", "-y", sha + "^!", "--", path},
                                                true);
                            }));
        for (MenuItem& entry : open_file_items(state, path))
        {
            open.push_back(std::move(entry));
        }
        return separated_menu({stash ? MenuItems{} : history_items(state, path, sha),
                               std::move(open),
                               {item("Copy file path",
                                     [&app, &state, path]
                                     {
                                         app.copy_to_clipboard(full_path(state, path));
                                     })},
                               edit_delete_items(app, state, path),
                               std::move(apply),
                               std::move(restore)});
    }

    MenuItems multiple_files_menu(app::Application& app, app::RepoState& state, std::vector<std::string> paths,
                                  bool staged)
    {
        const std::string n = std::to_string(paths.size());
        MenuItems ops;
        if (staged)
        {
            ops.push_back(item(fmt("Unstage {0} files", {n}),
                               [&app, &state, paths]
                               {
                                   app.run(state, "Unstage",
                                           [&]
                                           {
                                               return state.repo.unstage(paths);
                                           });
                               }));
        }
        else
        {
            ops.push_back(item(fmt("Stage {0} files", {n}),
                               [&app, &state, paths]
                               {
                                   app.run(state, "Stage",
                                           [&]
                                           {
                                               return state.repo.stage(paths);
                                           });
                               }));
        }
        ops.push_back(item(fmt("Discard {0} files", {n}),
                           [&app, &state, paths, staged]
                           {
                               std::vector<git::FileChange> changes;
                               for (const git::FileChange& change :
                                    staged ? state.status.staged : state.status.unstaged)
                               {
                                   if (std::find(paths.begin(), paths.end(), change.path) != paths.end())
                                   {
                                       changes.push_back(change);
                                   }
                               }
                               if (staged)
                               {
                                   (void)state.repo.unstage(paths);
                               }
                               app.run(state, "Discard",
                                       [&]
                                       {
                                           return state.repo.discard(changes);
                                       });
                           }));
        ops.push_back(item(fmt("Ignore {0} files", {n}),
                           [&app, &state, paths]
                           {
                               std::ofstream file(full_path(state, ".gitignore"), std::ios::app);
                               for (const std::string& path : paths)
                               {
                                   file << path << '\n';
                               }
                               app.toasts.push(app::ToastKind::SUCCESS, "Added to .gitignore");
                               state.refresh_status();
                           }));
        if (state.repo.state() != git::RepoState::MERGE)
        {
            ops.push_back(item(fmt("Stash {0} files", {n}),
                               [&app, &state, paths]
                               {
                                   std::vector<std::string> args{"stash", "push", "--"};
                                   args.insert(args.end(), paths.begin(), paths.end());
                                   app.run_git_cli(state, "Stash files", args);
                               }));
        }
        return separated_menu({std::move(ops),
                               {item(fmt("Create patch from changes in {0} files", {n}),
                                     [&app, &state, paths, staged]
                                     {
                                         std::vector<std::string> args{"diff"};
                                         if (staged)
                                         {
                                             args.push_back("--cached");
                                         }
                                         args.push_back("--");
                                         args.insert(args.end(), paths.begin(), paths.end());
                                         save_git_output(app, state, "changes.patch", args);
                                     })}});
    }

    MenuItems folder_menu(app::Application& app, app::RepoState& state, const std::string& folder,
                          std::vector<std::string> files, bool staged, bool uncommitted)
    {
        MenuItems ops;
        const std::string base = std::filesystem::path(folder).filename().string();
        if (uncommitted)
        {
            if (staged)
            {
                ops.push_back(item("Unstage folder",
                                   [&app, &state, files]
                                   {
                                       app.run(state, "Unstage",
                                               [&]
                                               {
                                                   return state.repo.unstage(files);
                                               });
                                   }));
            }
            else
            {
                ops.push_back(item("Stage folder",
                                   [&app, &state, files]
                                   {
                                       app.run(state, "Stage",
                                               [&]
                                               {
                                                   return state.repo.stage(files);
                                               });
                                   }));
            }
            ops.push_back(item("Discard all changes in folder",
                               [&app, &state, files, staged]
                               {
                                   std::vector<git::FileChange> changes;
                                   for (const git::FileChange& change :
                                        staged ? state.status.staged : state.status.unstaged)
                                   {
                                       if (std::find(files.begin(), files.end(), change.path) != files.end())
                                       {
                                           changes.push_back(change);
                                       }
                                   }
                                   if (staged)
                                   {
                                       (void)state.repo.unstage(files);
                                   }
                                   app.run(state, "Discard",
                                           [&]
                                           {
                                               return state.repo.discard(changes);
                                           });
                               }));
            ops.push_back(item(fmt("Ignore all files in '{0}/'", {base}),
                               [&app, &state, folder]
                               {
                                   std::ofstream(full_path(state, ".gitignore"), std::ios::app) << folder << "/\n";
                                   app.toasts.push(app::ToastKind::SUCCESS, "Added to .gitignore", folder + "/");
                                   state.refresh_status();
                               }));
            if (state.repo.state() != git::RepoState::MERGE)
            {
                ops.push_back(item("Stash folder",
                                   [&app, &state, files]
                                   {
                                       std::vector<std::string> args{"stash", "push", "--"};
                                       args.insert(args.end(), files.begin(), files.end());
                                       app.run_git_cli(state, "Stash folder", args);
                                   }));
            }
            ops.push_back(item("Create Patch from changes in directory",
                               [&app, &state, files, staged]
                               {
                                   std::vector<std::string> args{"diff"};
                                   if (staged)
                                   {
                                       args.push_back("--cached");
                                   }
                                   args.push_back("--");
                                   args.insert(args.end(), files.begin(), files.end());
                                   save_git_output(app, state, "changes.patch", args);
                               }));
        }
        MenuItems open;
        const std::string full = full_path(state, folder);
        if (std::filesystem::is_directory(full))
        {
            open.push_back(item("Open folder",
                                [full]
                                {
                                    platform::shell::open_path(full);
                                }));
            open.push_back(item("Create a file in this folder",
                                placeholder(app, "Create file", "Creating files is not available in ImGit yet.")));
        }
        return separated_menu({std::move(ops), std::move(open)});
    }

    MenuItems file_history_commit_menu(app::Application& app, app::RepoState& state, const git::Sha& sha,
                                       const std::string& path)
    {
        return separated_menu({{item(
                                       "Show in graph",
                                       [&state, sha]
                                       {
                                           state.close_file_history();
                                           jump_to_commit(state, sha.c_str());
                                       },
                                       state.history.row_by_sha.contains(sha))},
                               {item("Copy commit sha",
                                     [&app, sha]
                                     {
                                         app.copy_to_clipboard(sha);
                                     }),
                                item("Copy file path",
                                     [&app, &state, path]
                                     {
                                         app.copy_to_clipboard(full_path(state, path));
                                     })}});
    }

    MenuItems tab_menu(app::Application& app, int index)
    {
        const int count = static_cast<int>(app.tabs.size());
        return separated_menu({
                {item("Close tab",
                      [&app, index]
                      {
                          app.close_tab(index);
                      }),
                 item(
                         "Close other tabs",
                         [&app, index]
                         {
                             for (int i = static_cast<int>(app.tabs.size()) - 1; i >= 0; --i)
                             {
                                 if (i != index)
                                 {
                                     app.close_tab(i);
                                 }
                             }
                             app.active_tab = app.tabs.empty() ? -1 : 0;
                         },
                         count > 1),
                 item(
                         "Close tabs to the right",
                         [&app, index]
                         {
                             for (int i = static_cast<int>(app.tabs.size()) - 1; i > index; --i)
                             {
                                 app.close_tab(i);
                             }
                         },
                         index < count - 1),
                 item("Alias repository",
                      placeholder(app, "Alias repository", "Repository aliases are not available in ImGit yet."))},
                {[&app]
                 {
                     MenuItem reopen = item(
                             "Reopen Closed Tab",
                             [&app]
                             {
                                 app.reopen_closed_tab();
                             },
                             !app.closed_tabs.empty());
                     reopen.accelerator = shortcut_label("Ctrl+Shift+T");
                     return reopen;
                 }()},
        });
    }

    MenuItems graph_header_menu(app::Application& app)
    {
        // The column checkboxes (a column cannot be hidden when it is the last one shown),
        // the layout toggles and the resets.
        app::Settings& s = app.settings;
        const std::function<void()> fixed_column =
                placeholder(app, "Graph columns", "This column cannot be hidden in ImGit yet.");
        const auto toggle = [&app](bool* value)
        {
            return [&app, value]
            {
                *value = !*value;
                app.settings.save(app.config_path("settings.txt"));
            };
        };
        return separated_menu({
                {menu_checkbox("Branch / Tag", true, fixed_column), menu_checkbox("Graph", true, fixed_column),
                 menu_checkbox("Commit message", true, fixed_column),
                 menu_checkbox("Author", s.show_author, toggle(&s.show_author)),
                 menu_checkbox("Date / Time", s.show_date, toggle(&s.show_date)),
                 menu_checkbox("Sha", s.show_sha, toggle(&s.show_sha))},
                {menu_checkbox("Compact Graph Column", s.compact_graph, toggle(&s.compact_graph)),
                 menu_checkbox("Smart Branch Visibility", false,
                               placeholder(app, "Smart Branch Visibility", "Not available in ImGit yet."))},
                {item("Reset columns to default layout",
                      [&app]
                      {
                          app.settings.show_author = false;
                          app.settings.show_date = false;
                          app.settings.show_sha = false;
                          app.settings.compact_graph = false;
                          app.settings.graph_column_order.clear();
                          app.settings.save(app.config_path("settings.txt"));
                      }),
                 item("Reset columns to compact layout",
                      [&app]
                      {
                          app.settings.show_author = false;
                          app.settings.show_date = false;
                          app.settings.show_sha = false;
                          app.settings.compact_graph = true;
                          app.settings.graph_column_order.clear();
                          app.settings.save(app.config_path("settings.txt"));
                      })},
        });
    }

    MenuItems remote_menu(app::Application& app, app::RepoState& state, const std::string& name, bool in_left_panel)
    {
        const std::string full_name = "refs/remotes/" + name;
        std::string url;
        for (const git::Remote& remote : state.history.remotes)
        {
            if (remote.name == name)
            {
                url = remote.url;
            }
        }
        return separated_menu({
                {item(fmt("Fetch {0}", {name}),
                      [&app, &state, name]
                      {
                          app.run_git_cli(state, "Fetch", {"fetch", name, "--prune"});
                      })},
                {item(fmt("Edit {0}", {name}),
                      [&app, name, url]
                      {
                          app.dialog.open(app::Dialog::Kind::EDIT_REMOTE);
                          app.dialog.ref_name = name;
                          app.dialog.text_a = name;
                          app.dialog.text_b = url;
                      }),
                 item(fmt("Remove {0}", {name}),
                      [&app, &state, name]
                      {
                          confirm(app, "Remove Remote", "Are you sure you want to remove the remote " + name + "?",
                                  "Remove", true,
                                  [&app, &state, name]
                                  {
                                      app.run(
                                              state, "Remove remote",
                                              [&]
                                              {
                                                  return state.repo.remove_remote(name);
                                              },
                                              "Removed remote " + name);
                                  });
                      })},
                {toggle_hide_item(state, full_name, in_left_panel ? state.solo_ref.empty() : true),
                 toggle_solo_item(state, full_name)},
        });
    }

    MenuItems local_folder_menu(app::Application& app, app::RepoState& state, const std::string& folder)
    {
        std::vector<std::string> branches;
        for (const git::Ref& ref : state.history.refs)
        {
            if (ref.type == git::RefType::LOCAL_BRANCH && ref.name.rfind(folder + "/", 0) == 0)
            {
                branches.push_back(ref.name);
            }
        }
        const std::string label = branches.size() == 1 ? fmt("Delete 1 branch in folder \"{0}\"", {folder})
                                                       : fmt("Delete {0} branches in folder \"{1}\"",
                                                             {std::to_string(branches.size()), folder});
        return {item(label,
                     [&app, &state, branches, folder]
                     {
                         confirm(app, "Delete Branches",
                                 "Delete " + std::to_string(branches.size()) + " branches in folder \"" + folder
                                         + "\"?",
                                 "Delete", true,
                                 [&app, &state, branches]
                                 {
                                     for (const std::string& branch : branches)
                                     {
                                         if (auto result = state.repo.delete_branch(branch); !result)
                                         {
                                             app.toasts.push(app::ToastKind::ERROR, "Could not delete " + branch,
                                                             result.error().message);
                                         }
                                     }
                                     state.refresh();
                                 });
                     })};
    }

    MenuItems left_section_menu(app::Application& app, app::RepoState& state, const std::string& section,
                                const std::vector<std::string>& sections)
    {
        // Hide / show everything in the section, maximize it, and the section switches.
        MenuItems visibility;
        const auto set_all = [&app, &state, section](bool hide)
        {
            return [&app, &state, section, hide]
            {
                (void)app;
                for (const git::Ref& ref : state.history.refs)
                {
                    const bool in_section =
                            (section == "Local" && ref.type == git::RefType::LOCAL_BRANCH && !ref.is_head)
                            || (section == "Remote" && ref.type == git::RefType::REMOTE_BRANCH)
                            || (section == "Stashes" && ref.type == git::RefType::STASH)
                            || (section == "Tags" && ref.type == git::RefType::TAG);
                    if (!in_section)
                    {
                        continue;
                    }
                    if (hide)
                    {
                        state.hidden_refs.insert(ref.full_name);
                    }
                    else
                    {
                        state.hidden_refs.erase(ref.full_name);
                    }
                }
                state.solo_ref.clear();
                state.refresh();
            };
        };
        static const std::map<std::string, std::pair<const char*, const char*>> LABELS = {
                {"Local", {"Hide all local branches", "Show all local branches"}},
                {"Remote", {"Hide all remotes", "Show all remotes"}},
                {"Stashes", {"Hide all stashes", "Show all stashes"}},
                {"Tags", {"Hide all tags", "Show all tags"}},
        };
        if (const auto labels = LABELS.find(section); labels != LABELS.end())
        {
            bool any_visible = false;
            bool any_hidden = false;
            for (const git::Ref& ref : state.history.refs)
            {
                const bool in_section = (section == "Local" && ref.type == git::RefType::LOCAL_BRANCH && !ref.is_head)
                                        || (section == "Remote" && ref.type == git::RefType::REMOTE_BRANCH)
                                        || (section == "Stashes" && ref.type == git::RefType::STASH)
                                        || (section == "Tags" && ref.type == git::RefType::TAG);
                if (in_section)
                {
                    (state.hidden_refs.contains(ref.full_name) ? any_hidden : any_visible) = true;
                }
            }
            visibility.push_back(item(labels->second.first, set_all(true), any_visible));
            visibility.push_back(item(labels->second.second, set_all(false), any_hidden));
        }
        MenuItems maximize;
        maximize.push_back(item("Maximize this section",
                                [&state, section]
                                {
                                    // collapseOtherLeftPanelSections
                                    state.expanded_left_sections = {section};
                                }));
        MenuItems switches;
        for (const std::string& name : sections)
        {
            switches.push_back(menu_checkbox(
                    name, !app.settings.hidden_left_sections.contains(name),
                    [&app, name]
                    {
                        if (!app.settings.hidden_left_sections.erase(name))
                        {
                            app.settings.hidden_left_sections.insert(name);
                        }
                        app.settings.save(app.config_path("settings.txt"));
                    },
                    name != "Local"));
        }
        return separated_menu({std::move(visibility), std::move(maximize), std::move(switches)});
    }

    MenuItems terminal_menu(app::Application& app)
    {
        const auto with_accelerator = [](MenuItem entry, std::string accelerator)
        {
            entry.accelerator = std::move(accelerator);
            return entry;
        };
        return separated_menu({
                {with_accelerator(item(
                                          "Copy",
                                          [&app]
                                          {
                                              terminal_copy(app);
                                          },
                                          terminal_has_selection(app)),
                                  shortcut_label("Ctrl+C")),
                 with_accelerator(item("Paste",
                                       [&app]
                                       {
                                           terminal_paste(app);
                                       }),
                                  shortcut_label("Ctrl+V")),
                 item("Select All",
                      [&app]
                      {
                          terminal_select_all(app);
                      })},
                {with_accelerator(item("Delete Line",
                                       [&app]
                                       {
                                           // Windows: Ctrl+Home, PSReadLine's BackwardDeleteLine.
                                           terminal_write(app, "\x1b[1;5H");
                                       }),
                                  "Esc"),
                 with_accelerator(item("Clear",
                                       [&app]
                                       {
                                           terminal_clear(app);
                                       }),
                                  "Ctrl+L")}, // the shell's own key: the Control key on macOS too
                {item("Minimize Terminal",
                      [&app]
                      {
                          app.terminal.minimized = true;
                      }),
                 item("Kill Terminal",
                      [&app]
                      {
                          app.kill_terminals(app.active());
                      }),
                 item("Hide Terminal",
                      [&app]
                      {
                          app.show_terminal_panel = false;
                      })},
        });
    }

    namespace
    {
        // Gives a text field the keyboard back, keeping its cursor and selection (a plain activation selects all).
        void refocus_field(ImGuiID id)
        {
            ImGui::ActivateItemByID(id);
            GImGui->NavNextActivateFlags = ImGuiActivateFlags_PreferInput | ImGuiActivateFlags_TryToPreserveState;
        }
    } // namespace

    void text_field_menu(const std::string& value)
    {
        // A menu command waiting for its field to have the keyboard again, then sent as a key or as typed text.
        struct PendingEdit
        {
            ImGuiID id = 0;
            ImGuiKey key = ImGuiKey_None;
            bool ctrl = false;
            std::string text;
        };
        static PendingEdit pending;
        const ImGuiID id = ImGui::GetItemID();
        if (pending.id == id && ImGui::IsItemActive())
        {
            ImGuiIO& io = ImGui::GetIO();
            if (!pending.text.empty())
            {
                io.AddInputCharactersUTF8(pending.text.c_str());
            }
            else
            {
                if (pending.ctrl)
                {
                    // Ctrl chords only reach a field whose window has focus (the menu took it).
                    ImGui::SetWindowFocus();
                    io.AddKeyEvent(shortcut_modifier_event(), true);
                }
                io.AddKeyEvent(pending.key, true);
                io.AddKeyEvent(pending.key, false);
                if (pending.ctrl)
                {
                    io.AddKeyEvent(shortcut_modifier_event(), false);
                }
            }
            pending = {};
        }

        // Electron's editable context menu: Cut / Copy / Paste /
        // Select All, enabled by the field's state. A right-click focuses the field like Chromium does.
        if (!ImGui::IsItemHovered() || !ImGui::IsMouseReleased(ImGuiMouseButton_Right))
        {
            return;
        }
        std::string selected;
        if (const ImGuiInputTextState* state = ImGui::GetInputTextState(id);
            state != nullptr && ImGui::IsItemActive() && state->HasSelection())
        {
            const int from = std::min(state->GetSelectionStart(), state->GetSelectionEnd());
            const int to = std::max(state->GetSelectionStart(), state->GetSelectionEnd());
            selected.assign(state->TextA.Data + from, state->TextA.Data + to);
        }
        if (!ImGui::IsItemActive())
        {
            refocus_field(id);
        }
        // The menu takes the mouse, so each command gives the field the keyboard back first.
        const auto keys = [id](ImGuiKey key, bool ctrl)
        {
            return [id, key, ctrl]
            {
                refocus_field(id);
                pending = {id, key, ctrl, {}};
            };
        };
        const char* clipboard = ImGui::GetClipboardText();
        const bool has_selection = !selected.empty();
        MenuItem cut = item(
                "Cut",
                [selected, cut_keys = keys(ImGuiKey_Delete, false)]
                {
                    ImGui::SetClipboardText(selected.c_str());
                    cut_keys();
                },
                has_selection);
        cut.accelerator = shortcut_label("Ctrl+X");
        MenuItem copy = item(
                "Copy",
                [selected, id]
                {
                    ImGui::SetClipboardText(selected.c_str());
                    refocus_field(id);
                },
                has_selection);
        copy.accelerator = shortcut_label("Ctrl+C");
        // Paste types the clipboard into the field (replacing the selection), like Ctrl+V.
        MenuItem paste = item(
                "Paste",
                [id]
                {
                    refocus_field(id);
                    const char* text = ImGui::GetClipboardText();
                    pending = {id, ImGuiKey_None, false, text != nullptr ? text : ""};
                },
                clipboard != nullptr && *clipboard != '\0');
        paste.accelerator = shortcut_label("Ctrl+V");
        MenuItem select_all = item("Select All", keys(ImGuiKey_A, true), !value.empty());
        select_all.accelerator = shortcut_label("Ctrl+A");
        popup_menu({std::move(cut), std::move(copy), std::move(paste), std::move(select_all)});
    }

    MenuItems text_input_menu(bool read_only)
    {
        // Electron roles, which send the editing commands to the focused field.
        const auto chord = [](ImGuiKey key)
        {
            return [key]
            {
                ImGuiIO& io = ImGui::GetIO();
                io.AddKeyEvent(shortcut_modifier_event(), true);
                io.AddKeyEvent(key, true);
                io.AddKeyEvent(key, false);
                io.AddKeyEvent(shortcut_modifier_event(), false);
            };
        };
        if (read_only)
        {
            return {item("Copy", chord(ImGuiKey_C)), item("Select All", chord(ImGuiKey_A))};
        }
        return {item("Cut", chord(ImGuiKey_X)), item("Copy", chord(ImGuiKey_C)), item("Paste", chord(ImGuiKey_V)),
                item("Delete",
                     []
                     {
                         ImGui::GetIO().AddKeyEvent(ImGuiKey_Delete, true);
                         ImGui::GetIO().AddKeyEvent(ImGuiKey_Delete, false);
                     }),
                item("Select All", chord(ImGuiKey_A))};
    }
} // namespace imgit::ui
