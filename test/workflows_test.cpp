#include "app/workflows.hpp"
#include <gtest/gtest.h>

namespace
{
    namespace gitflow = imgit::app::gitflow;
    namespace cleanup = imgit::app::cleanup;
    using Runs = std::vector<std::vector<std::string>>;

    gitflow::Config config()
    {
        gitflow::Config c;
        c.master = "main";
        c.develop = "develop";
        c.version_tag = "v";
        return c;
    }

    TEST(Gitflow, Classify)
    {
        const auto feature = gitflow::classify(config(), "feature/login");
        ASSERT_TRUE(feature);
        EXPECT_EQ(feature->kind, gitflow::Kind::FEATURE);
        EXPECT_EQ(feature->name, "login");
        const auto hotfix = gitflow::classify(config(), "hotfix/1.0.1");
        ASSERT_TRUE(hotfix);
        EXPECT_EQ(hotfix->kind, gitflow::Kind::HOTFIX);
        EXPECT_FALSE(gitflow::classify(config(), "feature/"));
        EXPECT_FALSE(gitflow::classify(config(), "main"));
        EXPECT_FALSE(gitflow::classify(gitflow::Config{}, "feature/login")); // not initialized
    }

    TEST(Gitflow, Start)
    {
        EXPECT_EQ(gitflow::start_runs(config(), gitflow::Kind::FEATURE, "x"),
                  (Runs{{"checkout", "-b", "feature/x", "develop"}}));
        EXPECT_EQ(gitflow::start_runs(config(), gitflow::Kind::RELEASE, "1.0"),
                  (Runs{{"checkout", "-b", "release/1.0", "develop"}}));
        EXPECT_EQ(gitflow::start_runs(config(), gitflow::Kind::HOTFIX, "1.0.1"),
                  (Runs{{"checkout", "-b", "hotfix/1.0.1", "main"}}));
    }

    TEST(Gitflow, FinishFeature)
    {
        EXPECT_EQ(gitflow::finish_runs(config(), gitflow::Kind::FEATURE, "x", false),
                  (Runs{{"checkout", "develop"},
                        {"merge", "--no-ff", "--no-edit", "feature/x"},
                        {"branch", "-d", "feature/x"}}));
    }

    TEST(Gitflow, FinishRelease)
    {
        EXPECT_EQ(gitflow::finish_runs(config(), gitflow::Kind::RELEASE, "1.0", false),
                  (Runs{{"checkout", "main"},
                        {"merge", "--no-ff", "--no-edit", "release/1.0"},
                        {"tag", "-a", "v1.0", "-m", "v1.0"},
                        {"checkout", "develop"},
                        {"merge", "--no-ff", "--no-edit", "release/1.0"},
                        {"branch", "-d", "release/1.0"}}));
        // Finished again after a conflict: the tag is there already.
        EXPECT_EQ(gitflow::finish_runs(config(), gitflow::Kind::HOTFIX, "1.0.1", true),
                  (Runs{{"checkout", "main"},
                        {"merge", "--no-ff", "--no-edit", "hotfix/1.0.1"},
                        {"checkout", "develop"},
                        {"merge", "--no-ff", "--no-edit", "hotfix/1.0.1"},
                        {"branch", "-d", "hotfix/1.0.1"}}));
    }

    TEST(BranchCleanup, Candidates)
    {
        const std::vector<cleanup::BranchFacts> branches = {
                {"main", false, true, false}, {"develop", false, true, false}, {"current", true, true, true},
                {"zeta", false, true, false}, {"alpha", false, false, true},   {"wip", false, false, false},
                {"both", false, true, true},
        };
        const auto found = cleanup::candidates(branches, {"main", "develop"});
        ASSERT_EQ(found.size(), 3u);
        EXPECT_EQ(found[0].name, "alpha");
        EXPECT_EQ(cleanup::reason(found[0]), "upstream gone");
        EXPECT_EQ(found[1].name, "both");
        EXPECT_EQ(cleanup::reason(found[1]), "merged, upstream gone");
        EXPECT_EQ(found[2].name, "zeta");
        EXPECT_EQ(cleanup::reason(found[2]), "merged");
    }
} // namespace
