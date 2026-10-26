#include "app/theme.hpp"
#include <algorithm>
#include <cmath>
#include <iterator>

namespace imgit::theme
{
    namespace
    {
        constexpr ImVec4 rgb(unsigned hex, float alpha = 1.f) noexcept
        {
            return {static_cast<float>((hex >> 16) & 0xFF) / 255.f, static_cast<float>((hex >> 8) & 0xFF) / 255.f,
                    static_cast<float>(hex & 0xFF) / 255.f, alpha};
        }

        struct Hsl
        {
            float h, s, l;
        };

        Hsl to_hsl(ImVec4 c) noexcept
        {
            const float max = std::max({c.x, c.y, c.z});
            const float min = std::min({c.x, c.y, c.z});
            const float l = (max + min) / 2.f;
            if (max == min)
            {
                return {0.f, 0.f, l};
            }
            const float d = max - min;
            const float s = l > 0.5f ? d / (2.f - max - min) : d / (max + min);
            float h;
            if (max == c.x)
            {
                h = (c.y - c.z) / d + (c.y < c.z ? 6.f : 0.f);
            }
            else if (max == c.y)
            {
                h = (c.z - c.x) / d + 2.f;
            }
            else
            {
                h = (c.x - c.y) / d + 4.f;
            }
            return {h / 6.f, s, l};
        }

        float hue_to_rgb(float p, float q, float t) noexcept
        {
            if (t < 0.f)
            {
                t += 1.f;
            }
            if (t > 1.f)
            {
                t -= 1.f;
            }
            if (t < 1.f / 6.f)
            {
                return p + (q - p) * 6.f * t;
            }
            if (t < 1.f / 2.f)
            {
                return q;
            }
            if (t < 2.f / 3.f)
            {
                return p + (q - p) * (2.f / 3.f - t) * 6.f;
            }
            return p;
        }

        ImVec4 from_hsl(Hsl c, float alpha) noexcept
        {
            if (c.s == 0.f)
            {
                return {c.l, c.l, c.l, alpha};
            }
            const float q = c.l < 0.5f ? c.l * (1.f + c.s) : c.l + c.s - c.l * c.s;
            const float p = 2.f * c.l - q;
            return {hue_to_rgb(p, q, c.h + 1.f / 3.f), hue_to_rgb(p, q, c.h), hue_to_rgb(p, q, c.h - 1.f / 3.f), alpha};
        }

        // LESS lighten()/saturate(): absolute lightness / saturation offsets in HSL space.
        ImVec4 lighten(ImVec4 c, float amount) noexcept
        {
            Hsl hsl = to_hsl(c);
            hsl.l = std::clamp(hsl.l + amount, 0.f, 1.f);
            return from_hsl(hsl, c.w);
        }

        // What a theme changes: its background (the rest of the surfaces are lighter steps of it) and its accent. A
        // theme with `exact` replaces the derived surfaces and text colours with its own afterwards.
        struct ThemeSpec
        {
            const char* name;
            unsigned background; // app__bg0
            unsigned accent;
            unsigned link;
            unsigned text_accent; // counts, highlighted numbers
            unsigned avatar; // the default avatar disc
            unsigned prompt; // the terminal's repository name
            int first_lane; // the lane colour the graph starts with, so its first lane matches the accent
            void (*exact)(Palette&) = nullptr;
        };

        // The colours of Claude Code Desktop's dark theme, as it draws them: the window, its darker sidebar, the
        // neutral grey hover and selection rows, its text ramp and the diff backgrounds of its code preview.
        void claude_code_palette(Palette& p) noexcept
        {
            p.blue = rgb(0x2776D0); // the toggles

            // The chrome is as dark as the content: the title bar and sidebar one step darker than the window,
            // the toolbar the window itself, and only inputs, popovers and dropdowns a step lighter.
            p.app_bg0 = rgb(0x151515);
            p.toolbar_bg0 = rgb(0x101010); // the tabs bar
            p.toolbar_bg1 = rgb(0x151515); // the toolbar
            p.toolbar_bg2 = rgb(0x1F1F1F); // popovers
            p.panel_bg0 = rgb(0x101010); // the sidebar
            p.panel_bg1 = rgb(0x1A1A18); // the preferences sidebar, popups, toasts
            p.panel_bg2 = rgb(0x1F1F1F); // dropdowns, segmented controls
            p.input_bg = rgb(0x1F1F1F); // the prompt box
            p.panel_border = rgb(0x2F2F2F);
            p.section_border = rgb(0x2C2D2A);
            p.subtle_border = rgb(0x222222);

            p.text_selected = rgb(0xEEEEEC);
            p.text_normal = rgb(0xC2C3C0);
            p.text_secondary = rgb(0x8B8B89);
            p.text_disabled = rgb(0x626260);
            p.text_dimmed = rgb(0x414141);

            p.hover_row = rgb(0x2F2F2F);
            p.selected_row = rgb(0x414141);
            p.added_line = rgb(0x253B24);
            p.deleted_line = rgb(0x452524);
            p.hunk_header = rgb(0x1A1A18);
            p.scroll_thumb = rgb(0x464646);
            p.toggle_border = rgb(0x464644);
            p.terminal_bg = rgb(0x101010);
            p.terminal_fg = rgb(0xC2C3C0);
        }

        // Monaco's token colours: vs-dark on a dark background, vs on a light one.
        void syntax_palette(Palette& p) noexcept
        {
            if (to_hsl(p.app_bg0).l > 0.5f)
            {
                p.syntax_comment = rgb(0x008000);
                p.syntax_string = rgb(0xA31515);
                p.syntax_number = rgb(0x098658);
                p.syntax_keyword = rgb(0x0000FF);
                p.syntax_type = rgb(0x267F99);
                p.syntax_function = rgb(0x795E26);
                p.syntax_variable = rgb(0x001080);
                p.syntax_operator = rgb(0x000000);
                p.syntax_punctuation = rgb(0x383838);
                p.syntax_tag = rgb(0x800000);
                p.syntax_attribute = rgb(0xE50000);
                p.syntax_regexp = rgb(0x811F3F);
                p.syntax_constant = rgb(0x0000FF);
                return;
            }
            p.syntax_comment = rgb(0x6A9955);
            p.syntax_string = rgb(0xCE9178);
            p.syntax_number = rgb(0xB5CEA8);
            p.syntax_keyword = rgb(0x569CD6);
            p.syntax_type = rgb(0x4EC9B0);
            p.syntax_function = rgb(0xDCDCAA);
            p.syntax_variable = rgb(0x9CDCFE);
            p.syntax_operator = rgb(0xD4D4D4);
            p.syntax_punctuation = rgb(0xD4D4D4);
            p.syntax_tag = rgb(0x569CD6);
            p.syntax_attribute = rgb(0x9CDCFE);
            p.syntax_regexp = rgb(0xD16969);
            p.syntax_constant = rgb(0x569CD6);
        }

        constexpr ThemeSpec THEMES[] = {
                {"ImGit Dark", 0x1E1C1A, 0xF28C28, 0xFFAE5C, 0xFFBE7D, 0xC2661D, 0xFFAE5C, 0},
                {"Ocean", 0x161D26, 0x3A9DE8, 0x6CC4F5, 0x9CCBF2, 0x2A6FA8, 0x6CC4F5, 5},
                {"Midnight Teal", 0x121518, 0x1FB5A3, 0x4FD8C8, 0x86E0D5, 0x167F73, 0x4FD8C8, 4},
                {"Amethyst", 0x1D1B24, 0x9B6BFF, 0xC59BFF, 0xC6AEF7, 0x6B46B5, 0xC59BFF, 7},
                {"Rose", 0x211C1F, 0xE5486C, 0xFF8FA8, 0xF7A8B9, 0xAD3450, 0xFF8FA8, 9},
                // Claude Code Desktop's dark theme: its terracotta accent over its own greys (claude_code_palette).
                {"Claude Code", 0x151515, 0xD97757, 0xD97757, 0xD97757, 0x444444, 0xD97757, 0, claude_code_palette},
        };

        constexpr const char* THEME_NAMES[] = {THEMES[0].name, THEMES[1].name, THEMES[2].name, THEMES[3].name,
                                               THEMES[4].name, THEMES[5].name};
        static_assert(std::size(THEME_NAMES) == std::size(THEMES));

        Palette build_palette(const ThemeSpec& theme) noexcept
        {
            Palette p;
            p.red = rgb(0xE5534B);
            p.orange = rgb(0xE8A23A);
            p.yellow = rgb(0xE8C547);
            p.green = rgb(0x46B864);
            p.teal = rgb(0x2DBE9C);
            p.blue = rgb(0x4C8DF0);
            p.ltblue = rgb(0x4FB3E0);
            p.purple = rgb(0xB05CF0);
            p.accent = rgb(theme.accent);

            p.app_bg0 = rgb(theme.background);
            // The surfaces are lighter steps of the background.
            p.toolbar_bg0 = lighten(p.app_bg0, 0.06f);
            p.toolbar_bg1 = lighten(p.toolbar_bg0, 0.04f);
            p.toolbar_bg2 = lighten(p.toolbar_bg1, 0.06f);
            p.panel_bg0 = lighten(p.app_bg0, 0.05f);
            p.panel_bg1 = lighten(p.panel_bg0, 0.05f);
            p.panel_bg2 = lighten(p.panel_bg1, 0.05f);
            p.input_bg = rgb(0x000000, 0.20f);
            p.panel_border = rgb(0xFFFFFF, 0.08f);
            p.section_border = rgb(0xFFFFFF, 0.08f);
            p.subtle_border = rgb(0xFFFFFF, 0.04f);
            p.modal_overlay = rgb(0x000000, 0.5f);

            // The lane colours, round the colour wheel from orange; a theme starts at its accent's.
            const unsigned graph[GRAPH_COLOR_COUNT] = {0xF28C28, 0xF2C230, 0xB9D23A, 0x52C45A, 0x1FB5A3,
                                                       0x2BA6E0, 0x5B7CFA, 0xA85CF0, 0xD9479F, 0xE5486C};
            for (int i = 0; i < GRAPH_COLOR_COUNT; ++i)
            {
                p.graph_colors[i] = rgb(graph[(i + theme.first_lane) % GRAPH_COLOR_COUNT]);
            }
            p.stats_added = rgb(0x347d39);
            p.stats_deleted = rgb(0xc93c37);
            p.stats_files = rgb(0xe2c08d);

            p.text_selected = rgb(0xFFFFFF);
            p.text_normal = rgb(0xFFFFFF, 0.75f);
            p.text_secondary = rgb(0xFFFFFF, 0.60f);
            p.text_disabled = rgb(0xFFFFFF, 0.40f);
            p.text_accent = rgb(theme.text_accent);
            p.text_inverse = rgb(0x222222);
            p.text_dimmed = rgb(0xFFFFFF, 0.75f * 0.20f);

            p.hover_row = with_alpha(p.accent, 0.10f);
            p.selected_row = with_alpha(p.accent, 0.20f);
            p.link = rgb(theme.link);
            p.wip_status = with_alpha(p.accent, 0.40f);
            p.checked_out = with_alpha(p.green, 0.30f);
            p.added_line = with_alpha(p.green, 0.20f);
            p.deleted_line = with_alpha(p.red, 0.20f);
            p.hunk_header = rgb(0x000000, 0.25f);
            p.avatar_default = rgb(theme.avatar);
            p.scroll_thumb = rgb(0xFFFFFF, 0.15f);
            p.toggle_border = rgb(0xFFFFFF, 0.20f);
            p.filter_match = with_alpha(p.accent, 0.50f);
            p.droppable = with_alpha(p.yellow, 0.30f);
            p.drop_target = with_alpha(p.green, 0.50f);
            p.terminal_bg = lighten(p.app_bg0, -0.04f);
            p.terminal_fg = rgb(0xDCDCDC);
            p.terminal_prompt = rgb(theme.prompt);
            p.terminal_red = rgb(0xF0716A);
            if (theme.exact != nullptr)
            {
                theme.exact(p);
            }
            syntax_palette(p);
            return p;
        }
    } // namespace

    namespace
    {
        const ThemeSpec* g_theme = &THEMES[0];
        Palette g_palette = build_palette(THEMES[0]);
    } // namespace

    const Palette& palette() noexcept
    {
        return g_palette;
    }

    std::span<const char* const> theme_names() noexcept
    {
        return THEME_NAMES;
    }

    void set_theme(std::string_view name)
    {
        g_theme = &THEMES[0];
        for (const ThemeSpec& theme : THEMES)
        {
            if (name == theme.name)
            {
                g_theme = &theme;
            }
        }
        g_palette = build_palette(*g_theme);
    }

    const char* current_theme() noexcept
    {
        return g_theme->name;
    }

    ImVec4 with_alpha(ImVec4 color, float alpha) noexcept
    {
        color.w = alpha;
        return color;
    }

    ImVec4 mix(ImVec4 color, ImVec4 background, float weight) noexcept
    {
        return {color.x * weight + background.x * (1.f - weight), color.y * weight + background.y * (1.f - weight),
                color.z * weight + background.z * (1.f - weight), 1.f};
    }

    ImU32 u32(ImVec4 color) noexcept
    {
        return ImGui::ColorConvertFloat4ToU32(color);
    }

    ImVec4 graph_color(int column) noexcept
    {
        const int index = column % GRAPH_COLOR_COUNT;
        return palette().graph_colors[index < 0 ? index + GRAPH_COLOR_COUNT : index];
    }

    void apply_imgui_style()
    {
        const Palette& p = palette();
        ImGuiStyle& style = ImGui::GetStyle();

        style.WindowPadding = {0.f, 0.f};
        style.WindowRounding = 0.f;
        style.WindowBorderSize = 0.f;
        style.ChildRounding = 0.f;
        style.ChildBorderSize = 0.f;
        style.FramePadding = {8.f, 5.f};
        style.FrameRounding = 3.f; // --input-radius
        style.FrameBorderSize = 0.f;
        style.ItemSpacing = {6.f, 4.f};
        style.ItemInnerSpacing = {4.f, 4.f};
        style.ScrollbarSize = 10.f;
        style.ScrollbarRounding = 4.f;
        style.GrabRounding = 3.f;
        style.PopupRounding = 4.f;
        style.PopupBorderSize = 1.f;
        style.TabRounding = 0.f;
        style.WindowTitleAlign = {0.5f, 0.5f};
        style.SelectableTextAlign = {0.f, 0.5f};
        style.CellPadding = {6.f, 3.f};
        style.AntiAliasedLinesUseTex = false; // the font atlas is sampled with GL_NEAREST (see main.cpp)

        ImVec4* c = style.Colors;
        c[ImGuiCol_Text] = p.text_normal;
        c[ImGuiCol_TextDisabled] = p.text_disabled;
        c[ImGuiCol_WindowBg] = p.app_bg0;
        c[ImGuiCol_ChildBg] = {0.f, 0.f, 0.f, 0.f};
        c[ImGuiCol_PopupBg] = p.panel_bg1;
        c[ImGuiCol_Border] = p.panel_border;
        c[ImGuiCol_BorderShadow] = {0.f, 0.f, 0.f, 0.f};
        c[ImGuiCol_FrameBg] = p.input_bg;
        c[ImGuiCol_FrameBgHovered] = rgb(0x000000, 0.30f);
        c[ImGuiCol_FrameBgActive] = rgb(0x000000, 0.35f);
        c[ImGuiCol_TitleBg] = p.panel_bg0;
        c[ImGuiCol_TitleBgActive] = p.panel_bg0;
        c[ImGuiCol_TitleBgCollapsed] = p.panel_bg0;
        c[ImGuiCol_MenuBarBg] = p.toolbar_bg0;
        c[ImGuiCol_ScrollbarBg] = {0.f, 0.f, 0.f, 0.f};
        c[ImGuiCol_ScrollbarGrab] = p.scroll_thumb;
        c[ImGuiCol_ScrollbarGrabHovered] = rgb(0xFFFFFF, 0.25f);
        c[ImGuiCol_ScrollbarGrabActive] = rgb(0xFFFFFF, 0.35f);
        c[ImGuiCol_CheckMark] = p.text_inverse;
        c[ImGuiCol_SliderGrab] = p.accent;
        c[ImGuiCol_SliderGrabActive] = p.accent;
        c[ImGuiCol_Button] = {0.f, 0.f, 0.f, 0.f};
        c[ImGuiCol_ButtonHovered] = p.hover_row;
        c[ImGuiCol_ButtonActive] = p.selected_row;
        c[ImGuiCol_Header] = p.selected_row;
        c[ImGuiCol_HeaderHovered] = p.hover_row;
        c[ImGuiCol_HeaderActive] = p.selected_row;
        c[ImGuiCol_Separator] = p.section_border;
        c[ImGuiCol_SeparatorHovered] = p.accent;
        c[ImGuiCol_SeparatorActive] = p.accent;
        c[ImGuiCol_ResizeGrip] = {0.f, 0.f, 0.f, 0.f};
        c[ImGuiCol_ResizeGripHovered] = p.hover_row;
        c[ImGuiCol_ResizeGripActive] = p.selected_row;
        c[ImGuiCol_Tab] = {0.f, 0.f, 0.f, 0.f};
        c[ImGuiCol_TabHovered] = p.hover_row;
        c[ImGuiCol_TabSelected] = p.app_bg0;
        c[ImGuiCol_TextSelectedBg] = with_alpha(p.accent, 0.35f);
        c[ImGuiCol_NavCursor] = {0.f, 0.f, 0.f, 0.f};
        c[ImGuiCol_ModalWindowDimBg] = p.modal_overlay;
    }
} // namespace imgit::theme
