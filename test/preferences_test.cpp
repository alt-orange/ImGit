#include "app/date_format.hpp"
#include "app/preferences.hpp"
#include <filesystem>
#include <fstream>
#include <gtest/gtest.h>

namespace
{
    using imgit::app::custom_tool_command;
    using imgit::app::locale_code;
    using imgit::app::moment_format;
    using imgit::app::parse_tenths;
    using imgit::app::Preference;
    using imgit::app::Preferences;
    using imgit::app::ssh_command;
    using imgit::app::with_prune_preference;

    // Thursday, March 5, 2026 14:07:09
    std::tm sample()
    {
        std::tm t{};
        t.tm_year = 126;
        t.tm_mon = 2;
        t.tm_mday = 5;
        t.tm_wday = 4;
        t.tm_hour = 14;
        t.tm_min = 7;
        t.tm_sec = 9;
        return t;
    }

    TEST(MomentFormat, Defaults)
    {
        EXPECT_EQ(moment_format("MM/DD/YYYY", sample()), "03/05/2026");
        EXPECT_EQ(moment_format("MMMM D, YYYY", sample()), "March 5, 2026");
        EXPECT_EQ(moment_format("MM/DD/YYYY @ h:mm A", sample()), "03/05/2026 @ 2:07 PM");
        EXPECT_EQ(moment_format("MMMM D, YYYY h:mm A", sample()), "March 5, 2026 2:07 PM");
    }

    TEST(MomentFormat, Tokens)
    {
        EXPECT_EQ(moment_format("YY-M-D H:m:s", sample()), "26-3-5 14:7:9");
        EXPECT_EQ(moment_format("dddd, MMM Do HH:mm:ss a", sample()), "Thursday, Mar 5th 14:07:09 pm");
        EXPECT_EQ(moment_format("hh A", sample()), "02 PM");
        EXPECT_EQ(moment_format("[Today is] dddd", sample()), "Today is Thursday");
        std::tm midnight = sample();
        midnight.tm_hour = 0;
        EXPECT_EQ(moment_format("h:mm a", midnight), "12:07 am");
    }

    TEST(MomentFormat, Locales)
    {
        EXPECT_EQ(moment_format("D MMMM YYYY", sample(), "fr"), "5 mars 2026");
        EXPECT_EQ(moment_format("dddd D. MMMM", sample(), "de"), "Donnerstag 5. März");
        EXPECT_EQ(moment_format("MMMM", sample(), "en-gb"), "March");
        EXPECT_EQ(moment_format("MMMM", sample(), "xx"), "March"); // unknown: English
        EXPECT_EQ(moment_format("Do", sample(), "es"), "5.");
    }

    TEST(LocaleCode, Options)
    {
        EXPECT_EQ(locale_code("French [fr]", "de-de"), "fr");
        EXPECT_EQ(locale_code("English (United Kingdom) [en-gb]", ""), "en-gb");
        EXPECT_EQ(locale_code("System [system]", "de_DE"), "de-de");
        EXPECT_EQ(locale_code("System [system]", ""), "en");
    }

    TEST(SshCommand, KeyWithoutAgent)
    {
        EXPECT_EQ(ssh_command("C:\\Users\\me\\.ssh\\id_ed25519", false),
                  "ssh -i 'C:/Users/me/.ssh/id_ed25519' -o IdentitiesOnly=yes");
        EXPECT_EQ(ssh_command("/home/o'neil/.ssh/key", false),
                  "ssh -i '/home/o'\\''neil/.ssh/key' -o IdentitiesOnly=yes");
    }

    TEST(SshCommand, AgentOrNoKey)
    {
        EXPECT_FALSE(ssh_command("/home/me/.ssh/key", true));
        EXPECT_FALSE(ssh_command("", false));
    }

    TEST(Prune, Fetch)
    {
        using Args = std::vector<std::string>;
        EXPECT_EQ(with_prune_preference({"fetch", "--all", "--prune"}, false), (Args{"fetch", "--no-prune", "--all"}));
        EXPECT_EQ(with_prune_preference({"fetch", "origin"}, true), (Args{"fetch", "--prune", "origin"}));
        EXPECT_EQ(with_prune_preference({"-c", "a=b", "fetch", "-p"}, true), (Args{"-c", "a=b", "fetch", "--prune"}));
        EXPECT_EQ(with_prune_preference({"push", "--prune"}, false), (Args{"push", "--prune"}));
    }

    TEST(Tenths, Parse)
    {
        EXPECT_EQ(parse_tenths("1.2", 12, 10, 30), 12);
        EXPECT_EQ(parse_tenths("1", 12, 10, 30), 10);
        EXPECT_EQ(parse_tenths("1.25", 12, 10, 30), 12);
        EXPECT_EQ(parse_tenths("2,5", 12, 10, 30), 25);
        EXPECT_EQ(parse_tenths("abc", 12, 10, 30), 12);
        EXPECT_EQ(parse_tenths("9", 12, 10, 30), 12);
        EXPECT_EQ(parse_tenths("", 12, 10, 30), 12);
    }

    TEST(CustomTool, Command)
    {
        using Args = std::vector<std::string>;
        EXPECT_EQ(custom_tool_command("subl -n", "/r/a b"), (Args{"subl", "-n", "/r/a b"}));
        EXPECT_EQ(custom_tool_command("\"C:\\Program Files\\Ed\\ed.exe\" --open=%path% -w", "C:/r"),
                  (Args{"C:\\Program Files\\Ed\\ed.exe", "--open=C:/r", "-w"}));
        EXPECT_TRUE(custom_tool_command("   ", "/r").empty());
    }

    TEST(PreferenceStore, RoundTrip)
    {
        constexpr Preference FLAG{"Test/Flag", "1"};
        constexpr Preference SIZE{"Test/Size", "12"};
        constexpr Preference TEXT{"Test/Text", ""};
        const auto path = std::filesystem::temp_directory_path() / "imgit_preferences_test.txt";
        std::filesystem::remove(path);
        {
            Preferences store;
            store.load(path.string());
            EXPECT_TRUE(store.flag(FLAG));
            EXPECT_EQ(store.number(SIZE, 6, 40), 12);
            store.set_flag(FLAG, false);
            store.set(SIZE, "99");
            store.set(TEXT, "a\tb\nc");
        }
        Preferences store;
        store.load(path.string());
        EXPECT_FALSE(store.flag(FLAG));
        EXPECT_EQ(store.number(SIZE, 6, 40), 12); // out of range: the fallback
        EXPECT_EQ(store.get(TEXT), "a b c");
        std::filesystem::remove(path);
    }
} // namespace
