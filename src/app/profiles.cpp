#include "app/profiles.hpp"
#include <algorithm>
#include <cctype>
#include <fstream>
#include <map>
#include <random>

namespace imgit::app
{
    namespace
    {
        constexpr const char* PROFILE_FILE = "profile.txt";
        constexpr const char* CONFIG_FILE = "config.txt";

        // key=value lines (the value runs to the end of the line).
        std::map<std::string, std::string> read_values(const std::filesystem::path& path)
        {
            std::map<std::string, std::string> values;
            std::ifstream file(path);
            std::string line;
            while (std::getline(file, line))
            {
                if (!line.empty() && line.back() == '\r')
                {
                    line.pop_back();
                }
                if (const auto equals = line.find('='); equals != std::string::npos)
                {
                    values[line.substr(0, equals)] = line.substr(equals + 1);
                }
            }
            return values;
        }

        std::string one_line(std::string text)
        {
            std::replace(text.begin(), text.end(), '\n', ' ');
            std::erase(text, '\r');
            return text;
        }

        // ProfileService.getNewUUID: a v4 uuid without its dashes.
        std::string new_guid()
        {
            static std::mt19937_64 engine{std::random_device{}()};
            static constexpr char HEX[] = "0123456789abcdef";
            std::string guid(32, '0');
            for (char& c : guid)
            {
                c = HEX[engine() & 15u];
            }
            guid[12] = '4';
            guid[16] = HEX[8u + (engine() & 3u)];
            return guid;
        }

        std::string lowercase(std::string text)
        {
            std::transform(text.begin(), text.end(), text.begin(),
                           [](unsigned char c)
                           {
                               return static_cast<char>(std::tolower(c));
                           });
            return text;
        }
    } // namespace

    void Profiles::load(const std::filesystem::path& root, const std::string& user_name, const std::string& user_email,
                        std::span<const char* const> legacy_files)
    {
        m_root = root;
        m_profiles.clear();
        std::error_code ec;
        std::filesystem::create_directories(m_root / "profiles", ec);
        for (const auto& entry : std::filesystem::directory_iterator(m_root / "profiles", ec))
        {
            if (!entry.is_directory())
            {
                continue;
            }
            auto values = read_values(entry.path() / PROFILE_FILE);
            // ProfileService skips a profile without its required fields.
            if (values["name"].empty())
            {
                continue;
            }
            m_profiles.push_back({entry.path().filename().string(), values["name"], values["user_name"],
                                  values["user_email"]});
        }
        auto config = read_values(m_root / CONFIG_FILE);
        m_current = config["profile_guid"];
        m_keep_git_config = config["keep_git_config"] != "0";

        if (m_profiles.empty())
        {
            // The default profile takes its author from the git configuration.
            const Profile profile{new_guid(), "Default Profile", user_name, user_email};
            std::filesystem::create_directories(directory(profile.guid), ec);
            for (const char* file : legacy_files)
            {
                if (std::filesystem::exists(m_root / file, ec))
                {
                    std::filesystem::rename(m_root / file, directory(profile.guid) / file, ec);
                }
            }
            save(profile);
            m_profiles.push_back(profile);
        }
        sort();
        if (find(m_current) == nullptr)
        {
            m_current = m_profiles.front().guid;
        }
        save_config();
    }

    const std::vector<Profile>& Profiles::all() const noexcept
    {
        return m_profiles;
    }

    const Profile& Profiles::current() const
    {
        const Profile* profile = find(m_current);
        return profile != nullptr ? *profile : m_profiles.front();
    }

    const Profile* Profiles::find(const std::string& guid) const
    {
        const auto found = std::find_if(m_profiles.begin(), m_profiles.end(),
                                        [&](const Profile& profile)
                                        {
                                            return profile.guid == guid;
                                        });
        return found != m_profiles.end() ? &*found : nullptr;
    }

    std::filesystem::path Profiles::directory(const std::string& guid) const
    {
        return m_root / "profiles" / guid;
    }

    std::filesystem::path Profiles::directory() const
    {
        return directory(current().guid);
    }

    std::string Profiles::create(Profile values, bool copy_current)
    {
        values.guid = new_guid();
        if (values.name.empty())
        {
            values.name = "New Profile";
        }
        std::error_code ec;
        if (copy_current)
        {
            std::filesystem::copy(directory(), directory(values.guid), std::filesystem::copy_options::recursive, ec);
        }
        std::filesystem::create_directories(directory(values.guid), ec);
        save(values);
        m_profiles.push_back(values);
        sort();
        return values.guid;
    }

    void Profiles::update(const Profile& profile)
    {
        for (Profile& existing : m_profiles)
        {
            if (existing.guid == profile.guid)
            {
                existing = profile;
                save(existing);
            }
        }
        sort();
    }

    bool Profiles::remove(const std::string& guid)
    {
        if (guid == m_current || find(guid) == nullptr)
        {
            return false;
        }
        std::error_code ec;
        std::filesystem::remove_all(directory(guid), ec);
        std::erase_if(m_profiles,
                      [&](const Profile& profile)
                      {
                          return profile.guid == guid;
                      });
        return true;
    }

    void Profiles::set_current(const std::string& guid)
    {
        if (find(guid) != nullptr)
        {
            m_current = guid;
            save_config();
        }
    }

    bool Profiles::keep_git_config() const noexcept
    {
        return m_keep_git_config;
    }

    void Profiles::set_keep_git_config(bool keep)
    {
        m_keep_git_config = keep;
        save_config();
    }

    void Profiles::save(const Profile& profile) const
    {
        std::ofstream file(directory(profile.guid) / PROFILE_FILE);
        file << "name=" << one_line(profile.name) << '\n'
             << "user_name=" << one_line(profile.user_name) << '\n'
             << "user_email=" << one_line(profile.user_email) << '\n';
    }

    void Profiles::save_config() const
    {
        std::ofstream file(m_root / CONFIG_FILE);
        file << "profile_guid=" << m_current << '\n' << "keep_git_config=" << (m_keep_git_config ? 1 : 0) << '\n';
    }

    void Profiles::sort()
    {
        std::stable_sort(m_profiles.begin(), m_profiles.end(),
                         [](const Profile& a, const Profile& b)
                         {
                             return lowercase(a.name) < lowercase(b.name);
                         });
    }
} // namespace imgit::app
