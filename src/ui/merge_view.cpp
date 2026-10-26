#include "app/application.hpp"
#include "app/theme.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <imgui.h>
#include <string>

// The merge tool, in place of the whole tab under the toolbar: a 38px header (the file, its conflict count, Open
// in external merge tool, Save, close), A (ours) and B (theirs) side by side, and the Output under them with the
// conflict navigator. Each conflict is highlighted in A / B with a checkbox on its first line that picks the whole
// side; a click on a line picks or drops that line alone. The Output shows the merge: agreed lines, the picked
// lines tagged A / B, and the base's lines (magenta) where nothing was picked yet; a double-click edits it as
// text. Save writes the Output and marks the file resolved. Positions are CSS px; code is Consolas 12px on 16px
// lines with an 82px margin (glyphs, line numbers, decorations).
namespace imgit::ui
{
    namespace
    {
        constexpr float HEADER_H = 38.2f;
        constexpr float PANE_HEADER_H = 35.f;
        constexpr float LINE_H = 16.f;
        constexpr float MARGIN_W = 82.f;
        constexpr float NUMBERS_RIGHT = 62.f;
        constexpr float DECORATION_X = 18.f; // the margin decoration: a 4px bar, then the side's tint
        constexpr float GLYPH_X = 67.f; // the in-output / add / remove glyph, 14.4px

        // The sides' colours: A sky blue, B amber, Output violet.
        constexpr ImVec4 rgb(unsigned hex, float alpha = 1.f)
        {
            return {static_cast<float>((hex >> 16) & 0xFF) / 255.f, static_cast<float>((hex >> 8) & 0xFF) / 255.f,
                    static_cast<float>(hex & 0xFF) / 255.f, alpha};
        }
        constexpr ImVec4 A_COLOR = rgb(0x2BA6E0);
        constexpr ImVec4 B_COLOR = rgb(0xF2B134);
        constexpr ImVec4 OUTPUT_COLOR = rgb(0xA85CF0);
        const ImU32 LINE_NUMBER = IM_COL32(133, 133, 133, 255);

        ImVec4 side_color(app::Side side)
        {
            return side == app::Side::A ? A_COLOR : B_COLOR;
        }

        float css_y(float size, float box_top, bool mono)
        {
            const float ascent = size * (mono ? 1884.f : 2189.f) / 2048.f * dpi_scale();
            return std::round(box_top + std::round(ascent)) - std::ceil(ascent);
        }

        // `text` in a `line_h` line box at `line_top` (device px), centred like Chromium.
        void line_text(ImDrawList* draw, ImFont* font, float size, float x, float line_top, float line_h, ImU32 color,
                       const char* text)
        {
            const float content = std::round(size * 2189.f / 2048.f) + std::round(size * 600.f / 2048.f);
            draw_text(draw, font, size,
                      ImVec2(std::round(x), css_y(size, line_top + std::floor((line_h - content) / 2.f) * dpi_scale(),
                                                  false)),
                      color, text);
        }

        void mono_text(ImDrawList* draw, float x, float line_top, ImU32 color, const char* text)
        {
            draw_text(draw, fonts().mono, 12.f, ImVec2(x, css_y(12.f, line_top + px(2.f), true)), color, text);
        }

        // The design-system checkbox: 12px, rgba(0,0,0,.2) with a 1px 40% white border, radius 1; checked: the
        // accent with a white check.
        bool checkbox(ImDrawList* draw, const char* id, ImVec2 min, bool checked)
        {
            const theme::Palette& p = theme::palette();
            const ImVec2 max(min.x + px(12.f), min.y + px(12.f));
            ImGui::SetCursorScreenPos(min);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(max.x - min.x, max.y - min.y));
            if (checked)
            {
                draw->AddRectFilled(min, max, theme::u32(p.accent), px(1.f));
                draw_fa_icon(draw, "fas:check", ImVec2((min.x + max.x) / 2.f, (min.y + max.y) / 2.f), px(9.f),
                             theme::u32(p.white));
            }
            else
            {
                draw->AddRectFilled(min, max, theme::u32(p.input_bg), px(1.f));
                draw->AddRect(min, max, theme::u32(theme::with_alpha(p.white, ImGui::IsItemHovered() ? 0.7f : 0.4f)),
                              px(1.f), 0, 1.f);
            }
            return clicked;
        }

        enum class Tone
        {
            PRIMARY,
            SUCCESS,
        };

        // A .btn-xs (22px, 1px 5px padding, 12px text) with an optional icon 10px after the label, its right edge
        // at `right`. Returns true when clicked; `width` gets its width.
        bool xs_button(ImDrawList* draw, const char* id, const char* label, const char* icon, float right, float top,
                       Tone tone, float* width = nullptr)
        {
            const theme::Palette& p = theme::palette();
            const ImVec4 color = tone == Tone::PRIMARY ? p.accent : p.green;
            const float label_w = label != nullptr ? text_size(fonts().regular, 12.f, label).x : 0.f;
            const float icon_w = icon != nullptr ? fa_icon_width(icon, px(12.f)) : 0.f;
            const float gap = label != nullptr && icon != nullptr ? px(10.f) : 0.f;
            const float w = label_w + gap + icon_w + px(12.f);
            const ImVec2 min(std::round(right - w), top);
            const ImVec2 max(min.x + w, top + px(22.f));
            ImGui::SetCursorScreenPos(min);
            const bool clicked = ImGui::InvisibleButton(id, ImVec2(w, max.y - min.y));
            const bool hovered = ImGui::IsItemHovered();
            draw->AddRectFilled(min, max, theme::u32(theme::with_alpha(color, hovered ? 0.3f : 0.2f)), px(2.f));
            draw->AddRect(min, max, theme::u32(color), px(2.f), 0, 1.f);
            const ImU32 text = theme::u32(hovered ? p.text_selected : p.text_normal);
            if (label != nullptr)
            {
                line_text(draw, fonts().regular, 12.f, min.x + px(6.f), min.y + px(2.f), px(18.f), text, label);
            }
            if (icon != nullptr)
            {
                draw_fa_icon(draw, icon, ImVec2(min.x + px(6.f) + label_w + gap + icon_w / 2.f, min.y + px(11.f)),
                             px(12.f), text);
            }
            if (width != nullptr)
            {
                *width = w;
            }
            return clicked;
        }

        // The decorations of one code line: a tinted background from the text on, and in the margin a 4px bar of
        // the solid colour then the tint.
        void decorate(ImDrawList* draw, ImVec4 color, float left, float top, float right)
        {
            const ImU32 tint = theme::u32(theme::with_alpha(color, 0.25f));
            draw->AddRectFilled(ImVec2(left + px(DECORATION_X), top), ImVec2(right, top + px(LINE_H)), tint);
            draw->AddRectFilled(ImVec2(left + px(DECORATION_X), top), ImVec2(left + px(DECORATION_X + 4.f), top + px(LINE_H)),
                                theme::u32(color));
        }

        void line_number(ImDrawList* draw, float left, float top, int number)
        {
            const std::string text = std::to_string(number);
            mono_text(draw, left + px(NUMBERS_RIGHT) - text_size(fonts().mono, 12.f, text.c_str()).x, top,
                      LINE_NUMBER, text.c_str());
        }

        // The glyph beside the line number: a green check for a line in the output; on hover a red minus to take
        // it out or a green plus to put it in.
        void glyph(ImDrawList* draw, float left, float top, bool picked, bool hovered)
        {
            const theme::Palette& p = theme::palette();
            const char* icon = nullptr;
            ImVec4 color = p.green;
            if (hovered)
            {
                icon = picked ? "fas:minus-circle" : "fas:plus-circle";
                color = picked ? p.red : p.green;
            }
            else if (picked)
            {
                icon = "fas:check-circle";
            }
            if (icon != nullptr)
            {
                draw_fa_icon(draw, icon, ImVec2(left + px(GLYPH_X + 7.2f), top + px(8.f)), px(14.f), theme::u32(color));
            }
        }

        // Scrolls the current child so `line` sits a few lines below the top.
        void scroll_to_line(int line)
        {
            ImGui::SetScrollY(std::max(0.f, static_cast<float>(line - 3)) * px(LINE_H));
        }

        // A (ours) or B (theirs): the side's file with its conflicts highlighted and pickable, its code coloured by
        // its syntax when `syntax_highlighting`.
        void draw_side(app::MergeTool& tool, app::Side side, ImVec2 origin, ImVec2 size, bool syntax_highlighting)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            const bool a = side == app::Side::A;
            const ImVec4 color = side_color(side);

            // Header: the take-all checkbox, the boxed letter, "Commit <sha6> on <branch>".
            const ImVec2 hmax(origin.x + size.x, origin.y + px(PANE_HEADER_H));
            draw->AddRectFilled(origin, hmax, theme::u32(p.panel_bg0));
            draw->AddRectFilled(ImVec2(origin.x, hmax.y - 1.f), hmax, theme::u32(p.section_border));
            const bool all = tool.side_fully_picked(-1, side);
            if (checkbox(draw, a ? "##take_all_a" : "##take_all_b", ImVec2(origin.x + px(12.f), origin.y + px(14.f)),
                         all)
                && !tool.edited)
            {
                tool.toggle_side(-1, side);
            }
            const ImVec2 bmin(origin.x + px(32.f), origin.y + px(5.f));
            const ImVec2 bmax(bmin.x + px(24.f), bmin.y + px(24.f));
            draw->AddRectFilled(bmin, bmax, theme::u32(p.panel_bg2));
            draw->AddRect(ImVec2(bmin.x + 1.f, bmin.y + 1.f), ImVec2(bmax.x - 1.f, bmax.y - 1.f), theme::u32(color), 0.f,
                          0, px(2.f));
            const char* letter = a ? "A" : "B";
            const ImVec2 letter_size = text_size(fonts().bold, 20.f, letter);
            draw_text(draw, fonts().bold, 20.f,
                      ImVec2(std::round((bmin.x + bmax.x - letter_size.x) / 2.f),
                             std::round((bmin.y + bmax.y - letter_size.y) / 2.f)),
                      theme::u32(color), letter);
            float x = origin.x + px(68.f);
            const float label_top = origin.y + px(7.f);
            const auto word = [&](ImFont* font, const std::string& text, ImU32 text_color)
            {
                line_text(draw, font, 14.f, x, label_top, px(19.f), text_color, text.c_str());
                x += text_size(font, 14.f, text.c_str()).x + px(4.f);
            };
            const std::string& sha = a ? tool.ours_sha : tool.theirs_sha;
            const std::string& branch = a ? tool.ours_label : tool.theirs_label;
            word(fonts().regular, "Commit", theme::u32(p.text_normal));
            word(fonts().regular, sha.substr(0, 6), theme::u32(p.text_normal));
            if (!branch.empty())
            {
                word(fonts().regular, "on", theme::u32(p.text_normal));
                x += px(3.f);
                word(fonts().bold, branch, theme::u32(color));
            }

            // The code, its syntax colours kept per side until its text changes.
            const std::vector<std::string>& lines = a ? tool.ours : tool.theirs;
            static syntax::TextCache syntax_caches[2];
            const syntax::LineSpans* colours =
                    syntax_highlighting ? syntax_caches[a ? 0 : 1].get(tool.path, lines, tool.revision) : nullptr;
            ImGui::SetCursorScreenPos(ImVec2(origin.x, hmax.y));
            ImGui::PushStyleColor(ImGuiCol_ChildBg, p.app_bg0);
            ImGui::BeginChild(a ? "##merge_a" : "##merge_b", ImVec2(size.x, size.y - px(PANE_HEADER_H)),
                              ImGuiChildFlags_None, ImGuiWindowFlags_HorizontalScrollbar);
            ImGui::PopStyleColor();
            if (tool.scroll_request >= 0 && tool.scroll_request < tool.conflict_count())
            {
                const app::MergeChunk& chunk =
                        tool.chunks[static_cast<std::size_t>(tool.conflicts[static_cast<std::size_t>(tool.scroll_request)])];
                scroll_to_line(a ? chunk.a_start : chunk.b_start);
            }
            ImDrawList* code = ImGui::GetWindowDrawList();
            const ImVec2 top_left = ImGui::GetCursorScreenPos();
            const float right = top_left.x + ImGui::GetContentRegionAvail().x;
            // A final line break shows as an empty last line, like any editor.
            const int shown = static_cast<int>(lines.size()) + (tool.final_newline && !lines.empty() ? 1 : 0);
            ImGuiListClipper clipper;
            clipper.Begin(shown, px(LINE_H));
            float widest = 0.f;
            while (clipper.Step())
            {
                for (int i = clipper.DisplayStart; i < clipper.DisplayEnd; ++i)
                {
                    const float top = top_left.y + static_cast<float>(i) * px(LINE_H);
                    if (i >= static_cast<int>(lines.size()))
                    {
                        line_number(code, top_left.x, top, i + 1);
                        continue;
                    }
                    const int conflict = tool.conflict_at(side, i);
                    ImGui::PushID(i);
                    if (conflict >= 0)
                    {
                        decorate(code, color, top_left.x, top, right);
                        const app::MergeChunk& chunk =
                                tool.chunks[static_cast<std::size_t>(tool.conflicts[static_cast<std::size_t>(conflict)])];
                        // The conflict's checkbox on its first line, in the glyph margin.
                        if (i == (a ? chunk.a_start : chunk.b_start)
                            && checkbox(code, "##take_side", ImVec2(top_left.x + px(2.f), top + px(2.f)),
                                        tool.side_fully_picked(conflict, side))
                            && !tool.edited)
                        {
                            tool.toggle_side(conflict, side);
                        }
                        // The rest of the line picks it alone.
                        ImGui::SetCursorScreenPos(ImVec2(top_left.x + px(16.f), top));
                        const bool clicked = ImGui::InvisibleButton("##pick_line",
                                                                    ImVec2(std::max(1.f, right - top_left.x - px(16.f)),
                                                                           px(LINE_H)));
                        const bool hovered = ImGui::IsItemHovered() && !tool.edited;
                        if (clicked && !tool.edited)
                        {
                            tool.toggle_line(conflict, side, i);
                        }
                        glyph(code, top_left.x, top, tool.is_picked(conflict, side, i), hovered);
                    }
                    ImGui::PopID();
                    line_number(code, top_left.x, top, i + 1);
                    const std::string& text = lines[static_cast<std::size_t>(i)];
                    draw_code_text(code, fonts().mono, 12.f,
                                   ImVec2(top_left.x + px(MARGIN_W), css_y(12.f, top + px(2.f), true)), text,
                                   colours != nullptr ? colours->line(static_cast<std::size_t>(i))
                                                      : std::span<const syntax::Span>(),
                                   theme::u32(p.text_normal));
                    widest = std::max(widest, text_size(fonts().mono, 12.f, text.c_str()).x);
                }
            }
            ImGui::SetCursorScreenPos(top_left);
            ImGui::Dummy(ImVec2(px(MARGIN_W) + widest + px(40.f), static_cast<float>(shown) * px(LINE_H) + size.y / 2.f));
            ImGui::EndChild();
        }

        // The Output: the merge as it stands, or the text being edited.
        void draw_output(app::MergeTool& tool, ImVec2 origin, ImVec2 size)
        {
            const theme::Palette& p = theme::palette();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            draw->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + 1.f), theme::u32(p.panel_border));
            const ImVec2 hmin(origin.x, origin.y + 1.f);
            const ImVec2 hmax(origin.x + size.x, hmin.y + px(PANE_HEADER_H));
            draw->AddRectFilled(hmin, hmax, theme::u32(p.panel_bg0));
            draw->AddRectFilled(ImVec2(hmin.x, hmax.y - 1.f), hmax, theme::u32(p.section_border));
            const char* title = "Output";
            line_text(draw, fonts().regular, 18.f, hmin.x + px(10.f), origin.y + px(6.1f), px(25.7f),
                      theme::u32(p.text_normal), title);
            const float title_end = hmin.x + px(10.f) + text_size(fonts().regular, 18.f, title).x;

            // Reset on the right once something was picked or typed.
            const float reset_right = hmax.x - px(10.f);
            if (tool.any_picked() || tool.edited)
            {
                if (xs_button(draw, "##merge_reset", "Reset", nullptr, reset_right, hmin.y + px(7.f), Tone::PRIMARY))
                {
                    tool.reset();
                }
            }
            // The navigator, centred between the title and the Reset area: "conflict N of M", up, down.
            const int count = tool.conflict_count();
            if (count > 0)
            {
                tool.current = std::clamp(tool.current, 0, count - 1);
                const std::string info = "conflict " + std::to_string(tool.current + 1) + " of " + std::to_string(count);
                const float info_w = text_size(fonts().semibold, 14.f, info.c_str()).x;
                const float group = info_w + px(10.f + 24.f + 10.f + 24.f);
                const float center = (title_end + hmax.x - px(65.f)) / 2.f;
                float x = std::round(center - group / 2.f);
                line_text(draw, fonts().semibold, 14.f, x, hmin.y + px(7.f), px(22.f), theme::u32(A_COLOR),
                          info.c_str());
                x += info_w + px(10.f);
                if (xs_button(draw, "##conflict_up", nullptr, "far:chevron-up", x + px(24.f), hmin.y + px(7.f),
                              Tone::PRIMARY))
                {
                    tool.current = (tool.current + count - 1) % count;
                    tool.scroll_request = tool.current;
                }
                x += px(24.f + 10.f);
                if (xs_button(draw, "##conflict_down", nullptr, "far:chevron-down", x + px(24.f), hmin.y + px(7.f),
                              Tone::PRIMARY))
                {
                    tool.current = (tool.current + 1) % count;
                    tool.scroll_request = tool.current;
                }
            }

            const ImVec2 body(origin.x, hmax.y + 1.f);
            const ImVec2 body_size(size.x, std::max(1.f, origin.y + size.y - body.y));
            if (tool.edited)
            {
                // Typed-in output: a plain text editor with the code font.
                ImGui::SetCursorScreenPos(body);
                ImGui::PushStyleColor(ImGuiCol_FrameBg, p.app_bg0);
                ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(MARGIN_W), px(2.f)));
                push_font(fonts().mono, 12.f);
                input_multiline("##merge_output_text", tool.edited_text, "", body_size);
                ImGui::PopFont();
                ImGui::PopStyleVar();
                ImGui::PopStyleColor();
                return;
            }

            const std::vector<app::OutputLine> lines = tool.output();
            ImGui::SetCursorScreenPos(body);
            ImGui::PushStyleColor(ImGuiCol_ChildBg, p.app_bg0);
            ImGui::BeginChild("##merge_output", body_size, ImGuiChildFlags_None, ImGuiWindowFlags_HorizontalScrollbar);
            ImGui::PopStyleColor();
            if (tool.scroll_request >= 0)
            {
                for (std::size_t i = 0; i < lines.size(); ++i)
                {
                    if (lines[i].conflict == tool.scroll_request)
                    {
                        scroll_to_line(static_cast<int>(i));
                        break;
                    }
                }
            }
            ImDrawList* code = ImGui::GetWindowDrawList();
            const ImVec2 top_left = ImGui::GetCursorScreenPos();
            const float right = top_left.x + ImGui::GetContentRegionAvail().x;
            // Line numbers skip the placeholders; count them up to each row once.
            std::vector<int> numbers(lines.size(), 0);
            int number = 0;
            for (std::size_t i = 0; i < lines.size(); ++i)
            {
                numbers[i] = lines[i].placeholder ? 0 : ++number;
            }
            const int shown = static_cast<int>(lines.size()) + (tool.final_newline && number > 0 ? 1 : 0);
            ImGuiListClipper clipper;
            clipper.Begin(shown, px(LINE_H));
            float widest = 0.f;
            int remove_conflict = -1;
            std::pair<app::Side, int> remove_pick{app::Side::A, -1};
            while (clipper.Step())
            {
                for (int i = clipper.DisplayStart; i < clipper.DisplayEnd; ++i)
                {
                    const float top = top_left.y + static_cast<float>(i) * px(LINE_H);
                    if (i >= static_cast<int>(lines.size()))
                    {
                        line_number(code, top_left.x, top, number + 1);
                        continue;
                    }
                    const app::OutputLine& line = lines[static_cast<std::size_t>(i)];
                    ImGui::PushID(i);
                    switch (line.origin)
                    {
                    case app::OutputLine::Origin::UNRESOLVED:
                        decorate(code, OUTPUT_COLOR, top_left.x, top, right);
                        break;
                    case app::OutputLine::Origin::PICKED_A:
                    case app::OutputLine::Origin::PICKED_B:
                    {
                        const bool a = line.origin == app::OutputLine::Origin::PICKED_A;
                        decorate(code, a ? A_COLOR : B_COLOR, top_left.x, top, right);
                        line_text(code, fonts().regular, 12.f, top_left.x + px(4.f), top, px(LINE_H),
                                  theme::u32(p.text_normal), a ? "A" : "B");
                        // The glyph takes the line back out.
                        ImGui::SetCursorScreenPos(ImVec2(top_left.x + px(GLYPH_X), top));
                        const bool clicked = ImGui::InvisibleButton("##drop_line", ImVec2(px(15.f), px(LINE_H)));
                        glyph(code, top_left.x, top, true, ImGui::IsItemHovered());
                        if (clicked)
                        {
                            remove_conflict = line.conflict;
                            remove_pick = {a ? app::Side::A : app::Side::B, line.source_line};
                        }
                        break;
                    }
                    default:
                        break;
                    }
                    ImGui::PopID();
                    if (numbers[static_cast<std::size_t>(i)] > 0)
                    {
                        line_number(code, top_left.x, top, numbers[static_cast<std::size_t>(i)]);
                    }
                    mono_text(code, top_left.x + px(MARGIN_W), top, theme::u32(p.text_normal), line.text.c_str());
                    widest = std::max(widest, text_size(fonts().mono, 12.f, line.text.c_str()).x);
                }
            }
            // A double-click on the text switches to editing the output by hand.
            ImGui::SetCursorScreenPos(ImVec2(top_left.x + px(MARGIN_W), top_left.y));
            const float content_h = static_cast<float>(shown) * px(LINE_H) + body_size.y / 2.f;
            ImGui::InvisibleButton("##output_text", ImVec2(std::max(1.f, right - top_left.x - px(MARGIN_W)), content_h));
            if (ImGui::IsItemHovered() && ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left))
            {
                tool.edited_text.clear();
                for (std::size_t i = 0; i < lines.size(); ++i)
                {
                    if (!lines[i].placeholder)
                    {
                        tool.edited_text += lines[i].text + "\n";
                    }
                }
                tool.edited = true;
            }
            ImGui::SetCursorScreenPos(top_left);
            ImGui::Dummy(ImVec2(px(MARGIN_W) + widest + px(40.f), content_h));
            ImGui::EndChild();
            if (remove_conflict >= 0)
            {
                tool.toggle_line(remove_conflict, remove_pick.first, remove_pick.second);
            }
        }
    } // namespace

    void draw_merge_view(app::Application& app, app::RepoState& state, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        app::MergeTool& tool = *state.merge_tool;
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        ImGui::BeginChild("##merge_view", size, ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + size.y), theme::u32(p.app_bg0));
        bool close = false;
        bool save = false;
        bool external = false;

        // Header: panel-bg0, 5px 10px padding, a 1px section border under it.
        {
            const ImVec2 hmax(origin.x + size.x, origin.y + px(HEADER_H));
            draw->AddRectFilled(origin, hmax, theme::u32(p.panel_bg0));
            draw->AddRectFilled(ImVec2(origin.x, hmax.y - 1.f), hmax, theme::u32(p.section_border));
            draw_fa_icon(draw, "fas:exclamation-triangle", ImVec2(origin.x + px(17.9f), origin.y + px(18.3f)), px(14.f),
                         theme::u32(p.orange));
            const std::string name = std::filesystem::path(tool.path).filename().string();
            line_text(draw, fonts().regular, 14.f, origin.x + px(30.8f), origin.y + px(8.6f), px(20.f),
                      theme::u32(p.text_normal), name.c_str());
            const int count = tool.conflict_count();
            const std::string conflicts =
                    "(" + std::to_string(count) + (count == 1 ? " conflict)" : " conflicts)");
            line_text(draw, fonts().regular, 12.6f,
                      origin.x + px(30.8f) + text_size(fonts().regular, 14.f, name.c_str()).x + px(10.f),
                      origin.y + px(10.6f), px(18.f), theme::u32(p.text_secondary), conflicts.c_str());

            // Right to left: close, Save, Open in external merge tool, 10px apart.
            const float top = origin.y + px(9.1f);
            float right = hmax.x - px(10.f);
            const ImVec2 cmin(right - px(18.5f), origin.y + px(7.6f));
            ImGui::SetCursorScreenPos(cmin);
            close = ImGui::InvisibleButton("##merge_close", ImVec2(px(18.5f), px(22.f)));
            draw_fa_icon(draw, "fas:times", ImVec2(cmin.x + px(9.25f), cmin.y + px(11.f)), px(20.f),
                         theme::u32(ImGui::IsItemHovered() ? p.text_selected : p.text_normal));
            right = cmin.x - px(10.f);
            float w = 0.f;
            save = xs_button(draw, "##merge_save", "Save", "far:save", right, top, Tone::SUCCESS, &w);
            right -= w + px(10.f);
            external = xs_button(draw, "##merge_external", "Open in external merge tool", "far:external-link-alt", right,
                                 top, Tone::PRIMARY, &w);
        }

        // A | B over the Output, halving the rest.
        const float body_top = origin.y + px(HEADER_H);
        const float body_h = size.y - px(HEADER_H);
        const float panes_h = std::round(body_h / 2.f);
        const float half = std::round(size.x / 2.f);
        draw_side(tool, app::Side::A, ImVec2(origin.x, body_top), ImVec2(half - 1.f, panes_h),
                  app.settings.syntax_highlighting);
        draw->AddRectFilled(ImVec2(origin.x + half - 1.f, body_top), ImVec2(origin.x + half, body_top + panes_h),
                            theme::u32(p.panel_border));
        draw_side(tool, app::Side::B, ImVec2(origin.x + half, body_top), ImVec2(size.x - half, panes_h),
                  app.settings.syntax_highlighting);
        draw_output(tool, ImVec2(origin.x, body_top + panes_h), ImVec2(size.x, body_h - panes_h));
        tool.scroll_request = -1;
        ImGui::EndChild();

        const std::string path = tool.path;
        if (save)
        {
            const std::string text = tool.output_text();
            const std::filesystem::path file = std::filesystem::path(state.repo.workdir()) / path;
            app.run(
                    state, "Save resolution",
                    [&]() -> git::Result<void>
                    {
                        std::ofstream out(file, std::ios::binary | std::ios::trunc);
                        out.write(text.data(), static_cast<std::streamsize>(text.size()));
                        if (!out)
                        {
                            return std::unexpected(git::Error{"could not write " + path});
                        }
                        out.close();
                        // Staging the file clears its conflict.
                        return state.repo.stage({path});
                    });
            state.merge_tool.reset();
        }
        else if (external)
        {
            state.merge_tool.reset();
            app.run_git_cli(state, "External merge tool", {"mergetool", "--no-prompt", "--", path});
        }
        else if (close)
        {
            state.merge_tool.reset();
        }
    }
} // namespace imgit::ui
