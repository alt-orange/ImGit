#include "app/application.hpp"
#include "app/theme.hpp"
#include "git/types.hpp"
#include "platform/shell.hpp"
#include "platform/window.hpp"
#include "ui/avatars.hpp"
#include "ui/popover.hpp"
#include "ui/panels.hpp"
#include "ui/profile_widgets.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <imgui.h>
#include <string>
#include <vector>

namespace imgit::ui
{
    namespace
    {

        // .tabs-bar .tab: padding 0 25px 0 20px, icon, 6px, Open Sans Semibold 14px. Selected and hovered tabs get
        // toolbar__bg1 with 4px top corners; the others end in a half-transparent 1px divider (80% of the height).
        constexpr float TAB_PADDING_LEFT = 20.f;
        constexpr float TAB_PADDING_RIGHT = 25.f;
        constexpr float TAB_ICON_GAP = 6.f;
        constexpr float TAB_FONT = 14.f;
        constexpr float TAB_ICON_EM = 12.f;

        int g_menu_tab = -1; // repository tab right-clicked this frame

        bool tab_item(ImDrawList* draw, float& x, float y, float height, const char* id, Icon icon,
                      const std::string& label, bool active, bool closable, int* close_request, int index,
                      const char* tip, bool divider, bool plain = false)
        {
            // `plain`: the "New Tab" entry, no icon and the regular 14px font.
            const theme::Palette& p = theme::palette();
            ImFont* font = plain ? fonts().regular : fonts().semibold;
            const float icon_w = plain ? 0.f : icon_width(icon, px(TAB_ICON_EM));
            const float gap = plain ? 0.f : px(TAB_ICON_GAP);
            const float text_w = std::min(text_size(font, TAB_FONT, label.c_str()).x, px(200.f));
            const float tab_width = px(TAB_PADDING_LEFT) + icon_w + gap + text_w + px(TAB_PADDING_RIGHT);

            ImGui::SetCursorScreenPos(ImVec2(x, y));
            ImGui::PushID(id);
            ImGui::PushID(index);
            bool clicked = false;
            if (closable)
            {
                ImGui::SetNextItemAllowOverlap();
            }
            ImGui::InvisibleButton("##tab", ImVec2(tab_width, height));
            // The close button overlapping the tab takes its hover; the tab still counts as hovered for its plate
            // and the close button itself, else the button would vanish under the mouse every other frame.
            const bool hovered = ImGui::IsItemHovered(ImGuiHoveredFlags_AllowWhenOverlappedByItem);
            const bool hovered_self = ImGui::IsItemHovered();
            if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
            {
                clicked = true;
            }
            if (closable && ImGui::IsItemClicked(ImGuiMouseButton_Middle) && close_request != nullptr)
            {
                *close_request = index;
            }
            if (closable && hovered && ImGui::IsMouseReleased(ImGuiMouseButton_Right))
            {
                g_menu_tab = index;
            }
            // transition: background-color .25s ease
            const float bg_t = smooth(("##tab_bg" + std::to_string(index) + id).c_str(), active || hovered, 0.25f);
            if (bg_t > 0.f)
            {
                draw->AddRectFilled(ImVec2(x, y), ImVec2(x + tab_width, y + height),
                                    theme::u32(theme::with_alpha(p.toolbar_bg1, bg_t)), px(4.f),
                                    ImDrawFlags_RoundCornersTop);
            }
            const float mid = y + height / 2.f;
            const float icon_x = x + px(TAB_PADDING_LEFT);
            if (!plain)
            {
                draw_icon(draw, icon, ImVec2(icon_x + icon_w / 2.f, mid), px(TAB_ICON_EM),
                          theme::u32(active ? theme::with_alpha(p.white, 0.35f) : p.text_secondary));
            }
            const float line_h = text_size(font, TAB_FONT, "A").y;
            draw_text_ellipsis(draw, font, TAB_FONT, ImVec2(icon_x + icon_w + gap, mid - line_h / 2.f), text_w + 1.f,
                               theme::u32(active ? p.text_selected : p.text_secondary), label.c_str());
            if (closable && (active || hovered))
            {
                const float close_x = x + tab_width - px(13.f);
                const float box = px(16.f);
                ImGui::SetCursorScreenPos(ImVec2(close_x - box / 2.f, mid - box / 2.f));
                ImGui::InvisibleButton("##close", ImVec2(box, box));
                const bool close_hovered = ImGui::IsItemHovered();
                draw_icon(draw, Icon::CLOSE, ImVec2(close_x, mid), px(14.f),
                          theme::u32(close_hovered ? p.text_selected : p.text_secondary));
                if (close_hovered)
                {
                    tooltip("Close tab");
                }
                if (ImGui::IsItemClicked(ImGuiMouseButton_Left) && close_request != nullptr)
                {
                    *close_request = index;
                }
            }
            if (hovered_self && tip != nullptr)
            {
                tooltip(tip);
            }
            ImGui::PopID();
            ImGui::PopID();
            x += tab_width;
            if (divider && !active)
            {
                draw->AddLine(ImVec2(x - 0.5f, y + height * 0.1f), ImVec2(x - 0.5f, y + height * 0.9f),
                              theme::u32(theme::with_alpha(p.section_border, p.section_border.w * 0.5f)), 1.f);
            }
            return clicked;
        }

        // A bare icon button of the tabs bar (folder, +, tab list, bell, cog).
        // `active`: its popover is open (a 20% #4D88FF plate, rad2).
        bool bar_icon(const char* id, Icon icon, ImVec2 center, float em, ImVec4 color, const char* tip,
                      bool active = false)
        {
            const float box = px(28.f);
            const ImVec2 min(center.x - box / 2.f, center.y - box / 2.f);
            ImGui::SetCursorScreenPos(min);
            const bool pressed = ImGui::InvisibleButton(id, ImVec2(box, box));
            const bool hovered = ImGui::IsItemHovered();
            if (active || hovered)
            {
                // hover-row: 10% #4D88FF; the open popover's button: 20%.
                ImGui::GetWindowDrawList()->AddRectFilled(
                        min, ImVec2(min.x + box, min.y + box),
                        theme::u32(active ? theme::palette().selected_row : theme::palette().hover_row), px(2.f));
            }
            draw_icon(ImGui::GetWindowDrawList(), icon, center, px(em),
                      theme::u32(hovered ? theme::palette().text_selected : color));
            if (hovered && tip != nullptr && !active)
            {
                tooltip(tip);
            }
            return pressed;
        }

    } // namespace

    void draw_tabs_bar(app::Application& app)
    {
        const theme::Palette& p = theme::palette();
        const float height = px(theme::TABS_BAR_HEIGHT);
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        const float width = ImGui::GetContentRegionAvail().x;
        const float mid = origin.y + height / 2.f;

        ImGui::BeginChild("##tabs_bar", ImVec2(width, height), ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(origin, ImVec2(origin.x + width, origin.y + height), theme::u32(p.toolbar_bg0));

        // Left: the window's own controls where the bar is the title bar (macOS), open-repo folder, repository
        // tabs, +
        const float left = origin.x + px(platform::title_bar_inset());
        if (bar_icon("##open_repo", Icon::FOLDER, ImVec2(left + px(21.f), mid), 14.f, p.text_secondary,
                     "Repository Management", app.repo_management_open))
        {
            app.repo_management_open = !app.repo_management_open;
        }
        float x = left + px(41.f);
        int close_request = -1;
        const int tab_count = static_cast<int>(app.tabs.size());
        for (int i = 0; i < tab_count; ++i)
        {
            const app::RepoState& state = *app.tabs[static_cast<std::size_t>(i)];
            if (tab_item(draw, x, origin.y, height, "repo", Icon::BRANCH, state.repo.name(), i == app.active_tab, true,
                         &close_request, i, state.repo.workdir().c_str(), app.active_tab != i + 1))
            {
                app.active_tab = i;
            }
        }
        if (app.new_tab_open || tab_count == 0)
        {
            int close_new = -1;
            if (tab_item(draw, x, origin.y, height, "new_tab", Icon::PLUS, "New Tab", app.active_tab == -1, true,
                         &close_new, 0, nullptr, true, true))
            {
                app.active_tab = -1;
            }
            // The New Tab page is all there is without repositories, so it stays open then.
            if (close_new >= 0 && tab_count > 0)
            {
                app.new_tab_open = false;
                if (app.active_tab == -1)
                {
                    app.active_tab = tab_count - 1;
                }
            }
        }
        if (bar_icon("##new_tab", Icon::PLUS, ImVec2(x + px(21.5f), mid), 14.f, p.text_secondary, "New Tab (Ctrl+T)"))
        {
            app.new_tab_open = true;
            app.active_tab = -1;
        }

        // Right: tabs list, notifications, preferences, profile.
        const ImVec4 bright = theme::with_alpha(p.white, 0.9f);
        float right = origin.x + width;
        {
            const app::Profile& current = app.profiles.current();
            const char* profile = current.name.c_str();
            ImFont* font = fonts().regular;
            const float text_w = text_size(font, theme::FONT_SIZE_SMALL, profile).x;
            const float caret_w = icon_width(Icon::CARET_DOWN, px(12.f));
            const float avatar = px(20.f);
            const float w = avatar + px(6.f) + text_w + px(5.f) + caret_w;
            const float x0 = right - px(14.f) - w;
            ImGui::SetCursorScreenPos(ImVec2(x0, origin.y + px(4.f)));
            ImGui::InvisibleButton("##profile", ImVec2(w, height - px(8.f)));
            const bool hovered = ImGui::IsItemHovered();
            if (ImGui::IsPopupOpen("##profile_menu") || hovered)
            {
                draw->AddRectFilled(ImVec2(x0 - px(3.f), origin.y + px(3.f)),
                                    ImVec2(x0 + w + px(3.f), origin.y + px(31.f)),
                                    theme::u32(theme::with_alpha(theme::palette().accent,
                                                      ImGui::IsPopupOpen("##profile_menu") ? 0.2f : 0.1f)),
                                    px(2.f));
            }
            const ImU32 color = theme::u32(hovered ? p.text_selected : bright);
            draw_avatar(draw, ImVec2(x0 + avatar / 2.f, mid), avatar, git::Author{current.user_name, current.user_email},
                        px(3.f));
            const float line_h = text_size(font, theme::FONT_SIZE_SMALL, "A").y;
            draw_text(draw, font, theme::FONT_SIZE_SMALL, ImVec2(x0 + avatar + px(6.f), mid - line_h / 2.f), color,
                      profile);
            draw_icon(draw, Icon::CARET_DOWN, ImVec2(x0 + w - caret_w / 2.f, mid), px(12.f), color);
            if (ImGui::IsItemClicked(ImGuiMouseButton_Left))
            {
                open_popover("##profile_menu");
            }
            draw_profile_popover(app, ImVec2(x0 + w, origin.y + px(31.f)));
            right = x0;
        }
        if (bar_icon("##prefs", Icon::GEAR, ImVec2(right - px(24.5f), mid), 16.f, bright, "Preferences"))
        {
            app.preferences_open = true;
        }
        if (bar_icon("##notifications", Icon::BELL, ImVec2(right - px(57.f), mid), 16.f, bright, "Notifications",
                     ImGui::IsPopupOpen("##notifications")))
        {
            open_popover("##notifications");
        }
        if (const int unread = app.toasts.unread(); unread > 0)
        {
            // The unread count: a 12px #4D88FF disc at the bell's top right, 8px semibold white.
            const ImVec2 badge(right - px(57.f) - px(14.f) + px(20.3f), origin.y + px(11.f));
            draw->AddCircleFilled(badge, px(6.f), theme::u32(theme::palette().accent));
            const std::string count = std::to_string(std::min(unread, 99));
            const ImVec2 size = text_size(fonts().semibold, 8.f, count.c_str());
            draw_text(draw, fonts().semibold, 8.f, ImVec2(badge.x - size.x / 2.f, badge.y - size.y / 2.f),
                      IM_COL32_WHITE, count.c_str());
        }
        draw_notifications_popover(app, ImVec2(right - px(57.f) + px(14.f), origin.y + px(31.f)));
        if (bar_icon("##tabs_list", Icon::CHEVRON_DOWN, ImVec2(right - px(90.5f), mid), 16.f, bright,
                     "Tabs list (Ctrl + Shift + A)", ImGui::IsPopupOpen("##tabs_list"))
            || ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Shift | ImGuiKey_A))
        {
            open_popover("##tabs_list");
        }
        draw_tabs_list_popover(app, ImVec2(right - px(90.5f) + px(14.f), origin.y + px(31.f)));

        // Where the bar is the title bar (macOS), a press on its empty part moves the window and a double-click
        // does what one on a title bar does; the release then goes to the system, so ImGui is told of it here.
        if (ImGui::IsWindowHovered() && !ImGui::IsAnyItemHovered() && !ImGui::IsAnyItemActive()
            && !ImGui::IsPopupOpen(nullptr, ImGuiPopupFlags_AnyPopupId))
        {
            if (ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left))
            {
                platform::title_bar_double_click(platform::main_window());
            }
            else if (ImGui::IsMouseClicked(ImGuiMouseButton_Left) && platform::title_bar_inset() > 0.f)
            {
                platform::drag_window(platform::main_window());
                ImGui::GetIO().AddMouseButtonEvent(ImGuiMouseButton_Left, false);
            }
        }
        ImGui::EndChild();

        if (close_request >= 0)
        {
            app.close_tab(close_request);
        }
        if (g_menu_tab >= 0)
        {
            popup_menu(tab_menu(app, g_menu_tab));
            g_menu_tab = -1;
        }
    }

    // The New Tab page: a centred container (max 2000px, 40px 100px padding) holding Repositories (Open / Clone /
    // Create, the search box, Recent).
    void draw_new_tab_view(app::Application& app, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        ImGui::BeginChild("##new_tab", size);
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const float container = std::min(px(2000.f), size.x);
        const float content_x = origin.x + (size.x - container) / 2.f + px(100.f);
        const float content_w = container - px(200.f);
        const float left_w = content_w;
        const float top = origin.y + px(40.f);
        const auto text_at = [&](ImFont* font, float font_size, float x, float box_top, ImU32 color, const char* text)
        {
            const float ascent = font_size * 2189.f / 2048.f * dpi_scale();
            draw_text(draw, font, font_size, ImVec2(x, std::round(box_top + std::round(ascent)) - std::ceil(ascent)),
                      color, text);
        };
        // .btn-plain.fs-4: panel-bg0, a 1px 8% border, radius 4, 10px padding, a 16px icon and 16px white text.
        const auto plain_button = [&](const char* id, const char* icon, const char* label, float x, float y,
                                      bool icon_after) -> std::pair<bool, float>
        {
            const float icon_w = fa_icon_width(icon, px(16.f));
            const float text_w = text_size(fonts().regular, 16.f, label).x;
            const float w = px(22.f) + icon_w + px(10.f) + text_w;
            const ImVec2 min(x, y);
            const ImVec2 max(x + w, y + px(44.8f));
            ImGui::SetCursorScreenPos(min);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, max.y - min.y));
            const bool hovered = ImGui::IsItemHovered();
            draw->AddRectFilled(min, max, theme::u32(hovered ? p.panel_bg1 : p.panel_bg0), px(4.f));
            draw->AddRect(min, max, theme::u32(p.section_border), px(4.f), 0, 1.f);
            const float mid = y + px(22.4f);
            if (icon_after)
            {
                text_at(fonts().regular, 16.f, x + px(11.f), y + px(11.f), theme::u32(p.text_normal), label);
                draw_fa_icon(draw, icon, ImVec2(x + px(21.f) + text_w + icon_w / 2.f, mid), px(16.f),
                             theme::u32(p.text_disabled));
            }
            else
            {
                draw_fa_icon(draw, icon, ImVec2(x + px(11.f) + icon_w / 2.f, mid), px(16.f), theme::u32(p.text_normal));
                text_at(fonts().regular, 16.f, x + px(21.f) + icon_w, y + px(11.f), theme::u32(p.text_selected), label);
            }
            return {clicked, w};
        };

        // Repositories
        text_at(fonts().regular, 20.f, content_x, top - px(3.f), theme::u32(p.text_normal), "Repositories");
        {
            float x = content_x;
            const float y = top + px(42.f);
            auto [open, ow] = plain_button("##nt_open", "fas:folder-open", "Open", x, y, false);
            x += ow + px(10.f);
            auto [clone, cw] = plain_button("##nt_clone", "fas:cloud-download", "Clone", x, y, false);
            x += cw + px(10.f);
            auto [create, crw] = plain_button("##nt_create", "fas:plus-square", "Create", x, y, false);
            (void)crw;
            if (open)
            {
                app.browse_for_repository();
            }
            if (clone)
            {
                app.dialog.open(app::Dialog::Kind::CLONE_REPO);
            }
            if (create)
            {
                app.dialog.open(app::Dialog::Kind::INIT_REPO);
            }
        }
        static std::string filter;
        {
            // .input-bg with a 20px left padding for the magnifier, focused on open.
            const ImVec2 min(content_x, top + px(106.8f));
            const ImVec2 max(content_x + left_w, min.y + px(29.1f));
            draw->AddRectFilled(min, max, theme::u32(p.input_bg), px(3.f));
            ImGui::SetCursorScreenPos(ImVec2(min.x + px(20.f), min.y));
            const float text_h = font_px(fonts().regular, theme::FONT_SIZE_ROW);
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(1.f), (max.y - min.y - text_h) / 2.f));
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, ImVec4(0.f, 0.f, 0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_FrameBgActive, ImVec4(0.f, 0.f, 0.f, 0.f));
            push_font(fonts().regular, theme::FONT_SIZE_ROW);
            if (ImGui::IsWindowAppearing() || ImGui::IsKeyChordPressed(ImGuiMod_Ctrl | ImGuiMod_Alt | ImGuiKey_F))
            {
                ImGui::SetKeyboardFocusHere();
            }
            input_text("##repo_search", filter, shortcut_label("Search repositories (Ctrl + Alt + F)").c_str(), 0,
                       max.x - min.x - px(25.f));
            const bool focused = ImGui::IsItemActive();
            ImGui::PopFont();
            ImGui::PopStyleColor(3);
            ImGui::PopStyleVar(2);
            draw->AddRect(min, max, focused ? theme::u32(p.accent) : theme::u32(p.section_border), px(3.f), 0,
                          1.f);
            draw_fa_icon(draw, "far:search", ImVec2(min.x + px(13.f), min.y + px(14.5f)), px(12.f),
                         theme::u32(p.text_secondary));
        }
        text_at(fonts().regular, 14.f, content_x, top + px(153.98f), theme::u32(p.text_normal), "Recent");
        {
            float y = top + px(176.4f);
            for (std::size_t i = 0; i < app.recent_repos.size(); ++i)
            {
                std::string path = app.recent_repos[i];
                std::string name = path;
                while (!name.empty() && (name.back() == '/' || name.back() == '\\'))
                {
                    name.pop_back();
                }
                const auto slash = name.find_last_of("/\\");
                if (slash != std::string::npos)
                {
                    name = name.substr(slash + 1);
                }
                if (!filter.empty() && !contains_ci(path, filter))
                {
                    continue;
                }
                std::replace(path.begin(), path.end(), '/', '\\');
                while (!path.empty() && path.back() == '\\')
                {
                    path.pop_back();
                }
                // a.bold.btn-link (2px padding, 1px border) then the path in text-disabled, 10px on.
                const float name_w = text_size(fonts().semibold, 14.f, name.c_str()).x + px(6.f);
                ImGui::SetCursorScreenPos(ImVec2(content_x, y));
                ImGui::PushID(static_cast<int>(i));
                const bool clicked = ImGui::InvisibleButton("##recent", ImVec2(name_w, px(22.f)));
                const bool hovered = ImGui::IsItemHovered();
                ImGui::PopID();
                text_at(fonts().semibold, 14.f, content_x + px(3.f), y + px(1.f), theme::u32(p.link), name.c_str());
                if (hovered)
                {
                    const float underline = std::round(y + px(17.f));
                    draw->AddRectFilled(ImVec2(content_x + px(3.f), underline),
                                        ImVec2(content_x + name_w - px(3.f), underline + 1.f), theme::u32(p.link));
                }
                // The path is cut at the column's edge, without an ellipsis.
                draw->PushClipRect(ImVec2(content_x, y), ImVec2(content_x + left_w, y + px(22.f)), true);
                draw_text(draw, fonts().regular, 14.f,
                          ImVec2(content_x + name_w + px(10.f),
                                 std::round(y + std::round(14.f * 2189.f / 2048.f * dpi_scale()))
                                         - std::ceil(14.f * 2189.f / 2048.f * dpi_scale())),
                          theme::u32(p.text_disabled), path.c_str());
                draw->PopClipRect();
                if (clicked)
                {
                    app.open_repository(app.recent_repos[i]);
                    ImGui::EndChild();
                    return;
                }
                y += px(22.f);
            }
        }

        ImGui::EndChild();
    }
} // namespace imgit::ui
