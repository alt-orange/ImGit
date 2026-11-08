#pragma once

#include <map>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

// The Preferences ImGit keeps itself (the profile's preferences.txt): what the Preferences pages show and what the
// app reads, from one store. Settings git has its own key for (commit.gpgsign, core.autocrlf, core.hooksPath...)
// are not kept here: the pages read and write the git configuration instead.
namespace imgit::app
{
    // A key and the value it has until the user sets one.
    struct Preference
    {
        std::string_view key;
        std::string_view fallback;
    };

    namespace pref
    {
        // General
        inline constexpr Preference AUTO_PRUNE{"General/Auto-Prune", "1"}; // fetches prune deleted remote branches
        inline constexpr Preference UPDATE_SUBMODULES{"General/Keep submodules up to date", "1"};
        inline constexpr Preference SHOW_ALL_COMMITS{"General/Show All Commits in Graph", "0"};
        inline constexpr Preference LAZY_LOAD_COMMITS{"General/Lazy Load Commits in Graph", "1"};
        inline constexpr Preference EXTENDED_LOGGING{"General/Extended logging", "0"};

        // SSH
        inline constexpr Preference SSH_AGENT{"SSH/Use local SSH agent", "0"};
        inline constexpr Preference SSH_PRIVATE_KEY{"SSH/Private key", ""}; // empty: ssh's own default keys
        inline constexpr Preference SSH_PUBLIC_KEY{"SSH/Public key", ""};
        inline constexpr Preference CREDENTIAL_MANAGER{"SSH/Use Git Credential Manager", "0"};

        // External Tools
        inline constexpr Preference MERGE_TOOL{"External Tools/Merge Tool", "Git Config Default"};
        inline constexpr Preference DIFF_TOOL{"External Tools/Diff Tool", "<Use Merge Tool>"};
        inline constexpr Preference EDITOR{"External Tools/Editor", "System Default"};
        inline constexpr Preference EDITOR_COMMAND{"External Tools/Editor command", ""};
        inline constexpr Preference TERMINAL{"External Tools/Terminal", "System Default"};
        inline constexpr Preference TERMINAL_COMMAND{"External Tools/Terminal command", ""};

        // Notifications
        inline constexpr Preference DESKTOP_NOTIFICATIONS{"Notifications/Desktop", "0"};

        // UI Customization
        inline constexpr Preference NOTIFICATION_LOCATION{"UI Customization/Notification Location", "Bottom Left"};
        inline constexpr Preference LOCALE{"UI Customization/Locale", "System [system]"};
        // moment.js formats; empty: the placeholder (DEFAULT_*_FORMAT).
        inline constexpr Preference DATE_FORMAT{"UI Customization/Date Format", ""};
        inline constexpr Preference DATE_WORD_FORMAT{"UI Customization/Date word Format", ""};
        inline constexpr Preference DATE_TIME_FORMAT{"UI Customization/Date/Time Format", ""};
        inline constexpr Preference DATE_VERBOSE_FORMAT{"UI Customization/Date verbose Format", ""};
        inline constexpr Preference GROUP_WORKTREES{"UI Customization/Group worktree tabs", "1"};

        // Editor (the file view, the diff and the merge tool)
        inline constexpr Preference EDITOR_FONT{"Editor/Font", ""}; // empty: the system's monospace face
        inline constexpr Preference EDITOR_FONT_SIZE{"Editor/Font Size", "12"};
        inline constexpr Preference EDITOR_TAB_SIZE{"Editor/Tab Size", "4"};
#ifdef _WIN32
        inline constexpr Preference EDITOR_EOL{"Editor/End of Line", "CRLF"};
#else
        inline constexpr Preference EDITOR_EOL{"Editor/End of Line", "LF"};
#endif
        inline constexpr Preference EDITOR_LINE_NUMBERS{"Editor/Show Line Numbers", "1"};

        // In-App Terminal
        inline constexpr Preference TERMINAL_FONT{"In-App Terminal/Font", ""};
        inline constexpr Preference TERMINAL_FONT_SIZE{"In-App Terminal/Font Size", "12"};
        inline constexpr Preference TERMINAL_LINE_HEIGHT{"In-App Terminal/Line Height", "1.2"};
        inline constexpr Preference TERMINAL_CURSOR{"In-App Terminal/Cursor Style", "Bar"};
        inline constexpr Preference TERMINAL_DIM{"In-App Terminal/Dim when unfocused", "1"};
        inline constexpr Preference TERMINAL_SHELL{"In-App Terminal/Default", ""}; // empty: the platform's shell
    } // namespace pref

    inline constexpr std::string_view DEFAULT_DATE_FORMAT = "MM/DD/YYYY";
    inline constexpr std::string_view DEFAULT_DATE_WORD_FORMAT = "MMMM D, YYYY";
    inline constexpr std::string_view DEFAULT_DATE_TIME_FORMAT = "MM/DD/YYYY @ h:mm A";
    inline constexpr std::string_view DEFAULT_DATE_VERBOSE_FORMAT = "MMMM D, YYYY h:mm A";

    // The store: read and written on the UI thread, read by worker threads too (each call locks).
    class Preferences
    {
    public:
        // Reads `path` (key, tab, value lines); later saves go there.
        void load(std::string path);
        [[nodiscard]] std::string get(const Preference& preference) const;
        [[nodiscard]] bool flag(const Preference& preference) const;
        // The value as a whole number in [low, high]; the fallback's when it is not one.
        [[nodiscard]] int number(const Preference& preference, int low, int high) const;
        // Writes the file at once.
        void set(const Preference& preference, std::string value);
        void set_flag(const Preference& preference, bool value);

    private:
        void save() const;

        mutable std::mutex m_mutex;
        std::string m_path;
        std::map<std::string, std::string, std::less<>> m_values;
    };

    // The current profile's preferences.
    [[nodiscard]] Preferences& preferences();

    // External Tools: whether `git mergetool` / `git difftool` are offered (merge.tool / diff.tool decide which tool
    // git starts; the diff tool's "<Use Merge Tool>" follows the merge tool's choice, as git falls back to it).
    [[nodiscard]] bool external_merge_tool_enabled();
    [[nodiscard]] bool external_diff_tool_enabled();

    // Pure helpers of what the preferences drive (unit tested).

    // GIT_SSH_COMMAND for a chosen private key: ssh with only that key, unless the SSH agent is used or there is no
    // key (then ssh's own configuration, agent and default keys decide). The path is quoted for git's shell.
    [[nodiscard]] std::optional<std::string> ssh_command(std::string_view private_key, bool use_agent);

    // `git fetch ...` with the Auto-Prune preference applied (its own --prune / --no-prune replaced); other commands
    // unchanged.
    [[nodiscard]] std::vector<std::string> with_prune_preference(std::vector<std::string> args, bool prune);

    // "1.2" -> 12 (tenths), for the line height field; `fallback` when it is not a number in [low, high] tenths.
    [[nodiscard]] int parse_tenths(std::string_view text, int fallback, int low, int high);

    // The command line of a "<Custom>" external tool: `command` split into words (double quotes group), with
    // `path` in place of each %path%, or appended when there is none.
    [[nodiscard]] std::vector<std::string> custom_tool_command(std::string_view command, const std::string& path);
} // namespace imgit::app
