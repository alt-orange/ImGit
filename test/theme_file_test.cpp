#include "app/theme_file.hpp"
#include <gtest/gtest.h>

namespace
{
    using imgit::theme::parse_color;
    using imgit::theme::parse_theme_file;

    TEST(ThemeColor, HexForms)
    {
        const auto six = parse_color("#FF8000");
        ASSERT_TRUE(six);
        EXPECT_FLOAT_EQ(six->r, 1.f);
        EXPECT_NEAR(six->g, 128.f / 255.f, 1e-6f);
        EXPECT_FLOAT_EQ(six->b, 0.f);
        EXPECT_FLOAT_EQ(six->a, 1.f);

        const auto three = parse_color("#f80");
        ASSERT_TRUE(three);
        EXPECT_NEAR(three->g, 136.f / 255.f, 1e-6f);

        const auto eight = parse_color(" #00000080 ");
        ASSERT_TRUE(eight);
        EXPECT_NEAR(eight->a, 128.f / 255.f, 1e-6f);
    }

    TEST(ThemeColor, RgbForms)
    {
        const auto rgb = parse_color("rgb(255, 0, 51)");
        ASSERT_TRUE(rgb);
        EXPECT_NEAR(rgb->b, 0.2f, 1e-6f);
        const auto rgba = parse_color("rgba(0,0,0,0.25)");
        ASSERT_TRUE(rgba);
        EXPECT_FLOAT_EQ(rgba->a, 0.25f);
    }

    TEST(ThemeColor, Rejects)
    {
        EXPECT_FALSE(parse_color("FF8000"));
        EXPECT_FALSE(parse_color("#FF800"));
        EXPECT_FALSE(parse_color("#GG0000"));
        EXPECT_FALSE(parse_color("rgb(1, 2)"));
        EXPECT_FALSE(parse_color("rgba(1, 2, 3)"));
        EXPECT_FALSE(parse_color("red"));
    }

    TEST(ThemeFile, Full)
    {
        const auto theme = parse_theme_file(R"json({
            "name": "Paper \u00e9",
            "base": "ImGit Light",
            "background": "#FAF7F0",
            "accent": "#2F7D6D",
            "comment": ["ignored", 1, true, null, {"nested": false}],
            "colors": {"text_normal": "#1F2328E0", "graph_3": "rgb(10, 20, 30)"}
        })json");
        ASSERT_TRUE(theme) << theme.error();
        EXPECT_EQ(theme->name, "Paper \xC3\xA9");
        EXPECT_EQ(theme->base, "ImGit Light");
        ASSERT_TRUE(theme->background);
        EXPECT_NEAR(theme->background->r, 0xFA / 255.f, 1e-6f);
        ASSERT_TRUE(theme->accent);
        EXPECT_FALSE(theme->link);
        ASSERT_EQ(theme->colors.size(), 2u);
        EXPECT_EQ(theme->colors[0].first, "text_normal");
        EXPECT_EQ(theme->colors[1].first, "graph_3");
    }

    TEST(ThemeFile, Errors)
    {
        EXPECT_FALSE(parse_theme_file(""));
        EXPECT_FALSE(parse_theme_file("[]"));
        EXPECT_FALSE(parse_theme_file(R"({"base": "ImGit Dark"})")); // no name
        EXPECT_FALSE(parse_theme_file(R"({"name": "  "})"));
        EXPECT_FALSE(parse_theme_file(R"({"name": "x", "accent": "blue"})"));
        EXPECT_FALSE(parse_theme_file(R"({"name": "x", "colors": {"accent": 3}})"));
        EXPECT_FALSE(parse_theme_file(R"({"name": "x",})"));
        EXPECT_FALSE(parse_theme_file(R"({"name": "x"} trailing)"));
        const auto unterminated = parse_theme_file("{\n\"name\": \"x");
        ASSERT_FALSE(unterminated);
        EXPECT_NE(unterminated.error().find("line 2"), std::string::npos);
    }

    TEST(ThemeFile, Minimal)
    {
        const auto theme = parse_theme_file(R"json({"name": "Mine"})json");
        ASSERT_TRUE(theme);
        EXPECT_EQ(theme->name, "Mine");
        EXPECT_TRUE(theme->base.empty());
        EXPECT_TRUE(theme->colors.empty());
    }
} // namespace
