#include "app/preferences.hpp"
#include <algorithm>
#include <cstdlib>
#include <fstream>

namespace imgit::app
{
    void Preferences::load(std::string path)
    {
        std::lock_guard lock(m_mutex);
        m_path = std::move(path);
        m_values.clear();
        std::ifstream file(m_path);
        std::string line;
        bool first = true;
        while (std::getline(file, line))
        {
            // A UTF-8 byte order mark (an editor's) is not part of the first key.
            if (std::exchange(first, false) && line.starts_with("\xEF\xBB\xBF"))
            {
                line.erase(0, 3);
            }
            if (!line.empty() && line.back() == '\r')
            {
                line.pop_back();
            }
            const auto tab = line.find('\t');
            if (tab != std::string::npos)
            {
                m_values[line.substr(0, tab)] = line.substr(tab + 1);
            }
        }
    }

    std::string Preferences::get(const Preference& preference) const
    {
        std::lock_guard lock(m_mutex);
        const auto found = m_values.find(preference.key);
        return found != m_values.end() ? found->second : std::string(preference.fallback);
    }

    bool Preferences::flag(const Preference& preference) const
    {
        return get(preference) == "1";
    }

    int Preferences::number(const Preference& preference, int low, int high) const
    {
        const auto parse = [](std::string_view text) -> std::optional<int>
        {
            if (text.empty() || text.size() > 6
                || !std::all_of(text.begin(), text.end(),
                                [](char c)
                                {
                                    return c >= '0' && c <= '9';
                                }))
            {
                return std::nullopt;
            }
            return std::atoi(std::string(text).c_str());
        };
        const std::optional<int> value = parse(get(preference));
        if (value && *value >= low && *value <= high)
        {
            return *value;
        }
        return std::clamp(parse(preference.fallback).value_or(low), low, high);
    }

    void Preferences::set(const Preference& preference, std::string value)
    {
        std::lock_guard lock(m_mutex);
        // A tab or line break would split the line it is saved on.
        std::replace_if(
                value.begin(), value.end(),
                [](char c)
                {
                    return c == '\t' || c == '\n' || c == '\r';
                },
                ' ');
        m_values[std::string(preference.key)] = std::move(value);
        save();
    }

    void Preferences::set_flag(const Preference& preference, bool value)
    {
        set(preference, value ? "1" : "0");
    }

    void Preferences::save() const
    {
        if (m_path.empty())
        {
            return;
        }
        std::ofstream file(m_path);
        for (const auto& [key, value] : m_values)
        {
            file << key << '\t' << value << '\n';
        }
    }

    Preferences& preferences()
    {
        static Preferences instance;
        return instance;
    }

    bool external_merge_tool_enabled()
    {
        return preferences().get(pref::MERGE_TOOL) != "<None>";
    }

    bool external_diff_tool_enabled()
    {
        const std::string tool = preferences().get(pref::DIFF_TOOL);
        return tool == "<Use Merge Tool>" ? external_merge_tool_enabled() : tool != "<None>";
    }

    std::optional<std::string> ssh_command(std::string_view private_key, bool use_agent)
    {
        if (use_agent || private_key.empty())
        {
            return std::nullopt;
        }
        // git runs it through sh: forward slashes (ssh on Windows takes them) and single quotes, with a quote in
        // the path closed, escaped and reopened.
        std::string quoted = "'";
        for (const char c : private_key)
        {
            if (c == '\'')
            {
                quoted += "'\\''";
            }
            else
            {
                quoted += c == '\\' ? '/' : c;
            }
        }
        quoted += '\'';
        return "ssh -i " + quoted + " -o IdentitiesOnly=yes";
    }

    std::vector<std::string> with_prune_preference(std::vector<std::string> args, bool prune)
    {
        // The subcommand: the first argument that is not a `-c key=value` pair.
        std::size_t command = 0;
        while (command + 1 < args.size() && args[command] == "-c")
        {
            command += 2;
        }
        if (command >= args.size() || args[command] != "fetch")
        {
            return args;
        }
        std::erase_if(args,
                      [](const std::string& arg)
                      {
                          return arg == "--prune" || arg == "--no-prune" || arg == "-p";
                      });
        args.insert(args.begin() + static_cast<std::ptrdiff_t>(command) + 1, prune ? "--prune" : "--no-prune");
        return args;
    }

    int parse_tenths(std::string_view text, int fallback, int low, int high)
    {
        // Digits, optionally a point and one more digit ("1", "1.2", "1.25" rounds down to 1.2).
        int whole = 0;
        int tenths = 0;
        std::size_t i = 0;
        bool digits = false;
        while (i < text.size() && text[i] >= '0' && text[i] <= '9' && whole < 1000)
        {
            whole = whole * 10 + (text[i++] - '0');
            digits = true;
        }
        if (i < text.size() && (text[i] == '.' || text[i] == ','))
        {
            ++i;
            if (i < text.size() && text[i] >= '0' && text[i] <= '9')
            {
                tenths = text[i++] - '0';
                digits = true;
            }
            while (i < text.size() && text[i] >= '0' && text[i] <= '9')
            {
                ++i;
            }
        }
        const int value = whole * 10 + tenths;
        if (!digits || i != text.size() || value < low || value > high)
        {
            return fallback;
        }
        return value;
    }

    std::vector<std::string> custom_tool_command(std::string_view command, const std::string& path)
    {
        std::vector<std::string> words;
        std::string word;
        bool quoted = false;
        bool in_word = false;
        for (const char c : command)
        {
            if (c == '"')
            {
                quoted = !quoted;
                in_word = true;
            }
            else if ((c == ' ' || c == '\t') && !quoted)
            {
                if (in_word)
                {
                    words.push_back(std::move(word));
                    word.clear();
                    in_word = false;
                }
            }
            else
            {
                word += c;
                in_word = true;
            }
        }
        if (in_word)
        {
            words.push_back(std::move(word));
        }
        bool placed = false;
        for (std::string& w : words)
        {
            for (std::size_t at = w.find("%path%"); at != std::string::npos; at = w.find("%path%", at + path.size()))
            {
                w.replace(at, 6, path);
                placed = true;
            }
        }
        if (!placed && !words.empty())
        {
            words.push_back(path);
        }
        return words;
    }
} // namespace imgit::app
