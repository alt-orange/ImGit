#include "app/workflows.hpp"
#include "app/application.hpp"
#include "app/theme.hpp"
#include "ui/panels.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <imgui.h>
#include <map>
#include <string>
#include <vector>

// The workflows: branch cleanup (delete the local branches merged into the default branch or whose upstream is
// gone, after a review) and Gitflow's start / finish of features, releases and hotfixes. Both run git through the
// application (libgit2 for the branch deletions, the git CLI for the gitflow merges, so a conflict leaves the
// repository in git's usual merge state).
namespace imgit::ui
{
    namespace
    {
        namespace gitflow = app::gitflow;

        // The repository's gitflow.* configuration, read again at most once a second.
        const gitflow::Config& gitflow_config(const app::RepoState& state)
        {
            struct Cached
            {
                double read_at = -1.0;
                gitflow::Config config;
            };
            static std::map<std::string, Cached> cache;
            Cached& cached = cache[state.repo.workdir()];
            const double now = ImGui::GetTime();
            if (cached.read_at < 0.0 || now - cached.read_at > 1.0 || now < cached.read_at)
            {
                const auto value = [&](const char* key, const char* fallback)
                {
                    std::string text = state.repo.config_string(key);
                    return text.empty() ? std::string(fallback) : text;
                };
                cached.config.master = state.repo.config_string("gitflow.branch.master");
                cached.config.develop = state.repo.config_string("gitflow.branch.develop");
                cached.config.feature_prefix = value("gitflow.prefix.feature", "feature/");
                cached.config.release_prefix = value("gitflow.prefix.release", "release/");
                cached.config.hotfix_prefix = value("gitflow.prefix.hotfix", "hotfix/");
                cached.config.version_tag = state.repo.config_string("gitflow.prefix.versiontag");
                cached.read_at = now;
            }
            return cached.config;
        }

        bool has_branch(const app::RepoState& state, const std::string& name)
        {
            return state.find_ref("refs/heads/" + name) != nullptr;
        }

        // Gitflow merges and checkouts need a working directory without changes (untracked files aside).
        bool working_directory_clean(app::Application& app, const app::RepoState& state, const std::string& what)
        {
            const git::WorkDirStatus& status = state.status;
            const bool dirty = !status.staged.empty() || !status.conflicted.empty()
                               || std::any_of(status.unstaged.begin(), status.unstaged.end(),
                                              [](const git::FileChange& change)
                                              {
                                                  return change.status != git::FileStatus::UNTRACKED;
                                              });
            if (dirty)
            {
                app.toasts.push(app::ToastKind::ERROR, what + " failed",
                                "The working directory has changes. Commit or stash them first.");
            }
            return !dirty;
        }

        void start(app::Application& app, app::RepoState& state, gitflow::Kind kind)
        {
            const gitflow::Config config = gitflow_config(state);
            const std::string title = std::string("Start ") + gitflow::kind_name(kind);
            const std::string& base = gitflow::base_branch(config, kind);
            if (!has_branch(state, base))
            {
                app.toasts.push(app::ToastKind::ERROR, title + " failed", "There is no local branch '" + base + "'.");
                return;
            }
            const std::string workdir = state.repo.workdir();
            open_text_prompt(app, std::string("Name of the new ") + gitflow::kind_name(kind) + ":", "", false,
                             [&app, workdir, kind, config, title](const std::string& text)
                             {
                                 app::RepoState* tab = app.find_repo_state(workdir);
                                 std::string name = text;
                                 while (!name.empty() && name.back() == ' ')
                                 {
                                     name.pop_back();
                                 }
                                 while (!name.empty() && name.front() == ' ')
                                 {
                                     name.erase(name.begin());
                                 }
                                 if (tab == nullptr || name.empty())
                                 {
                                     return;
                                 }
                                 const std::string branch = gitflow::prefix(config, kind) + name;
                                 if (has_branch(*tab, branch))
                                 {
                                     app.toasts.push(app::ToastKind::ERROR, title + " failed",
                                                     "The branch '" + branch + "' exists already.");
                                     return;
                                 }
                                 if (!working_directory_clean(app, *tab, title))
                                 {
                                     return;
                                 }
                                 app.run_git_cli_sequence(*tab, title + " " + name,
                                                          gitflow::start_runs(config, kind, name));
                             });
        }

        void finish(app::Application& app, app::RepoState& state, const gitflow::Branch& branch)
        {
            const gitflow::Config config = gitflow_config(state);
            const std::string kind = gitflow::kind_name(branch.kind);
            const std::string full = gitflow::prefix(config, branch.kind) + branch.name;
            const std::string title = "Finish " + kind + " " + branch.name;
            for (const std::string& target : {config.master, config.develop})
            {
                if (!has_branch(state, target))
                {
                    app.toasts.push(app::ToastKind::ERROR, title + " failed",
                                    "There is no local branch '" + target + "'.");
                    return;
                }
            }
            if (!working_directory_clean(app, state, title))
            {
                return;
            }
            const std::string tag = gitflow::tag_name(config, branch.name);
            const bool tag_exists = state.find_ref("refs/tags/" + tag) != nullptr;
            std::string message;
            if (branch.kind == gitflow::Kind::FEATURE)
            {
                message = "Merge '" + full + "' into '" + config.develop + "' and delete it?";
            }
            else
            {
                message = "Merge '" + full + "' into '" + config.master + "', "
                          + (tag_exists ? "" : "tag it '" + tag + "', ") + "merge it into '" + config.develop
                          + "' and delete it?";
            }
            message += " A merge conflict stops the finish there; resolve and commit it, then finish again.";
            app.dialog.open(app::Dialog::Kind::CONFIRM);
            app.dialog.workdir = state.repo.workdir();
            app.dialog.title = "Finish " + kind;
            app.dialog.message = std::move(message);
            app.dialog.confirm_label = "Finish " + std::string(gitflow::kind_title(branch.kind));
            app.dialog.on_confirm = [&app, workdir = state.repo.workdir(), config, branch, tag_exists, title]
            {
                if (app::RepoState* tab = app.find_repo_state(workdir); tab != nullptr)
                {
                    app.run_git_cli_sequence(*tab, title,
                                             gitflow::finish_runs(config, branch.kind, branch.name, tag_exists));
                }
            };
        }
    } // namespace

    // -------------------------------------------------------------------------------------------------------
    // Gitflow

    bool gitflow_initialized(const app::RepoState& state)
    {
        return gitflow_config(state).initialized();
    }

    MenuItems gitflow_start_items(app::Application& app, app::RepoState& state)
    {
        if (!gitflow_initialized(state))
        {
            return {};
        }
        MenuItems items;
        for (const gitflow::Kind kind : {gitflow::Kind::FEATURE, gitflow::Kind::RELEASE, gitflow::Kind::HOTFIX})
        {
            items.push_back(menu_item(std::string("Start ") + gitflow::kind_title(kind),
                                      [&app, &state, kind]
                                      {
                                          start(app, state, kind);
                                      }));
        }
        return items;
    }

    MenuItems gitflow_finish_items(app::Application& app, app::RepoState& state, const std::string& branch)
    {
        const auto found = gitflow::classify(gitflow_config(state), branch);
        if (!found)
        {
            return {};
        }
        return {menu_item("Finish " + std::string(gitflow::kind_name(found->kind)) + " " + found->name,
                          [&app, &state, flow = *found]
                          {
                              finish(app, state, flow);
                          })};
    }

    std::vector<const git::Ref*> gitflow_branches(const app::RepoState& state)
    {
        std::vector<const git::Ref*> branches;
        const gitflow::Config& config = gitflow_config(state);
        for (const git::Ref& ref : state.history.refs)
        {
            if (ref.type == git::RefType::LOCAL_BRANCH && gitflow::classify(config, ref.name))
            {
                branches.push_back(&ref);
            }
        }
        std::sort(branches.begin(), branches.end(),
                  [](const git::Ref* a, const git::Ref* b)
                  {
                      return a->name < b->name;
                  });
        return branches;
    }

    // -------------------------------------------------------------------------------------------------------
    // Branch cleanup

    void open_branch_cleanup(app::Application& app, app::RepoState& state)
    {
        const std::string remote = state.history.remotes.empty() ? "origin" : state.history.remotes.front().name;
        const std::string default_branch = state.repo.default_branch(remote);
        const git::Ref* default_ref = default_branch.empty() ? nullptr : state.find_ref("refs/heads/" + default_branch);
        std::vector<app::cleanup::BranchFacts> branches;
        for (const git::Ref& ref : state.history.refs)
        {
            if (ref.type != git::RefType::LOCAL_BRANCH)
            {
                continue;
            }
            app::cleanup::BranchFacts facts;
            facts.name = ref.name;
            facts.is_head = ref.is_head || state.repo.branch_checked_out(ref.name);
            facts.merged =
                    default_ref != nullptr
                    && (ref.target == default_ref->target || state.repo.is_ancestor(ref.target, default_ref->target));
            facts.gone = state.repo.upstream_gone(ref.name);
            branches.push_back(std::move(facts));
        }
        std::vector<std::string> protected_branches{default_branch};
        if (const gitflow::Config& config = gitflow_config(state); config.initialized())
        {
            protected_branches.push_back(config.master);
            protected_branches.push_back(config.develop);
        }
        const std::vector<app::cleanup::BranchFacts> found = app::cleanup::candidates(branches, protected_branches);
        if (found.empty())
        {
            app.toasts.push(app::ToastKind::INFO, "No branches to clean up",
                            default_branch.empty() ? "No local branch has lost its upstream."
                                                   : "No local branch is merged into " + default_branch
                                                             + " or has lost its upstream.");
            return;
        }
        app.dialog.open(app::Dialog::Kind::BRANCH_CLEANUP);
        app.dialog.workdir = state.repo.workdir();
        app.dialog.text_a = default_branch;
        for (const app::cleanup::BranchFacts& branch : found)
        {
            app.dialog.checks.push_back({branch.name, app::cleanup::reason(branch), true});
        }
    }

    void draw_branch_cleanup(app::Application& app, app::RepoState& state, app::Dialog& dialog)
    {
        const theme::Palette& p = theme::palette();
        const std::string intro = dialog.text_a.empty() ? std::string("These local branches have lost their upstream.")
                                                        : "These local branches are merged into " + dialog.text_a
                                                                  + " or have lost their upstream.";
        ImGui::PushTextWrapPos(0.f);
        ImGui::TextUnformatted((intro + " Untick the ones to keep.").c_str());
        ImGui::PopTextWrapPos();
        ImGui::Dummy(ImVec2(0.f, px(6.f)));
        if (link_text("Select all"))
        {
            for (app::Dialog::Check& check : dialog.checks)
            {
                check.checked = true;
            }
        }
        ImGui::SameLine(0.f, px(16.f));
        if (link_text("Select none"))
        {
            for (app::Dialog::Check& check : dialog.checks)
            {
                check.checked = false;
            }
        }
        ImGui::Dummy(ImVec2(0.f, px(4.f)));
        const float row_h = ImGui::GetFrameHeightWithSpacing();
        const float list_h = std::min(px(280.f), row_h * static_cast<float>(dialog.checks.size()) + px(12.f));
        ImGui::PushStyleColor(ImGuiCol_ChildBg, p.input_bg);
        ImGui::BeginChild("##cleanup_list", ImVec2(-1.f, list_h), ImGuiChildFlags_Borders);
        ImGui::PopStyleColor();
        for (std::size_t i = 0; i < dialog.checks.size(); ++i)
        {
            app::Dialog::Check& check = dialog.checks[i];
            ImGui::PushID(static_cast<int>(i));
            checkbox(check.label.c_str(), &check.checked);
            ImGui::SameLine(0.f, px(10.f));
            // On the label's baseline: checkbox() frames its box with 2px padding.
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(2.f), px(2.f)));
            ImGui::AlignTextToFramePadding();
            ImGui::PopStyleVar();
            ImGui::TextColored(p.text_secondary, "%s", check.note.c_str());
            ImGui::PopID();
        }
        ImGui::EndChild();

        std::size_t ticked = 0;
        for (const app::Dialog::Check& check : dialog.checks)
        {
            ticked += check.checked ? 1 : 0;
        }
        const std::string label =
                ticked == 1 ? std::string("Delete 1 Branch") : "Delete " + std::to_string(ticked) + " Branches";
        ImGui::Dummy(ImVec2(0.f, px(8.f)));
        const float confirm_w = px(150.f);
        ImGui::SetCursorPosX(ImGui::GetContentRegionMax().x - confirm_w - px(110.f) - ImGui::GetStyle().ItemSpacing.x);
        const bool cancel = default_button("Cancel", ImVec2(px(110.f), px(32.f)));
        ImGui::SameLine();
        const bool confirm = danger_button(label.c_str(), ImVec2(confirm_w, px(32.f)), ticked > 0);
        if (cancel || ImGui::IsKeyPressed(ImGuiKey_Escape))
        {
            ImGui::CloseCurrentPopup();
            return;
        }
        if (!confirm)
        {
            return;
        }
        // Each branch deleted with its tip remembered: one undo entry puts them all back.
        std::vector<std::pair<std::string, git::Sha>> deleted;
        std::string error;
        for (const app::Dialog::Check& check : dialog.checks)
        {
            const git::Ref* ref = check.checked ? state.find_ref("refs/heads/" + check.label) : nullptr;
            if (ref == nullptr)
            {
                continue;
            }
            const git::Sha sha = ref->target;
            if (auto result = state.repo.delete_branch(check.label); !result)
            {
                error = check.label + ": " + result.error().message;
                break;
            }
            deleted.emplace_back(check.label, sha);
        }
        if (!deleted.empty())
        {
            git::Repository* repo = &state.repo;
            state.push_undo({deleted.size() == 1 ? "delete branch " + deleted.front().first
                                                 : "delete " + std::to_string(deleted.size()) + " branches",
                             [repo, deleted]() -> git::Result<void>
                             {
                                 for (const auto& [name, sha] : deleted)
                                 {
                                     if (auto result = repo->set_branch_target(name, sha); !result)
                                     {
                                         return result;
                                     }
                                 }
                                 return {};
                             },
                             [repo, deleted]() -> git::Result<void>
                             {
                                 for (const auto& [name, sha] : deleted)
                                 {
                                     if (auto result = repo->delete_branch(name); !result)
                                     {
                                         return result;
                                     }
                                 }
                                 return {};
                             }});
        }
        if (!error.empty())
        {
            app.toasts.push(app::ToastKind::ERROR, "Branch cleanup failed", error);
        }
        else
        {
            app.toasts.push(app::ToastKind::SUCCESS,
                            deleted.size() == 1 ? std::string("Deleted 1 branch")
                                                : "Deleted " + std::to_string(deleted.size()) + " branches");
        }
        state.refresh();
        ImGui::CloseCurrentPopup();
    }
} // namespace imgit::ui
