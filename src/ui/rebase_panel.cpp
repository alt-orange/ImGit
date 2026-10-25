#include "app/application.hpp"
#include "app/theme.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <imgui.h>
#include <imgui_internal.h>
#include <string>
#include <utility>

// The pending interactive rebase, in place of the left panel and the graph: a 36px header ("Interactive Rebase",
// "Rebasing <branch> onto <base>", Cancel Rebase), one 32px row per commit (newest first: the action select, the
// commit's node and streak in its lane colour, the message) and a footer with the shortcuts and Reset / Cancel
// Rebase / Start Rebase. Keys on the selected row: P / S / R / D set its action, Up / Down select, Ctrl + Up /
// Down move it. Positions are CSS px.
namespace imgit::ui
{
    namespace
    {
        constexpr float HEADER_HEIGHT = 36.f;
        constexpr float FOOTER_HEIGHT = 57.140625f;
        constexpr float PADDING = 20.f;
        constexpr float ROW_HEIGHT = 32.f; // 4px padding around the 24px controls
        constexpr float SELECT_WIDTH = 90.f;
        constexpr float NODE_X = 110.f; // after the select and its 20px margin
        constexpr float MESSAGE_X = 170.f; // the node zone is 60px wide
        constexpr float REWORD_WIDTH = 400.f;
        constexpr float REWORD_HEIGHT = 200.f;

        const char* const ACTION_LABELS[] = {"Pick", "Reword", "Squash", "Drop"};

        // `text` in a `line_h` tall line box at `line_top` (device px), centred like Chromium.
        void line_text(ImDrawList* draw, ImFont* font, float size, float x, float line_top, float line_h, ImU32 color,
                       const char* text)
        {
            const float content = std::round(size * 2189.f / 2048.f) + std::round(size * 600.f / 2048.f);
            const float top = line_top + std::floor((line_h - content) / 2.f) * dpi_scale();
            const float ascent = size * 2189.f / 2048.f * dpi_scale();
            draw_text(draw, font, size, ImVec2(std::round(x), std::round(top + std::round(ascent)) - std::ceil(ascent)),
                      color, text);
        }

        float text_w(ImFont* font, float size, const char* text)
        {
            return text_size(font, size, text).x;
        }

        enum class Tone
        {
            PRIMARY,
            DANGER,
            SUCCESS,
        };

        // A bootstrap button (12px text, translucent fill, 1px border, radius 2) with its top-right corner at
        // `right`, `top`; returns true when clicked. `padding_x` / `h` follow .btn (12 / 31.14) or .btn-xs (5 / 22).
        bool button(ImDrawList* draw, const char* id, const char* label, float right, float top, float h,
                    float padding_x, Tone tone, float* width = nullptr)
        {
            const theme::Palette& p = theme::palette();
            const ImVec4 color = tone == Tone::PRIMARY ? p.accent : (tone == Tone::DANGER ? p.red : p.green);
            const float w = text_w(fonts().regular, 12.f, label) + px(padding_x * 2.f + 2.f);
            const ImVec2 min(std::round(right - w), top);
            const ImVec2 max(min.x + w, top + h);
            ImGui::SetCursorScreenPos(min);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, h));
            const bool hovered = ImGui::IsItemHovered();
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(color, hovered ? 0.3f : 0.2f)), px(2.f));
            draw->AddRect(min, max, theme::u32(color), px(2.f), 0, 1.f);
            line_text(draw, fonts().regular, 12.f, min.x + px(padding_x + 1.f), min.y, h,
                      theme::u32(hovered ? p.text_selected : p.text_normal), label);
            if (width != nullptr)
            {
                *width = w;
            }
            return clicked;
        }

        ImVec4 lane_color(const app::RepoState& state, const git::Sha& sha)
        {
            const auto row = state.history.row_by_sha.find(sha);
            if (row == state.history.row_by_sha.end() || row->second >= state.layout.rows.size())
            {
                return theme::graph_color(0);
            }
            return theme::graph_color(state.layout.rows[row->second].column);
        }

        // The reword editor's summary / description, split from and joined into the step's message.
        struct RewordFields
        {
            const app::RepoState* state = nullptr; // the tab the fields belong to: another tab's plan is not written
            int step = -1;
            std::string summary;
            std::string description;
        };
        RewordFields g_reword;

        void open_reword(app::RepoState& state, int step)
        {
            app::RebasePlan& plan = state.rebase;
            plan.editing = step;
            g_reword.state = &state;
            g_reword.step = step;
            const git::RebaseStep& s = plan.steps[static_cast<std::size_t>(step)];
            std::string message = s.message;
            if (message.empty())
            {
                if (const git::Commit* commit = state.commit_for(s.sha))
                {
                    message = commit->summary + (commit->body.empty() ? "" : "\n\n" + commit->body);
                }
                else
                {
                    message = plan.summaries[static_cast<std::size_t>(step)];
                }
            }
            const auto newline = message.find('\n');
            g_reword.summary = message.substr(0, newline);
            g_reword.description.clear();
            if (newline != std::string::npos)
            {
                std::string body = message.substr(newline + 1);
                while (!body.empty() && (body.front() == '\n' || body.front() == '\r'))
                {
                    body.erase(body.begin());
                }
                g_reword.description = body;
            }
        }

        void set_action(app::RepoState& state, int step, git::RebaseAction action)
        {
            app::RebasePlan& plan = state.rebase;
            plan.steps[static_cast<std::size_t>(step)].action = action;
            if (action == git::RebaseAction::REWORD)
            {
                open_reword(state, step);
            }
            else if (plan.editing == step)
            {
                plan.editing = -1;
            }
        }

        void select_step(app::RepoState& state, int step)
        {
            state.rebase.selected = step;
            state.select_only(state.rebase.steps[static_cast<std::size_t>(step)].sha);
        }

        // The floating reword editor under row `row_min`: panel-bg1, 5px 10px 10px padding, the summary with its
        // characters-left count over the description. Closes on Escape or a click outside it.
        void draw_reword_editor(app::RepoState& state, ImVec2 pos)
        {
            const theme::Palette& p = theme::palette();
            app::RebasePlan& plan = state.rebase;
            const int step = plan.editing;
            const ImVec2 size(px(REWORD_WIDTH), px(REWORD_HEIGHT));
            ImGui::SetNextWindowPos(pos);
            ImGui::SetNextWindowSize(size);
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::Begin("##reword_editor", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                 | ImGuiWindowFlags_NoBackground);
            ImGui::PopStyleVar(2);
            ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow());
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const ImVec2 max(pos.x + size.x, pos.y + size.y);
            draw->AddRectFilled(ImVec2(pos.x + 2.f, pos.y + 3.f), ImVec2(max.x + 2.f, max.y + 3.f),
                                IM_COL32(0, 0, 0, 70), px(2.f));
            draw->AddRectFilled(pos, max, theme::u32(p.panel_bg1), px(2.f));

            const bool other = g_reword.state != &state || g_reword.step != step;
            const bool appearing = other || ImGui::IsWindowAppearing();
            if (other)
            {
                open_reword(state, step);
            }
            ImGui::PushStyleColor(ImGuiCol_FrameBg, p.input_bg);
            ImGui::PushStyleColor(ImGuiCol_Text, p.text_selected);
            push_font(fonts().regular, 14.f);
            // Summary: 5px 30px 5px 10px padding.
            const float inner_w = size.x - px(20.f);
            ImGui::SetCursorScreenPos(ImVec2(pos.x + px(10.f), pos.y + px(5.f)));
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(10.f), px(5.f)));
            if (appearing)
            {
                ImGui::SetKeyboardFocusHere();
            }
            const bool entered = input_text("##reword_summary", g_reword.summary, "Commit summary",
                                            ImGuiInputTextFlags_EnterReturnsTrue, inner_w - px(20.f));
            const ImVec2 smin = ImGui::GetItemRectMin();
            const ImVec2 smax = ImGui::GetItemRectMax();
            ImGui::PopStyleVar();
            const int left = 72 - static_cast<int>(g_reword.summary.size());
            const std::string count = std::to_string(left);
            draw->AddText(fonts().regular, font_px(fonts().regular, 14.f),
                          ImVec2(smin.x + inner_w - px(10.f) - text_w(fonts().regular, 14.f, count.c_str()),
                                 smin.y + (smax.y - smin.y - font_px(fonts().regular, 14.f)) / 2.f),
                          theme::u32(left < 0 ? p.orange : p.text_disabled), count.c_str());
            // Description: 5px 10px padding, the rest of the editor.
            ImGui::SetCursorScreenPos(ImVec2(pos.x + px(10.f), smax.y + px(5.f)));
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(10.f), px(5.f)));
            input_multiline("##reword_description", g_reword.description, "Description",
                            ImVec2(inner_w, max.y - px(10.f) - (smax.y + px(5.f))));
            ImGui::PopStyleVar();
            ImGui::PopFont();
            ImGui::PopStyleColor(2);

            std::string message = g_reword.summary;
            if (!g_reword.description.empty())
            {
                message += "\n\n" + g_reword.description;
            }
            plan.steps[static_cast<std::size_t>(step)].message = message;

            const bool outside = ImGui::IsMouseClicked(ImGuiMouseButton_Left)
                                 && !ImGui::IsMouseHoveringRect(pos, max, false) && !appearing;
            if (entered || outside || ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                plan.editing = -1;
                g_reword.step = -1;
            }
            ImGui::End();
        }
    } // namespace

    void draw_rebase_panel(app::Application& app, app::RepoState& state, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        app::RebasePlan& plan = state.rebase;
        const int count = static_cast<int>(plan.steps.size());
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImGui::BeginChild("##rebase_panel", size, ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + size.y), theme::u32(p.app_bg0));
        if (plan.selected < 0 && count > 0)
        {
            select_step(state, count - 1); // the newest, at the top
        }
        bool cancel = false;
        bool start = false;

        // Header: panel-bg0, 0 20px padding, a 1px section border under it.
        {
            const ImVec2 hmax(origin.x + size.x, origin.y + px(HEADER_HEIGHT));
            draw->AddRectFilled(origin, hmax, theme::u32(p.panel_bg0));
            draw->AddRectFilled(ImVec2(origin.x, hmax.y - 1.f), hmax, theme::u32(p.section_border));
            const char* title = "Interactive Rebase";
            float x = origin.x + px(PADDING);
            line_text(draw, fonts().semibold, 14.f, x, origin.y + px(7.5f), px(20.f), theme::u32(p.text_normal),
                      title);
            x += text_w(fonts().semibold, 14.f, title) + px(20.f);
            const float line_top = origin.y + px(8.92f);
            const float line_h = px(17.140625f);
            const ImU32 text = theme::u32(p.text_normal);
            const auto words = [&](const char* label)
            {
                line_text(draw, fonts().regular, 12.f, x, line_top, line_h, text, label);
                x += text_w(fonts().regular, 12.f, label) + px(5.f);
            };
            // The branch pills: 0 5px padding on the lane colour mixed 45% into the background.
            const auto pill = [&](const std::string& label)
            {
                const float w = text_w(fonts().regular, 12.f, label.c_str()) + px(10.f);
                const ImVec4 bg = theme::mix(lane_color(state, plan.steps.empty() ? git::Sha{} : plan.steps.back().sha),
                                             p.app_bg0, 0.45f);
                draw->AddRectFilled(ImVec2(x, line_top), ImVec2(x + w, line_top + px(17.f)), theme::u32(bg));
                line_text(draw, fonts().regular, 12.f, x + px(5.f), line_top, px(17.f), text, label.c_str());
                x += w + px(5.f);
            };
            words("Rebasing");
            pill(plan.head_label);
            words("onto");
            pill(plan.base_label);
            cancel |= button(draw, "##cancel_header", "Cancel Rebase", hmax.x - px(PADDING), origin.y + px(6.5f),
                             px(22.f), 5.f, Tone::DANGER);
        }

        // The rows, newest first, in a scrolling list with 20px padding.
        const float list_top = origin.y + px(HEADER_HEIGHT);
        const float list_h = size.y - px(HEADER_HEIGHT) - px(FOOTER_HEIGHT);
        ImGui::SetCursorScreenPos(ImVec2(origin.x, list_top));
        ImGui::PushStyleColor(ImGuiCol_ScrollbarBg, ImVec4(0.f, 0.f, 0.f, 0.f));
        ImGui::BeginChild("##rebase_rows", ImVec2(size.x, std::max(1.f, list_h)));
        ImGui::PopStyleColor();
        ImDrawList* rows = ImGui::GetWindowDrawList();
        const ImVec2 list_origin = ImGui::GetCursorScreenPos();
        const float rows_right = list_origin.x + ImGui::GetContentRegionAvail().x - px(PADDING);
        static int menu_step = -1;
        static ImVec2 menu_pos;
        bool open_menu = false; // opened after the rows, at the ID stack level the popup is begun at
        ImVec2 editor_pos(-1.f, -1.f);
        for (int display = 0; display < count; ++display)
        {
            const int i = count - 1 - display;
            git::RebaseStep& step = plan.steps[static_cast<std::size_t>(i)];
            const float y = list_origin.y + px(PADDING) + px(ROW_HEIGHT) * static_cast<float>(display);
            const float x0 = list_origin.x + px(PADDING);
            const float top = y + px(4.f);
            const float bottom = top + px(24.f);
            const bool drop = step.action == git::RebaseAction::DROP;
            const bool squash = step.action == git::RebaseAction::SQUASH;
            ImGui::PushID(i);

            // The message zone: hover / selected backgrounds, the row's click target.
            const ImVec2 wmin(x0 + px(MESSAGE_X), top);
            const ImVec2 wmax(std::max(wmin.x + 1.f, rows_right), bottom);
            ImGui::SetCursorScreenPos(wmin);
            const bool row_clicked = ImGui::InvisibleButton("##row", ImVec2(wmax.x - wmin.x, wmax.y - wmin.y));
            const bool row_hovered = ImGui::IsItemHovered();
            if (ImGui::IsItemHovered() && ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left)
                && step.action == git::RebaseAction::REWORD)
            {
                open_reword(state, i);
            }
            const bool selected = plan.selected == i;
            if (selected || row_hovered)
            {
                rows->AddRectFilled(wmin, wmax, theme::u32(selected ? p.selected_row : p.hover_row));
            }
            if (row_clicked)
            {
                select_step(state, i);
            }

            // The action select: 90 x 24, rgba(0,0,0,.2), a 1px border green (pick), orange (reword / squash) or
            // red (drop), radius 4, 14px text 5px in and the chevron at the end.
            const ImVec2 smin(x0, top);
            const ImVec2 smax(x0 + px(SELECT_WIDTH), bottom);
            ImGui::SetCursorScreenPos(smin);
            if (ImGui::InvisibleButton("##action", ImVec2(smax.x - smin.x, smax.y - smin.y)))
            {
                select_step(state, i);
                menu_step = i;
                menu_pos = ImVec2(smin.x, smax.y + px(2.f));
                open_menu = true;
            }
            const ImVec4 border = step.action == git::RebaseAction::PICK ? p.green : (drop ? p.red : p.orange);
            rows->AddRectFilled(smin, smax, theme::u32(p.input_bg), px(4.f));
            rows->AddRect(smin, smax, theme::u32(border), px(4.f), 0, 1.f);
            line_text(rows, fonts().regular, 14.f, smin.x + px(6.f), top, px(24.f), theme::u32(p.text_normal),
                      ACTION_LABELS[static_cast<int>(step.action)]);
            draw_svg_path(rows, "chevron-down", svg_path("chevron-down"), 20.f, 20.f,
                          ImVec2(smax.x - px(20.f), top + px(4.f)), ImVec2(smax.x - px(4.f), top + px(20.f)),
                          theme::u32(p.text_normal));

            // The node zone: squash adds a level-down indicator before the node, drop shifts it the same 24px and
            // fades it; the streak runs from the node's centre to the message.
            const ImVec4 lane = lane_color(state, step.sha);
            const float zone_x = x0 + px(NODE_X);
            const float node_x = squash || drop ? zone_x + px(24.f) : zone_x;
            const float alpha = drop ? 0.5f : 1.f;
            if (squash)
            {
                draw_fa_icon_mirrored(rows, "fas:level-down", ImVec2(zone_x + px(12.f), bottom - px(7.f)), px(14.f),
                                      theme::u32(p.text_secondary));
            }
            const ImVec2 center(node_x + px(12.f), top + px(12.f));
            // The streak: the lane colour at 10% (50% on the selected row) with a 2px lane-coloured right edge.
            rows->AddRectFilled(ImVec2(center.x, top), ImVec2(wmin.x, bottom),
                                theme::u32(theme::with_alpha(lane, (selected ? 0.5f : 0.1f) * alpha)));
            rows->AddRectFilled(ImVec2(wmin.x - px(2.f), top), ImVec2(wmin.x, bottom),
                                theme::u32(theme::with_alpha(lane, alpha)));
            rows->AddCircleFilled(center, px(12.f), theme::u32(theme::with_alpha(lane, alpha)));
            draw_avatar(rows, center, px(20.f), plan.authors[static_cast<std::size_t>(i)]);
            if (drop)
            {
                rows->AddCircleFilled(center, px(12.f), theme::u32(theme::with_alpha(p.app_bg0, 0.5f)));
            }

            // The message: 14px, 10px in; squashed ones disabled, dropped ones struck through.
            // A reworded commit shows its new summary.
            const std::string summary = step.action == git::RebaseAction::REWORD && !step.message.empty()
                                                ? step.message.substr(0, step.message.find('\n'))
                                                : plan.summaries[static_cast<std::size_t>(i)];
            const ImVec4 text = drop || squash ? p.text_disabled : (selected ? p.text_selected : p.text_normal);
            const float text_x = wmin.x + px(10.f);
            const float content = std::round(14.f * 2189.f / 2048.f) + std::round(14.f * 600.f / 2048.f);
            const float ascent = 14.f * 2189.f / 2048.f * dpi_scale();
            const float text_y = std::round(top + std::floor((24.f - content) / 2.f) * dpi_scale() + std::round(ascent))
                                 - std::ceil(ascent);
            const float shown = draw_text_ellipsis(rows, fonts().regular, 14.f, ImVec2(text_x, text_y),
                                                   wmax.x - text_x - px(10.f), theme::u32(text), summary.c_str());
            if (drop)
            {
                const float strike = std::round(top + px(13.f));
                rows->AddRectFilled(ImVec2(text_x, strike), ImVec2(text_x + shown, strike + 1.f), theme::u32(text));
            }
            if (plan.editing == i)
            {
                editor_pos = ImVec2(wmin.x - px(3.f), top + px(28.f));
            }
            ImGui::PopID();
        }
        ImGui::SetCursorScreenPos(list_origin);
        ImGui::Dummy(ImVec2(1.f, px(PADDING * 2.f) + px(ROW_HEIGHT) * static_cast<float>(count)));

        // The action select's menu: panel-bg1 options, 24px each.
        ImGui::SetNextWindowPos(menu_pos);
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, px(2.f)));
        ImGui::PushStyleVar(ImGuiStyleVar_PopupRounding, px(2.f));
        ImGui::PushStyleColor(ImGuiCol_PopupBg, p.panel_bg1);
        if (open_menu)
        {
            ImGui::OpenPopup("##rebase_action_menu");
        }
        if (ImGui::BeginPopup("##rebase_action_menu"))
        {
            for (int a = 0; a < 4; ++a)
            {
                const ImVec2 omin = ImGui::GetCursorScreenPos();
                ImGui::PushID(a);
                const bool picked = ImGui::InvisibleButton("##option", ImVec2(px(SELECT_WIDTH), px(24.f)));
                ImGui::PopID();
                ImDrawList* menu = ImGui::GetWindowDrawList();
                const bool current = menu_step >= 0 && menu_step < count
                                     && static_cast<int>(plan.steps[static_cast<std::size_t>(menu_step)].action) == a;
                if (ImGui::IsItemHovered() || current)
                {
                    menu->AddRectFilled(omin, ImVec2(omin.x + px(SELECT_WIDTH), omin.y + px(24.f)),
                                        theme::u32(ImGui::IsItemHovered() ? p.hover_row : p.selected_row));
                }
                line_text(menu, fonts().regular, 14.f, omin.x + px(6.f), omin.y, px(24.f), theme::u32(p.text_normal),
                          ACTION_LABELS[a]);
                if (picked && menu_step >= 0 && menu_step < count)
                {
                    set_action(state, menu_step, static_cast<git::RebaseAction>(a));
                    ImGui::CloseCurrentPopup();
                }
            }
            ImGui::EndPopup();
        }
        ImGui::PopStyleColor();
        ImGui::PopStyleVar(2);

        // Keys on the selected row, while no field has the keyboard.
        if (plan.selected >= 0 && plan.selected < count && !ImGui::GetIO().WantTextInput && plan.editing < 0
            && !ImGui::IsPopupOpen("##rebase_action_menu"))
        {
            const int s = plan.selected;
            const ImGuiIO& io = ImGui::GetIO();
            const bool ctrl = io.KeyCtrl;
            // The letters alone: Ctrl+P is the command palette, Ctrl+S / Ctrl+R / Ctrl+D other shortcuts.
            const bool plain = !io.KeyCtrl && !io.KeyAlt && !io.KeySuper;
            if (plain && ImGui::IsKeyPressed(ImGuiKey_P))
            {
                set_action(state, s, git::RebaseAction::PICK);
            }
            if (plain && ImGui::IsKeyPressed(ImGuiKey_S))
            {
                set_action(state, s, git::RebaseAction::SQUASH);
            }
            if (plain && ImGui::IsKeyPressed(ImGuiKey_R))
            {
                set_action(state, s, git::RebaseAction::REWORD);
            }
            if (plain && ImGui::IsKeyPressed(ImGuiKey_D))
            {
                set_action(state, s, git::RebaseAction::DROP);
            }
            // Up is newer (a higher step index), down older.
            if (ImGui::IsKeyPressed(ImGuiKey_UpArrow) && s + 1 < count)
            {
                if (ctrl)
                {
                    plan.swap_steps(s, s + 1);
                }
                select_step(state, s + 1);
            }
            if (ImGui::IsKeyPressed(ImGuiKey_DownArrow) && s > 0)
            {
                if (ctrl)
                {
                    plan.swap_steps(s, s - 1);
                }
                select_step(state, s - 1);
            }
        }
        ImGui::EndChild();
        if (plan.editing >= 0 && plan.editing < count && editor_pos.x >= 0.f)
        {
            draw_reword_editor(state, editor_pos);
        }

        // Footer: 10px 20px padding; the shortcuts on the left, the buttons on the right.
        {
            const float ftop = origin.y + size.y - px(FOOTER_HEIGHT);
            const float x0 = origin.x + px(PADDING);
            const ImU32 secondary = theme::u32(p.text_secondary);
            const ImU32 normal = theme::u32(p.text_normal);
            const float line1 = ftop + px(12.f);
            const float line2 = ftop + px(30.f);
            const float line_h = px(17.f);
            draw_fa_icon(draw, "far:keyboard", ImVec2(x0 + px(6.75f), line1 + px(8.5f)), px(12.f), secondary, 0.f);
            line_text(draw, fonts().regular, 12.f, x0 + px(18.5f), line1, line_h, secondary, "shortcuts:");
            float x = x0;
            for (const char* word : {"Pick", "Squash", "Reword", "Drop"})
            {
                const char first[2] = {word[0], '\0'};
                line_text(draw, fonts().semibold, 12.f, x, line2, line_h, normal, first);
                x += text_w(fonts().semibold, 12.f, first);
                line_text(draw, fonts().regular, 12.f, x, line2, line_h, secondary, word + 1);
                x += text_w(fonts().regular, 12.f, word + 1) + px(10.f);
            }
            for (const auto& [icon, label] : {std::pair{"fas:arrow-up", "Move Up"}, {"fas:arrow-down", "Move Down"}})
            {
                const std::string ctrl = shortcut_label("Ctrl + ");
                line_text(draw, fonts().semibold, 12.f, x, line2, line_h, normal, ctrl.c_str());
                x += text_w(fonts().semibold, 12.f, ctrl.c_str());
                draw_fa_icon(draw, icon, ImVec2(x + px(5.25f), line2 + px(8.5f)), px(12.f), normal, 0.f);
                x += px(10.5f) + px(5.f);
                line_text(draw, fonts().regular, 12.f, x, line2, line_h, secondary, label);
                x += text_w(fonts().regular, 12.f, label) + px(10.f);
            }

            const float btop = ftop + px(13.f);
            const float bh = px(31.140625f);
            float right = origin.x + size.x - px(PADDING);
            float w = 0.f;
            start = button(draw, "##start_rebase", "Start Rebase", right, btop, bh, 12.f, Tone::SUCCESS, &w);
            right -= w + px(20.f);
            cancel |= button(draw, "##cancel_rebase", "Cancel Rebase", right, btop, bh, 12.f, Tone::DANGER, &w);
            right -= w + px(20.f);
            if (button(draw, "##reset_rebase", "Reset", right, btop, bh, 12.f, Tone::PRIMARY, &w))
            {
                plan.reset();
                g_reword.step = -1;
            }
        }
        ImGui::EndChild();

        if (cancel)
        {
            plan = {};
            g_reword.step = -1;
            return;
        }
        if (start)
        {
            const git::Sha base = plan.base;
            const std::vector<git::RebaseStep> steps = plan.steps;
            git::Repository* repo = &state.repo;
            const git::Sha previous = state.history.head_sha;
            auto result = state.repo.rewrite_history(base, steps);
            if (!result)
            {
                app.toasts.push(app::ToastKind::ERROR, "Interactive rebase failed", result.error().message);
            }
            else
            {
                state.push_undo({"interactive rebase",
                                 [repo, previous]
                                 {
                                     return repo->reset(previous, git::ResetMode::HARD);
                                 },
                                 {}});
                app.toasts.push(app::ToastKind::SUCCESS, "Rebase complete");
                plan = {};
                g_reword.step = -1;
            }
            state.refresh();
        }
    }
} // namespace imgit::ui
