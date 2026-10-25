#pragma once

#include "app/profiles.hpp"
#include "app/repo_state.hpp"
#include "app/toasts.hpp"
#include "app/vt_screen.hpp"
#include "platform/pseudo_console.hpp"
#include "ui/native_menu.hpp"
#include <functional>
#include <imgui.h>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <vector>

namespace imgit::app
{
    // Preferences, persisted as key=value lines.
    struct Settings
    {
        int auto_fetch_minutes = 1;
        int initial_commits = 2000;
        bool show_toolbar_labels = true;
        bool author_initials = true;
        bool show_author = false;
        bool show_date = false;
        bool show_sha = false;
        bool show_changes = false;
        bool show_description = true;
        bool show_ghost_refs = true;
        bool highlight_rows_on_ref_hover = true;
        bool remember_tabs = true;
        bool push_after_commit = false;
        bool compact_graph = false;
        float zoom = 1.f;
        std::string theme = "ImGit Dark"; // a theme::theme_names() entry
        std::string default_branch = "main";
        int pull_mode = 1; // the pull button's default: 0 fetch all, 1 ff if possible, 2 ff only, 3 rebase
        // Left panel sections switched off from the section header's context menu (ImGit hides Worktrees by
        // default).
        std::set<std::string> hidden_left_sections = {"Worktrees"};
        // Panel sizes and the graph columns' preferred widths dragged by the user, in CSS px (ImGit keeps them in
        // its profile's layout settings).
        float left_panel_width = 208.f; // LEFT_PANEL_DEFAULT_WIDTH
        float detail_panel_width = 438.f; // DETAIL_PANEL_DEFAULT_WIDTH
        float terminal_height = 275.f; // TERMINAL_DEFAULT_HEIGHT
        float commit_message_height = 150.f; // layout.CommitMessage.rightPanelHeight (initialCommitMessageHeight)
        std::map<std::string, float> graph_column_widths; // by zone: ref, graph, message, author...
        std::vector<std::string> graph_column_order; // the zones' keys as dragged into place; empty: the default

        void load(const std::string& path);
        void save(const std::string& path) const;
    };

    // The modal currently shown, with its form fields.
    struct Dialog
    {
        enum class Kind
        {
            NONE,
            CLONE_REPO,
            INIT_REPO,
            CREATE_BRANCH,
            CREATE_TAG,
            RENAME_BRANCH,
            DELETE_BRANCH,
            DELETE_TAG,
            DELETE_STASH,
            DISCARD_ALL,
            RESET,
            PUSH_NEW_BRANCH,
            ADD_REMOTE,
            ADD_SUBMODULE,
            EDIT_REMOTE,
            REWORD_COMMIT,
            SQUASH_COMMITS,
            SET_UPSTREAM,
            PREFERENCES,
            KEYBOARD_SHORTCUTS,
            ABOUT,
            CONFIRM, // generic: title / message / confirm_label / on_confirm
        };
        Kind kind = Kind::NONE;
        bool just_opened = false;
        // The repository the dialog was opened for (its working directory, set on the first frame it is drawn): the
        // prompt bar acts on that tab, whichever one is active, and closes when it is gone.
        std::string workdir;
        std::string text_a; // name / path / url
        std::string text_b; // message / destination
        bool flag = false; // "checkout after create", "also delete remote"...
        git::Sha sha;
        std::string ref_name;
        std::string ref_remote;
        int stash_index = -1;
        int page = 0;
        git::ResetMode reset_mode = git::ResetMode::MIXED;
        std::vector<git::Sha> shas; // SQUASH_COMMITS
        std::string error;
        // CONFIRM
        std::string title;
        std::string message;
        std::string confirm_label;
        bool danger = false;
        std::function<void()> on_confirm;

        void open(Kind new_kind)
        {
            *this = Dialog{};
            kind = new_kind;
            just_opened = true;
        }
        void close()
        {
            *this = Dialog{};
        }
    };

    // The Add / Edit Profile modal (PROFILE_ADD_EDIT) and its Delete profile confirmation (PROFILE_DELETE).
    struct ProfileDialog
    {
        enum class Kind
        {
            NONE,
            ADD_EDIT,
            DELETE,
        };
        Kind kind = Kind::NONE;
        bool editing = false;
        bool just_opened = false;
        Profile values; // the fields; guid: the profile edited / deleted
        Profile defaults; // the fields' placeholders, used for the ones left empty
        bool copy_current = true;
        std::string gravatar_email; // the email the preview shows, updated 500ms after typing stops
        double email_changed_at = 0.0;

        void close()
        {
            *this = ProfileDialog{};
        }
    };

    // The command palette (Ctrl+P): commands, branch checkouts and recent repositories.
    struct FuzzyFinder
    {
        bool open = false;
        bool focus_requested = false;
        std::string text;
        int selected = 0;
    };

    // One shell of the terminal panel: a pseudo console and the screen its output draws.
    struct TerminalSession
    {
        std::string workdir;
        platform::PseudoConsole console;
        VtScreen screen;
        bool started = false;
        bool failed = false;
        std::string pending_input; // typed before the shell started
        int scroll = 0; // lines the view is scrolled up from the bottom
        std::uint64_t scrolled_seen = 0;
        // The selection, in line ids (VtScreen::first_line_id() based) and columns; anchor then head.
        bool selecting = false;
        bool has_selection = false;
        std::uint64_t anchor_line = 0;
        int anchor_column = 0;
        std::uint64_t head_line = 0;
        int head_column = 0;
    };

    // The terminal panel: per repository, tabs of shells; expanded, minimized to its toolbar, or hidden.
    struct TerminalPanel
    {
        struct Tabs
        {
            std::vector<std::unique_ptr<TerminalSession>> sessions;
            int active = 0;
        };
        std::map<std::string, Tabs> repos; // by working directory
        bool minimized = false;
        bool focus_requested = false;
        bool focused = false;
    };

    class Application
    {
    public:
        Application();
        ~Application();

        void draw_frame();

        std::vector<std::unique_ptr<RepoState>> tabs;
        int active_tab = -1; // -1: the "New Tab" page
        bool new_tab_open = false; // the "+" button's "New Tab" entry in the tabs bar
        bool repo_management_open = false; // the tabs bar folder's Repository Management view
        Toasts toasts;
        Dialog dialog;
        ProfileDialog profile_dialog;
        FuzzyFinder fuzzy;
        TerminalPanel terminal;
        Profiles profiles;
        Settings settings;
        std::vector<std::string> recent_repos;
        void save_recent() const;

        // `git clone` in flight; the destination opens as a tab when it finishes. Its progress is written by the
        // clone's thread and read by the Clone dialog's progress bar.
        struct SharedCloneProgress
        {
            std::mutex mutex;
            git::cli::CloneProgress progress;
        };
        struct PendingClone
        {
            std::future<git::cli::Output> future;
            std::string destination;
            std::string parent;
            std::string url;
            std::optional<git::cli::Credential> attempt;
            std::shared_ptr<SharedCloneProgress> progress;
        };
        std::optional<PendingClone> pending_clone;
        void start_clone(std::string parent, std::string url, std::string destination,
                         std::optional<git::cli::Credential> attempt = std::nullopt);
        // A snapshot of the running clone's progress (empty when none runs).
        [[nodiscard]] git::cli::CloneProgress clone_progress() const;

        // The credentials prompt: a git command refused for want of a username / password waits here for the
        // user, then `retry` runs it again with what they entered.
        struct CredentialPrompt
        {
            std::string url; // scheme://host
            std::string username;
            std::string password;
            bool remember = false;
            bool rejected = false; // "Authentication failed. ..."
            bool just_opened = true;
            std::function<void(git::cli::Credential)> retry;
        };
        std::optional<CredentialPrompt> credential_prompt;
        // The credentials git accepted this session (ImGit keeps them for the session unless remembered).
        std::vector<git::cli::Credential> credentials;
        // Opens the prompt when a failed command's `output` asked for credentials; false otherwise (or when a
        // prompt is already open). `attempt` is what that run was given.
        bool prompt_for_credentials(const std::string& output, const std::optional<git::cli::Credential>& attempt,
                                    std::function<void(git::cli::Credential)> retry);
        void keep_credential(const git::cli::Credential& credential);

        // Paths of closed tabs, most recent last (File > Reopen Closed Tab).
        std::vector<std::string> closed_tabs;
        // Window-level actions provided by main().
        std::function<void()> quit;
        std::function<void()> relaunch;
        std::function<void()> toggle_full_screen;

        // The Preferences view (replaces the tab content).
        bool preferences_open = false;
        std::string preferences_page = "General";

        bool show_left_panel = true;
        bool show_detail_panel = true;
        bool show_terminal_panel = false;
        bool show_toolbar = true;
        float base_dpi_scale = 1.f;

        [[nodiscard]] RepoState* active() noexcept;
        void open_repository(const std::string& path);
        // Open Repo: a native folder picker starting at the last opened repository; a folder that is not
        // a repository gets an "Open Failed" toast.
        void browse_for_repository();
        void close_tab(int index);
        void reopen_closed_tab();
        void select_adjacent_tab(int direction);
        void copy_to_clipboard(const std::string& text);
        void set_zoom(float zoom);
        void apply_settings();
        // A file of the current profile (settings, tabs, recent repositories...).
        [[nodiscard]] std::string config_path(const char* file) const;
        // A file shared by every profile (the avatar cache).
        [[nodiscard]] std::string app_path(const char* file) const;

        // Switches to another profile once the frame is drawn: its settings, tabs and author replace the current
        // ones.
        void switch_profile(const std::string& guid);
        // Creates a profile (from a copy of the current one's settings when asked) and switches to it.
        void add_profile(const Profile& values, bool copy_current);
        void edit_profile(const Profile& profile);
        void delete_profile(const std::string& guid);
        void set_keep_git_config(bool keep);

        // Runs a repository operation, reports failure as a toast, success as `success_message` when given,
        // and reloads the tab.
        template<class Fn>
        bool run(RepoState& state, const char* what, Fn&& fn, const std::string& success_message = {})
        {
            auto result = fn();
            if (!result)
            {
                toasts.push(ToastKind::ERROR, std::string(what) + " failed", result.error().message);
                state.refresh();
                return false;
            }
            if (!success_message.empty())
            {
                toasts.push(ToastKind::SUCCESS, success_message);
            }
            state.refresh();
            return true;
        }

        // Starts `git <args>` for the tab in the background; the result is reported when it finishes.
        // `attempt`: a credential from the credentials prompt, tried before the session's own.
        void run_git_cli(RepoState& state, std::string title, std::vector<std::string> args, bool silent = false,
                         std::optional<git::cli::Credential> attempt = std::nullopt);
        // Runs several `git` commands one after another in the background (stopping at the first failure); the
        // result toast reports the last.
        void run_git_cli_sequence(RepoState& state, std::string title, std::vector<std::vector<std::string>> runs);
        // A failed `git push <...>` whose `output` says the remote has commits the branch lacks (non-fast-forward /
        // fetch first): asks in the prompt bar whether to push again with --force-with-lease. False when it is not
        // that refusal, or another prompt is already open.
        bool offer_force_push(RepoState& state, const std::string& output, std::vector<std::string> args);
        // Writes the settings, the open tabs and the recent repositories (done again at exit).
        void save_state() const;
        // Types `line` and Enter into the repository's active terminal (opening the panel and a shell if needed).
        void run_terminal_command(RepoState* state, const std::string& line);
        // The repository's terminal tabs (the shell starts when the panel first draws it).
        [[nodiscard]] TerminalPanel::Tabs& terminal_tabs(const RepoState* state);
        [[nodiscard]] TerminalSession* active_terminal(const RepoState* state);
        void new_terminal_tab(const RepoState* state);
        void close_terminal_tab(const RepoState* state, int index);
        // Kill All Terminals: asks first when there are two or more, then hides the panel.
        void kill_terminals(RepoState* state);
        void undo(RepoState& state);
        void redo(RepoState& state);
        void restore_tabs();

    private:
        void apply_profile_switch();
        // The current profile's author signs commits and, with "Keep my .gitconfig updated", goes to the global
        // .gitconfig.
        void apply_profile_identity(bool write_git_config);
        void poll_async();
        void auto_fetch(RepoState& state);
        void handle_shortcuts(RepoState* state);
        void load_recent();
        void save_tabs() const;
        void draw_menu_bar(RepoState* state);
        [[nodiscard]] ui::MenuItems bar_menu(int index);
        bool native_menu_installed = false; // the menus sit in the system menu bar (macOS)
        void draw_status_bar();
        void draw_repo_tab(RepoState& state, ImVec2 size);

        std::string m_pending_profile;
    };
} // namespace imgit::app
