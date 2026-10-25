#include "ui/profile_widgets.hpp"
#include "app/application.hpp"
#include "app/theme.hpp"
#include "git/types.hpp"
#include "ui/avatars.hpp"
#include "ui/popover.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>

namespace imgit::ui
{
    namespace
    {
        constexpr float OPTION_HEIGHT = 45.f; // 8px padding, the 28px icon, 8px, a 1px bottom border
        constexpr float MAX_LIST_HEIGHT = 270.f;

        // 14px text in a 20px line box whose top is `line_top` (device px), placed like Chromium.
        void line_text(ImDrawList* draw, float x, float line_top, ImU32 color, const std::string& text, float max_width)
        {
            const float content = std::round(14.f * 2189.f / 2048.f) + std::round(14.f * 600.f / 2048.f);
            const float top = line_top + std::floor((20.f - content) / 2.f) * dpi_scale();
            const float ascent = 14.f * 2189.f / 2048.f * dpi_scale();
            const float baseline = std::round(top + std::round(ascent));
            draw_text_ellipsis(draw, fonts().regular, 14.f, ImVec2(x, baseline - std::ceil(ascent)), max_width, color,
                               text.c_str());
        }
    } // namespace

    const ProfileMenuStyle POPOVER_PROFILE_MENU{theme::palette().toolbar_bg0, ImVec4(1.f, 1.f, 1.f, 0.2f),
                                                ImVec4(1.f, 1.f, 1.f, 0.9f)};
    const ProfileMenuStyle SIDEBAR_PROFILE_MENU{theme::palette().app_bg0, ImVec4(1.f, 1.f, 1.f, 0.08f),
                                                ImVec4(1.f, 1.f, 1.f, 0.75f)};

    void draw_profile_icon(ImDrawList* draw, ImVec2 min, float size, const app::Profile& profile)
    {
        draw_avatar(draw, ImVec2(min.x + size / 2.f, min.y + size / 2.f), size,
                    git::Author{profile.user_name, profile.user_email}, px(2.f));
    }

    void draw_select_focus(ImDrawList* draw, ImVec2 min, ImVec2 max, float rounding)
    {
        draw->AddRect(ImVec2(min.x - px(0.5f), min.y - px(0.5f)), ImVec2(max.x + px(0.5f), max.y + px(0.5f)),
                      theme::u32(theme::palette().accent), rounding + px(0.5f), 0, px(1.f));
    }

    std::string profile_select_menu(app::Application& app, const char* id, ImVec2 control_min, ImVec2 control_max,
                                    const ProfileMenuStyle& style)
    {
        if (!ImGui::IsPopupOpen(id))
        {
            return {};
        }
        const std::vector<app::Profile>& profiles = app.profiles.all();
        const float content = OPTION_HEIGHT * static_cast<float>(profiles.size());
        const float list_h = std::min(MAX_LIST_HEIGHT, 4.f + content + 4.f + 2.f);
        const ImVec2 min(control_min.x, control_max.y + px(8.f));
        const ImVec2 max(control_max.x, min.y + px(list_h));
        ImGui::SetNextWindowPos(min);
        ImGui::SetNextWindowSize(ImVec2(max.x - min.x, max.y - min.y));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_PopupBorderSize, 0.f);
        ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(0.f, 0.f));
        ImGui::PushStyleColor(ImGuiCol_PopupBg, ImVec4(0.f, 0.f, 0.f, 0.f));
        const bool open = ImGui::BeginPopup(id, ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoScrollbar
                                                        | ImGuiWindowFlags_NoScrollWithMouse);
        ImGui::PopStyleColor();
        ImGui::PopStyleVar(3);
        if (!open)
        {
            return {};
        }
        // The option the keyboard / mouse is on starts at the current profile.
        static int focused = 0;
        static float scroll = 0.f;
        if (ImGui::IsWindowAppearing())
        {
            scroll = 0.f;
            focused = 0;
            for (std::size_t i = 0; i < profiles.size(); ++i)
            {
                if (profiles[i].guid == app.profiles.current().guid)
                {
                    focused = static_cast<int>(i);
                }
            }
        }
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->PushClipRectFullScreen();
        // 0 0 0 1px and 0 4px 11px rgba(0, 0, 0, .1)
        draw_box_shadow(draw, min, max, ImVec2(0.f, 0.f), 0.f, px(1.f), ImVec4(0.f, 0.f, 0.f, 0.1f), px(4.f));
        draw_box_shadow(draw, min, max, ImVec2(0.f, px(4.f)), px(11.f), 0.f, ImVec4(0.f, 0.f, 0.f, 0.1f), px(4.f));
        draw->AddRectFilled(min, max, theme::u32(style.background), px(4.f));
        draw->AddRect(min, max, theme::u32(style.border), px(4.f), 0, px(1.f));
        draw->PopClipRect();

        const ImVec2 list_min(min.x + px(1.f), min.y + px(5.f));
        const ImVec2 list_max(max.x - px(1.f), max.y - px(5.f));
        const float max_scroll = std::max(0.f, px(content) - (list_max.y - list_min.y));
        if (ImGui::IsWindowHovered())
        {
            scroll = std::clamp(scroll - ImGui::GetIO().MouseWheel * px(OPTION_HEIGHT), 0.f, max_scroll);
        }
        if (ImGui::IsKeyPressed(ImGuiKey_DownArrow))
        {
            focused = std::min(focused + 1, static_cast<int>(profiles.size()) - 1);
        }
        if (ImGui::IsKeyPressed(ImGuiKey_UpArrow))
        {
            focused = std::max(focused - 1, 0);
        }
        std::string picked;
        if (ImGui::IsKeyPressed(ImGuiKey_Enter) && focused < static_cast<int>(profiles.size()))
        {
            picked = profiles[static_cast<std::size_t>(focused)].guid;
        }
        if (ImGui::IsKeyPressed(ImGuiKey_Escape))
        {
            ImGui::CloseCurrentPopup();
        }
        draw->PushClipRect(list_min, list_max, true);
        for (std::size_t i = 0; i < profiles.size(); ++i)
        {
            const app::Profile& profile = profiles[i];
            const ImVec2 row_min(list_min.x, list_min.y + px(OPTION_HEIGHT) * static_cast<float>(i) - scroll);
            const ImVec2 row_max(list_max.x, row_min.y + px(OPTION_HEIGHT));
            if (row_max.y <= list_min.y || row_min.y >= list_max.y)
            {
                continue;
            }
            ImGui::SetCursorScreenPos(row_min);
            ImGui::PushID(static_cast<int>(i));
            if (ImGui::InvisibleButton("##option", ImVec2(row_max.x - row_min.x, row_max.y - row_min.y)))
            {
                picked = profile.guid;
            }
            ImGui::PopID();
            if (ImGui::IsItemHovered() && ImGui::IsMouseHoveringRect(list_min, list_max))
            {
                focused = static_cast<int>(i);
            }
            if (focused == static_cast<int>(i))
            {
                draw->AddRectFilled(row_min, ImVec2(row_max.x, row_max.y - px(1.f)),
                                    theme::u32(theme::palette().hover_row));
            }
            draw->AddRectFilled(ImVec2(row_min.x, row_max.y - px(1.f)), row_max, theme::u32(style.border));
            const ImVec2 icon(row_min.x + px(12.f), row_min.y + px(8.f));
            draw_profile_icon(draw, icon, px(28.f), profile);
            const float text_x = icon.x + px(38.f);
            line_text(draw, text_x, row_min.y + px(12.f), theme::u32(style.text), profile.name,
                      row_max.x - px(12.f) - text_x);
        }
        draw->PopClipRect();
        if (!picked.empty())
        {
            ImGui::CloseCurrentPopup();
        }
        ImGui::EndPopup();
        return picked;
    }
} // namespace imgit::ui
