#pragma once

#include <filesystem>
#include <span>
#include <string>
#include <vector>

namespace imgit::app
{
    // Replaces `path` with `text` through a temporary file next to it, so a crash or a full disk never leaves the
    // file empty; false when it could not be written.
    bool write_text_file(const std::filesystem::path& path, const std::string& text);

    // A profile: a name and the commit author, with its own preferences, tabs and repository
    // lists (the files in its directory).
    struct Profile
    {
        std::string guid;
        std::string name;
        std::string user_name;
        std::string user_email;
    };

    // The profiles under <root>/profiles/<guid>/ and the app-wide <root>/config.txt (the current profile and
    // whether the global .gitconfig follows it), one folder per profile.
    class Profiles
    {
    public:
        // Loads the profiles. With none, creates "Default Profile" for the given author and moves `legacy_files`
        // (written before profiles existed) from `root` into it.
        void load(const std::filesystem::path& root, const std::string& user_name, const std::string& user_email,
                  std::span<const char* const> legacy_files);

        // Sorted by name, like the profile table and pickers.
        [[nodiscard]] const std::vector<Profile>& all() const noexcept;
        [[nodiscard]] const Profile& current() const;
        [[nodiscard]] const Profile* find(const std::string& guid) const;
        [[nodiscard]] std::filesystem::path directory(const std::string& guid) const;
        [[nodiscard]] std::filesystem::path directory() const;

        // Creates a profile ("New Profile" when unnamed), starting from a copy of the current one's files when
        // asked; returns its guid.
        std::string create(Profile values, bool copy_current);
        void update(const Profile& profile);
        // Deletes a profile other than the current one.
        bool remove(const std::string& guid);
        void set_current(const std::string& guid);

        // keepGitConfigInSyncWithProfile: switching or editing the current profile writes its author to the
        // global .gitconfig.
        [[nodiscard]] bool keep_git_config() const noexcept;
        void set_keep_git_config(bool keep);

    private:
        void save(const Profile& profile) const;
        void save_config() const;
        void sort();

        std::filesystem::path m_root;
        std::vector<Profile> m_profiles;
        std::string m_current;
        bool m_keep_git_config = true;
    };
} // namespace imgit::app
