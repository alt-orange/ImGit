#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/shell.hpp"
#include "platform/system.hpp"
#include "ui/avatars.hpp"
#include "ui/native_menu.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <imgui.h>
#include <imgui_internal.h>
#include <sstream>
#include <utility>

namespace imgit::app
{
    namespace
    {
        constexpr double STATUS_POLL_SECONDS = 2.0;
        constexpr std::size_t MAX_RECENT = 12;
        constexpr const char* VERSION = "0.2.0";

        // Files that belong to a profile; before profiles existed they sat in the settings folder itself.
        constexpr const char* PROFILE_FILES[] = {"settings.txt", "preferences.txt", "recent.txt", "tabs.txt",
                                                 "favorites.txt"};

        // The settings folder: %APPDATA%\.imgit on Windows, ~/.imgit elsewhere.
        std::filesystem::path config_directory()
        {
            const std::filesystem::path directory = platform::app_data_directory() / ".imgit";
            // where ImGit kept its settings before profiles
            const std::filesystem::path legacy = platform::config_directory() / "imgit";
            std::error_code ec;
            if (!std::filesystem::exists(directory, ec) && std::filesystem::is_directory(legacy, ec))
            {
                std::filesystem::rename(legacy, directory, ec);
            }
            return directory;
        }

        // A float from a settings value; `fallback` for anything that is not a finite number (a hand-edited "nan"
        // would otherwise poison the layout).
        float finite_or(const std::string& value, float fallback)
        {
            const float parsed = static_cast<float>(std::atof(value.c_str()));
            return std::isfinite(parsed) ? parsed : fallback;
        }

        std::string trim(std::string text)
        {
            while (!text.empty() && std::isspace(static_cast<unsigned char>(text.back())))
            {
                text.pop_back();
            }
            std::size_t start = 0;
            while (start < text.size() && std::isspace(static_cast<unsigned char>(text[start])))
            {
                ++start;
            }
            return text.substr(start);
        }

        // The success / error toast titles for the toolbar's LFS commands; nullptr for others.
        const char* lfs_toast(const std::string& title, bool success)
        {
            static constexpr const char* TOASTS[][3] = {
                    {"LFS Fetch", "LFS fetched successfully", "Error on LFS Fetch"},
                    {"LFS Checkout", "LFS checkout successful", "Error on LFS Checkout"},
                    {"LFS Pull", "LFS pulled successfully", "Error on LFS Pull"},
                    {"LFS Push", "LFS pushed successfully", "Error on LFS Push"},
                    {"LFS Prune", "LFS pruned successfully", "Error on LFS Prune"},
            };
            for (const auto& toast : TOASTS)
            {
                if (title == toast[0])
                {
                    return success ? toast[1] : toast[2];
                }
            }
            return nullptr;
        }
    } // namespace

    void Settings::load(const std::string& path)
    {
        std::ifstream file(path);
        std::string line;
        while (std::getline(file, line))
        {
            const auto eq = line.find('=');
            if (eq == std::string::npos)
            {
                continue;
            }
            const std::string key = trim(line.substr(0, eq));
            const std::string value = trim(line.substr(eq + 1));
            const bool truthy = value == "1" || value == "true";
            if (key == "auto_fetch_minutes")
            {
                auto_fetch_minutes = std::clamp(std::atoi(value.c_str()), 0, 60);
            }
            else if (key == "initial_commits")
            {
                initial_commits = std::max(500, std::atoi(value.c_str()));
            }
            else if (key == "show_toolbar_labels")
            {
                show_toolbar_labels = truthy;
            }
            else if (key == "author_initials")
            {
                author_initials = truthy;
            }
            else if (key == "show_author")
            {
                show_author = truthy;
            }
            else if (key == "show_date")
            {
                show_date = truthy;
            }
            else if (key == "show_sha")
            {
                show_sha = truthy;
            }
            else if (key == "show_changes")
            {
                show_changes = truthy;
            }
            else if (key == "show_description")
            {
                show_description = truthy;
            }
            else if (key == "show_ghost_refs")
            {
                show_ghost_refs = truthy;
            }
            else if (key == "highlight_rows_on_ref_hover")
            {
                highlight_rows_on_ref_hover = truthy;
            }
            else if (key == "remember_tabs")
            {
                remember_tabs = truthy;
            }
            else if (key == "push_after_commit")
            {
                push_after_commit = truthy;
            }
            else if (key == "compact_graph")
            {
                compact_graph = truthy;
            }
            else if (key == "zoom")
            {
                zoom = std::clamp(finite_or(value, zoom), 0.5f, 2.f);
            }
            else if (key == "theme")
            {
                theme = value;
            }
            else if (key == "default_branch")
            {
                default_branch = value;
            }
            else if (key == "pull_mode")
            {
                pull_mode = std::clamp(std::atoi(value.c_str()), 0, 3);
            }
            else if (key == "left_panel_width")
            {
                left_panel_width = finite_or(value, left_panel_width);
            }
            else if (key == "detail_panel_width")
            {
                detail_panel_width = finite_or(value, detail_panel_width);
            }
            else if (key == "terminal_height")
            {
                terminal_height = finite_or(value, terminal_height);
            }
            else if (key == "commit_message_height")
            {
                commit_message_height = finite_or(value, commit_message_height);
            }
            else if (key == "graph_column_widths")
            {
                graph_column_widths.clear();
                std::stringstream stream(value);
                std::string column;
                while (std::getline(stream, column, ','))
                {
                    if (const auto colon = column.find(':'); colon != std::string::npos)
                    {
                        graph_column_widths[column.substr(0, colon)] = finite_or(column.substr(colon + 1), 0.f);
                    }
                }
            }
            else if (key == "hidden_left_sections")
            {
                hidden_left_sections.clear();
                std::stringstream stream(value);
                std::string section;
                while (std::getline(stream, section, ','))
                {
                    if (!section.empty())
                    {
                        hidden_left_sections.insert(section);
                    }
                }
            }
        }
    }

    void Settings::save(const std::string& path) const
    {
        std::ostringstream file;
        std::string hidden_sections;
        for (const std::string& section : hidden_left_sections)
        {
            hidden_sections += (hidden_sections.empty() ? "" : ",") + section;
        }
        file << "hidden_left_sections=" << hidden_sections << '\n';
        file << "pull_mode=" << pull_mode << '\n';
        file << "left_panel_width=" << left_panel_width << '\n'
             << "detail_panel_width=" << detail_panel_width << '\n'
             << "terminal_height=" << terminal_height << '\n'
             << "commit_message_height=" << commit_message_height << '\n'
             << "graph_column_widths=";
        // Through the stream, not std::to_string: the latter follows the C locale, and a decimal comma would split
        // the list.
        const char* separator = "";
        for (const auto& [zone, width] : graph_column_widths)
        {
            file << separator << zone << ':' << width;
            separator = ",";
        }
        file << '\n';
        file << "auto_fetch_minutes=" << auto_fetch_minutes << '\n'
             << "initial_commits=" << initial_commits << '\n'
             << "show_toolbar_labels=" << show_toolbar_labels << '\n'
             << "author_initials=" << author_initials << '\n'
             << "show_author=" << show_author << '\n'
             << "show_date=" << show_date << '\n'
             << "show_sha=" << show_sha << '\n'
             << "show_changes=" << show_changes << '\n'
             << "show_description=" << show_description << '\n'
             << "show_ghost_refs=" << show_ghost_refs << '\n'
             << "highlight_rows_on_ref_hover=" << highlight_rows_on_ref_hover << '\n'
             << "remember_tabs=" << remember_tabs << '\n'
             << "push_after_commit=" << push_after_commit << '\n'
             << "compact_graph=" << compact_graph << '\n'
             << "zoom=" << zoom << '\n'
             << "theme=" << theme << '\n'
             << "default_branch=" << default_branch << '\n';
        (void)write_text_file(path, file.str());
    }

    Application::Application()
    {
        std::error_code ec;
        std::filesystem::create_directories(config_directory(), ec);
        profiles.load(config_directory(), git::global_user_name(), git::global_user_email(), PROFILE_FILES);
        apply_profile_identity(false);
        settings.load(config_path("settings.txt"));
        load_recent();
    }

    Application::~Application()
    {
        save_state();
    }

    void Application::save_state() const
    {
        settings.save(config_path("settings.txt"));
        if (settings.remember_tabs)
        {
            save_tabs();
        }
        save_recent();
    }

    std::string Application::config_path(const char* file) const
    {
        return (profiles.directory() / file).string();
    }

    std::string Application::app_path(const char* file) const
    {
        return (config_directory() / file).string();
    }

    void Application::switch_profile(const std::string& guid)
    {
        if (guid != profiles.current().guid && profiles.find(guid) != nullptr)
        {
            m_pending_profile = guid;
        }
    }

    void Application::apply_profile_switch()
    {
        if (m_pending_profile.empty())
        {
            return;
        }
        const std::string guid = std::exchange(m_pending_profile, {});
        // The profile being left keeps its settings and tabs.
        settings.save(config_path("settings.txt"));
        if (settings.remember_tabs)
        {
            save_tabs();
        }
        save_recent();
        tabs.clear();
        closed_tabs.clear();
        recent_repos.clear();
        active_tab = -1;
        new_tab_open = false;
        repo_management_open = false;
        dialog.close();

        profiles.set_current(guid);
        apply_profile_identity(profiles.keep_git_config());
        settings = Settings{};
        settings.load(config_path("settings.txt"));
        load_recent();
        apply_settings();
        if (settings.remember_tabs)
        {
            restore_tabs();
        }
    }

    void Application::apply_profile_identity(bool write_git_config)
    {
        const Profile& profile = profiles.current();
        git::set_profile_identity(profile.user_name, profile.user_email);
        if (write_git_config)
        {
            if (auto written = git::set_global_identity(profile.user_name, profile.user_email); !written)
            {
                toasts.push(ToastKind::ERROR, "Could not update .gitconfig", written.error().message);
            }
        }
    }

    void Application::add_profile(const Profile& values, bool copy_current)
    {
        if (copy_current)
        {
            // The copy starts from what is open now.
            settings.save(config_path("settings.txt"));
            if (settings.remember_tabs)
            {
                save_tabs();
            }
            save_recent();
        }
        switch_profile(profiles.create(values, copy_current));
    }

    void Application::edit_profile(const Profile& profile)
    {
        profiles.update(profile);
        if (profile.guid == profiles.current().guid)
        {
            apply_profile_identity(profiles.keep_git_config());
            for (auto& tab : tabs)
            {
                tab->refresh();
            }
        }
    }

    void Application::delete_profile(const std::string& guid)
    {
        profiles.remove(guid);
    }

    void Application::set_keep_git_config(bool keep)
    {
        profiles.set_keep_git_config(keep);
    }

    RepoState* Application::active() noexcept
    {
        if (active_tab < 0 || active_tab >= static_cast<int>(tabs.size()))
        {
            return nullptr;
        }
        return tabs[static_cast<std::size_t>(active_tab)].get();
    }

    void Application::open_repository(const std::string& raw_path)
    {
        repo_management_open = false;
        new_tab_open = false;
        const std::string path = trim(raw_path);
        if (path.empty())
        {
            return;
        }
        auto repo = git::Repository::open(path);
        if (!repo)
        {
            toasts.push(ToastKind::ERROR, "Could not open repository", repo.error().message);
            return;
        }
        const std::string workdir = repo->workdir();
        for (std::size_t i = 0; i < tabs.size(); ++i)
        {
            if (tabs[i]->repo.workdir() == workdir)
            {
                active_tab = static_cast<int>(i);
                return;
            }
        }
        auto state = std::make_unique<RepoState>(std::move(*repo));
        state->commit_limit = static_cast<std::size_t>(settings.initial_commits);
        state->push_after_commit = settings.push_after_commit;
        tabs.push_back(std::move(state));
        active_tab = static_cast<int>(tabs.size()) - 1;

        std::erase(recent_repos, workdir);
        recent_repos.insert(recent_repos.begin(), workdir);
        if (recent_repos.size() > MAX_RECENT)
        {
            recent_repos.resize(MAX_RECENT);
        }
        save_recent();
    }

    void Application::browse_for_repository()
    {
        const auto picked = platform::shell::pick_folder_dialog(recent_repos.empty() ? "" : recent_repos.front());
        if (!picked)
        {
            return;
        }
        if (!git::Repository::open(*picked))
        {
            std::string name = *picked;
            while (!name.empty() && (name.back() == '/' || name.back() == '\\'))
            {
                name.pop_back();
            }
            name = name.substr(name.find_last_of("/\\") + 1);
            toasts.push(ToastKind::ERROR, "Open Failed", "Could not find a compatible repository at '" + name + "'.");
            return;
        }
        open_repository(*picked);
    }

    void Application::close_tab(int index)
    {
        if (index < 0 || index >= static_cast<int>(tabs.size()))
        {
            return;
        }
        const std::string workdir = tabs[static_cast<std::size_t>(index)]->repo.workdir();
        closed_tabs.push_back(workdir);
        // A dialog opened for this repository (its on_confirm may hold the tab) goes with it.
        if (dialog.kind != Dialog::Kind::NONE && dialog.workdir == workdir)
        {
            dialog.close();
        }
        tabs.erase(tabs.begin() + index);
        // New Tab (-1) stays where it is; a repository tab right of the closed one keeps
        // showing the same repository.
        if (active_tab < 0)
        {
            return;
        }
        if (tabs.empty())
        {
            active_tab = -1;
        }
        else if (active_tab > index || active_tab >= static_cast<int>(tabs.size()))
        {
            active_tab = std::max(active_tab - 1, 0);
        }
    }

    void Application::reopen_closed_tab()
    {
        if (closed_tabs.empty())
        {
            return;
        }
        const std::string path = closed_tabs.back();
        closed_tabs.pop_back();
        open_repository(path);
    }

    void Application::select_adjacent_tab(int direction)
    {
        if (tabs.empty())
        {
            return;
        }
        const int count = static_cast<int>(tabs.size());
        active_tab = ((active_tab < 0 ? 0 : active_tab) + direction + count) % count;
    }

    void Application::copy_to_clipboard(const std::string& text)
    {
        ImGui::SetClipboardText(text.c_str());
        toasts.push(ToastKind::INFO, "Copied to clipboard", text.size() > 80 ? text.substr(0, 77) + "..." : text);
    }

    void Application::set_zoom(float zoom)
    {
        settings.zoom = std::clamp(zoom, 0.5f, 2.f);
        apply_settings();
    }

    void Application::apply_settings()
    {
        const float scale = base_dpi_scale * settings.zoom;
        ui::set_dpi_scale(scale);
        theme::set_theme(settings.theme);
        theme::apply_imgui_style();
        ImGuiStyle& style = ImGui::GetStyle();
        style.ScaleAllSizes(scale);
        style.FontScaleDpi = base_dpi_scale;
        style.FontScaleMain = settings.zoom;
        for (auto& tab : tabs)
        {
            tab->commit_limit = static_cast<std::size_t>(settings.initial_commits);
        }
        settings.save(config_path("settings.txt"));
    }

    namespace
    {
        // The session's credentials with `attempt` in place of its host's entry.
        std::vector<git::cli::Credential> credentials_with(std::vector<git::cli::Credential> credentials,
                                                           const std::optional<git::cli::Credential>& attempt)
        {
            if (attempt)
            {
                std::erase_if(credentials,
                              [&](const git::cli::Credential& credential)
                              {
                                  return credential.url == attempt->url;
                              });
                credentials.insert(credentials.begin(), *attempt);
            }
            return credentials;
        }
    } // namespace

    void Application::run_git_cli(RepoState& state, std::string title, std::vector<std::string> args, bool silent,
                                  std::optional<git::cli::Credential> attempt)
    {
        if (state.async)
        {
            if (!silent)
            {
                toasts.push(ToastKind::INFO, "Please wait", state.async->title + " is still running");
            }
            return;
        }
        const std::string workdir = state.repo.workdir();
        std::vector<git::cli::Credential> credentials_for_run = credentials_with(credentials, attempt);
        state.async = AsyncOperation{std::move(title),
                                     std::async(std::launch::async,
                                                [workdir, args, credentials_for_run = std::move(credentials_for_run)]
                                                {
                                                    return git::cli::run(workdir, args, {}, credentials_for_run);
                                                }),
                                     silent, std::move(args), std::move(attempt)};
    }

    void Application::run_git_cli_sequence(RepoState& state, std::string title,
                                           std::vector<std::vector<std::string>> runs)
    {
        if (runs.empty())
        {
            return;
        }
        if (state.async)
        {
            toasts.push(ToastKind::INFO, "Please wait", state.async->title + " is still running");
            return;
        }
        std::vector<std::string> first = std::move(runs.front());
        runs.erase(runs.begin());
        run_git_cli(state, std::move(title), std::move(first));
        if (state.async)
        {
            state.async->queued = std::move(runs);
        }
    }

    void Application::start_clone(std::string parent, std::string url, std::string destination,
                                  std::optional<git::cli::Credential> attempt)
    {
        std::vector<git::cli::Credential> credentials_for_run = credentials_with(credentials, attempt);
        std::future<git::cli::Output> future =
                std::async(std::launch::async,
                           [parent, url, destination, credentials_for_run = std::move(credentials_for_run)]
                           {
                               return git::cli::run(parent, {"clone", "--progress", url, destination}, {},
                                                    credentials_for_run);
                           });
        pending_clone = PendingClone{std::move(future), std::move(destination), std::move(parent), std::move(url),
                                     std::move(attempt)};
    }

    bool Application::prompt_for_credentials(const std::string& output,
                                             const std::optional<git::cli::Credential>& attempt,
                                             std::function<void(git::cli::Credential)> retry)
    {
        const std::optional<git::cli::AuthRequest> request = git::cli::auth_request(output);
        if (!request || credential_prompt)
        {
            return false;
        }
        const bool retried = attempt && attempt->url == request->url;
        CredentialPrompt prompt;
        prompt.url = request->url;
        prompt.username = retried ? attempt->username : request->username;
        prompt.remember = retried && attempt->remember;
        prompt.rejected = request->rejected || retried;
        prompt.retry = std::move(retry);
        credential_prompt = std::move(prompt);
        return true;
    }

    void Application::keep_credential(const git::cli::Credential& credential)
    {
        std::erase_if(credentials,
                      [&](const git::cli::Credential& other)
                      {
                          return other.url == credential.url;
                      });
        credentials.push_back(credential);
    }

    TerminalPanel::Tabs& Application::terminal_tabs(const RepoState* state)
    {
        const std::string workdir = state != nullptr ? state->repo.workdir() : std::filesystem::current_path().string();
        return terminal.repos[workdir];
    }

    TerminalSession* Application::active_terminal(const RepoState* state)
    {
        TerminalPanel::Tabs& tabs = terminal_tabs(state);
        if (tabs.sessions.empty())
        {
            return nullptr;
        }
        tabs.active = std::clamp(tabs.active, 0, static_cast<int>(tabs.sessions.size()) - 1);
        return tabs.sessions[static_cast<std::size_t>(tabs.active)].get();
    }

    void Application::new_terminal_tab(const RepoState* state)
    {
        TerminalPanel::Tabs& tabs = terminal_tabs(state);
        auto session = std::make_unique<TerminalSession>();
        session->workdir = state != nullptr ? state->repo.workdir() : std::filesystem::current_path().string();
        tabs.sessions.push_back(std::move(session));
        tabs.active = static_cast<int>(tabs.sessions.size()) - 1;
        show_terminal_panel = true;
        terminal.minimized = false;
        terminal.focus_requested = true;
    }

    void Application::close_terminal_tab(const RepoState* state, int index)
    {
        TerminalPanel::Tabs& tabs = terminal_tabs(state);
        if (index < 0 || index >= static_cast<int>(tabs.sessions.size()))
        {
            return;
        }
        // The last tab hides the panel (cliPanelViewMode "hidden").
        if (tabs.sessions.size() == 1)
        {
            show_terminal_panel = false;
        }
        tabs.sessions.erase(tabs.sessions.begin() + index);
        if (tabs.active >= index && tabs.active > 0)
        {
            --tabs.active;
        }
    }

    void Application::kill_terminals(RepoState* state)
    {
        // By working directory: the confirmation outlives this frame, and the tab may be gone by then.
        const auto kill = [this, workdir = state != nullptr ? state->repo.workdir()
                                                            : std::filesystem::current_path().string()]
        {
            terminal.repos[workdir].sessions.clear();
            show_terminal_panel = false;
        };
        const std::size_t count = terminal_tabs(state).sessions.size();
        if (count < 2)
        {
            kill();
            return;
        }
        dialog.open(Dialog::Kind::CONFIRM);
        dialog.message = "Kill " + std::to_string(count) + " terminals? Any running processes will be stopped.";
        dialog.confirm_label = "Kill All Terminals";
        dialog.danger = true;
        dialog.on_confirm = kill;
    }

    void Application::run_terminal_command(RepoState* state, const std::string& raw_line)
    {
        const std::string line = trim(raw_line);
        if (line.empty())
        {
            return;
        }
        if (active_terminal(state) == nullptr)
        {
            new_terminal_tab(state);
        }
        show_terminal_panel = true;
        terminal.minimized = false;
        TerminalSession* session = active_terminal(state);
        session->scroll = 0;
        if (session->started)
        {
            session->console.write(line + "\r");
        }
        else
        {
            // Before the shell has started the keystrokes wait in the console's input pipe.
            session->pending_input += line + "\r";
        }
    }

    void Application::undo(RepoState& state)
    {
        if (state.undo_stack.empty())
        {
            return;
        }
        UndoEntry entry = std::move(state.undo_stack.back());
        state.undo_stack.pop_back();
        const git::Sha before_sha = state.history.head_sha;
        const std::string before_branch = state.history.head_branch;
        auto result = entry.undo();
        if (!result)
        {
            toasts.push(ToastKind::ERROR, "Undo Failed", result.error().message);
            state.refresh();
            return;
        }
        toasts.push(ToastKind::SUCCESS, "Undo Successful", entry.label);
        state.refresh();
        if (!entry.redo && !before_branch.empty() && state.history.head_branch != before_branch)
        {
            // Undoing a checkout: redo checks the branch out again.
            git::Repository* repo = &state.repo;
            entry.redo = [repo, before_branch]
            {
                return repo->checkout_branch(before_branch);
            };
        }
        else if (!entry.redo && !before_sha.empty() && state.history.head_sha != before_sha)
        {
            // Undoing a commit / merge / cherry pick...: redo moves the branch back to the commit it left (which
            // still exists), keeping uncommitted changes aside like undo does.
            git::Repository* repo = &state.repo;
            const git::ResetMode mode = entry.redo_mode;
            entry.redo = [repo, before_sha, mode]() -> git::Result<void>
            {
                bool stashed = false;
                if (mode == git::ResetMode::HARD && repo->load_status().value_or(git::WorkDirStatus{}).count() > 0)
                {
                    stashed = repo->stash_push("ImGit: auto stash before redo").has_value();
                }
                auto reset = repo->reset(before_sha, mode);
                if (stashed)
                {
                    (void)repo->stash_pop(0);
                }
                return reset;
            };
        }
        if (entry.redo)
        {
            state.redo_stack.push_back(std::move(entry));
        }
    }

    void Application::redo(RepoState& state)
    {
        if (state.redo_stack.empty())
        {
            return;
        }
        UndoEntry entry = std::move(state.redo_stack.back());
        state.redo_stack.pop_back();
        auto result = entry.redo();
        if (!result)
        {
            toasts.push(ToastKind::ERROR, "Redo Failed", result.error().message);
            state.refresh();
            return;
        }
        toasts.push(ToastKind::SUCCESS, "Redo Successful", entry.label);
        state.undo_stack.push_back(std::move(entry));
        state.refresh();
    }

    void Application::poll_async()
    {
        for (auto& tab : tabs)
        {
            if (!tab->async || tab->async->future.wait_for(std::chrono::seconds(0)) != std::future_status::ready)
            {
                continue;
            }
            const git::cli::Output output = tab->async->future.get();
            const std::string title = tab->async->title;
            const bool silent = tab->async->silent;
            std::vector<std::string> args = std::move(tab->async->args);
            const std::optional<git::cli::Credential> attempt = std::move(tab->async->attempt);
            std::vector<std::vector<std::string>> queued = std::move(tab->async->queued);
            tab->async.reset();
            if (output.exit_code == 0 && attempt)
            {
                keep_credential(*attempt);
            }
            // The next command of a sequence; the toast waits for the last one.
            if (output.exit_code == 0 && !queued.empty())
            {
                std::vector<std::string> next = std::move(queued.front());
                queued.erase(queued.begin());
                run_git_cli(*tab, title, std::move(next), silent, attempt);
                if (tab->async)
                {
                    tab->async->queued = std::move(queued);
                }
                tab->refresh();
                continue;
            }
            // Refused for want of credentials: ImGit asks for them and runs the command again.
            if (output.exit_code != 0 && !silent
                && prompt_for_credentials(output.text, attempt,
                                          [this, workdir = tab->repo.workdir(), title,
                                           args = std::move(args)](git::cli::Credential credential)
                                          {
                                              for (auto& other : tabs)
                                              {
                                                  if (other->repo.workdir() == workdir)
                                                  {
                                                      run_git_cli(*other, title, args, false, std::move(credential));
                                                      break;
                                                  }
                                              }
                                          }))
            {
                continue;
            }
            // git's progress lines are separated by '\r'; keep the last meaningful line of each.
            std::string cleaned;
            for (const char c : output.text)
            {
                cleaned += c == '\r' ? '\n' : c;
            }
            std::string text;
            {
                std::istringstream lines(cleaned);
                std::string line;
                while (std::getline(lines, line))
                {
                    line = trim(line);
                    if (!line.empty())
                    {
                        text += (text.empty() ? "" : "\n") + line;
                    }
                }
            }
            if (output.exit_code == 0)
            {
                if (silent)
                {
                }
                else if (title == "Pull")
                {
                    // "Already Up-to-Date" / "Pulled Successfully".
                    if (text.find("Already up to date") != std::string::npos)
                    {
                        std::string upstream;
                        for (const git::Ref& ref : tab->history.refs)
                        {
                            if (ref.is_head)
                            {
                                upstream = ref.upstream;
                            }
                        }
                        toasts.push(ToastKind::INFO, "Already Up-to-Date",
                                    upstream.empty() ? "No merge necessary."
                                                     : "No merge necessary. Branch " + upstream + " was not moved.");
                    }
                    else
                    {
                        toasts.push(ToastKind::SUCCESS, "Pulled Successfully");
                    }
                }
                else if (title == "Push")
                {
                    toasts.push(ToastKind::SUCCESS, "Pushed Successfully");
                }
                else if (const char* lfs = lfs_toast(title, true); lfs != nullptr)
                {
                    toasts.push(ToastKind::SUCCESS, lfs);
                }
                else if (title != "Fetch")
                {
                    toasts.push(ToastKind::SUCCESS, title + " complete",
                                text.size() > 200 ? text.substr(0, 197) + "..." : text);
                }
            }
            else if (!silent)
            {
                const char* lfs = lfs_toast(title, false);
                toasts.push(ToastKind::ERROR, lfs != nullptr ? std::string(lfs) : title + " failed", text);
            }
            tab->refresh();
        }
        if (pending_clone && pending_clone->future.wait_for(std::chrono::seconds(0)) == std::future_status::ready)
        {
            const git::cli::Output output = pending_clone->future.get();
            PendingClone clone = std::move(*pending_clone);
            const std::string destination = clone.destination;
            pending_clone.reset();
            if (output.exit_code == 0)
            {
                if (clone.attempt)
                {
                    keep_credential(*clone.attempt);
                }
                toasts.push(ToastKind::SUCCESS, "Clone complete", destination);
                open_repository(destination);
            }
            else if (!prompt_for_credentials(output.text, clone.attempt,
                                             [this, parent = std::move(clone.parent), url = std::move(clone.url),
                                              destination](git::cli::Credential credential)
                                             {
                                                 start_clone(parent, url, destination, std::move(credential));
                                             }))
            {
                toasts.push(ToastKind::ERROR, "Clone failed", trim(output.text));
            }
        }
        // The shells: their output into the screens, the replies back; a shell that exits closes its tab.
        for (auto& [workdir, tabs] : terminal.repos)
        {
            for (std::size_t i = 0; i < tabs.sessions.size();)
            {
                TerminalSession& session = *tabs.sessions[i];
                if (session.started)
                {
                    if (const std::string output = session.console.read(); !output.empty())
                    {
                        session.screen.feed(output);
                        session.console.write(session.screen.take_replies());
                    }
                    if (session.console.exited())
                    {
                        tabs.sessions.erase(tabs.sessions.begin() + static_cast<std::ptrdiff_t>(i));
                        tabs.active =
                                std::clamp(tabs.active, 0, std::max(0, static_cast<int>(tabs.sessions.size()) - 1));
                        if (tabs.sessions.empty() && active() != nullptr && active()->repo.workdir() == workdir)
                        {
                            show_terminal_panel = false;
                        }
                        continue;
                    }
                }
                ++i;
            }
        }
    }

    void Application::auto_fetch(RepoState& state)
    {
        if (settings.auto_fetch_minutes <= 0 || state.history.remotes.empty() || state.busy())
        {
            return;
        }
        const double now = ImGui::GetTime();
        if (state.last_auto_fetch == 0.0)
        {
            state.last_auto_fetch = now; // first fetch one interval after opening
            return;
        }
        if (now - state.last_auto_fetch >= settings.auto_fetch_minutes * 60.0)
        {
            state.last_auto_fetch = now;
            run_git_cli(state, "Auto-fetch", {"fetch", "--all", "--prune"}, true);
        }
    }

    void Application::handle_shortcuts(RepoState* state)
    {
        const ImGuiIO& io = ImGui::GetIO();
        if (dialog.kind != Dialog::Kind::NONE || ui::menu_open())
        {
            return;
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_P)
            || ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_P))
        {
            fuzzy.open = !fuzzy.open;
            fuzzy.text.clear();
            fuzzy.selected = 0;
            fuzzy.focus_requested = fuzzy.open;
        }
        if (fuzzy.open)
        {
            return;
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Equal)
            || ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_KeypadAdd))
        {
            set_zoom(settings.zoom + 0.1f);
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Minus)
            || ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_KeypadSubtract))
        {
            set_zoom(settings.zoom - 0.1f);
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_0))
        {
            set_zoom(1.f);
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_GraveAccent))
        {
            show_terminal_panel = !show_terminal_panel;
            terminal.minimized = false;
            terminal.focus_requested = show_terminal_panel;
        }
        // Terminal tabs: new, close, next / previous, by number.
        if (state != nullptr && show_terminal_panel)
        {
            TerminalPanel::Tabs& terminal_tab_list = terminal_tabs(state);
            const int count = static_cast<int>(terminal_tab_list.sessions.size());
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_N))
            {
                new_terminal_tab(state);
            }
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_W))
            {
                close_terminal_tab(state, terminal_tab_list.active);
            }
            if (count > 0 && ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_PageDown))
            {
                terminal_tab_list.active = (terminal_tab_list.active + 1) % count;
            }
            if (count > 0 && ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_PageUp))
            {
                terminal_tab_list.active = (terminal_tab_list.active - 1 + count) % count;
            }
            for (int i = 0; i < 9 && i < count; ++i)
            {
                if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | (ImGuiKey_1 + i)))
                {
                    terminal_tab_list.active = i;
                }
            }
        }
        // The File menu accelerators work from text fields too.
        if (!(show_terminal_panel && terminal.focused))
        {
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_O))
            {
                browse_for_repository();
            }
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Alt | ImGuiKey_O))
            {
                repo_management_open = true;
            }
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_T))
            {
                active_tab = -1;
            }
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_T))
            {
                reopen_closed_tab();
            }
            if (state != nullptr && ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_W))
            {
                close_tab(active_tab);
                return;
            }
        }
        // While a text field or the terminal has the keyboard, keys are typing.
        if (io.WantTextInput || (show_terminal_panel && terminal.focused))
        {
            return;
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Slash))
        {
            dialog.open(Dialog::Kind::KEYBOARD_SHORTCUTS);
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_J))
        {
            show_left_panel = !show_left_panel;
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_K))
        {
            show_detail_panel = !show_detail_panel;
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Tab) && !tabs.empty())
        {
            active_tab = (active_tab + 1) % static_cast<int>(tabs.size());
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_Tab) && !tabs.empty())
        {
            active_tab = (active_tab - 1 + static_cast<int>(tabs.size())) % static_cast<int>(tabs.size());
        }
        for (int i = 0; i < 9 && i < static_cast<int>(tabs.size()); ++i)
        {
            if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | (ImGuiKey_1 + i)))
            {
                active_tab = i;
            }
        }
        if (state == nullptr)
        {
            return;
        }
        if (ImGui::IsKeyPressed(ImGuiKey_F5))
        {
            state->refresh();
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_L))
        {
            run_git_cli(*state, "Fetch", {"fetch", "--all", "--prune"});
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_F))
        {
            state->search_open = true;
            state->search_focus_requested = true;
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Z))
        {
            undo(*state);
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_Y)
            || ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_Z))
        {
            redo(*state);
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_S))
        {
            run(*state, "Stage all",
                [&]
                {
                    return state->repo.stage_all();
                });
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_U))
        {
            run(*state, "Unstage all",
                [&]
                {
                    return state->repo.unstage_all();
                });
        }
        if (ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiKey_B))
        {
            const git::Commit* commit = state->single_selected();
            dialog.open(Dialog::Kind::CREATE_BRANCH);
            dialog.flag = true;
            dialog.sha = commit != nullptr && commit->sha != WIP_SHA ? commit->sha : state->history.head_sha;
        }
        if (ImGui::IsKeyPressed(ImGuiKey_Escape))
        {
            if (state->search_open)
            {
                state->search_open = false;
                state->run_search();
            }
            else if (state->merge_tool && !ImGui::GetIO().WantTextInput)
            {
                state->merge_tool.reset();
            }
            else if (state->diff_target)
            {
                state->close_diff();
            }
            else if (state->rebase.active && state->rebase.editing >= 0)
            {
                state->rebase.editing = -1; // closes the reword editor only
            }
            else if (state->rebase.active)
            {
                state->rebase = {};
            }
        }
    }

    void Application::draw_frame()
    {
        apply_profile_switch();
        poll_async();

        // Shortcuts and the tabs bar can close the active tab: the state is looked up again after each.
        handle_shortcuts(active());
        RepoState* state = active();
        draw_menu_bar(state);

        const ImGuiViewport* viewport = ImGui::GetMainViewport();
        ImGui::SetNextWindowPos(viewport->WorkPos);
        ImGui::SetNextWindowSize(viewport->WorkSize);
        ImGui::Begin("##root", nullptr,
                     ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoBringToFrontOnFocus
                             | ImGuiWindowFlags_NoNavFocus | ImGuiWindowFlags_NoScrollWithMouse);

        // The chrome bands abut: place each one explicitly instead of letting ImGui add item spacing.
        const ImVec2 root = ImGui::GetCursorScreenPos();
        const float root_bottom = viewport->WorkPos.y + viewport->WorkSize.y;
        ui::draw_tabs_bar(*this);
        state = active();
        const float tabs_bottom = root.y + ui::px(theme::TABS_BAR_HEIGHT);
        ImGui::SetCursorScreenPos(ImVec2(root.x, tabs_bottom));
        const float status_height = ui::px(theme::INFO_BAR_HEIGHT);
        const ImVec2 body(viewport->WorkSize.x, root_bottom - tabs_bottom - status_height);
        if (preferences_open)
        {
            // The Preferences view covers everything under the tabs bar, the info bar included.
            ui::draw_preferences(*this, ImVec2(viewport->WorkSize.x, root_bottom - tabs_bottom));
            ImGui::End();
            ui::draw_dialogs(*this);
            ui::draw_fuzzy_finder(*this);
            toasts.draw();
            ui::draw_menus();
            return;
        }
        if (repo_management_open)
        {
            ui::draw_repo_management(*this, body);
        }
        else if (state == nullptr)
        {
            ui::draw_new_tab_view(*this, body);
        }
        else
        {
            if (!state->busy())
            {
                state->poll_status(ImGui::GetTime(), STATUS_POLL_SECONDS);
            }
            auto_fetch(*state);
            float toolbar_h = 0.f;
            if (show_toolbar)
            {
                ui::draw_toolbar(*this, *state);
                toolbar_h = ui::px(theme::TOOLBAR_HEIGHT);
                ImGui::SetCursorScreenPos(ImVec2(root.x, tabs_bottom + toolbar_h));
            }
            draw_repo_tab(*state, ImVec2(body.x, body.y - toolbar_h));
        }
        ImGui::SetCursorScreenPos(ImVec2(root.x, root_bottom - status_height));
        draw_status_bar();
        ImGui::End();

        ui::draw_dialogs(*this);
        ui::draw_fuzzy_finder(*this);
        toasts.draw();
        ui::draw_menus();
    }

    void Application::draw_repo_tab(RepoState& state, ImVec2 size)
    {
        // The merge tool takes the whole tab.
        if (state.merge_tool)
        {
            ui::draw_merge_view(*this, state, size);
            return;
        }
        const float handle = ui::px(7.f); // --handle-width
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        const float window_width = size.x / ui::dpi_scale(); // window.innerWidth, CSS px

        // Panels open and close at once (no transitions).
        const bool left_open = show_left_panel && (!state.diff_target || state.left_open_in_diff);
        // A pending interactive rebase covers the left panel and the graph.
        const bool rebasing = state.rebase.active && !state.diff_target;
        // The file view covers the terminal too.
        const bool terminal_open = show_terminal_panel && !state.diff_target;
        const float collapsed_strip = ui::px(36.f); // LEFT_PANEL_COLLAPSED_WIDTH

        // The terminal sits under the graph only; the side panels run the full height.
        const float min_terminal = ui::px(80.f);
        const float max_terminal = std::max(min_terminal, size.y - ui::px(120.f));
        float terminal_h = 0.f;
        if (terminal_open)
        {
            terminal_h = terminal.minimized ? ui::px(30.f)
                                            : std::clamp(ui::px(settings.terminal_height), min_terminal, max_terminal);
        }
        const float upper_h = size.y - terminal_h;
        const float full_h = size.y;

        // The limits: the left panel's resizable is 215..800 wide with its 7px handle margin (the panel
        // shows 7px less); the detail panel's 353..innerWidth - 687.
        const float min_left = ui::px(theme::LEFT_PANEL_DEFAULT_WIDTH);
        const float max_left = ui::px(800.f - 7.f);
        const float min_detail = ui::px(353.f);
        const float max_detail = std::max(min_detail, ui::px(window_width - 687.f));
        float left = rebasing    ? 0.f
                     : left_open ? std::clamp(ui::px(settings.left_panel_width), min_left, max_left)
                                 : collapsed_strip;
        float detail =
                show_detail_panel ? std::clamp(ui::px(settings.detail_panel_width), min_detail, max_detail) : 0.f;
        // Nothing else keeps the panels apart; on a narrow window the detail panel gives way.
        detail = std::min(detail, std::max(0.f, size.x - left));
        const float center_x = origin.x + left;
        const float center_width = size.x - left - detail;

        // The panels abut; their borders are drawn by the panels, the resize handles overlay them.
        ImGui::SetCursorScreenPos(origin);
        if (!rebasing)
        {
            ui::draw_left_panel(*this, state, ImVec2(left, full_h), left_open);
        }
        ImGui::SetCursorScreenPos(ImVec2(center_x, origin.y));
        if (state.diff_target)
        {
            ui::draw_diff_view(*this, state, ImVec2(center_width, upper_h));
        }
        else if (rebasing)
        {
            ui::draw_rebase_panel(*this, state, ImVec2(center_width, upper_h));
        }
        else
        {
            ui::draw_graph_view(*this, state, ImVec2(center_width, upper_h));
        }
        if (detail > 0.f)
        {
            ImGui::SetCursorScreenPos(ImVec2(center_x + center_width, origin.y));
            ui::draw_detail_panel(*this, state, ImVec2(detail, full_h));
        }
        if (terminal_h > 0.f)
        {
            ImGui::SetCursorScreenPos(ImVec2(center_x, origin.y + upper_h));
            ui::draw_terminal_panel(*this, &state, ImVec2(center_width, terminal_h));
        }

        // Sizes follow the mouse while a handle is held and are saved when it is let go.
        bool resized = false;
        const float shown_left = left;
        const float shown_detail = detail;
        const float shown_terminal = terminal_h;
        if (left_open && !rebasing)
        {
            // .ref-panel-container's handle: 7px wide, 3px in from the resizable's right edge.
            const ImVec2 min(center_x - ui::px(3.f), origin.y);
            resized |= ui::overlay_resize_drag("##left_resize", min, ImVec2(min.x + handle, origin.y + full_h), true,
                                               1.f, &left, min_left, max_left);
            if (left != shown_left)
            {
                settings.left_panel_width = left / ui::dpi_scale();
            }
        }
        if (detail > 0.f)
        {
            // .detail-panel's handle: 10px wide on its left edge.
            const ImVec2 min(center_x + center_width, origin.y);
            resized |= ui::overlay_resize_drag("##detail_resize", min, ImVec2(min.x + ui::px(10.f), origin.y + full_h),
                                               true, -1.f, &detail, min_detail, max_detail);
            if (detail != shown_detail)
            {
                settings.detail_panel_width = detail / ui::dpi_scale();
            }
        }
        if (terminal_h > 0.f && !terminal.minimized)
        {
            const ImVec2 min(center_x, origin.y + upper_h - handle / 2.f);
            resized |= ui::overlay_resize_drag("##terminal_resize", min, ImVec2(min.x + center_width, min.y + handle),
                                               false, -1.f, &terminal_h, min_terminal, max_terminal);
            if (terminal_h != shown_terminal)
            {
                settings.terminal_height = terminal_h / ui::dpi_scale();
            }
        }
        if (resized)
        {
            settings.save(config_path("settings.txt"));
        }
    }

    ui::MenuItems Application::bar_menu(int index)
    {
        // The application menu, with its labels and accelerators. The items outlive this frame (the menu stays open),
        // so they look the active tab up when clicked instead of keeping a pointer to it.
        const auto item =
                [](const char* label, const char* accelerator, std::function<void()> click, bool enabled = true)
        {
            ui::MenuItem result = ui::menu_item(label, std::move(click), enabled);
            result.accelerator = accelerator;
            return result;
        };
        const auto checkbox = [&](const char* label, const char* accelerator, std::function<void()> click)
        {
            // Left unchecked whatever the panel state.
            ui::MenuItem result = item(label, accelerator, std::move(click));
            result.type = ui::MenuItem::Type::CHECKBOX;
            return result;
        };
        const auto placeholder = [this](const char* what) -> std::function<void()>
        {
            return [this, what]
            {
                toasts.push(ToastKind::INFO, what,
                            "Not available in ImGit yet.");
            };
        };
        const auto send_chord = [](ImGuiKey key) -> std::function<void()>
        {
            return [key]
            {
                ImGuiIO& io = ImGui::GetIO();
                io.AddKeyEvent(ImGuiMod_Ctrl, true);
                io.AddKeyEvent(key, true);
                io.AddKeyEvent(key, false);
                io.AddKeyEvent(ImGuiMod_Ctrl, false);
            };
        };
        switch (index)
        {
        case 0:
            return ui::separated_menu({
                    {item("New &Tab", "Ctrl+T",
                          [this]
                          {
                              active_tab = -1;
                          }),
                     item("Close Tab", "Ctrl+W",
                          [this]
                          {
                              close_tab(active_tab);
                          }),
                     item("Reopen Closed &Tab", "Ctrl+Shift+T",
                          [this]
                          {
                              reopen_closed_tab();
                          })},
                    {item("Clo&ne Repo...", "Ctrl+N",
                          [this]
                          {
                              dialog.open(Dialog::Kind::CLONE_REPO);
                          }),
                     item("&Init Repo...", "Ctrl+I",
                          [this]
                          {
                              dialog.open(Dialog::Kind::INIT_REPO);
                          }),
                     item("&Open Repo...", "Ctrl+O",
                          [this]
                          {
                              browse_for_repository();
                          }),
                     item("&Open Repo Management", "Alt+Ctrl+O",
                          [this]
                          {
                              repo_management_open = true;
                          }),
                     item("Open Repo in External &Editor", "Ctrl+Shift+E",
                          [this]
                          {
                              if (const RepoState* state = active(); state != nullptr)
                              {
                                  platform::shell::open_in_editor(state->repo.workdir());
                              }
                          })},
                    {item("Open External &Terminal", "Alt+T",
                          [this]
                          {
                              if (const RepoState* state = active(); state != nullptr)
                              {
                                  platform::shell::open_external_terminal(state->repo.workdir());
                              }
                          }),
                     item("Open in File Manager", "Alt+O",
                          [this]
                          {
                              if (const RepoState* state = active(); state != nullptr)
                              {
                                  platform::shell::open_in_file_manager(state->repo.workdir());
                              }
                          })},
                    {item("&Preferences...", "Ctrl+Comma",
                          [this]
                          {
                              preferences_open = true;
                          })},
                    {item("Check for Update (Last checked a few seconds ago)", "",
                          [this]
                          {
                              toasts.push(ToastKind::INFO, "ImGit is up to date");
                          }),
                     item("Sign Out", "", placeholder("Sign Out")),
                     item("E&xit", "Alt+F4",
                          [this]
                          {
                              if (quit)
                              {
                                  quit();
                              }
                          })},
            });
        case 1:
            return ui::separated_menu({
                    {item("&Undo", "Ctrl+Z",
                          [this]
                          {
                              if (RepoState* state = active(); state != nullptr)
                              {
                                  undo(*state);
                              }
                          }),
                     item("&Redo", "Ctrl+Y",
                          [this]
                          {
                              if (RepoState* state = active(); state != nullptr)
                              {
                                  redo(*state);
                              }
                          })},
                    {item("&Cut", "Ctrl+X", send_chord(ImGuiKey_X)), item("C&opy", "Ctrl+C", send_chord(ImGuiKey_C)),
                     item("&Paste", "Ctrl+V", send_chord(ImGuiKey_V)),
                     item("Select &All", "Ctrl+A", send_chord(ImGuiKey_A))},
            });
        case 2:
        {
            ui::MenuItems numbered;
            for (int i = 1; i <= 9; ++i)
            {
                ui::MenuItem select = item("", "", nullptr);
                select.label = "Select Tab " + std::to_string(i);
                select.accelerator = "Ctrl+" + std::to_string(i);
                select.click = [this, i]
                {
                    if (i - 1 < static_cast<int>(tabs.size()))
                    {
                        active_tab = i - 1;
                    }
                };
                numbered.push_back(std::move(select));
            }
            ui::MenuItems tabs_menu = ui::separated_menu({
                    {item("Select Next Tab", "Ctrl+Tab",
                          [this]
                          {
                              select_adjacent_tab(1);
                          }),
                     item("Select Previous Tab", "Ctrl+Shift+Tab",
                          [this]
                          {
                              select_adjacent_tab(-1);
                          }),
                     item("Open Tabs List", "Ctrl+Shift+A", placeholder("Open Tabs List"))},
                    std::move(numbered),
            });

            const auto select_terminal_tab = [this](int index, int step)
            {
                TerminalPanel::Tabs& list = terminal_tabs(active());
                const int count = static_cast<int>(list.sessions.size());
                if (count == 0 || index >= count)
                {
                    return;
                }
                list.active = index >= 0 ? index : (list.active + step + count) % count;
                show_terminal_panel = true;
                terminal.minimized = false;
                terminal.focus_requested = true;
            };
            ui::MenuItems terminal_numbered;
            for (int i = 1; i <= 9; ++i)
            {
                ui::MenuItem select = item("", "",
                                           [select_terminal_tab, i]
                                           {
                                               select_terminal_tab(i - 1, 0);
                                           });
                select.label = "Select Terminal Tab " + std::to_string(i);
                select.accelerator = "Ctrl+Shift+" + std::to_string(i);
                terminal_numbered.push_back(std::move(select));
            }
            ui::MenuItems terminal_menu = ui::separated_menu({
                    {item("Select Next Terminal Tab", "Ctrl+Page Down",
                          [select_terminal_tab]
                          {
                              select_terminal_tab(-1, 1);
                          }),
                     item("Select Previous Terminal Tab", "Ctrl+Page Up",
                          [select_terminal_tab]
                          {
                              select_terminal_tab(-1, -1);
                          }),
                     item("New Terminal Tab", "Ctrl+Shift+N",
                          [this]
                          {
                              new_terminal_tab(active());
                          }),
                     item("Close Terminal Tab", "Ctrl+Shift+W",
                          [this]
                          {
                              close_terminal_tab(active(), terminal_tabs(active()).active);
                          })},
                    std::move(terminal_numbered),
            });

            ui::MenuItems favorites;
            for (int i = 1; i <= 9; ++i)
            {
                ui::MenuItem open = item("", "", placeholder("Favorite repositories"));
                open.label = "Open Favorite Repo " + std::to_string(i);
                open.accelerator = "Alt+Ctrl+" + std::to_string(i);
                favorites.push_back(std::move(open));
            }

            return ui::separated_menu({
                    {item("&Relaunch ImGit Desktop", "Ctrl+Shift+R",
                          [this]
                          {
                              if (relaunch)
                              {
                                  relaunch();
                              }
                          }),
                     item("Toggle &Full Screen", "Ctrl+Shift+F",
                          [this]
                          {
                              if (toggle_full_screen)
                              {
                                  toggle_full_screen();
                              }
                          })},
                    {item("&Increase Zoom", "Ctrl+=",
                          [this]
                          {
                              set_zoom(settings.zoom + 0.1f);
                          }),
                     item("Decrease Z&oom", "Ctrl+-",
                          [this]
                          {
                              set_zoom(settings.zoom - 0.1f);
                          }),
                     item("&Reset Zoom", "Ctrl+0",
                          [this]
                          {
                              set_zoom(1.f);
                          }),
                     ui::menu_submenu("Tabs", std::move(tabs_menu)),
                     ui::menu_submenu("Terminal Tabs", std::move(terminal_menu)),
                     ui::menu_submenu("Favorites", std::move(favorites))},
                    {checkbox("Show Left Panel", "Ctrl+J",
                              [this]
                              {
                                  show_left_panel = !show_left_panel;
                              }),
                     checkbox("Show Commit Details Panel", "Ctrl+K",
                              [this]
                              {
                                  show_detail_panel = !show_detail_panel;
                              }),
                     checkbox("Show Terminal Panel", "Ctrl+`",
                              [this]
                              {
                                  show_terminal_panel = !show_terminal_panel;
                                  terminal.focus_requested = show_terminal_panel;
                              })},
            });
        }
        case 3:
        {
            ui::MenuItem version = item("", "", nullptr, false);
            version.label = std::string("Version ") + VERSION + " (x64 64-bit)";
            return ui::separated_menu({
                    {item("Open Command Palette", "Ctrl+P",
                          [this]
                          {
                              fuzzy.open = true;
                              fuzzy.text.clear();
                              fuzzy.selected = 0;
                              fuzzy.focus_requested = true;
                          }),
                     item("Keyboard Shortcuts", "Ctrl+/",
                          [this]
                          {
                              dialog.open(Dialog::Kind::KEYBOARD_SHORTCUTS);
                          })},
                    {item("About ImGit Desktop", "",
                          [this]
                          {
                              dialog.open(Dialog::Kind::ABOUT);
                          }),
                     std::move(version)},
            });
        }
        default:
            return {};
        }
    }

    void Application::draw_menu_bar(RepoState* state)
    {
        (void)state;
        // Chromium's views menu bar (Electron on Windows): Segoe UI 9pt on #1F1F1F, 26px tall, the pressed entry
        // on #3E3E3E. Each entry is its label (rounded up) plus 6px on both sides.
        static constexpr const char* LABELS[] = {"&File", "&Edit", "&View", "&Help"};
        constexpr int COUNT = 4;
        ImGuiViewport* viewport = ImGui::GetMainViewport();
        const float height = ui::px(theme::MENU_BAR_HEIGHT);
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
        ImGui::PushStyleVar(ImGuiStyleVar_WindowMinSize, ImVec2(0.f, 0.f));
        const bool open =
                ImGui::BeginViewportSideBar("##menu_bar", viewport, ImGuiDir_Up, height,
                                            ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoSavedSettings
                                                    | ImGuiWindowFlags_NoNav | ImGuiWindowFlags_NoScrollWithMouse);
        ImGui::PopStyleVar(3);
        if (!open)
        {
            ImGui::End();
            return;
        }
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 origin = ImGui::GetWindowPos();
        const float right = origin.x + viewport->Size.x;
        draw->AddRectFilled(origin, ImVec2(right, origin.y + height), IM_COL32(0x1F, 0x1F, 0x1F, 255));
        // The bar's bottom rows are a shade darker, as Chromium paints them.
        const ImU32 shade[] = {IM_COL32(30, 30, 30, 255), IM_COL32(30, 30, 30, 255), IM_COL32(30, 30, 30, 255),
                               IM_COL32(29, 29, 29, 255)};
        for (int i = 0; i < 4; ++i)
        {
            const float y = origin.y + height - ui::px(5.f) + static_cast<float>(i) * ui::px(1.f);
            draw->AddRectFilled(ImVec2(origin.x, y), ImVec2(right, y + ui::px(1.f)), shade[i]);
        }

        ImFont* font = ui::fonts().system;
        const float line = ui::font_px(font, theme::FONT_SIZE_SMALL);
        float x = origin.x;
        std::array<ImVec2, COUNT> mins;
        std::array<ImVec2, COUNT> maxs;
        for (int i = 0; i < COUNT; ++i)
        {
            const std::string text = ui::strip_mnemonic(LABELS[i]);
            const float w = std::ceil(ui::text_size(font, theme::FONT_SIZE_SMALL, text.c_str()).x) + ui::px(12.f);
            mins[static_cast<std::size_t>(i)] = ImVec2(x, origin.y);
            maxs[static_cast<std::size_t>(i)] = ImVec2(x + w, origin.y + height);
            x += w;
        }
        const int open_index = ui::open_bar_menu();
        const bool underline =
                (ImGui::GetIO().KeyAlt && !ui::menu_open()) || (open_index >= 0 && ui::menu_keyboard_mode());
        for (int i = 0; i < COUNT; ++i)
        {
            const ImVec2 min = mins[static_cast<std::size_t>(i)];
            const ImVec2 max = maxs[static_cast<std::size_t>(i)];
            ImGui::SetCursorScreenPos(min);
            ImGui::PushID(i);
            ImGui::InvisibleButton("##bar_entry", ImVec2(max.x - min.x, height));
            ImGui::PopID();
            if (ImGui::IsItemActivated() && open_index != i)
            {
                ui::popup_bar_menu(bar_menu(i), i, min, max, false);
            }
            if (open_index == i)
            {
                // A 4px-rounded #3E3E3E plate, 1px narrower than the entry, fading a little at the bottom.
                const ImVec2 plate_max(max.x - ui::px(1.f), max.y);
                draw->AddRectFilled(min, plate_max, IM_COL32(62, 62, 62, 255), ui::px(4.f));
                draw->AddRectFilledMultiColor(ImVec2(min.x + ui::px(4.f), max.y - ui::px(5.f)),
                                              ImVec2(plate_max.x - ui::px(4.f), max.y), IM_COL32(62, 62, 62, 255),
                                              IM_COL32(62, 62, 62, 255), IM_COL32(59, 59, 59, 255),
                                              IM_COL32(59, 59, 59, 255));
            }
            const std::string text = ui::strip_mnemonic(LABELS[i]);
            const ImVec2 text_pos(min.x + ui::px(6.f), std::floor(min.y + (height - line) / 2.f) - ui::px(1.f));
            ui::draw_text(draw, font, theme::FONT_SIZE_SMALL, text_pos, IM_COL32(0xE3, 0xE3, 0xE3, 255), text.c_str());
            if (underline)
            {
                const float w = ui::text_size(font, theme::FONT_SIZE_SMALL, text.c_str(), text.c_str() + 1).x;
                const float y = std::round(text_pos.y + line * 0.86f);
                draw->AddRectFilled(ImVec2(text_pos.x, y), ImVec2(text_pos.x + std::round(w), y + ui::px(1.f)),
                                    IM_COL32(0xE3, 0xE3, 0xE3, 255));
            }
        }
        ImGui::End();

        ui::set_menu_bar(
                COUNT,
                [this](int i)
                {
                    return bar_menu(i);
                },
                [mins, maxs](int i, ImVec2* min, ImVec2* max)
                {
                    if (i < 0 || i >= COUNT)
                    {
                        return false;
                    }
                    *min = mins[static_cast<std::size_t>(i)];
                    *max = maxs[static_cast<std::size_t>(i)];
                    return true;
                });

        // Alt + the underlined letter opens an entry from the keyboard.
        if (!ui::menu_open() && dialog.kind == Dialog::Kind::NONE && ImGui::GetIO().KeyAlt && !ImGui::GetIO().KeyCtrl)
        {
            constexpr ImGuiKey KEYS[COUNT] = {ImGuiKey_F, ImGuiKey_E, ImGuiKey_V, ImGuiKey_H};
            for (int i = 0; i < COUNT; ++i)
            {
                if (ImGui::IsKeyPressed(KEYS[i], false))
                {
                    ui::popup_bar_menu(bar_menu(i), i, mins[static_cast<std::size_t>(i)],
                                       maxs[static_cast<std::size_t>(i)], true);
                }
            }
        }
    }

    void Application::draw_status_bar()
    {
        // The status bar: a 1px panel border over 25px of panel-bg0, 10px right padding. Right, inwards: the
        // version button, the zoom select and the activity log / keyboard shortcuts / release notes icons.
        const theme::Palette& p = theme::palette();
        const float height = ui::px(theme::INFO_BAR_HEIGHT);
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        const float width = ImGui::GetContentRegionAvail().x;
        ImGui::BeginChild("##status_bar", ImVec2(width, height), ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(origin, ImVec2(origin.x + width, origin.y + height), theme::u32(p.panel_bg0));
        draw->AddRectFilled(origin, ImVec2(origin.x + width, origin.y + 1.f), theme::u32(p.section_border));
        ImFont* font = ui::fonts().regular;
        const auto text_at = [&](ImFont* f, float box_top, float x, ImU32 color, const char* text, float ascent_units)
        {
            const float ascent = theme::FONT_SIZE_ROW * ascent_units / 2048.f * ui::dpi_scale();
            ui::draw_text(draw, f, theme::FONT_SIZE_ROW,
                          ImVec2(x, std::round(origin.y + box_top + std::round(ascent)) - std::ceil(ascent)), color,
                          text);
        };
        const auto clickable = [&](const char* id, ImVec2 min, ImVec2 max, const char* tip) -> bool
        {
            ImGui::SetCursorScreenPos(min);
            const bool pressed = ImGui::InvisibleButton(id, ImVec2(max.x - min.x, max.y - min.y));
            if (ImGui::IsItemHovered() && tip != nullptr)
            {
                ui::styled_tooltip(min, max, tip, true);
            }
            return pressed;
        };

        // Right, from the edge inwards.
        float x = origin.x + width - ui::px(10.f);
        {
            // .version-btn: monospace 12px / 15px, 0 5px padding, 1px border, radius 2.
            const std::string version = VERSION;
            ImFont* mono = ui::fonts().mono;
            const float text_w = ui::text_size(mono, theme::FONT_SIZE_ROW, version.c_str()).x;
            const float box_w = text_w + ui::px(12.f);
            const ImVec2 bmin(x - box_w, origin.y + ui::px(5.f));
            const ImVec2 bmax(x, bmin.y + ui::px(17.f));
            const bool pressed = clickable("##version", bmin, bmax, "About ImGit Desktop");
            draw->AddRectFilled(bmin, bmax, theme::u32(ImGui::IsItemHovered() ? p.hover_row : p.panel_bg0),
                                ui::px(2.f));
            draw->AddRect(bmin, bmax, theme::u32(p.section_border), ui::px(2.f), 0, 1.f);
            text_at(mono, ui::px(6.5f), bmin.x + ui::px(6.f), theme::u32(p.text_normal), version.c_str(), 1884.f);
            if (pressed)
            {
                dialog.open(Dialog::Kind::ABOUT);
            }
            x = bmin.x - ui::px(10.f);
        }
        {
            // .zoom-select: the search-plus icon (text-secondary) and a 41px select, 5px padding.
            const std::string zoom = std::to_string(static_cast<int>(std::round(settings.zoom * 100.f))) + "%";
            const ImVec2 bmin(x - ui::px(63.f), origin.y + ui::px(1.f));
            const ImVec2 bmax(x, origin.y + height);
            if (clickable("##zoom", bmin, bmax, nullptr))
            {
                ImGui::OpenPopup("##zoom_levels");
            }
            const bool hovered = ImGui::IsItemHovered();
            ui::draw_fa_icon(draw, "far:search-plus", ImVec2(bmin.x + ui::px(11.f), origin.y + ui::px(13.f)),
                             ui::px(12.f), theme::u32(p.text_secondary));
            text_at(font, ui::px(5.f), bmin.x + ui::px(22.3f), theme::u32(hovered ? p.text_selected : p.text_normal),
                    zoom.c_str(), 2189.f);
            ImGui::SetNextWindowPos(ImVec2(bmin.x, bmin.y), ImGuiCond_Always, ImVec2(0.f, 1.f));
            if (ImGui::BeginPopup("##zoom_levels"))
            {
                for (const int level : {50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200})
                {
                    const std::string label = std::to_string(level) + "%";
                    if (ImGui::Selectable(label.c_str(), std::lround(settings.zoom * 100.f) == level))
                    {
                        set_zoom(static_cast<float>(level) / 100.f);
                    }
                }
                ImGui::EndPopup();
            }
            x = bmin.x - ui::px(10.f);
        }
        // .p1 .hover-text icons (5px padding, 10px apart), right to left.
        const auto icon_item = [&](const char* id, const char* icon, const char* tip) -> bool
        {
            const float w = ui::fa_icon_width(icon, ui::px(12.f));
            const float block = w + ui::px(10.f);
            x -= ui::px(10.f) + block;
            const ImVec2 bmin(x, origin.y);
            const ImVec2 bmax(x + block, origin.y + height);
            const bool pressed = clickable(id, bmin, bmax, tip);
            const bool hovered = ImGui::IsItemHovered();
            ui::draw_fa_icon(draw, icon, ImVec2(x + ui::px(5.f) + w / 2.f, origin.y + ui::px(13.4f)), ui::px(12.f),
                             theme::u32(hovered ? p.text_selected : p.text_normal));
            return pressed;
        };
        if (icon_item("##release_notes", "far:gift", "What's new"))
        {
            toasts.push(ToastKind::INFO, "Release Notes", std::string("ImGit ") + VERSION);
        }
        if (icon_item("##shortcuts", "far:keyboard", "Keyboard Shortcuts"))
        {
            dialog.open(Dialog::Kind::KEYBOARD_SHORTCUTS);
        }
        if (icon_item("##activity", "far:list", "Activity Log"))
        {
            show_terminal_panel = true;
            terminal.focus_requested = true;
        }
        ImGui::EndChild();
    }

    void Application::load_recent()
    {
        std::ifstream file(config_path("recent.txt"));
        std::string line;
        while (std::getline(file, line))
        {
            line = trim(line);
            std::error_code ec;
            if (!line.empty() && std::filesystem::exists(line, ec))
            {
                recent_repos.push_back(line);
            }
        }
    }

    void Application::save_recent() const
    {
        std::string text;
        for (const std::string& path : recent_repos)
        {
            text += path + '\n';
        }
        (void)write_text_file(config_path("recent.txt"), text);
    }

    void Application::save_tabs() const
    {
        std::string text;
        for (const auto& tab : tabs)
        {
            text += tab->repo.workdir() + '\n';
        }
        text += "#active=" + std::to_string(active_tab) + '\n';
        (void)write_text_file(config_path("tabs.txt"), text);
    }

    void Application::restore_tabs()
    {
        std::ifstream file(config_path("tabs.txt"));
        std::string line;
        int wanted = -1;
        while (std::getline(file, line))
        {
            line = trim(line);
            if (line.starts_with("#active="))
            {
                wanted = std::atoi(line.c_str() + 8);
            }
            else if (std::error_code ec; !line.empty() && std::filesystem::exists(line, ec))
            {
                open_repository(line);
            }
        }
        if (wanted >= -1 && wanted < static_cast<int>(tabs.size()))
        {
            active_tab = wanted;
        }
    }
} // namespace imgit::app
