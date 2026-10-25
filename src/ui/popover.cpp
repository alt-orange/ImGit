#include "ui/popover.hpp"
#include "app/theme.hpp"
#include "git/types.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <cstring>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <unordered_map>

namespace imgit::ui
{
    namespace
    {
        constexpr float ROW_HEIGHT = 27.1429f; // 5 + 17.1429 (12px / line-height 1.4286) + 5
        constexpr float HEADER_HEIGHT = 25.1429f;
        const ImVec4& BACKGROUND = theme::palette().toolbar_bg2; // the current theme's, by reference
        const ImVec4 SHADOW(0.f, 0.f, 0.f, 0.176f);
        const ImVec4& HOVER = theme::palette().hover_row;
        const ImVec4& CURRENT = theme::palette().checked_out;
        const ImVec4& FOCUS = theme::palette().accent;
        const ImVec4 TEXT(1.f, 1.f, 1.f, 0.9f);
        const ImVec4 SECONDARY(1.f, 1.f, 1.f, 0.6f);

        struct PopoverState
        {
            bool just_opened = false;
            float y = 0.f; // layout cursor (logical px from the popover top), fractional like Chromium's
            float width = 0.f;
            ImVec2 origin;
            int row = 0;
            int keyboard = -1;
            bool enter = false;
        };
        std::unordered_map<ImGuiID, PopoverState> g_states;
        PopoverState* g_current = nullptr;

        float gauss_tail(float d, float sigma)
        {
            // Fraction of a Gaussian-blurred edge past distance d (0.5 * erfc(d / (sigma * sqrt 2))).
            return 0.5f * std::erfc(d / (sigma * 1.41421356f));
        }

        ImVec2 snap(ImVec2 p)
        {
            return ImVec2(std::round(p.x), std::round(p.y));
        }

        // Rows react to the mouse and to the keyboard selection.
        bool row_interaction(ImVec2 min, ImVec2 max, bool* hovered)
        {
            PopoverState& s = *g_current;
            const int index = s.row++;
            ImGui::SetCursorScreenPos(min);
            ImGui::PushID(index);
            const bool clicked = ImGui::InvisibleButton("##row", ImVec2(max.x - min.x, max.y - min.y));
            ImGui::PopID();
            *hovered = ImGui::IsItemHovered() || s.keyboard == index;
            if (ImGui::IsItemHovered() && ImGui::GetIO().MouseDelta.y != 0.f)
            {
                s.keyboard = -1;
            }
            if (*hovered)
            {
                ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
            }
            return clicked || (s.enter && s.keyboard == index);
        }
    } // namespace

    void draw_box_shadow(ImDrawList* draw, ImVec2 min, ImVec2 max, ImVec2 offset, float blur, float spread,
                         ImVec4 color, float rounding)
    {
        // Concentric 1px rings: each pixel ring at signed distance d from the shadow box gets the Gaussian tail.
        const float sigma = std::max(blur / 2.f, 0.5f);
        const ImVec2 smin(min.x + offset.x - spread, min.y + offset.y - spread);
        const ImVec2 smax(max.x + offset.x + spread, max.y + offset.y + spread);
        draw->PushClipRectFullScreen();
        const int reach = static_cast<int>(std::ceil(blur * 1.5f));
        for (int i = -reach; i < reach; ++i)
        {
            const float d = static_cast<float>(i) + 0.5f;
            ImVec4 c = color;
            c.w *= gauss_tail(d, sigma);
            if (c.w < 0.002f)
            {
                continue;
            }
            const float grow = d;
            if (smax.x - smin.x + 2.f * grow <= 0.f || smax.y - smin.y + 2.f * grow <= 0.f)
            {
                continue;
            }
            draw->AddRect(ImVec2(smin.x - grow, smin.y - grow), ImVec2(smax.x + grow, smax.y + grow),
                          ImGui::ColorConvertFloat4ToU32(c), std::max(0.f, rounding + grow), 0, 1.f);
        }
        draw->PopClipRect();
    }

    void open_popover(const char* id)
    {
        ImGui::OpenPopup(id);
        g_states[ImGui::GetID(id)].just_opened = true;
    }

    void close_popover()
    {
        ImGui::CloseCurrentPopup();
    }

    bool begin_popover(const char* id, ImVec2 pos, float width)
    {
        const ImGuiID key = ImGui::GetID(id);
        if (!ImGui::IsPopupOpen(id))
        {
            return false;
        }
        ImGui::SetNextWindowPos(snap(pos));
        ImGui::SetNextWindowSize(ImVec2(px(width), 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_PopupBorderSize, 0.f);
        ImGui::PushStyleVar(ImGuiStyleVar_PopupRounding, 0.f);
        ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(0.f, 0.f));
        ImGui::PushStyleColor(ImGuiCol_PopupBg, ImVec4(0.f, 0.f, 0.f, 0.f));
        const bool open = ImGui::BeginPopup(id, ImGuiWindowFlags_NoMove | ImGuiWindowFlags_AlwaysAutoResize
                                                        | ImGuiWindowFlags_NoScrollbar);
        ImGui::PopStyleColor();
        ImGui::PopStyleVar(4);
        if (!open)
        {
            return false;
        }
        PopoverState& state = g_states[key];
        state.y = 0.f;
        state.width = width;
        state.origin = ImGui::GetWindowPos();
        state.row = 0;
        state.enter = ImGui::IsKeyPressed(ImGuiKey_Enter) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter);
        g_current = &state;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->ChannelsSplit(2);
        draw->ChannelsSetCurrent(1);
        if (ImGui::IsKeyPressed(ImGuiKey_Escape))
        {
            ImGui::CloseCurrentPopup();
        }
        return true;
    }

    void end_popover()
    {
        PopoverState& state = *g_current;
        if (ImGui::IsKeyPressed(ImGuiKey_DownArrow))
        {
            state.keyboard = std::min(state.keyboard + 1, state.row - 1);
        }
        if (ImGui::IsKeyPressed(ImGuiKey_UpArrow))
        {
            state.keyboard = std::max(state.keyboard - 1, 0);
        }
        // Reserve the laid-out height so the window sizes to it.
        ImGui::SetCursorScreenPos(ImVec2(state.origin.x, state.origin.y));
        ImGui::Dummy(ImVec2(px(state.width), px(state.y)));
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->ChannelsSetCurrent(0);
        const ImVec2 min = state.origin;
        const ImVec2 max(min.x + px(state.width), min.y + std::round(px(state.y)));
        draw_box_shadow(draw, min, max, ImVec2(0.f, px(6.f)), px(12.f), 0.f, SHADOW);
        draw->AddRectFilled(min, max, theme::u32(BACKGROUND));
        draw->ChannelsMerge();
        state.just_opened = false;
        g_current = nullptr;
        ImGui::EndPopup();
    }

    void popover_reset_keyboard()
    {
        if (g_current != nullptr)
        {
            g_current->keyboard = -1;
        }
    }

    bool popover_search(std::string& text, const char* placeholder)
    {
        // .pl1.pr1.pt1 (5px) around a 34px .form-control with 40px left padding; focused: #4D88FF border and a
        // 1px #4D88FF ring.
        PopoverState& s = *g_current;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 min = snap(ImVec2(s.origin.x + px(5.f), s.origin.y + px(s.y + 5.f)));
        const ImVec2 max(min.x + px(s.width - 10.f), min.y + px(34.f));
        draw->AddRectFilled(ImVec2(min.x - 1.f, min.y - 1.f), ImVec2(max.x + 1.f, max.y + 1.f), theme::u32(FOCUS),
                            px(5.f));
        draw->AddRectFilled(min, max, theme::u32(BACKGROUND), px(4.f));
        draw->AddRectFilled(ImVec2(min.x + 1.f, min.y + 1.f), ImVec2(max.x - 1.f, max.y - 1.f),
                            theme::u32(ImVec4(0.f, 0.f, 0.f, 0.2f)), px(3.f));
        // The border is drawn over the inner fill's edge pixels: 1px #4D88FF.
        draw->AddRect(ImVec2(min.x + 0.5f, min.y + 0.5f), ImVec2(max.x - 0.5f, max.y - 0.5f), theme::u32(FOCUS),
                      px(4.f), 0, 1.f);
        draw_fa_icon(draw, "far:search", ImVec2(min.x + px(17.f), min.y + px(17.f)), px(14.f), theme::u32(SECONDARY));

        ImFont* font = fonts().regular;
        const float ascent = 14.f * 2189.f / 2048.f * dpi_scale();
        const float baseline = std::round(min.y + px((34.f - 19.f) / 2.f) + std::round(ascent));
        ImGui::SetCursorScreenPos(ImVec2(min.x + px(40.f), baseline - std::ceil(ascent)));
        if (s.just_opened)
        {
            ImGui::SetKeyboardFocusHere();
        }
        ImGui::PushStyleColor(ImGuiCol_FrameBg, ImVec4(0.f, 0.f, 0.f, 0.f));
        ImGui::PushStyleColor(ImGuiCol_Text, TEXT);
        ImGui::PushStyleColor(ImGuiCol_TextDisabled, ImVec4(1.f, 1.f, 1.f, 0.4f));
        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(0.f, 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
        push_font(font, 14.f);
        ImGui::SetNextItemWidth(max.x - min.x - px(52.f));
        const bool changed = ImGui::InputTextWithHint("##popover_search", placeholder, &text);
        text_field_menu(text);
        ImGui::PopFont();
        ImGui::PopStyleVar(2);
        ImGui::PopStyleColor(3);
        if (changed)
        {
            s.keyboard = 0;
        }
        s.y += 44.f;
        return changed;
    }

    void popover_header(const char* text, const char* icon)
    {
        // padding 5px 20px 3px; the icon (12px) then 5px; 12px text-secondary.
        PopoverState& s = *g_current;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const float top = s.origin.y + px(s.y + 5.f);
        float x = s.origin.x + px(20.f);
        if (icon != nullptr)
        {
            draw_fa_icon(draw, icon, ImVec2(x + px(6.f), std::round(top) + px(8.5f)), px(12.f), theme::u32(SECONDARY));
            x += px(17.f);
        }
        draw_text(draw, fonts().regular, 12.f, ImVec2(x, std::round(top)), theme::u32(SECONDARY), text);
        s.y += HEADER_HEIGHT;
    }

    bool popover_row(const char* text, const char* icon, bool current)
    {
        PopoverState& s = *g_current;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 min(s.origin.x, std::round(s.origin.y + px(s.y)));
        const ImVec2 max(s.origin.x + px(s.width), std::round(s.origin.y + px(s.y + ROW_HEIGHT)));
        bool hovered = false;
        const bool clicked = row_interaction(min, max, &hovered);
        if (current)
        {
            draw->AddRectFilled(min, max, theme::u32(CURRENT));
        }
        else if (hovered)
        {
            draw->AddRectFilled(min, max, theme::u32(HOVER));
        }
        const ImU32 color = theme::u32(current || hovered ? ImVec4(1.f, 1.f, 1.f, 1.f) : TEXT);
        const float top = std::round(s.origin.y + px(s.y + 5.f));
        float x = s.origin.x + px(20.f);
        if (icon != nullptr)
        {
            draw_fa_icon(draw, icon, ImVec2(x + px(4.5f), top + px(8.5f)), px(12.f), color);
            x += px(14.f);
        }
        draw_text_ellipsis(draw, fonts().regular, 12.f, ImVec2(x, top), s.origin.x + px(s.width - 20.f) - x, color,
                           text);
        s.y += ROW_HEIGHT;
        return clicked;
    }

    void popover_note(const char* text)
    {
        PopoverState& s = *g_current;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const float wrap = px(s.width - 20.f);
        ImFont* font = fonts().regular;
        const float size = font_px(font, 12.f);
        const float top = std::round(s.origin.y + px(s.y + 10.f));
        // line-height 17.14px for 12px text: the 17px text box sits at the top of each line.
        const ImVec2 measured = font->CalcTextSizeA(size, FLT_MAX, wrap, text);
        const int lines = std::max(1, static_cast<int>(std::round(measured.y / size)));
        const char* line = text;
        const char* end = text + std::strlen(text);
        for (int i = 0; i < lines && line < end; ++i)
        {
            const char* line_end = font->CalcWordWrapPosition(size, line, end, wrap);
            if (line_end == line)
            {
                line_end = end;
            }
            draw_text(draw, font, 12.f, ImVec2(s.origin.x + px(10.f), top + std::round(px(17.1429f * i))),
                      theme::u32(SECONDARY), line, line_end);
            line = line_end;
            while (line < end && *line == ' ')
            {
                ++line;
            }
        }
        s.y += 20.f + 17.1429f * static_cast<float>(lines);
    }

    bool popover_radio_row(const char* text, bool selected)
    {
        PopoverState& s = *g_current;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 min(s.origin.x, std::round(s.origin.y + px(s.y)));
        const ImVec2 max(s.origin.x + px(s.width), std::round(s.origin.y + px(s.y + ROW_HEIGHT)));
        bool hovered = false;
        const bool clicked = row_interaction(min, max, &hovered);
        if (selected)
        {
            draw->AddRectFilled(min, max, theme::u32(CURRENT));
        }
        else if (hovered)
        {
            draw->AddRectFilled(min, max, theme::u32(HOVER));
        }
        const ImU32 color = theme::u32(selected ? ImVec4(1.f, 1.f, 1.f, 1.f) : TEXT);
        const float top = std::round(s.origin.y + px(s.y + 5.f));
        draw_fa_icon(draw, selected ? "far:dot-circle" : "far:circle",
                     ImVec2(s.origin.x + px(17.f), std::round(s.origin.y + px(s.y + 6.5f)) + px(7.f)), px(14.f),
                     selected ? color : theme::u32(SECONDARY));
        draw_text(draw, fonts().regular, 12.f, ImVec2(s.origin.x + px(34.f), top), color, text);
        s.y += ROW_HEIGHT;
        return clicked;
    }

    void popover_padding(float height)
    {
        g_current->y += height;
    }

    bool popover_menu_item(const char* text)
    {
        constexpr float HEIGHT = 6.f + 17.1429f + 6.f;
        PopoverState& s = *g_current;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 min(s.origin.x, std::round(s.origin.y + px(s.y)));
        const ImVec2 max(s.origin.x + px(s.width), std::round(s.origin.y + px(s.y + HEIGHT)));
        bool hovered = false;
        const bool clicked = row_interaction(min, max, &hovered);
        if (hovered)
        {
            draw->AddRectFilled(min, max, theme::u32(HOVER));
        }
        draw_text(draw, fonts().regular, 12.f, ImVec2(s.origin.x + px(12.f), std::round(s.origin.y + px(s.y + 6.f))),
                  theme::u32(hovered ? ImVec4(1.f, 1.f, 1.f, 1.f) : TEXT), text);
        s.y += HEIGHT;
        return clicked;
    }

    bool popover_link_row(const char* text)
    {
        PopoverState& s = *g_current;
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 min(s.origin.x, std::round(s.origin.y + px(s.y)));
        const ImVec2 max(s.origin.x + px(s.width), std::round(s.origin.y + px(s.y + ROW_HEIGHT)));
        bool hovered = false;
        const bool clicked = row_interaction(min, max, &hovered);
        if (hovered)
        {
            draw->AddRectFilled(min, max, theme::u32(HOVER));
        }
        const float top = std::round(s.origin.y + px(s.y + 5.f));
        const ImU32 link = theme::u32(theme::palette().link);
        draw_text(draw, fonts().regular, 12.f, ImVec2(s.origin.x + px(23.f), top), link, text);
        if (hovered)
        {
            const float w = text_size(fonts().regular, 12.f, text).x;
            draw->AddRectFilled(ImVec2(s.origin.x + px(23.f), top + px(14.f)),
                                ImVec2(s.origin.x + px(23.f) + w, top + px(15.f)), link);
        }
        s.y += ROW_HEIGHT;
        return clicked;
    }
} // namespace imgit::ui
