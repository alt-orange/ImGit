#include "app/application.hpp"
#include "app/theme.hpp"
#include "git/repository.hpp"
#include "platform/system.hpp"
#include "ui/popover.hpp"
#include "ui/panels.hpp"
#include "ui/profile_widgets.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <ctime>
#include <imgui.h>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <string>
#include <vector>

// The tabs bar's popovers (toolbar-bg2 with a 0 6px 12px shadow, 5px under their button): the tabs
// list (a react-select), the notifications and the profile menu.
namespace imgit::ui
{
    namespace
    {

        const ImVec4& POPOVER_BG = theme::palette().toolbar_bg2; // the current theme's, by reference
        const ImVec4 TEXT(1.f, 1.f, 1.f, 0.9f);
        const ImVec4 SECONDARY(1.f, 1.f, 1.f, 0.6f);
        const ImVec4 DISABLED(1.f, 1.f, 1.f, 0.4f);
        const ImVec4 DIVIDER(1.f, 1.f, 1.f, 0.2f);
        const ImVec4& HOVER = theme::palette().hover_row;

        // Text whose CSS line box starts at `line_top` (logical, from `origin`), Chromium-style baseline.
        void text_at(ImDrawList* draw, ImFont* font, float size, ImVec2 origin, float x, float line_top,
                     float line_height, ImU32 color, const char* text, float max_width = 0.f)
        {
            const float box = std::round(size * 2189.f / 2048.f) + std::round(size * 600.f / 2048.f);
            const float ascent = size * 2189.f / 2048.f * dpi_scale();
            const float top = origin.y + (line_top + (line_height - box) / 2.f) * dpi_scale();
            const float baseline = std::round(top + std::round(ascent));
            const ImVec2 pos(origin.x + x * dpi_scale(), baseline - std::ceil(ascent));
            if (max_width > 0.f)
            {
                draw_text_ellipsis(draw, font, size, pos, max_width * dpi_scale(), color, text);
            }
            else
            {
                draw_text(draw, font, size, pos, color, text);
            }
        }

        // A popover window sized `size` (logical) at `pos`; content draws in screen space on its draw list.
        bool begin_tab_popover(const char* id, ImVec2 pos, ImVec2 size)
        {
            if (!ImGui::IsPopupOpen(id))
            {
                return false;
            }
            ImGui::SetNextWindowPos(ImVec2(std::round(pos.x), std::round(pos.y)));
            ImGui::SetNextWindowSize(ImVec2(px(size.x), px(size.y)));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_PopupBorderSize, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_PopupRounding, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_PopupBg, ImVec4(0.f, 0.f, 0.f, 0.f));
            const bool open = ImGui::BeginPopup(id, ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoScrollbar);
            ImGui::PopStyleColor();
            ImGui::PopStyleVar(4);
            if (open && ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                ImGui::CloseCurrentPopup();
            }
            return open;
        }

        void popover_frame(ImDrawList* draw, ImVec2 min, ImVec2 size)
        {
            const ImVec2 max(min.x + px(size.x), min.y + px(size.y));
            draw_box_shadow(draw, min, max, ImVec2(0.f, px(6.f)), px(12.f), 0.f, ImVec4(0.f, 0.f, 0.f, 0.176f),
                            px(2.f));
            draw->AddRectFilled(min, max, theme::u32(POPOVER_BG), px(2.f));
        }

        // A 14px row with a 20x16 icon in text-disabled and hover-text label (the profile menu's actions).
        bool icon_row(ImDrawList* draw, ImVec2 origin, float y, float width, const char* icon, const char* label)
        {
            const ImVec2 min(origin.x, origin.y + px(y));
            const ImVec2 max(origin.x + px(width), min.y + px(40.f));
            ImGui::SetCursorScreenPos(min);
            ImGui::PushID(label);
            const bool clicked = ImGui::InvisibleButton("##row", ImVec2(max.x - min.x, max.y - min.y));
            ImGui::PopID();
            const bool hovered = ImGui::IsItemHovered();
            if (hovered)
            {
                ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
            }
            draw_fa_icon(draw, icon, ImVec2(origin.x + px(30.f), origin.y + px(y + 20.f)), px(16.f),
                         theme::u32(DISABLED));
            text_at(draw, fonts().regular, 14.f, origin, 50.f, y + 10.f, 20.f,
                    theme::u32(hovered ? ImVec4(1.f, 1.f, 1.f, 1.f) : TEXT), label);
            return clicked;
        }

        void bold_header(ImDrawList* draw, ImVec2 origin, float y, float width, const char* text)
        {
            text_at(draw, fonts().semibold, 14.f, origin, 20.f, y + 10.f, 20.f, theme::u32(TEXT), text);
            draw->AddRectFilled(ImVec2(origin.x, origin.y + px(y + 40.f)),
                                ImVec2(origin.x + px(width), origin.y + px(y + 41.f)), theme::u32(DIVIDER));
        }

        std::string format_notification_time(std::int64_t unix_seconds)
        {
            // "October 12th @ 09:30am"
            const std::tm local = platform::local_time(static_cast<std::time_t>(unix_seconds));
            static constexpr const char* MONTHS[] = {"January",   "February", "March",    "April",
                                                     "May",       "June",     "July",     "August",
                                                     "September", "October",  "November", "December"};
            const int day = local.tm_mday;
            const char* suffix = (day % 10 == 1 && day != 11)   ? "st"
                                 : (day % 10 == 2 && day != 12) ? "nd"
                                 : (day % 10 == 3 && day != 13) ? "rd"
                                                                : "th";
            const int hour = local.tm_hour % 12 == 0 ? 12 : local.tm_hour % 12;
            char buffer[64];
            std::snprintf(buffer, sizeof(buffer), "%s %d%s @ %02d:%02d%s", MONTHS[local.tm_mon], day, suffix, hour,
                          local.tm_min, local.tm_hour < 12 ? "am" : "pm");
            return buffer;
        }
    } // namespace

    void open_tabs_list(app::Application& app)
    {
        (void)app;
        open_popover("##tabs_list");
    }

    void draw_tabs_list_popover(app::Application& app, ImVec2 anchor_right_bottom)
    {
        static std::string search;
        static int focused = -1;
        // Every repository tab.
        struct Item
        {
            int index; // -1: the New Tab page
            std::string label;
        };
        std::vector<Item> items;
        for (int i = 0; i < static_cast<int>(app.tabs.size()); ++i)
        {
            items.push_back({i, app.tabs[static_cast<std::size_t>(i)]->repo.name()});
        }
        items.push_back({-1, "New Tab"});
        std::erase_if(items,
                      [](const Item& item)
                      {
                          return !search.empty() && !contains_ci(item.label, search);
                      });
        const float menu_h = 4.f + 8.f + 15.f + 2.625f + 47.f * static_cast<float>(items.size());
        const ImVec2 pos(anchor_right_bottom.x - px(330.f), anchor_right_bottom.y + px(5.f));
        if (!begin_tab_popover("##tabs_list", pos, ImVec2(330.f, 36.f + menu_h + 8.f)))
        {
            search.clear();
            focused = -1;
            return;
        }
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 origin = ImGui::GetWindowPos();
        draw->PushClipRectFullScreen();
        popover_frame(draw, origin, ImVec2(330.f, 34.f));

        // react-select control: 20% black, 20% white border, 4px radius, the focus ring rgb(38,132,255).
        const ImVec2 control_max(origin.x + px(330.f), origin.y + px(34.f));
        draw->AddRectFilled(ImVec2(origin.x - 1.f, origin.y - 1.f), ImVec2(control_max.x + 1.f, control_max.y + 1.f),
                            theme::u32(theme::palette().accent), px(5.f));
        draw->AddRectFilled(origin, control_max, theme::u32(POPOVER_BG), px(4.f));
        draw->AddRectFilled(origin, control_max, theme::u32(ImVec4(0.f, 0.f, 0.f, 0.2f)), px(4.f));
        draw->AddRect(ImVec2(origin.x + 0.5f, origin.y + 0.5f), ImVec2(control_max.x - 0.5f, control_max.y - 0.5f),
                      theme::u32(DIVIDER), px(4.f), 0, 1.f);
        draw->AddRectFilled(ImVec2(origin.x + px(300.f), origin.y + px(7.f)),
                            ImVec2(origin.x + px(301.f), origin.y + px(27.f)), theme::u32(DIVIDER));
        draw_svg_path(draw, "chevron-down", svg_path("chevron-down"), 20.f, 20.f,
                      ImVec2(origin.x + px(305.f), origin.y + px(7.f)),
                      ImVec2(origin.x + px(325.f), origin.y + px(27.f)), theme::u32(TEXT));
        // The input (14px), placeholder "Search Tabs".
        const float ascent = 14.f * 2189.f / 2048.f * dpi_scale();
        const float baseline = std::round(origin.y + px(7.5f) + std::round(ascent));
        ImGui::SetCursorScreenPos(ImVec2(origin.x + px(11.f), baseline - std::ceil(ascent)));
        if (ImGui::IsWindowAppearing())
        {
            ImGui::SetKeyboardFocusHere();
        }
        ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
        ImGui::PushStyleColor(ImGuiCol_Text, TEXT);
        ImGui::PushStyleColor(ImGuiCol_TextDisabled, DISABLED);
        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(0.f, 0.f));
        push_font(fonts().regular, 14.f);
        ImGui::SetNextItemWidth(px(280.f));
        const bool text_changed = ImGui::InputTextWithHint("##tabs_search", "Search Tabs", &search);
        text_field_menu(search);
        if (text_changed)
        {
            focused = 0;
        }
        ImGui::PopFont();
        ImGui::PopStyleVar();
        ImGui::PopStyleColor(3);

        // The menu: #1C1E23, 20% white border, 4px radius, 0 6px 12px rgba(0,0,0,.4).
        const ImVec2 menu_min(origin.x, origin.y + px(36.f));
        const ImVec2 menu_max(origin.x + px(330.f), menu_min.y + px(menu_h));
        draw_box_shadow(draw, menu_min, menu_max, ImVec2(0.f, px(6.f)), px(12.f), 0.f, ImVec4(0.f, 0.f, 0.f, 0.4f),
                        px(4.f));
        draw->AddRectFilled(menu_min, menu_max, theme::u32(DIVIDER), px(4.f));
        const ImVec4 menu_bg(28 / 255.f, 30 / 255.f, 35 / 255.f, 1.f);
        draw->AddRectFilled(ImVec2(menu_min.x + 1.f, menu_min.y + 1.f), ImVec2(menu_max.x - 1.f, menu_max.y - 1.f),
                            theme::u32(menu_bg), px(3.f));
        // "OPEN TABS": 10.5px, weight 500, uppercase, #999.
        text_at(draw, fonts().regular, 10.5f, menu_min, 13.f, 13.f, 15.f, IM_COL32(153, 153, 153, 255), "OPEN TABS");

        if (ImGui::IsKeyPressed(ImGuiKey_DownArrow))
        {
            focused = std::min(focused + 1, static_cast<int>(items.size()) - 1);
        }
        if (ImGui::IsKeyPressed(ImGuiKey_UpArrow))
        {
            focused = std::max(focused - 1, 0);
        }
        int activate = -2;
        int close = -2;
        bool any = false;
        for (std::size_t i = 0; i < items.size(); ++i)
        {
            const Item& item = items[i];
            const float top = 30.625f + 47.f * static_cast<float>(i);
            const ImVec2 row_min(menu_min.x + 1.f, menu_min.y + px(top));
            const ImVec2 row_max(menu_max.x - 1.f, row_min.y + px(46.f));
            ImGui::SetCursorScreenPos(row_min);
            ImGui::PushID(static_cast<int>(i));
            ImGui::SetNextItemAllowOverlap();
            const bool clicked =
                    ImGui::InvisibleButton("##tab_row", ImVec2(row_max.x - row_min.x, row_max.y - row_min.y));
            if (ImGui::IsItemHovered() && ImGui::GetIO().MouseDelta.y != 0.f)
            {
                focused = static_cast<int>(i);
            }
            if (focused == static_cast<int>(i))
            {
                draw->AddRectFilled(row_min, row_max, theme::u32(HOVER));
            }
            draw->AddRectFilled(ImVec2(row_min.x, row_max.y), ImVec2(row_max.x, row_max.y + px(1.f)),
                                theme::u32(DIVIDER));
            float text_x = 13.f;
            if (item.index >= 0)
            {
                draw_svg_path(draw, "code-branch", svg_path("code-branch"), 448.f, 512.f,
                              ImVec2(menu_min.x + px(18.f), row_min.y + px(16.f)),
                              ImVec2(menu_min.x + px(32.f), row_min.y + px(30.f)), theme::u32(DISABLED));
                text_x = 42.f;
            }
            text_at(draw, fonts().regular, 14.f, ImVec2(menu_min.x, row_min.y), text_x, 13.f, 20.f, theme::u32(TEXT),
                    item.label.c_str(), 250.f);
            // The close "x" (far:times 17.5x14, text-secondary).
            const ImVec2 x_center(menu_min.x + px(308.25f), row_min.y + px(23.f));
            ImGui::SetCursorScreenPos(ImVec2(x_center.x - px(10.f), x_center.y - px(10.f)));
            const bool close_clicked = ImGui::InvisibleButton("##close", ImVec2(px(20.f), px(20.f)));
            draw_fa_icon(draw, "far:times", x_center, px(14.f),
                         theme::u32(ImGui::IsItemHovered() ? ImVec4(1.f, 1.f, 1.f, 1.f) : SECONDARY));
            ImGui::PopID();
            if (close_clicked)
            {
                close = item.index;
            }
            else if (clicked
                     || ((ImGui::IsKeyPressed(ImGuiKey_Enter) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter))
                         && focused == static_cast<int>(i)))
            {
                activate = item.index;
            }
            any = true;
        }
        if (!any)
        {
            text_at(draw, fonts().regular, 14.f, menu_min, 13.f, 37.f, 20.f, theme::u32(DISABLED), "No options");
        }
        draw->PopClipRect();
        if (activate != -2)
        {
            app.active_tab = activate;
            ImGui::CloseCurrentPopup();
        }
        ImGui::EndPopup();
        if (close != -2)
        {
            if (close >= 0)
            {
                app.close_tab(close);
            }
        }
    }

    void draw_profile_popover(app::Application& app, ImVec2 anchor_right_bottom)
    {
        const ImVec2 pos(anchor_right_bottom.x - px(330.f), anchor_right_bottom.y + px(5.f));
        if (!begin_tab_popover("##profile_menu", pos, ImVec2(330.f, 233.f)))
        {
            return;
        }
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 o = ImGui::GetWindowPos();
        draw->PushClipRectFullScreen();
        popover_frame(draw, o, ImVec2(330.f, 233.f));
        bold_header(draw, o, 0.f, 330.f, "Current profile");

        // The profile picker (react-select, 42px): its menu switches profiles.
        const app::Profile& profile = app.profiles.current();
        const ImVec2 picker_min(o.x + px(20.f), o.y + px(61.f));
        const ImVec2 picker_max(picker_min.x + px(290.f), picker_min.y + px(42.f));
        ImGui::SetCursorScreenPos(picker_min);
        if (ImGui::InvisibleButton("##profile_picker",
                                   ImVec2(picker_max.x - picker_min.x, picker_max.y - picker_min.y)))
        {
            ImGui::OpenPopup("##profile_select");
        }
        draw->AddRectFilled(picker_min, picker_max, theme::u32(ImVec4(0.f, 0.f, 0.f, 0.2f)), px(4.f));
        if (ImGui::IsPopupOpen("##profile_select"))
        {
            draw_select_focus(draw, picker_min, picker_max, px(4.f));
        }
        draw_profile_icon(draw, ImVec2(o.x + px(30.f), o.y + px(68.f)), px(28.f), profile);
        text_at(draw, fonts().regular, 14.f, o, 68.f, 72.f, 20.f, theme::u32(TEXT), profile.name.c_str(), 210.f);
        draw_svg_path(draw, "chevron-down", svg_path("chevron-down"), 20.f, 20.f,
                      ImVec2(o.x + px(286.f), o.y + px(72.f)), ImVec2(o.x + px(306.f), o.y + px(92.f)),
                      theme::u32(TEXT));

        // The profile's commit signature.
        text_at(draw, fonts().regular, 12.f, o, 20.f, 113.f, 17.1429f, theme::u32(SECONDARY), "Commit signature");
        text_at(draw, fonts().regular, 14.f, o, 30.f, 135.1f, 21.f, theme::u32(TEXT), profile.user_name.c_str(), 280.f);
        text_at(draw, fonts().regular, 12.f, o, 30.f, 156.1f, 21.f, theme::u32(TEXT), profile.user_email.c_str(),
                280.f);
        if (icon_row(draw, o, 184.3f, 330.f, "far:users-cog", "Manage profiles"))
        {
            app.preferences_open = true;
            app.preferences_page = "Profiles";
            ImGui::CloseCurrentPopup();
        }
        draw->PopClipRect();
        if (const std::string picked =
                    profile_select_menu(app, "##profile_select", picker_min, picker_max, POPOVER_PROFILE_MENU);
            !picked.empty())
        {
            app.switch_profile(picked);
            ImGui::CloseCurrentPopup();
        }
        ImGui::EndPopup();
    }

    void draw_notifications_popover(app::Application& app, ImVec2 anchor_right_bottom)
    {
        std::vector<app::Toasts::Entry>& history = app.toasts.history();
        // Each notification: 10px padding, an 18px header line, 5px, the wrapped body, 10px, a 1px divider.
        ImFont* font = fonts().regular;
        const float body_w = 362.f;
        std::vector<float> heights;
        float content = 0.f;
        for (const app::Toasts::Entry& entry : history)
        {
            const std::string body = entry.detail.empty() ? entry.title : entry.title + " " + entry.detail;
            const float wrapped = font->CalcTextSizeA(font_px(font, 12.f), FLT_MAX, px(body_w), body.c_str()).y;
            const int lines = std::max(1, static_cast<int>(std::round(wrapped / font_px(font, 12.f))));
            const float h = 10.f + 18.f + 5.f + 17.1429f * static_cast<float>(lines) + 10.f + 1.f;
            heights.push_back(h);
            content += h;
        }
        const float empty_h = history.empty() ? 60.f : 0.f;
        const float list_h = std::min(816.f, content + empty_h);
        const float height = 52.1f + list_h;
        const ImVec2 pos(anchor_right_bottom.x - px(450.f), anchor_right_bottom.y + px(5.f));
        if (!begin_tab_popover("##notifications", pos, ImVec2(450.f, height)))
        {
            return;
        }
        app.toasts.mark_all_read();
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 o = ImGui::GetWindowPos();
        draw->PushClipRectFullScreen();
        popover_frame(draw, o, ImVec2(450.f, height));
        text_at(draw, fonts().semibold, 14.f, o, 20.f, 15.6f, 20.f, theme::u32(TEXT), "Notifications");
        // The "All ▾" filter and the "⋮" menu (hover-text buttons).
        text_at(draw, fonts().regular, 12.f, o, 139.6f, 16.9f, 17.1429f, theme::u32(TEXT), "All");
        draw->AddTriangleFilled(ImVec2(o.x + px(156.4f), o.y + px(24.8f)), ImVec2(o.x + px(164.4f), o.y + px(24.8f)),
                                ImVec2(o.x + px(160.4f), o.y + px(28.8f)), theme::u32(TEXT));
        draw_fa_icon(draw, "far:ellipsis-v", ImVec2(o.x + px(417.f), o.y + px(25.6f)), px(16.f), theme::u32(TEXT));
        draw->AddRectFilled(ImVec2(o.x, o.y + px(51.1f)), ImVec2(o.x + px(450.f), o.y + px(52.1f)),
                            theme::u32(DIVIDER));

        if (history.empty())
        {
            text_at(draw, font, 12.f, o, 20.f, 72.1f, 17.1429f, theme::u32(SECONDARY), "You're all caught up.");
        }
        float y = 52.1f;
        int remove = -1;
        for (std::size_t i = 0; i < history.size() && y < 52.1f + 816.f; ++i)
        {
            const app::Toasts::Entry& entry = history[i];
            const ImVec4 accent = entry.kind == app::ToastKind::ERROR     ? theme::palette().red
                                  : entry.kind == app::ToastKind::SUCCESS ? theme::palette().green
                                                                          : theme::palette().accent;
            draw->AddRectFilled(ImVec2(o.x + px(20.f), o.y + px(y + 10.8f)),
                                ImVec2(o.x + px(38.f), o.y + px(y + 28.8f)), theme::u32(accent), px(2.f));
            text_at(draw, font, 12.f, o, 48.f, y + 10.5f, 17.1429f, theme::u32(TEXT), "ImGit");
            const float sender_w = text_size(font, 12.f, "ImGit").x / dpi_scale();
            text_at(draw, font, 12.f, o, 48.f + sender_w + 10.f, y + 10.5f, 17.1429f, theme::u32(SECONDARY),
                    format_notification_time(entry.time).c_str());
            draw_fa_icon(draw, "far:eye", ImVec2(o.x + px(383.f), o.y + px(y + 19.f)), px(12.f), theme::u32(DISABLED));
            const ImVec2 trash(o.x + px(421.f), o.y + px(y + 19.f));
            ImGui::SetCursorScreenPos(ImVec2(trash.x - px(9.f), trash.y - px(9.f)));
            ImGui::PushID(static_cast<int>(i));
            if (ImGui::InvisibleButton("##delete", ImVec2(px(18.f), px(18.f))))
            {
                remove = static_cast<int>(i);
            }
            draw_fa_icon(draw, "far:trash", trash, px(12.f),
                         theme::u32(ImGui::IsItemHovered() ? ImVec4(1.f, 1.f, 1.f, 1.f) : DISABLED));
            ImGui::PopID();
            const std::string body = entry.detail.empty() ? entry.title : entry.title + " " + entry.detail;
            const float size = font_px(font, 12.f);
            const char* line = body.c_str();
            const char* end = line + body.size();
            float line_y = y + 33.f;
            while (line < end)
            {
                const char* line_end = font->CalcWordWrapPosition(size, line, end, px(body_w));
                if (line_end == line)
                {
                    line_end = end;
                }
                const std::string piece(line, line_end);
                text_at(draw, font, 12.f, o, 48.f, line_y, 17.1429f, theme::u32(TEXT), piece.c_str());
                line_y += 17.1429f;
                line = line_end;
                while (line < end && *line == ' ')
                {
                    ++line;
                }
            }
            y += heights[i];
            draw->AddRectFilled(ImVec2(o.x, o.y + px(y - 1.f)), ImVec2(o.x + px(450.f), o.y + px(y)),
                                theme::u32(DIVIDER));
        }
        draw->PopClipRect();
        ImGui::EndPopup();
        if (remove >= 0)
        {
            history.erase(history.begin() + remove);
        }
    }
} // namespace imgit::ui
