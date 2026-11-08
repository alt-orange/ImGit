#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/shell.hpp"
#include "ui/popover.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <filesystem>
#include <functional>
#include <imgui.h>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <string>
#include <vector>

// The command palette (.fuzzy-finder): a modal 82px below the top, 600px wide, with a 38px react-select
// control (2px #4D88FF border + 1px ring) over a 200px list of 40px rows. Its commands are in
// alphabetical order; choosing Checkout / Open Repo / a stash command / Switch to Profile lists the branches /
// repositories / stashes / profiles next; the file commands (Blame, History, View / Edit / Open / Delete File, the
// external diff tool) list the repository's files.
namespace imgit::ui
{
    namespace
    {
        struct Entry
        {
            std::string label;
            std::function<void()> action;
            int score = 0;
        };

        enum class Stage
        {
            COMMANDS,
            CHECKOUT,
            OPEN_REPO,
            STASH_APPLY,
            STASH_POP,
            SWITCH_PROFILE,
            SWITCH_THEME,
            FILE,
        };
        Stage g_stage = Stage::COMMANDS;
        // What the FILE stage does with the file picked.
        enum class FileAction
        {
            BLAME,
            HISTORY,
            VIEW,
            EDIT,
            OPEN,
            DELETE,
            EXTERNAL_DIFF,
        };
        FileAction g_file_action = FileAction::VIEW;
        // The FILE stage lists at most this many matches (the rows are drawn every frame).
        constexpr std::size_t MAX_FILE_MATCHES = 200;

        // The repository's files: HEAD's tree plus the working directory's new files, sorted; read again when HEAD
        // or the status changes.
        const std::vector<std::string>& repository_files(app::RepoState& state)
        {
            static std::string key;
            static std::vector<std::string> files;
            std::string now = state.repo.workdir() + "|" + state.history.head_sha + "|"
                              + std::to_string(state.status.count());
            if (now == key)
            {
                return files;
            }
            key = std::move(now);
            files.clear();
            if (!state.history.head_sha.empty())
            {
                if (auto tree = state.repo.commit_tree_paths(state.history.head_sha))
                {
                    files = std::move(*tree);
                }
            }
            for (const auto* list : {&state.status.unstaged, &state.status.staged, &state.status.conflicted})
            {
                for (const git::FileChange& change : *list)
                {
                    files.push_back(change.path);
                }
            }
            std::sort(files.begin(), files.end());
            files.erase(std::unique(files.begin(), files.end()), files.end());
            return files;
        }

        // The working directory's change of `path` (`staged` set when it is in the index), nullptr when it has none.
        const git::FileChange* workdir_change(const app::RepoState& state, const std::string& path, bool& staged)
        {
            for (const auto* list : {&state.status.conflicted, &state.status.unstaged, &state.status.staged})
            {
                for (const git::FileChange& change : *list)
                {
                    if (change.path == path)
                    {
                        staged = list == &state.status.staged;
                        return &change;
                    }
                }
            }
            return nullptr;
        }

        void run_file_action(app::Application& app, app::RepoState& state, const std::string& path)
        {
            bool staged = false;
            const git::FileChange* change = workdir_change(state, path, staged);
            switch (g_file_action)
            {
            case FileAction::BLAME:
            case FileAction::HISTORY:
                state.open_file_history(path, {}, {}, g_file_action == FileAction::BLAME);
                break;
            case FileAction::VIEW:
                if (change != nullptr)
                {
                    state.open_diff({staged ? app::DiffTarget::Source::STAGED : app::DiffTarget::Source::UNSTAGED, {},
                                     path, change->status});
                    state.file_view = true;
                }
                else
                {
                    state.open_file_history(path, {}, {}, false);
                }
                break;
            case FileAction::EDIT:
                open_file_editor(app, state, path);
                break;
            case FileAction::OPEN:
                platform::shell::open_path((std::filesystem::path(state.repo.workdir()) / path).string());
                break;
            case FileAction::DELETE:
                app.dialog.open(app::Dialog::Kind::CONFIRM);
                app.dialog.workdir = state.repo.workdir();
                app.dialog.title = "Delete file";
                app.dialog.message = "Delete '" + path + "' from the working directory? A committed file can be "
                                     "brought back by discarding the deletion.";
                app.dialog.confirm_label = "Delete";
                app.dialog.danger = true;
                app.dialog.on_confirm = [&app, &state, path]
                {
                    std::error_code error;
                    std::filesystem::remove(std::filesystem::path(state.repo.workdir()) / path, error);
                    if (error)
                    {
                        app.toasts.push(app::ToastKind::ERROR, "Could not delete " + path, error.message());
                        return;
                    }
                    state.refresh_status();
                };
                break;
            case FileAction::EXTERNAL_DIFF:
                if (change != nullptr && change->status == git::FileStatus::CONFLICTED)
                {
                    app.run_git_cli(state, "External merge tool", {"mergetool", "--no-prompt", "--", path});
                }
                else if (change != nullptr)
                {
                    std::vector<std::string> args{"difftool", "-y"};
                    if (staged)
                    {
                        args.push_back("--cached");
                    }
                    args.push_back("--");
                    args.push_back(path);
                    app.run_git_cli(state, "Open in diff tool", args, true);
                }
                else
                {
                    app.toasts.push(app::ToastKind::INFO, "No changes in " + path,
                                    "The external diff tool opens files with uncommitted changes.");
                }
                break;
            }
        }
        bool g_was_open = false; // open on the previous frame

        std::vector<Entry> build_entries(app::Application& app, app::RepoState* state)
        {
            std::vector<Entry> entries;
            const auto add = [&](std::string label, std::function<void()> action)
            {
                entries.push_back({std::move(label), std::move(action)});
            };
            const auto next = [&app](Stage stage)
            {
                return [&app, stage]
                {
                    g_stage = stage;
                    app.fuzzy.text.clear();
                    app.fuzzy.selected = 0;
                    app.fuzzy.open = true;
                    app.fuzzy.focus_requested = true;
                };
            };
            const auto pick_file = [&app, state](FileAction action) -> std::function<void()>
            {
                return [&app, state, action]
                {
                    if (state == nullptr)
                    {
                        return;
                    }
                    g_file_action = action;
                    g_stage = Stage::FILE;
                    app.fuzzy.text.clear();
                    app.fuzzy.selected = 0;
                    app.fuzzy.open = true;
                    app.fuzzy.focus_requested = true;
                };
            };
            app::RepoState* s = state;
            const auto with_repo = [s](std::function<void(app::RepoState&)> action) -> std::function<void()>
            {
                return [s, action]
                {
                    if (s != nullptr)
                    {
                        action(*s);
                    }
                };
            };

            if (g_stage == Stage::CHECKOUT && s != nullptr)
            {
                for (const git::Ref& ref : s->history.refs)
                {
                    if (ref.type == git::RefType::LOCAL_BRANCH || ref.type == git::RefType::REMOTE_BRANCH)
                    {
                        const git::Ref copy = ref;
                        add(ref.name,
                            [&app, s, copy]
                            {
                                checkout_ref(app, *s, copy);
                            });
                    }
                }
                return entries;
            }
            if (g_stage == Stage::FILE && s != nullptr)
            {
                for (const std::string& path : repository_files(*s))
                {
                    add(path,
                        [&app, s, path]
                        {
                            run_file_action(app, *s, path);
                        });
                }
                return entries;
            }
            if (g_stage == Stage::SWITCH_PROFILE)
            {
                for (const app::Profile& profile : app.profiles.all())
                {
                    const std::string guid = profile.guid;
                    add(profile.name,
                        [&app, guid]
                        {
                            app.switch_profile(guid);
                        });
                }
                return entries;
            }
            if (g_stage == Stage::SWITCH_THEME)
            {
                for (const char* name : theme::theme_names())
                {
                    add(name,
                        [&app, name]
                        {
                            app.settings.theme = name;
                            app.apply_settings();
                        });
                }
                return entries;
            }
            if (g_stage == Stage::OPEN_REPO)
            {
                for (const std::string& path : app.recent_repos)
                {
                    add(path,
                        [&app, path]
                        {
                            app.open_repository(path);
                        });
                }
                add("Browse for a repository...",
                    [&app]
                    {
                        app.browse_for_repository();
                    });
                return entries;
            }
            if ((g_stage == Stage::STASH_APPLY || g_stage == Stage::STASH_POP) && s != nullptr)
            {
                const bool pop = g_stage == Stage::STASH_POP;
                for (const git::Ref& ref : s->history.refs)
                {
                    if (ref.type != git::RefType::STASH)
                    {
                        continue;
                    }
                    const int index = ref.stash_index;
                    add(ref.message.empty() ? ref.name : ref.message,
                        [&app, s, index, pop]
                        {
                            app.run(
                                    *s, pop ? "Pop stash" : "Apply stash",
                                    [&]
                                    {
                                        return pop ? s->repo.stash_pop(index) : s->repo.stash_apply(index);
                                    },
                                    pop ? "Stash popped" : "Stash applied");
                        });
                }
                return entries;
            }

            add("Activity Logs",
                [&app]
                {
                    app.notifications_requested = true;
                });
            add("Add Remote", with_repo(
                                      [&app](app::RepoState&)
                                      {
                                          app.dialog.open(app::Dialog::Kind::ADD_REMOTE);
                                      }));
            add("Apply patch", with_repo(
                                       [&app](app::RepoState& r)
                                       {
                                           if (const auto file = platform::shell::open_file_dialog("patch"))
                                           {
                                               app.run_git_cli(r, "Apply patch", {"apply", *file});
                                           }
                                       }));
            add("Blame", pick_file(FileAction::BLAME));
            add("Checkout", next(Stage::CHECKOUT));
            add("Clone",
                [&app]
                {
                    app.dialog.open(app::Dialog::Kind::CLONE_REPO);
                });
            add("Close Tab",
                [&app]
                {
                    app.close_tab(app.active_tab);
                });
            add("Configure Commit Signing",
                [&app]
                {
                    app.preferences_open = true;
                    app.preferences_page = "Commit Signing";
                });
            add("Configure Gitflow",
                [&app]
                {
                    app.preferences_open = true;
                    app.preferences_page = "Gitflow";
                });
            add("Configure LFS",
                [&app]
                {
                    app.preferences_open = true;
                    app.preferences_page = "LFS";
                });
            add("Create Annotated Tag", with_repo(
                                                [&app](app::RepoState& r)
                                                {
                                                    app.dialog.open(app::Dialog::Kind::CREATE_TAG);
                                                    app.dialog.sha = r.history.head_sha;
                                                    app.dialog.flag = true;
                                                }));
            add("Create Branch", with_repo(
                                         [&app](app::RepoState& r)
                                         {
                                             app.dialog.open(app::Dialog::Kind::CREATE_BRANCH);
                                             app.dialog.flag = true;
                                             app.dialog.sha = r.history.head_sha;
                                         }));
            add("Create File", with_repo(
                                       [&app](app::RepoState&)
                                       {
                                           app.dialog.open(app::Dialog::Kind::CREATE_FILE);
                                       }));
            add("Create Tag", with_repo(
                                      [&app](app::RepoState& r)
                                      {
                                          app.dialog.open(app::Dialog::Kind::CREATE_TAG);
                                          app.dialog.sha = r.history.head_sha;
                                      }));
            add("Create patch from all working directory changes",
                with_repo(
                        [](app::RepoState& r)
                        {
                            if (const auto file = platform::shell::save_file_dialog("changes.patch", "patch"))
                            {
                                const git::cli::Output output = git::cli::run(r.repo.workdir(), {"diff", "HEAD"});
                                std::FILE* out = std::fopen(file->c_str(), "wb");
                                if (out != nullptr)
                                {
                                    std::fwrite(output.text.data(), 1, output.text.size(), out);
                                    std::fclose(out);
                                }
                            }
                        }));
            add("Decrease Zoom",
                [&app]
                {
                    app.set_zoom(app.settings.zoom - 0.1f);
                });
            add("Delete File", pick_file(FileAction::DELETE));
            add("Discard all changes",
                [&app]
                {
                    app.dialog.open(app::Dialog::Kind::DISCARD_ALL);
                });
            add("Edit File", pick_file(FileAction::EDIT));
            add("Fetch All", with_repo(
                                     [&app](app::RepoState& r)
                                     {
                                         app.run_git_cli(r, "Fetch", {"fetch", "--all", "--prune"});
                                     }));
            add("History", pick_file(FileAction::HISTORY));
            add("Increase Zoom",
                [&app]
                {
                    app.set_zoom(app.settings.zoom + 0.1f);
                });
            add("Initialize LFS on this repo", with_repo(
                                                       [&app](app::RepoState& r)
                                                       {
                                                           app.run_git_cli(r, "LFS install", {"lfs", "install"});
                                                       }));
            add("Initialize a Repo",
                [&app]
                {
                    app.dialog.open(app::Dialog::Kind::INIT_REPO);
                });
            add("Keyboard Shortcuts",
                [&app]
                {
                    app.dialog.open(app::Dialog::Kind::KEYBOARD_SHORTCUTS);
                });
            add("Open File", pick_file(FileAction::OPEN));
            add("Open Repo", next(Stage::OPEN_REPO));
            add("Open Repository Management Tab",
                [&app]
                {
                    app.repo_management_open = true;
                });
            add("Open in External Diff/Merge Tool", pick_file(FileAction::EXTERNAL_DIFF));
            add("Open in VS Code", with_repo(
                                           [](app::RepoState& r)
                                           {
                                               platform::shell::open_in_editor(r.repo.workdir());
                                           }));
            add("Open in default terminal", with_repo(
                                                    [](app::RepoState& r)
                                                    {
                                                        platform::shell::open_external_terminal(r.repo.workdir());
                                                    }));
            add("Open in file manager", with_repo(
                                                [](app::RepoState& r)
                                                {
                                                    platform::shell::open_in_file_manager(r.repo.workdir());
                                                }));
            add("Perform Repo Maintenance", with_repo(
                                                    [&app](app::RepoState& r)
                                                    {
                                                        app.run_git_cli(r, "Git maintenance", {"maintenance", "run"});
                                                    }));
            add("Prune Remote Branches", with_repo(
                                                 [&app](app::RepoState& r)
                                                 {
                                                     app.run_git_cli(r, "Prune",
                                                                     {"remote", "prune",
                                                                      r.history.remotes.empty()
                                                                              ? "origin"
                                                                              : r.history.remotes.front().name});
                                                 }));
            add("Pull", with_repo(
                                [&app](app::RepoState& r)
                                {
                                    app.run_git_cli(r, "Pull", {"pull"});
                                }));
            add("Push", with_repo(
                                [&app](app::RepoState& r)
                                {
                                    app.run_git_cli(r, "Push", {"push"});
                                }));
            add("Redo", with_repo(
                                [&app](app::RepoState& r)
                                {
                                    app.redo(r);
                                }));
            add("Rename Branch", with_repo(
                                         [&app](app::RepoState& r)
                                         {
                                             app.dialog.open(app::Dialog::Kind::RENAME_BRANCH);
                                             app.dialog.ref_name = r.history.head_branch;
                                             app.dialog.text_a = r.history.head_branch;
                                         }));
            add("Reset Zoom",
                [&app]
                {
                    app.set_zoom(1.f);
                });
            add("Search Commits", with_repo(
                                          [](app::RepoState& r)
                                          {
                                              r.search_open = true;
                                              r.search_focus_requested = true;
                                          }));
            add("Settings",
                [&app]
                {
                    app.preferences_open = true;
                });
            add("Stage all changes", with_repo(
                                             [&app](app::RepoState& r)
                                             {
                                                 app.run(r, "Stage all",
                                                         [&]
                                                         {
                                                             return r.repo.stage_all();
                                                         });
                                             }));
            add("Stash: Apply", next(Stage::STASH_APPLY));
            add("Stash: Create", with_repo(
                                         [](app::RepoState& r)
                                         {
                                             r.composer_stash = true;
                                         }));
            add("Stash: Pop", next(Stage::STASH_POP));
            add("Switch Theme", next(Stage::SWITCH_THEME));
            add("Switch to Profile", next(Stage::SWITCH_PROFILE));
            add("Toggle Commit Details Panel",
                [&app]
                {
                    app.show_detail_panel = !app.show_detail_panel;
                });
            add("Toggle Left Panel",
                [&app]
                {
                    app.show_left_panel = !app.show_left_panel;
                });
            add("Toggle Syntax Highlighting",
                [&app]
                {
                    app.settings.syntax_highlighting = !app.settings.syntax_highlighting;
                    app.settings.save(app.config_path("settings.txt"));
                });
            add("Toggle Toolbar Labels",
                [&app]
                {
                    app.settings.show_toolbar_labels = !app.settings.show_toolbar_labels;
                    app.settings.save(app.config_path("settings.txt"));
                });
            add("Toggle terminal panel",
                [&app]
                {
                    app.show_terminal_panel = !app.show_terminal_panel;
                    app.terminal.focus_requested = app.show_terminal_panel;
                });
            add("Undo", with_repo(
                                [&app](app::RepoState& r)
                                {
                                    app.undo(r);
                                }));
            add("Unstage all changes", with_repo(
                                               [&app](app::RepoState& r)
                                               {
                                                   app.run(r, "Unstage all",
                                                           [&]
                                                           {
                                                               return r.repo.unstage_all();
                                                           });
                                               }));
            add("View File", pick_file(FileAction::VIEW));
            add("View Working Directory Changes", with_repo(
                                                          [](app::RepoState& r)
                                                          {
                                                              r.select_only(app::WIP_SHA);
                                                          }));
            return entries;
        }

        const char* placeholder_for(Stage stage)
        {
            switch (stage)
            {
            case Stage::CHECKOUT:
                return "Checkout: select a branch";
            case Stage::OPEN_REPO:
                return "Open Repo: select a repository";
            case Stage::STASH_APPLY:
                return "Stash: Apply: select a stash";
            case Stage::STASH_POP:
                return "Stash: Pop: select a stash";
            case Stage::SWITCH_PROFILE:
                return "Search for a profile to switch to";
            case Stage::SWITCH_THEME:
                return "Search for a theme to switch to";
            case Stage::FILE:
                return "Select a file";
            default:
                return "Search for commands and actions (e.g., Open Repo)";
            }
        }
    } // namespace

    void draw_fuzzy_finder(app::Application& app)
    {
        app::FuzzyFinder& finder = app.fuzzy;
        if (!finder.open)
        {
            g_stage = Stage::COMMANDS;
            g_was_open = false;
            return;
        }
        // The click that opened the palette (the toolbar's Actions button) is not a click outside it.
        const bool just_opened = !g_was_open;
        g_was_open = true;
        const ImGuiViewport* viewport = ImGui::GetMainViewport();
        ImGui::SetNextWindowPos(viewport->WorkPos);
        ImGui::SetNextWindowSize(viewport->WorkSize);
        ImGui::SetNextWindowFocus();
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
        ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(0.f, 0.f));
        ImGui::Begin("##fuzzy_finder", nullptr,
                     ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                             | ImGuiWindowFlags_NoBackground | ImGuiWindowFlags_NoNav);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 page = viewport->WorkPos;
        // .modal backdrop
        draw->AddRectFilled(page, ImVec2(page.x + viewport->WorkSize.x, page.y + viewport->WorkSize.y),
                            IM_COL32(0, 0, 0, 128));

        app::RepoState* state = app.active();
        std::vector<Entry> entries = build_entries(app, state);
        std::vector<Entry*> matches;
        for (Entry& entry : entries)
        {
            entry.score = finder.text.empty() ? 0 : fuzzy_score(entry.label, finder.text);
            if (entry.score >= 0)
            {
                matches.push_back(&entry);
            }
        }
        if (!finder.text.empty())
        {
            std::stable_sort(matches.begin(), matches.end(),
                             [](const Entry* a, const Entry* b)
                             {
                                 return a->score > b->score;
                             });
        }
        if (g_stage == Stage::FILE && matches.size() > MAX_FILE_MATCHES)
        {
            matches.resize(MAX_FILE_MATCHES); // type to narrow the list
        }
        finder.selected = matches.empty() ? 0 : std::clamp(finder.selected, 0, static_cast<int>(matches.size()) - 1);

        const float width = px(600.f);
        const ImVec2 min(std::round(page.x + (viewport->WorkSize.x - width) / 2.f), page.y + px(82.f));
        const ImVec2 control_max(min.x + width, min.y + px(38.f));
        const ImVec4 focus = theme::palette().accent;
        // .modal-content shadow, the control (2px border + 1px ring), its ▲ arrow.
        const float list_h = matches.empty() ? 0.f : std::min(px(200.f), px(40.f) * static_cast<float>(matches.size()));
        draw_box_shadow(draw, min, ImVec2(control_max.x, control_max.y + list_h), ImVec2(0.f, px(5.f)), px(15.f), 0.f,
                        ImVec4(0.f, 0.f, 0.f, 0.5f));
        draw->AddRectFilled(ImVec2(min.x - 1.f, min.y - 1.f), ImVec2(control_max.x + 1.f, control_max.y),
                            theme::u32(focus), px(5.f), ImDrawFlags_RoundCornersTop);
        draw->AddRectFilled(
                ImVec2(min.x + px(2.f), min.y + px(2.f)), ImVec2(control_max.x - px(2.f), control_max.y - px(2.f)),
                theme::u32(theme::palette().panel_bg1), px(2.f), ImDrawFlags_RoundCornersTop);
        const ImVec2 arrow(control_max.x - px(22.f), min.y + px(17.f));
        draw->AddTriangleFilled(ImVec2(arrow.x, arrow.y + px(5.f)), ImVec2(arrow.x + px(10.f), arrow.y + px(5.f)),
                                ImVec2(arrow.x + px(5.f), arrow.y), IM_COL32(153, 153, 153, 255));

        // The input: 14px, the text box 7px below the control's inner top (34px line).
        ImFont* font = fonts().regular;
        const float ascent = 14.f * 2189.f / 2048.f * dpi_scale();
        const float baseline = std::round(min.y + px(2.f + (34.f - 19.f) / 2.f) + std::round(ascent));
        ImGui::SetCursorScreenPos(ImVec2(min.x + px(12.f), baseline - std::ceil(ascent)));
        if (finder.focus_requested)
        {
            ImGui::SetKeyboardFocusHere();
            finder.focus_requested = false;
        }
        ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
        ImGui::PushStyleColor(ImGuiCol_Text, theme::ink(0.75f));
        ImGui::PushStyleColor(ImGuiCol_TextDisabled, theme::ink(0.4f));
        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(0.f, 0.f));
        push_font(font, 14.f);
        ImGui::SetNextItemWidth(width - px(50.f));
        const bool text_changed = ImGui::InputTextWithHint("##fuzzy_input", placeholder_for(g_stage), &finder.text);
        text_field_menu(finder.text);
        if (text_changed)
        {
            finder.selected = 0;
        }
        ImGui::PopFont();
        ImGui::PopStyleVar();
        ImGui::PopStyleColor(3);

        if (ImGui::IsKeyPressed(ImGuiKey_DownArrow))
        {
            finder.selected = std::min(finder.selected + 1, std::max(0, static_cast<int>(matches.size()) - 1));
        }
        if (ImGui::IsKeyPressed(ImGuiKey_UpArrow))
        {
            finder.selected = std::max(finder.selected - 1, 0);
        }
        const bool execute = ImGui::IsKeyPressed(ImGuiKey_Enter) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter);
        const ImVec2 list_min(min.x, control_max.y);
        const ImVec2 list_max(control_max.x, control_max.y + list_h);
        if (ImGui::IsKeyPressed(ImGuiKey_Escape)
            || (!just_opened && ImGui::IsMouseClicked(ImGuiMouseButton_Left)
                && !ImGui::IsMouseHoveringRect(ImVec2(min.x - 1.f, min.y - 1.f), list_max)))
        {
            finder.open = false;
        }

        int executed = -1;
        if (!matches.empty())
        {
            // .Select-menu-outer: #272A31, 1px rgba(255,255,255,.08) border, bottom corners 4px.
            draw->AddRectFilled(list_min, list_max, theme::u32(theme::ink(0.08f)), px(4.f),
                                ImDrawFlags_RoundCornersBottom);
            draw->AddRectFilled(ImVec2(list_min.x + 1.f, list_min.y + 1.f), ImVec2(list_max.x - 1.f, list_max.y - 1.f),
                                theme::u32(theme::palette().panel_bg0), px(3.f),
                                ImDrawFlags_RoundCornersBottom);
            ImGui::SetCursorScreenPos(ImVec2(list_min.x + 1.f, list_min.y + 1.f));
            ImGui::PushStyleVar(ImGuiStyleVar_ScrollbarSize, px(8.f));
            ImGui::BeginChild("##fuzzy_list", ImVec2(width - 2.f, list_h - 2.f), ImGuiChildFlags_None,
                              ImGuiWindowFlags_NoBackground);
            ImDrawList* list = ImGui::GetWindowDrawList();
            const float row_w = ImGui::GetContentRegionAvail().x;
            for (int i = 0; i < static_cast<int>(matches.size()); ++i)
            {
                const ImVec2 pos = ImGui::GetCursorScreenPos();
                ImGui::PushID(i);
                ImGui::InvisibleButton("##entry", ImVec2(row_w, px(40.f)));
                const bool hovered = ImGui::IsItemHovered();
                if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
                {
                    executed = i;
                }
                ImGui::PopID();
                if (i == finder.selected)
                {
                    list->AddRectFilled(pos, ImVec2(pos.x + row_w, pos.y + px(39.f)),
                                        theme::u32(theme::palette().selected_row));
                    if (ImGui::IsKeyPressed(ImGuiKey_DownArrow) || ImGui::IsKeyPressed(ImGuiKey_UpArrow))
                    {
                        ImGui::SetScrollHereY(0.5f);
                    }
                }
                else if (hovered)
                {
                    list->AddRectFilled(pos, ImVec2(pos.x + row_w, pos.y + px(39.f)),
                                        theme::u32(theme::palette().hover_row));
                }
                list->AddRectFilled(ImVec2(pos.x, pos.y + px(39.f)), ImVec2(pos.x + row_w, pos.y + px(40.f)),
                                    theme::u32(theme::ink(0.08f)));
                draw_text_ellipsis(list, font, 14.f, ImVec2(pos.x + px(10.f), pos.y + px(8.f)), row_w - px(20.f),
                                   theme::u32(theme::ink(0.75f)), matches[static_cast<std::size_t>(i)]->label.c_str());
            }
            ImGui::EndChild();
            ImGui::PopStyleVar();
        }
        if (execute && !matches.empty())
        {
            executed = finder.selected;
        }
        ImGui::End();
        ImGui::PopStyleVar(3);

        if (executed >= 0 && executed < static_cast<int>(matches.size()))
        {
            const Stage before = g_stage;
            finder.open = false;
            matches[static_cast<std::size_t>(executed)]->action();
            if (g_stage == before)
            {
                g_stage = Stage::COMMANDS;
            }
        }
    }
} // namespace imgit::ui
