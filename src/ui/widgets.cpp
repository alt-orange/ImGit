#include "ui/widgets.hpp"
#include "app/theme.hpp"
#include "gfx/lcd_renderer.hpp"
#include "git/repository.hpp"
#include "platform/system.hpp"
#include "ui/avatars.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include <algorithm>
#include <cctype>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <unordered_map>
#include <utility>

namespace imgit::ui
{
    namespace
    {
        float g_dpi_scale = 1.f;
        DragCursor g_drag_cursor = DragCursor::NONE;
        int g_animating_frame = -1;
        Fonts g_fonts;

        struct AnimState
        {
            float value = 0.f;
            int last_frame = -1;
        };
        std::unordered_map<ImGuiID, AnimState> g_anims;

        // A CSS transition between 0 and 1: a change of target restarts it from the value shown.
        struct Transition
        {
            float from = 0.f;
            float to = 0.f;
            float shown = 0.f;
            double start = 0.0;
            bool started = false;
        };
        std::unordered_map<ImGuiID, Transition> g_transitions;

        // CSS `ease`: cubic-bezier(0.25, 0.1, 0.25, 1), solved for x by Newton's method.
        float css_ease(float x)
        {
            const auto bezier = [](float t, float p1, float p2)
            {
                const float u = 1.f - t;
                return 3.f * u * u * t * p1 + 3.f * u * t * t * p2 + t * t * t;
            };
            float t = x;
            for (int i = 0; i < 8; ++i)
            {
                // dx/dt of the x curve (both x control points 0.25).
                const float u = 1.f - t;
                const float slope = 0.75f * u * u + 2.25f * t * t;
                if (slope < 1e-6f)
                {
                    break;
                }
                t = std::clamp(t - (bezier(t, 0.25f, 0.25f) - x) / slope, 0.f, 1.f);
            }
            return bezier(t, 0.1f, 1.f);
        }

        bool styled_button(const char* label, ImVec2 size, bool enabled, ImVec4 border, ImVec4 bg, ImVec4 hover,
                           ImVec4 text, ImVec4 text_hover)
        {
            const bool hovered_before = ImGui::GetHoveredID() == ImGui::GetID(label);
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 1.f);
            ImGui::PushStyleVar(ImGuiStyleVar_FrameRounding, px(3.f));
            ImGui::PushStyleColor(ImGuiCol_Border, border);
            ImGui::PushStyleColor(ImGuiCol_Button, bg);
            ImGui::PushStyleColor(ImGuiCol_ButtonHovered, hover);
            ImGui::PushStyleColor(ImGuiCol_ButtonActive, hover);
            ImGui::PushStyleColor(ImGuiCol_Text, hovered_before ? text_hover : text);
            if (!enabled)
            {
                ImGui::BeginDisabled();
            }
            const bool pressed = ImGui::Button(label, size);
            if (!enabled)
            {
                ImGui::EndDisabled();
            }
            ImGui::PopStyleColor(5);
            ImGui::PopStyleVar(2);
            return pressed;
        }
    } // namespace

    Fonts& fonts() noexcept
    {
        return g_fonts;
    }

    void set_dpi_scale(float scale) noexcept
    {
        g_dpi_scale = scale;
    }

    float dpi_scale() noexcept
    {
        return g_dpi_scale;
    }

    float px(float logical) noexcept
    {
        return std::round(logical * g_dpi_scale);
    }

    std::string shortcut_label(std::string_view label)
    {
        std::string result(label);
        if (!ImGui::GetIO().ConfigMacOSXBehaviors)
        {
            return result;
        }
        const auto is_word_char = [](char c)
        {
            return std::isalnum(static_cast<unsigned char>(c)) != 0;
        };
        for (const auto& [from, to] : {std::pair{"Ctrl", "Cmd"}, std::pair{"Alt", "Option"}})
        {
            const std::string_view word = from;
            for (std::size_t at = result.find(word); at != std::string::npos; at = result.find(word, at))
            {
                const bool starts = at == 0 || !is_word_char(result[at - 1]);
                const bool ends = at + word.size() == result.size() || !is_word_char(result[at + word.size()]);
                if (starts && ends)
                {
                    result.replace(at, word.size(), to);
                    at += std::strlen(to);
                }
                else
                {
                    at += word.size();
                }
            }
        }
        return result;
    }

    ImGuiKey shortcut_modifier_event() noexcept
    {
        return ImGui::GetIO().ConfigMacOSXBehaviors ? ImGuiMod_Super : ImGuiMod_Ctrl;
    }

    void set_drag_cursor(DragCursor cursor) noexcept
    {
        g_drag_cursor = cursor;
    }

    DragCursor take_drag_cursor() noexcept
    {
        const DragCursor cursor = g_drag_cursor;
        g_drag_cursor = DragCursor::NONE;
        return cursor;
    }

    namespace
    {
        // (usWinAscent + usWinDescent) / unitsPerEm from the font file's OS/2 and head tables, as DirectWrite (and
        // Chromium on Windows) report a face's height: Open Sans 2789, Segoe UI 2724, Consolas 2398 (its hhea is
        // smaller), Arial 2288, all over 2048. Zero when the tables are missing.
        float em_scale_from_file(const unsigned char* data, int size, int font_no) noexcept
        {
            const auto u16 = [&](std::size_t at) -> std::uint32_t
            {
                return at + 2 <= static_cast<std::size_t>(size)
                               ? static_cast<std::uint32_t>(data[at] << 8 | data[at + 1])
                               : 0u;
            };
            const auto u32 = [&](std::size_t at)
            {
                return u16(at) << 16 | u16(at + 2);
            };
            std::size_t base = 0;
            if (u32(0) == 0x74746366) // 'ttcf': a collection, offsets to each font's table directory
            {
                base = u32(12 + 4 * static_cast<std::size_t>(std::max(0, font_no)));
            }
            std::size_t head = 0;
            std::size_t os2 = 0;
            const std::uint32_t tables = u16(base + 4);
            for (std::uint32_t i = 0; i < tables; ++i)
            {
                const std::size_t record = base + 12 + 16 * static_cast<std::size_t>(i);
                const std::uint32_t tag = u32(record);
                if (tag == 0x68656164) // 'head'
                {
                    head = u32(record + 8);
                }
                else if (tag == 0x4F532F32) // 'OS/2'
                {
                    os2 = u32(record + 8);
                }
            }
            const std::uint32_t units_per_em = head != 0 ? u16(head + 18) : 0;
            if (units_per_em == 0 || os2 == 0)
            {
                return 0.f;
            }
            return static_cast<float>(u16(os2 + 74) + u16(os2 + 76)) / static_cast<float>(units_per_em);
        }
    } // namespace

    float font_em_scale(const ImFont* font) noexcept
    {
        constexpr float OPEN_SANS = 2789.f / 2048.f;
        if (font == nullptr || font->Sources.Size == 0 || font->Sources[0]->FontData == nullptr)
        {
            return OPEN_SANS;
        }
        // Measured once per font; ImGit loads a handful.
        static std::pair<const ImFont*, float> cache[16] = {};
        for (const auto& [cached, scale] : cache)
        {
            if (cached == font)
            {
                return scale;
            }
        }
        const ImFontConfig* source = font->Sources[0];
        float scale = em_scale_from_file(static_cast<const unsigned char*>(source->FontData), source->FontDataSize,
                                         source->FontNo);
        if (scale <= 0.f)
        {
            scale = OPEN_SANS;
        }
        for (auto& slot : cache)
        {
            if (slot.first == nullptr)
            {
                slot = {font, scale};
                break;
            }
        }
        return scale;
    }

    float font_px(const ImFont* font, float css_size) noexcept
    {
        return css_size * g_dpi_scale * font_em_scale(font);
    }

    void push_font(ImFont* font, float css_size)
    {
        ImGui::PushFont(font, css_size * font_em_scale(font));
    }

    float ease_in_out(float t) noexcept
    {
        t = std::clamp(t, 0.f, 1.f);
        return t < 0.5f ? 2.f * t * t : 1.f - std::pow(-2.f * t + 2.f, 2.f) / 2.f;
    }

    float smooth_value(const char* id, float target, float seconds)
    {
        AnimState& state = g_anims[ImGui::GetID(id)];
        const int frame = ImGui::GetFrameCount();
        if (state.last_frame < 0)
        {
            state.value = target; // first use: no animation from nothing
        }
        else if (state.last_frame != frame)
        {
            const float step = seconds <= 0.f ? 1.f : ImGui::GetIO().DeltaTime / seconds;
            if (state.value < target)
            {
                state.value = std::min(target, state.value + step);
            }
            else if (state.value > target)
            {
                state.value = std::max(target, state.value - step);
            }
        }
        state.last_frame = frame;
        if (state.value != target)
        {
            g_animating_frame = frame;
        }
        return state.value;
    }

    bool animating() noexcept
    {
        return g_animating_frame >= 0 && g_animating_frame >= ImGui::GetFrameCount() - 1;
    }

    void keep_animating() noexcept
    {
        g_animating_frame = ImGui::GetFrameCount();
    }

    float smooth(const char* id, bool on, float seconds)
    {
        Transition& transition = g_transitions[ImGui::GetID(id)];
        const float target = on ? 1.f : 0.f;
        const double now = ImGui::GetTime();
        if (!transition.started)
        {
            transition = {target, target, target, now, true}; // first use: no animation from nothing
        }
        else if (target != transition.to)
        {
            transition.from = transition.shown;
            transition.to = target;
            transition.start = now;
        }
        const float progress =
                seconds <= 0.f ? 1.f : std::clamp(static_cast<float>((now - transition.start) / seconds), 0.f, 1.f);
        transition.shown = transition.from + (transition.to - transition.from) * css_ease(progress);
        if (progress < 1.f)
        {
            g_animating_frame = ImGui::GetFrameCount();
        }
        return transition.shown;
    }

    ImVec2 text_size(ImFont* font, float size, const char* text, const char* text_end)
    {
        return font->CalcTextSizeA(font_px(font, size), FLT_MAX, 0.f, text, text_end);
    }

    std::vector<std::string_view> wrap_text(ImFont* font, float size, std::string_view text, float width)
    {
        const float glyph_h = font_px(font, size);
        std::vector<std::string_view> lines;
        std::size_t start = 0;
        while (start <= text.size())
        {
            std::size_t end = text.find('\n', start);
            if (end == std::string_view::npos)
            {
                end = text.size();
            }
            const char* line = text.data() + start;
            const char* line_end = text.data() + end;
            if (line == line_end)
            {
                lines.emplace_back();
            }
            while (line < line_end)
            {
                const char* wrap = font->CalcWordWrapPosition(glyph_h, line, line_end, width);
                if (wrap == line)
                {
                    // Not even one character fits: take one whole UTF-8 character (never half of one).
                    wrap = line + 1;
                    while (wrap < line_end && (static_cast<unsigned char>(*wrap) & 0xC0) == 0x80)
                    {
                        ++wrap;
                    }
                }
                lines.emplace_back(line, static_cast<std::size_t>(wrap - line));
                line = wrap;
                while (line < line_end && *line == ' ')
                {
                    ++line;
                }
            }
            start = end + 1;
        }
        return lines;
    }

    void draw_text(ImDrawList* draw, ImFont* font, float size, ImVec2 pos, ImU32 color, const char* text,
                   const char* text_end)
    {
        // ImGui truncates the start position; Chromium places text at fractional x (the LCD renderer resolves
        // 1/3 px), so the quads are moved by the dropped fraction.
        const int first = draw->VtxBuffer.Size;
        draw->AddText(font, font_px(font, size), pos, color, text, text_end);
        const float fraction = pos.x - std::floor(pos.x);
        if (fraction > 0.01f)
        {
            for (int i = first; i < draw->VtxBuffer.Size; ++i)
            {
                draw->VtxBuffer[i].pos.x += fraction;
            }
        }
    }

    void draw_code_text(ImDrawList* draw, ImFont* font, float size, ImVec2 pos, std::string_view text,
                        std::span<const syntax::Span> spans, ImU32 plain)
    {
        if (spans.empty())
        {
            draw_text(draw, font, size, pos, plain, text.data(), text.data() + text.size());
            return;
        }
        const theme::Palette& p = theme::palette();
        const auto color = [&](syntax::Token token)
        {
            switch (token)
            {
            case syntax::Token::COMMENT:
                return theme::u32(p.syntax_comment);
            case syntax::Token::STRING:
                return theme::u32(p.syntax_string);
            case syntax::Token::NUMBER:
                return theme::u32(p.syntax_number);
            case syntax::Token::KEYWORD:
                return theme::u32(p.syntax_keyword);
            case syntax::Token::TYPE:
                return theme::u32(p.syntax_type);
            case syntax::Token::FUNCTION:
                return theme::u32(p.syntax_function);
            case syntax::Token::VARIABLE:
                return theme::u32(p.syntax_variable);
            case syntax::Token::OPERATOR:
                return theme::u32(p.syntax_operator);
            case syntax::Token::PUNCTUATION:
                return theme::u32(p.syntax_punctuation);
            case syntax::Token::TAG:
                return theme::u32(p.syntax_tag);
            case syntax::Token::ATTRIBUTE:
                return theme::u32(p.syntax_attribute);
            case syntax::Token::REGEXP:
                return theme::u32(p.syntax_regexp);
            case syntax::Token::CONSTANT:
                return theme::u32(p.syntax_constant);
            default:
                return plain;
            }
        };
        // Span by span, each after the width of the ones before; nothing past the clip rect.
        const float clip_right = draw->GetClipRectMax().x;
        float x = pos.x;
        for (const syntax::Span& span : spans)
        {
            if (x > clip_right || span.end > text.size())
            {
                break;
            }
            const char* begin = text.data() + span.begin;
            const char* end = text.data() + span.end;
            draw_text(draw, font, size, ImVec2(x, pos.y), color(span.token), begin, end);
            x += text_size(font, size, begin, end).x;
        }
    }

    float draw_text_ellipsis(ImDrawList* draw, ImFont* font, float size, ImVec2 pos, float max_width, ImU32 color,
                             const char* text, const char* text_end)
    {
        if (text_end == nullptr)
        {
            text_end = text + strlen(text);
        }
        if (max_width <= 0.f || text == text_end)
        {
            return 0.f;
        }
        const float font_size = font_px(font, size);
        const ImVec2 full = font->CalcTextSizeA(font_size, FLT_MAX, 0.f, text, text_end);
        if (full.x <= max_width)
        {
            draw->AddText(font, font_size, pos, color, text, text_end);
            return full.x;
        }
        const char* ellipsis = "\xE2\x80\xA6"; // U+2026, as CSS text-overflow: ellipsis
        const float ellipsis_width = font->CalcTextSizeA(font_size, FLT_MAX, 0.f, ellipsis).x;
        const float budget = max_width - ellipsis_width;
        const char* cursor = text;
        const char* fit_end = text;
        while (cursor < text_end)
        {
            unsigned int code_point = 0;
            const int bytes = ImTextCharFromUtf8(&code_point, cursor, text_end);
            const char* next = cursor + std::max(bytes, 1);
            const float width = font->CalcTextSizeA(font_size, FLT_MAX, 0.f, text, next).x;
            if (width > budget)
            {
                break;
            }
            fit_end = next;
            cursor = next;
        }
        float used = 0.f;
        if (fit_end > text)
        {
            draw->AddText(font, font_size, pos, color, text, fit_end);
            used = font->CalcTextSizeA(font_size, FLT_MAX, 0.f, text, fit_end).x;
        }
        draw->AddText(font, font_size, ImVec2(pos.x + used, pos.y), color, ellipsis);
        return used + ellipsis_width;
    }

    void draw_dashed_line(ImDrawList* draw, ImVec2 a, ImVec2 b, ImU32 color, float thickness, float dash, float gap)
    {
        const float dx = b.x - a.x;
        const float dy = b.y - a.y;
        const float length = std::sqrt(dx * dx + dy * dy);
        if (length <= 0.f)
        {
            return;
        }
        const float ux = dx / length;
        const float uy = dy / length;
        const float step = px(dash) + px(gap);
        for (float t = 0.f; t < length; t += step)
        {
            const float end = std::min(t + px(dash), length);
            draw->AddLine(ImVec2(a.x + ux * t, a.y + uy * t), ImVec2(a.x + ux * end, a.y + uy * end), color, thickness);
        }
    }

    std::string initials(const git::Author& author)
    {
        std::string result;
        const std::string& source = author.name.empty() ? author.email : author.name;
        bool word_start = true;
        for (const char c : source)
        {
            if (c == ' ' || c == '.' || c == '-' || c == '_' || c == '@')
            {
                word_start = true;
                if (c == '@')
                {
                    break;
                }
                continue;
            }
            if (word_start && std::isalnum(static_cast<unsigned char>(c)))
            {
                result += static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
                if (result.size() == 2)
                {
                    break;
                }
            }
            word_start = false;
        }
        if (result.empty())
        {
            result = "?";
        }
        return result;
    }

    ImVec4 avatar_color(const std::string& email)
    {
        if (email.empty())
        {
            return theme::palette().avatar_default;
        }
        std::uint32_t hash = 2166136261u;
        for (const char c : email)
        {
            hash ^= static_cast<unsigned char>(std::tolower(static_cast<unsigned char>(c)));
            hash *= 16777619u;
        }
        const float hue = static_cast<float>(hash % 360u) / 360.f;
        float r, g, b;
        ImGui::ColorConvertHSVtoRGB(hue, 0.45f, 0.62f, r, g, b);
        return {r, g, b, 1.f};
    }

    void draw_avatar(ImDrawList* draw, ImVec2 center, float diameter, const git::Author& author, float rounding)
    {
        const float radius = diameter / 2.f;
        const float corner = rounding < 0.f ? radius : rounding;
        if (const ImTextureID texture = avatar_texture(author.email, static_cast<int>(std::lround(diameter)));
            texture != ImTextureID_Invalid)
        {
            draw->AddImageRounded(ImTextureRef(texture), ImVec2(center.x - radius, center.y - radius),
                                  ImVec2(center.x + radius, center.y + radius), ImVec2(0.f, 0.f), ImVec2(1.f, 1.f),
                                  IM_COL32_WHITE, corner);
            return;
        }
        draw->AddRectFilled(ImVec2(center.x - radius, center.y - radius), ImVec2(center.x + radius, center.y + radius),
                            theme::u32(avatar_color(author.email)), corner);
        const std::string text = initials(author);
        const float font_size = diameter * 0.42f / g_dpi_scale;
        const ImVec2 size = text_size(g_fonts.semibold, font_size, text.c_str());
        draw->AddText(g_fonts.semibold, font_px(g_fonts.semibold, font_size),
                      ImVec2(center.x - size.x / 2.f, center.y - size.y / 2.f), theme::u32(theme::palette().white),
                      text.c_str());
    }

    float pill_width(const char* text, bool bold, float height, float extra)
    {
        const float font_size = height * 0.62f;
        return text_size(bold ? g_fonts.semibold : g_fonts.regular, font_size, text).x + px(12.f) + px(extra);
    }

    bool pill(const char* id, const char* text, ImVec4 bg, ImVec4 bg_hover, ImVec4 fg, bool bold, float height,
              float max_width, ImVec2* out_size)
    {
        const float h = px(height);
        const float font_size = height * 0.62f;
        ImFont* font = bold ? g_fonts.semibold : g_fonts.regular;
        float width = text_size(font, font_size, text).x + px(12.f);
        if (max_width > 0.f)
        {
            width = std::min(width, max_width);
        }
        const ImVec2 pos = ImGui::GetCursorScreenPos();
        ImGui::InvisibleButton(id, ImVec2(std::max(width, 1.f), h));
        const bool hovered = ImGui::IsItemHovered();
        ImDrawList* draw = ImGui::GetWindowDrawList();
        draw->AddRectFilled(pos, ImVec2(pos.x + width, pos.y + h), theme::u32(hovered ? bg_hover : bg), px(2.f));
        const ImVec2 size = text_size(font, font_size, text);
        draw_text_ellipsis(draw, font, font_size, ImVec2(pos.x + px(6.f), pos.y + (h - size.y) / 2.f), width - px(12.f),
                           theme::u32(fg), text);
        if (out_size != nullptr)
        {
            *out_size = ImVec2(width, h);
        }
        return ImGui::IsItemClicked(ImGuiMouseButton_Left);
    }

    bool primary_button(const char* label, ImVec2 size, bool enabled)
    {
        const theme::Palette& p = theme::palette();
        return styled_button(label, size, enabled, p.accent, theme::with_alpha(p.accent, 0.2f),
                             theme::with_alpha(p.accent, 0.6f), p.text_normal, p.text_selected);
    }

    bool default_button(const char* label, ImVec2 size, bool enabled)
    {
        const theme::Palette& p = theme::palette();
        return styled_button(label, size, enabled, p.text_normal, ImVec4(0, 0, 0, 0), ImVec4(0, 0, 0, 0), p.text_normal,
                             p.text_selected);
    }

    bool danger_button(const char* label, ImVec2 size, bool enabled)
    {
        const theme::Palette& p = theme::palette();
        return styled_button(label, size, enabled, p.red, theme::with_alpha(p.red, 0.2f),
                             theme::with_alpha(p.red, 0.6f), p.text_normal, p.text_selected);
    }

    bool success_button(const char* label, ImVec2 size, bool enabled)
    {
        const theme::Palette& p = theme::palette();
        return styled_button(label, size, enabled, p.green, theme::with_alpha(p.green, 0.2f),
                             theme::with_alpha(p.green, 0.6f), p.text_normal, p.text_selected);
    }

    bool text_button(const char* label, ImVec2 size, bool enabled)
    {
        const theme::Palette& p = theme::palette();
        return styled_button(label, size, enabled, ImVec4(0, 0, 0, 0), ImVec4(0, 0, 0, 0), p.hover_row, p.text_normal,
                             p.text_selected);
    }

    bool link_text(const char* label)
    {
        const theme::Palette& p = theme::palette();
        ImGui::PushStyleColor(ImGuiCol_Text, p.link);
        ImGui::TextUnformatted(label);
        ImGui::PopStyleColor();
        const bool hovered = ImGui::IsItemHovered();
        if (hovered)
        {
            ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
            const ImVec2 min = ImGui::GetItemRectMin();
            const ImVec2 max = ImGui::GetItemRectMax();
            ImGui::GetWindowDrawList()->AddLine(ImVec2(min.x, max.y), ImVec2(max.x, max.y), theme::u32(p.link), 1.f);
        }
        return hovered && ImGui::IsMouseClicked(ImGuiMouseButton_Left);
    }

    bool segmented(const char* id, const char* const* labels, int count, int* selected, float height)
    {
        const theme::Palette& p = theme::palette();
        const float h = px(height);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        float total = 0.f;
        std::vector<float> widths;
        for (int i = 0; i < count; ++i)
        {
            const float w = text_size(g_fonts.regular, theme::FONT_SIZE_SMALL, labels[i]).x + px(20.f);
            widths.push_back(w);
            total += w;
        }
        bool changed = false;
        ImGui::PushID(id);
        draw->AddRect(origin, ImVec2(origin.x + total, origin.y + h), theme::u32(p.toggle_border), px(3.f), 0, 1.f);
        float x = origin.x;
        for (int i = 0; i < count; ++i)
        {
            ImGui::SetCursorScreenPos(ImVec2(x, origin.y));
            ImGui::PushID(i);
            ImGui::InvisibleButton("##seg", ImVec2(widths[static_cast<std::size_t>(i)], h));
            const bool hovered = ImGui::IsItemHovered();
            if (ImGui::IsItemClicked(ImGuiMouseButton_Left) && *selected != i)
            {
                *selected = i;
                changed = true;
            }
            const bool active = *selected == i;
            if (active)
            {
                draw->AddRectFilled(ImVec2(x, origin.y), ImVec2(x + widths[static_cast<std::size_t>(i)], origin.y + h),
                                    theme::u32(p.selected_row), px(3.f));
            }
            const ImVec2 size = text_size(g_fonts.regular, theme::FONT_SIZE_SMALL, labels[i]);
            draw_text(draw, g_fonts.regular, theme::FONT_SIZE_SMALL,
                      ImVec2(x + (widths[static_cast<std::size_t>(i)] - size.x) / 2.f, origin.y + (h - size.y) / 2.f),
                      theme::u32(active || hovered ? p.text_selected : p.text_normal), labels[i]);
            ImGui::PopID();
            x += widths[static_cast<std::size_t>(i)];
            if (i + 1 < count)
            {
                draw->AddLine(ImVec2(x, origin.y + px(4.f)), ImVec2(x, origin.y + h - px(4.f)),
                              theme::u32(p.toggle_border), 1.f);
            }
        }
        ImGui::PopID();
        ImGui::SetCursorScreenPos(ImVec2(origin.x, origin.y));
        ImGui::Dummy(ImVec2(total, h));
        return changed;
    }

    bool input_text(const char* id, std::string& value, const char* hint, ImGuiInputTextFlags flags, float width)
    {
        ImGui::SetNextItemWidth(width);
        const bool changed = hint != nullptr ? ImGui::InputTextWithHint(id, hint, &value, flags)
                                             : ImGui::InputText(id, &value, flags);
        text_field_menu(value);
        return changed;
    }

    bool input_multiline(const char* id, std::string& value, const char* hint, ImVec2 size)
    {
        const ImVec2 pos = ImGui::GetCursorScreenPos();
        const bool changed = ImGui::InputTextMultiline(id, &value, size);
        text_field_menu(value);
        if (value.empty() && hint != nullptr && !ImGui::IsItemActive())
        {
            const ImGuiStyle& style = ImGui::GetStyle();
            ImGui::GetWindowDrawList()->AddText(ImVec2(pos.x + style.FramePadding.x, pos.y + style.FramePadding.y),
                                                theme::u32(theme::palette().text_disabled), hint);
        }
        return changed;
    }

    bool checkbox(const char* label, bool* value)
    {
        const theme::Palette& p = theme::palette();
        ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(px(2.f), px(2.f)));
        ImGui::PushStyleColor(ImGuiCol_FrameBg, p.input_bg);
        ImGui::PushStyleColor(ImGuiCol_CheckMark, p.text_inverse);
        ImGui::PushStyleColor(ImGuiCol_FrameBgHovered, theme::with_alpha(p.link, 0.3f));
        ImGui::PushStyleColor(ImGuiCol_FrameBgActive, p.link);
        if (*value)
        {
            ImGui::PushStyleColor(ImGuiCol_FrameBg, p.link);
        }
        const bool changed = ImGui::Checkbox(label, value);
        if (!changed && *value)
        {
            ImGui::PopStyleColor();
        }
        else if (changed && !*value)
        {
            ImGui::PopStyleColor();
        }
        ImGui::PopStyleColor(4);
        ImGui::PopStyleVar();
        return changed;
    }

    void section_label(const char* text)
    {
        push_font(g_fonts.semibold, theme::FONT_SIZE_TINY);
        ImGui::PushStyleColor(ImGuiCol_Text, theme::palette().text_secondary);
        ImGui::TextUnformatted(to_upper(text).c_str());
        ImGui::PopStyleColor();
        ImGui::PopFont();
    }

    void secondary_text(const char* text)
    {
        ImGui::PushStyleColor(ImGuiCol_Text, theme::palette().text_secondary);
        ImGui::TextUnformatted(text);
        ImGui::PopStyleColor();
    }

    ResizeHandle resize_handle(const char* id, ImVec2 min, ImVec2 max, bool columns)
    {
        ResizeHandle result;
        const ImGuiID item = ImGui::GetID(id);
        const ImRect box(min, max);
        if (!ImGui::ItemAdd(box, item))
        {
            return result;
        }
        bool hovered = false;
        ImGui::ButtonBehavior(box, item, &hovered, &result.held);
        result.activated = ImGui::IsItemActivated();
        if (hovered || result.held)
        {
            if (columns)
            {
                set_drag_cursor(DragCursor::COL_RESIZE);
            }
            else
            {
                set_drag_cursor(DragCursor::ROW_RESIZE);
            }
        }
        return result;
    }

    bool resize_drag(const char* id, ImVec2 min, ImVec2 max, bool columns, float direction, float* size, float min_size,
                     float max_size)
    {
        ImGuiStorage* storage = ImGui::GetStateStorage();
        const ImGuiID start_size = ImGui::GetID((std::string(id) + "#size").c_str());
        const ImGuiID start_mouse = ImGui::GetID((std::string(id) + "#mouse").c_str());
        const ImGuiID was_held = ImGui::GetID((std::string(id) + "#held").c_str());
        const ResizeHandle handle = resize_handle(id, min, max, columns);
        const ImVec2 mouse = ImGui::GetIO().MousePos;
        const float position = columns ? mouse.x : mouse.y;
        if (handle.activated)
        {
            storage->SetFloat(start_size, *size);
            storage->SetFloat(start_mouse, position);
        }
        if (handle.held)
        {
            const float travel = (position - storage->GetFloat(start_mouse)) * direction;
            *size = std::max(min_size, std::min(storage->GetFloat(start_size) + travel, max_size));
            storage->SetBool(was_held, true);
            return false;
        }
        if (storage->GetBool(was_held))
        {
            storage->SetBool(was_held, false);
            return *size != storage->GetFloat(start_size);
        }
        return false;
    }

    ScrollbarX scrollbar_x(const char* id, ImVec2 min, ImVec2 max, float* scroll, float content, float view)
    {
        ScrollbarX bar;
        const float track = max.x - min.x;
        const float range = std::max(0.f, content - view);
        if (range <= 0.f || track <= 0.f)
        {
            return bar;
        }
        const auto place = [&]
        {
            bar.thumb_w = std::max(px(20.f), track * view / content);
            bar.thumb_x = min.x + (track - bar.thumb_w) * std::clamp(*scroll / range, 0.f, 1.f);
        };
        place();
        const ImGuiID item = ImGui::GetID(id);
        const ImRect box(min, max);
        if (!ImGui::ItemAdd(box, item))
        {
            return bar;
        }
        ImGui::ButtonBehavior(box, item, &bar.hovered, &bar.held);
        ImGuiStorage* storage = ImGui::GetStateStorage();
        const ImGuiID grab = ImGui::GetID((std::string(id) + "#grab").c_str());
        const float mouse = ImGui::GetIO().MousePos.x;
        if (ImGui::IsItemActivated())
        {
            if (mouse >= bar.thumb_x && mouse < bar.thumb_x + bar.thumb_w)
            {
                storage->SetFloat(grab, mouse - bar.thumb_x);
            }
            else
            {
                // Chromium pages by 87.5% of the view towards the press.
                storage->SetFloat(grab, -1.f);
                const float page = view * 0.875f;
                *scroll = std::clamp(*scroll + (mouse < bar.thumb_x ? -page : page), 0.f, range);
            }
        }
        else if (bar.held && storage->GetFloat(grab, -1.f) >= 0.f && track > bar.thumb_w)
        {
            const float left = mouse - storage->GetFloat(grab) - min.x;
            *scroll = std::clamp(left / (track - bar.thumb_w), 0.f, 1.f) * range;
        }
        place();
        return bar;
    }

    void draw_scrollbar_x(ImDrawList* draw, ImVec2 min, ImVec2 max, const ScrollbarX& bar)
    {
        if (bar.thumb_w <= 0.f)
        {
            return;
        }
        const ImGuiStyle& style = ImGui::GetStyle();
        draw->AddRectFilled(min, max, theme::u32(theme::palette().app_bg0));
        const ImGuiCol color = bar.held      ? ImGuiCol_ScrollbarGrabActive
                               : bar.hovered ? ImGuiCol_ScrollbarGrabHovered
                                             : ImGuiCol_ScrollbarGrab;
        draw->AddRectFilled(ImVec2(bar.thumb_x, min.y), ImVec2(bar.thumb_x + bar.thumb_w, max.y),
                            ImGui::GetColorU32(style.Colors[color]), (max.y - min.y) * 5.f / 6.f);
    }

    bool overlay_resize_drag(const char* id, ImVec2 min, ImVec2 max, bool columns, float direction, float* size,
                             float min_size, float max_size)
    {
        // A top-level window created after the root's children is hit-tested before them (child windows follow
        // their root in ImGui's window order); NoBringToFrontOnFocus would put it behind the root instead.
        ImGui::SetNextWindowPos(min);
        ImGui::SetNextWindowSize(ImVec2(max.x - min.x, max.y - min.y));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowMinSize, ImVec2(1.f, 1.f));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
        ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
        ImGui::Begin(id, nullptr,
                     ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoBackground | ImGuiWindowFlags_NoSavedSettings
                             | ImGuiWindowFlags_NoFocusOnAppearing | ImGuiWindowFlags_NoNav | ImGuiWindowFlags_NoMove
                             | ImGuiWindowFlags_NoScrollWithMouse);
        const bool released = resize_drag(id, min, max, columns, direction, size, min_size, max_size);
        ImGui::End();
        ImGui::PopStyleVar(3);
        return released;
    }

    ImVec4 status_color(git::FileStatus status)
    {
        const theme::Palette& p = theme::palette();
        switch (status)
        {
        case git::FileStatus::ADDED:
        case git::FileStatus::UNTRACKED:
            return p.green;
        case git::FileStatus::MODIFIED:
            return p.stats_files;
        case git::FileStatus::DELETED:
        case git::FileStatus::CONFLICTED:
            return p.red;
        case git::FileStatus::RENAMED:
            return p.ltblue;
        case git::FileStatus::TYPECHANGE:
            return p.purple;
        default:
            return p.text_secondary;
        }
    }

    void draw_status_badge(ImDrawList* draw, ImVec2 pos, float size, git::FileStatus status)
    {
        const ImVec4 color = status_color(status);
        draw->AddRectFilled(pos, ImVec2(pos.x + size, pos.y + size), theme::u32(theme::with_alpha(color, 0.25f)),
                            px(2.f));
        const std::string letter = git::status_letter(status);
        const float font_size = size * 0.7f / g_dpi_scale;
        const ImVec2 text = text_size(g_fonts.semibold, font_size, letter.c_str());
        draw->AddText(g_fonts.semibold, font_px(g_fonts.semibold, font_size),
                      ImVec2(pos.x + (size - text.x) / 2.f, pos.y + (size - text.y) / 2.f), theme::u32(color),
                      letter.c_str());
    }

    void draw_status_icon(ImDrawList* draw, ImVec2 center, float size, git::FileStatus status)
    {
        const ImU32 color = theme::u32(status_color(status));
        switch (status)
        {
        case git::FileStatus::ADDED:
        case git::FileStatus::UNTRACKED:
            draw_icon(draw, Icon::PLUS, center, size, color);
            break;
        case git::FileStatus::DELETED:
            draw_icon(draw, Icon::MINUS, center, size, color);
            break;
        case git::FileStatus::RENAMED:
            draw_icon(draw, Icon::ARROW_RIGHT, center, size, color);
            break;
        case git::FileStatus::CONFLICTED:
            draw_icon(draw, Icon::WARNING, center, size, color);
            break;
        default:
            draw_icon(draw, Icon::PENCIL, center, size, color);
            break;
        }
    }

    namespace
    {
        std::tm local_time(std::int64_t unix_seconds)
        {
            return platform::local_time(static_cast<std::time_t>(unix_seconds));
        }
    } // namespace

    std::string format_date_time(std::int64_t unix_seconds)
    {
        // ImGit: "10/24/2026 @ 4:43 PM" (no leading zeros on month, day and hour).
        const std::tm tm = local_time(unix_seconds);
        const int hour = tm.tm_hour % 12 == 0 ? 12 : tm.tm_hour % 12;
        char buffer[64];
        std::snprintf(buffer, sizeof(buffer), "%d/%d/%d @ %d:%02d %s", tm.tm_mon + 1, tm.tm_mday, tm.tm_year + 1900,
                      hour, tm.tm_min, tm.tm_hour < 12 ? "AM" : "PM");
        return buffer;
    }

    std::string format_date(std::int64_t unix_seconds)
    {
        const std::tm tm = local_time(unix_seconds);
        char buffer[32];
        std::snprintf(buffer, sizeof(buffer), "%d/%d/%d", tm.tm_mon + 1, tm.tm_mday, tm.tm_year + 1900);
        return buffer;
    }

    std::string format_date_long(std::int64_t unix_seconds)
    {
        const std::tm tm = local_time(unix_seconds);
        char buffer[96];
        std::strftime(buffer, sizeof(buffer), "%A, %B %d, %Y %I:%M %p", &tm);
        return buffer;
    }

    std::string format_relative(std::int64_t unix_seconds)
    {
        const auto now =
                std::chrono::duration_cast<std::chrono::seconds>(std::chrono::system_clock::now().time_since_epoch())
                        .count();
        const std::int64_t delta = std::max<std::int64_t>(0, now - unix_seconds);
        if (delta < 60)
        {
            return "just now";
        }
        if (delta < 3600)
        {
            return std::to_string(delta / 60) + " minutes ago";
        }
        if (delta < 86400)
        {
            const auto hours = delta / 3600;
            return hours == 1 ? "1 hour ago" : std::to_string(hours) + " hours ago";
        }
        if (delta < 86400 * 2)
        {
            return "yesterday";
        }
        if (delta < 86400 * 7)
        {
            return std::to_string(delta / 86400) + " days ago";
        }
        if (delta < 86400 * 30)
        {
            const auto weeks = delta / (86400 * 7);
            return weeks == 1 ? "a week ago" : std::to_string(weeks) + " weeks ago";
        }
        if (delta < 86400 * 365)
        {
            const auto months = delta / (86400 * 30);
            return months == 1 ? "a month ago" : std::to_string(months) + " months ago";
        }
        const auto years = delta / (86400 * 365);
        return years == 1 ? "a year ago" : std::to_string(years) + " years ago";
    }

    std::string file_name(const std::string& path)
    {
        const auto slash = path.find_last_of('/');
        return slash == std::string::npos ? path : path.substr(slash + 1);
    }

    std::string directory_name(const std::string& path)
    {
        const auto slash = path.find_last_of('/');
        return slash == std::string::npos ? std::string{} : path.substr(0, slash);
    }

    std::string to_upper(std::string text)
    {
        for (char& c : text)
        {
            c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
        }
        return text;
    }

    bool contains_ci(const std::string& haystack, const std::string& needle)
    {
        if (needle.empty())
        {
            return true;
        }
        const auto it = std::search(haystack.begin(), haystack.end(), needle.begin(), needle.end(),
                                    [](char a, char b)
                                    {
                                        return std::tolower(static_cast<unsigned char>(a))
                                               == std::tolower(static_cast<unsigned char>(b));
                                    });
        return it != haystack.end();
    }

    int fuzzy_score(const std::string& haystack, const std::string& needle)
    {
        if (needle.empty())
        {
            return 0;
        }
        if (contains_ci(haystack, needle))
        {
            return 1000 - static_cast<int>(haystack.size());
        }
        int score = 0;
        std::size_t h = 0;
        int streak = 0;
        for (const char raw : needle)
        {
            const char n = static_cast<char>(std::tolower(static_cast<unsigned char>(raw)));
            if (n == ' ')
            {
                continue;
            }
            bool found = false;
            while (h < haystack.size())
            {
                const char c = static_cast<char>(std::tolower(static_cast<unsigned char>(haystack[h])));
                ++h;
                if (c == n)
                {
                    found = true;
                    score += 10 + streak * 5;
                    ++streak;
                    break;
                }
                streak = 0;
            }
            if (!found)
            {
                return -1;
            }
        }
        return score - static_cast<int>(haystack.size()) / 4;
    }

    namespace
    {
        enum class TooltipSide
        {
            BOTTOM,
            TOP,
            LEFT,
        };

        void show_tooltip(ImVec2 anchor_min, ImVec2 anchor_max, const char* text, TooltipSide side, float wait)
        {
            // Bootstrap's OverlayTrigger shows it after a short delay and fades it in over .15s.
            const std::string platform_text = shortcut_label(text);
            text = platform_text.c_str();
            static ImGuiID shown_for = 0;
            static double since = 0.0;
            static int last_frame = -1;
            const ImGuiID key = ImGui::GetID(text) ^ static_cast<ImGuiID>(anchor_min.x * 31.f + anchor_min.y);
            const double now = ImGui::GetTime();
            const int frame = ImGui::GetFrameCount();
            if (shown_for != key || last_frame < frame - 1)
            {
                shown_for = key;
                since = now;
            }
            last_frame = frame;
            const float t = static_cast<float>(now - since);
            if (t < wait)
            {
                return;
            }
            const float alpha = 0.9f * std::clamp((t - wait) / 0.15f, 0.f, 1.f);
            ImFont* font = g_fonts.arial != nullptr ? g_fonts.arial : g_fonts.regular;
            const ImVec2 size = text_size(font, 12.f, text);
            const float inner_w = std::round(size.x + px(16.f));
            const float inner_h = std::round(px(23.1f));
            const float cx = std::round((anchor_min.x + anchor_max.x) / 2.f);
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            float x = std::round(cx - inner_w / 2.f);
            x = std::clamp(x, viewport->Pos.x + px(2.f), viewport->Pos.x + viewport->Size.x - inner_w - px(2.f));
            ImDrawList* draw = ImGui::GetForegroundDrawList();
            const ImU32 black = IM_COL32(0, 0, 0, static_cast<int>(alpha * 255.f));
            const ImU32 white = IM_COL32(255, 255, 255, static_cast<int>(alpha * 255.f));
            float top;
            if (side == TooltipSide::LEFT)
            {
                // .tooltip.left: margin-left -3px, padding 0 5px with the 5x10 arrow in it.
                const float tip = std::round(anchor_min.x - px(3.f));
                x = tip - px(5.f) - inner_w;
                top = std::round((anchor_min.y + anchor_max.y) / 2.f - inner_h / 2.f);
                const float cy = top + inner_h / 2.f;
                draw->AddTriangleFilled(ImVec2(tip - px(5.f), cy - px(5.f)), ImVec2(tip, cy),
                                        ImVec2(tip - px(5.f), cy + px(5.f)), black);
            }
            else if (side == TooltipSide::TOP)
            {
                const float arrow_tip = std::round(anchor_min.y + px(3.f));
                top = arrow_tip - px(5.f) - inner_h;
                draw->AddTriangleFilled(ImVec2(cx - px(5.f), arrow_tip - px(5.f)),
                                        ImVec2(cx + px(5.f), arrow_tip - px(5.f)), ImVec2(cx, arrow_tip), black);
            }
            else
            {
                const float arrow_tip = std::round(anchor_max.y - px(3.f));
                top = arrow_tip + px(5.f);
                draw->AddTriangleFilled(ImVec2(cx, arrow_tip), ImVec2(cx + px(5.f), top), ImVec2(cx - px(5.f), top),
                                        black);
            }
            draw->AddRectFilled(ImVec2(x, top), ImVec2(x + inner_w, top + inner_h), black, px(4.f));
            // 17.14px line box in 23.1 (3px padding): the 14px Arial text box 1.6px into it.
            const float ascent = 12.f * 1854.f / 2048.f * dpi_scale();
            const float baseline = std::round(top + px(3.f) + px((17.1429f - 14.f) / 2.f) + std::round(ascent));
            // Chromium composites the faded tooltip on its own layer: grey-scale anti-aliasing.
            gfx::set_grayscale_text(draw, true);
            draw_text(draw, font, 12.f, ImVec2(x + px(8.f), baseline - std::ceil(ascent)), white, text);
            gfx::set_grayscale_text(draw, false);
        }
    } // namespace

    void styled_tooltip(ImVec2 anchor_min, ImVec2 anchor_max, const char* text, bool above)
    {
        show_tooltip(anchor_min, anchor_max, text, above ? TooltipSide::TOP : TooltipSide::BOTTOM, 0.35f);
    }

    void styled_tooltip_left(ImVec2 anchor_min, ImVec2 anchor_max, const char* text)
    {
        show_tooltip(anchor_min, anchor_max, text, TooltipSide::LEFT, 0.25f);
    }

    void tooltip(const char* text)
    {
        if (ImGui::IsItemHovered())
        {
            styled_tooltip(ImGui::GetItemRectMin(), ImGui::GetItemRectMax(), text);
        }
    }

    void tooltip_on_rect(ImVec2 min, ImVec2 max, const char* text, bool above)
    {
        const ImVec2 mouse = ImGui::GetIO().MousePos;
        if (ImGui::IsWindowHovered(ImGuiHoveredFlags_ChildWindows) && mouse.x >= min.x && mouse.x < max.x
            && mouse.y >= min.y && mouse.y < max.y)
        {
            styled_tooltip(min, max, text, above);
        }
    }

    namespace
    {
        // The Font Awesome icon for each Icon; empty when it has none.
        const char* fa_name(Icon icon) noexcept
        {
            switch (icon)
            {
            case Icon::PULL:
                return "far:arrow-to-bottom";
            case Icon::PUSH:
                return "far:arrow-from-bottom";
            case Icon::BRANCH:
                return "far:code-branch";
            case Icon::STASH:
                return "far:inbox-in";
            case Icon::POP:
                return "far:inbox-out";
            case Icon::TAG:
                return "far:tag";
            case Icon::REMOTE:
                return "far:cloud";
            case Icon::FOLDER:
                return "far:folder";
            case Icon::LAPTOP:
                return "far:laptop";
            case Icon::CLOSE:
                return "far:times";
            case Icon::PLUS:
                return "far:plus";
            case Icon::MINUS:
                return "far:minus";
            case Icon::BACK:
                return "fas:chevron-circle-left";
            case Icon::REFRESH:
                return "far:sync";
            case Icon::CHEVRON_DOWN:
                return "far:chevron-down";
            case Icon::CHEVRON_RIGHT:
                return "far:chevron-right";
            case Icon::CHEVRON_LEFT:
                return "far:chevron-left";
            case Icon::CHEVRON_UP:
                return "far:chevron-up";
            case Icon::CHECK:
                return "far:check";
            case Icon::WARNING:
                return "far:exclamation-triangle";
            case Icon::COPY:
                return "far:copy";
            case Icon::UNDO:
                return "far:undo-alt";
            case Icon::REDO:
                return "far:redo-alt";
            case Icon::TERMINAL:
                return "far:terminal";
            case Icon::SEARCH:
                return "far:search";
            case Icon::GEAR:
                return "far:cog";
            case Icon::BELL:
                return "far:bell";
            case Icon::ROCKET:
                return "far:rocket";
            case Icon::TRASH:
                return "far:trash-alt";
            case Icon::EYE:
                return "far:eye";
            case Icon::EYE_OFF:
                return "far:eye-slash";
            case Icon::CLOUD:
                return "far:cloud";
            case Icon::PENCIL:
                return "far:pencil";
            case Icon::SORT:
                return "far:sort-alpha-up";
            case Icon::LIST:
                return "far:list";
            case Icon::COMMIT:
                return "far:code-commit";
            case Icon::PLUG:
                return "fas:plug";
            case Icon::USER:
                return "far:user";
            case Icon::ARROW_RIGHT:
                return "far:long-arrow-right";
            case Icon::WORKTREE:
                return "far:folder-tree";
            case Icon::SUBMODULE:
                return "far:layer-group";
            case Icon::PATCH:
                return "fas:flask";
            case Icon::ISSUE:
                return "fas:tasks";
            case Icon::TEAM:
                return "fas:users";
            case Icon::SPINNER:
                return "far:circle-notch";
            case Icon::EXTERNAL:
                return "far:external-link-alt";
            case Icon::FILE:
                return "far:file";
            case Icon::MAGIC:
                return "fal:magic";
            case Icon::ROBOT:
                return "fas:robot";
            case Icon::KEYBOARD:
                return "far:keyboard";
            case Icon::GLOBE:
                return "far:globe";
            case Icon::SEARCH_PLUS:
                return "far:search-plus";
            case Icon::CHECKED_OUT:
                return "fas:check";
            case Icon::CARET_DOWN:
                return "fas:caret-down";
            case Icon::SPARKLES:
                return "far:sparkles";
            case Icon::BARS:
                return "far:bars";
            case Icon::FOLDER_TREE:
                return "fas:folder-tree";
            case Icon::GIFT:
                return "far:gift";
            default:
                return "";
            }
        }
    } // namespace

    float icon_width(Icon icon, float size)
    {
        const char* name = fa_name(icon);
        return *name != '\0' && has_fa_icon(name) ? fa_icon_width(name, size) : size;
    }

    void draw_icon(ImDrawList* draw, Icon icon, ImVec2 c, float size, ImU32 color)
    {
        const char* name = fa_name(icon);
        if (*name != '\0' && has_fa_icon(name))
        {
            draw_fa_icon(draw, name, c, size, color);
            return;
        }
        draw_icon_vector(draw, icon, c, size, color);
    }

    void draw_icon_vector(ImDrawList* draw, Icon icon, ImVec2 c, float size, ImU32 color)
    {
        const float s = size / 2.f; // half size
        const float t = std::max(1.f, px(1.5f));
        switch (icon)
        {
        case Icon::PULL:
        case Icon::PUSH:
        {
            const float dir = icon == Icon::PULL ? 1.f : -1.f;
            const float tip = c.y + dir * s * 0.55f;
            draw->AddLine(ImVec2(c.x, c.y - dir * s * 0.9f), ImVec2(c.x, tip), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.5f, tip - dir * s * 0.5f), ImVec2(c.x, tip), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.5f, tip - dir * s * 0.5f), ImVec2(c.x, tip), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y + dir * s * 0.95f), ImVec2(c.x + s * 0.8f, c.y + dir * s * 0.95f),
                          color, t);
            break;
        }
        case Icon::BRANCH:
        {
            const float r = s * 0.28f;
            draw->AddLine(ImVec2(c.x - s * 0.45f, c.y - s * 0.55f), ImVec2(c.x - s * 0.45f, c.y + s * 0.6f), color, t);
            draw->AddCircle(ImVec2(c.x - s * 0.45f, c.y - s * 0.75f), r, color, 12, t);
            draw->AddCircle(ImVec2(c.x - s * 0.45f, c.y + s * 0.8f), r, color, 12, t);
            draw->AddCircle(ImVec2(c.x + s * 0.55f, c.y - s * 0.4f), r, color, 12, t);
            draw->PathLineTo(ImVec2(c.x + s * 0.55f, c.y - s * 0.4f + r));
            draw->PathBezierCubicCurveTo(ImVec2(c.x + s * 0.55f, c.y + s * 0.3f),
                                         ImVec2(c.x - s * 0.45f, c.y + s * 0.1f),
                                         ImVec2(c.x - s * 0.45f, c.y + s * 0.5f), 10);
            draw->PathStroke(color, 0, t);
            break;
        }
        case Icon::STASH:
        case Icon::POP:
        {
            draw->AddRect(ImVec2(c.x - s * 0.85f, c.y - s * 0.35f), ImVec2(c.x + s * 0.85f, c.y + s * 0.85f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x - s * 0.85f, c.y - s * 0.35f), ImVec2(c.x - s * 0.55f, c.y - s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.85f, c.y - s * 0.35f), ImVec2(c.x + s * 0.55f, c.y - s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.55f, c.y - s * 0.8f), ImVec2(c.x + s * 0.55f, c.y - s * 0.8f), color, t);
            if (icon == Icon::POP)
            {
                draw->AddLine(ImVec2(c.x, c.y + s * 0.5f), ImVec2(c.x, c.y - s * 0.1f), color, t);
                draw->AddLine(ImVec2(c.x - s * 0.3f, c.y + s * 0.2f), ImVec2(c.x, c.y - s * 0.1f), color, t);
                draw->AddLine(ImVec2(c.x + s * 0.3f, c.y + s * 0.2f), ImVec2(c.x, c.y - s * 0.1f), color, t);
            }
            break;
        }
        case Icon::TAG:
        {
            const ImVec2 points[] = {ImVec2(c.x - s * 0.8f, c.y - s * 0.8f), ImVec2(c.x + s * 0.1f, c.y - s * 0.8f),
                                     ImVec2(c.x + s * 0.85f, c.y), ImVec2(c.x + s * 0.1f, c.y + s * 0.8f),
                                     ImVec2(c.x - s * 0.8f, c.y + s * 0.8f)};
            draw->AddPolyline(points, 5, color, ImDrawFlags_Closed, t);
            draw->AddCircleFilled(ImVec2(c.x - s * 0.35f, c.y - s * 0.35f), t, color);
            break;
        }
        case Icon::REMOTE:
            draw->AddCircle(c, s * 0.8f, color, 16, t);
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y), ImVec2(c.x + s * 0.8f, c.y), color, t);
            draw->AddEllipse(c, ImVec2(s * 0.35f, s * 0.8f), color, 0.f, 16, t);
            break;
        case Icon::CLOUD:
            draw->PathArcTo(ImVec2(c.x - s * 0.4f, c.y + s * 0.1f), s * 0.45f, 2.2f, 4.4f, 10);
            draw->PathArcTo(ImVec2(c.x + s * 0.2f, c.y - s * 0.1f), s * 0.55f, 3.5f, 5.9f, 10);
            draw->PathArcTo(ImVec2(c.x + s * 0.4f, c.y + s * 0.25f), s * 0.35f, 5.6f, 7.6f, 8);
            draw->PathLineTo(ImVec2(c.x - s * 0.4f, c.y + s * 0.55f));
            draw->PathStroke(color, ImDrawFlags_Closed, t);
            break;
        case Icon::FOLDER:
            draw->AddRect(ImVec2(c.x - s * 0.85f, c.y - s * 0.45f), ImVec2(c.x + s * 0.85f, c.y + s * 0.65f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x - s * 0.85f, c.y - s * 0.45f), ImVec2(c.x - s * 0.6f, c.y - s * 0.75f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.6f, c.y - s * 0.75f), ImVec2(c.x - s * 0.05f, c.y - s * 0.75f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.05f, c.y - s * 0.75f), ImVec2(c.x + s * 0.2f, c.y - s * 0.45f), color, t);
            break;
        case Icon::LAPTOP:
            draw->AddRect(ImVec2(c.x - s * 0.65f, c.y - s * 0.6f), ImVec2(c.x + s * 0.65f, c.y + s * 0.35f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x - s * 0.95f, c.y + s * 0.65f), ImVec2(c.x + s * 0.95f, c.y + s * 0.65f), color, t);
            break;
        case Icon::CLOSE:
            draw->AddLine(ImVec2(c.x - s * 0.6f, c.y - s * 0.6f), ImVec2(c.x + s * 0.6f, c.y + s * 0.6f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.6f, c.y + s * 0.6f), ImVec2(c.x + s * 0.6f, c.y - s * 0.6f), color, t);
            break;
        case Icon::PLUS:
            draw->AddLine(ImVec2(c.x - s * 0.7f, c.y), ImVec2(c.x + s * 0.7f, c.y), color, t);
            draw->AddLine(ImVec2(c.x, c.y - s * 0.7f), ImVec2(c.x, c.y + s * 0.7f), color, t);
            break;
        case Icon::MINUS:
            draw->AddLine(ImVec2(c.x - s * 0.7f, c.y), ImVec2(c.x + s * 0.7f, c.y), color, t);
            break;
        case Icon::BACK:
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y), ImVec2(c.x + s * 0.8f, c.y), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y), ImVec2(c.x - s * 0.2f, c.y - s * 0.6f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y), ImVec2(c.x - s * 0.2f, c.y + s * 0.6f), color, t);
            break;
        case Icon::ARROW_RIGHT:
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y), ImVec2(c.x + s * 0.8f, c.y), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.8f, c.y), ImVec2(c.x + s * 0.2f, c.y - s * 0.6f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.8f, c.y), ImVec2(c.x + s * 0.2f, c.y + s * 0.6f), color, t);
            break;
        case Icon::REFRESH:
            draw->PathArcTo(c, s * 0.75f, 0.4f, 5.6f, 20);
            draw->PathStroke(color, 0, t);
            draw->AddTriangleFilled(ImVec2(c.x + s * 0.95f, c.y - s * 0.75f), ImVec2(c.x + s * 0.95f, c.y + s * 0.05f),
                                    ImVec2(c.x + s * 0.25f, c.y - s * 0.3f), color);
            break;
        case Icon::UNDO:
        case Icon::REDO:
        {
            const float dir = icon == Icon::UNDO ? 1.f : -1.f;
            draw->PathArcTo(ImVec2(c.x + dir * s * 0.1f, c.y + s * 0.15f), s * 0.7f, icon == Icon::UNDO ? 3.4f : -0.3f,
                            icon == Icon::UNDO ? 6.6f : 2.9f, 16);
            draw->PathStroke(color, 0, t);
            const ImVec2 tip(c.x - dir * s * 0.75f, c.y - s * 0.25f);
            draw->AddLine(tip, ImVec2(tip.x, tip.y - s * 0.6f), color, t);
            draw->AddLine(tip, ImVec2(tip.x + dir * s * 0.6f, tip.y), color, t);
            break;
        }
        case Icon::TERMINAL:
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y - s * 0.5f), ImVec2(c.x - s * 0.2f, c.y), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.2f, c.y), ImVec2(c.x - s * 0.8f, c.y + s * 0.5f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.05f, c.y + s * 0.6f), ImVec2(c.x + s * 0.85f, c.y + s * 0.6f), color, t);
            break;
        case Icon::SEARCH:
            draw->AddCircle(ImVec2(c.x - s * 0.15f, c.y - s * 0.15f), s * 0.55f, color, 16, t);
            draw->AddLine(ImVec2(c.x + s * 0.25f, c.y + s * 0.25f), ImVec2(c.x + s * 0.8f, c.y + s * 0.8f), color, t);
            break;
        case Icon::GEAR:
        {
            for (int i = 0; i < 8; ++i)
            {
                const float a = static_cast<float>(i) * 0.785f;
                draw->AddLine(ImVec2(c.x + std::cos(a) * s * 0.55f, c.y + std::sin(a) * s * 0.55f),
                              ImVec2(c.x + std::cos(a) * s * 0.9f, c.y + std::sin(a) * s * 0.9f), color, t * 1.3f);
            }
            draw->AddCircle(c, s * 0.5f, color, 16, t);
            draw->AddCircle(c, s * 0.18f, color, 8, t);
            break;
        }
        case Icon::BELL:
            draw->PathArcTo(ImVec2(c.x, c.y - s * 0.1f), s * 0.55f, 3.14159f, 6.28318f, 12);
            draw->PathLineTo(ImVec2(c.x + s * 0.7f, c.y + s * 0.5f));
            draw->PathLineTo(ImVec2(c.x - s * 0.7f, c.y + s * 0.5f));
            draw->PathStroke(color, ImDrawFlags_Closed, t);
            draw->AddLine(ImVec2(c.x - s * 0.2f, c.y + s * 0.8f), ImVec2(c.x + s * 0.2f, c.y + s * 0.8f), color, t);
            break;
        case Icon::ROCKET:
            draw->PathLineTo(ImVec2(c.x, c.y - s * 0.9f));
            draw->PathBezierQuadraticCurveTo(ImVec2(c.x + s * 0.6f, c.y - s * 0.2f),
                                             ImVec2(c.x + s * 0.3f, c.y + s * 0.5f), 8);
            draw->PathLineTo(ImVec2(c.x - s * 0.3f, c.y + s * 0.5f));
            draw->PathBezierQuadraticCurveTo(ImVec2(c.x - s * 0.6f, c.y - s * 0.2f), ImVec2(c.x, c.y - s * 0.9f), 8);
            draw->PathStroke(color, ImDrawFlags_Closed, t);
            draw->AddLine(ImVec2(c.x - s * 0.3f, c.y + s * 0.5f), ImVec2(c.x - s * 0.6f, c.y + s * 0.85f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.3f, c.y + s * 0.5f), ImVec2(c.x + s * 0.6f, c.y + s * 0.85f), color, t);
            draw->AddCircleFilled(ImVec2(c.x, c.y - s * 0.15f), t, color);
            break;
        case Icon::TRASH:
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y - s * 0.5f), ImVec2(c.x + s * 0.8f, c.y - s * 0.5f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.25f, c.y - s * 0.5f), ImVec2(c.x - s * 0.25f, c.y - s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.25f, c.y - s * 0.8f), ImVec2(c.x + s * 0.25f, c.y - s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.25f, c.y - s * 0.8f), ImVec2(c.x + s * 0.25f, c.y - s * 0.5f), color, t);
            draw->AddRect(ImVec2(c.x - s * 0.6f, c.y - s * 0.5f), ImVec2(c.x + s * 0.6f, c.y + s * 0.85f), color, 0.f,
                          0, t);
            draw->AddLine(ImVec2(c.x - s * 0.2f, c.y - s * 0.2f), ImVec2(c.x - s * 0.2f, c.y + s * 0.55f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.2f, c.y - s * 0.2f), ImVec2(c.x + s * 0.2f, c.y + s * 0.55f), color, t);
            break;
        case Icon::EYE:
        case Icon::EYE_OFF:
            draw->PathArcTo(ImVec2(c.x, c.y + s * 0.6f), s * 1.1f, 3.6f, 5.8f, 12);
            draw->PathStroke(color, 0, t);
            draw->PathArcTo(ImVec2(c.x, c.y - s * 0.6f), s * 1.1f, 0.5f, 2.65f, 12);
            draw->PathStroke(color, 0, t);
            draw->AddCircleFilled(c, s * 0.28f, color, 10);
            if (icon == Icon::EYE_OFF)
            {
                draw->AddLine(ImVec2(c.x - s * 0.8f, c.y + s * 0.8f), ImVec2(c.x + s * 0.8f, c.y - s * 0.8f), color, t);
            }
            break;
        case Icon::PENCIL:
            draw->AddLine(ImVec2(c.x - s * 0.7f, c.y + s * 0.7f), ImVec2(c.x + s * 0.5f, c.y - s * 0.5f), color,
                          t * 1.2f);
            draw->AddLine(ImVec2(c.x + s * 0.3f, c.y - s * 0.7f), ImVec2(c.x + s * 0.7f, c.y - s * 0.3f), color,
                          t * 1.2f);
            draw->AddLine(ImVec2(c.x - s * 0.7f, c.y + s * 0.7f), ImVec2(c.x - s * 0.85f, c.y + s * 0.85f), color, t);
            break;
        case Icon::SORT:
            draw->AddLine(ImVec2(c.x - s * 0.35f, c.y - s * 0.8f), ImVec2(c.x - s * 0.35f, c.y + s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.35f, c.y - s * 0.8f), ImVec2(c.x - s * 0.75f, c.y - s * 0.4f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.35f, c.y - s * 0.8f), ImVec2(c.x + s * 0.35f, c.y + s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.35f, c.y + s * 0.8f), ImVec2(c.x + s * 0.75f, c.y + s * 0.4f), color, t);
            break;
        case Icon::EXPAND:
        case Icon::COLLAPSE:
        {
            const float d = icon == Icon::EXPAND ? 1.f : -1.f;
            draw->AddLine(ImVec2(c.x - s * 0.7f, c.y - s * 0.7f), ImVec2(c.x + s * 0.7f, c.y + s * 0.7f), color, t);
            const ImVec2 a = icon == Icon::EXPAND ? ImVec2(c.x + s * 0.7f, c.y + s * 0.7f)
                                                  : ImVec2(c.x + s * 0.1f, c.y + s * 0.1f);
            draw->AddLine(a, ImVec2(a.x - d * s * 0.55f, a.y), color, t);
            draw->AddLine(a, ImVec2(a.x, a.y - d * s * 0.55f), color, t);
            const ImVec2 b = icon == Icon::EXPAND ? ImVec2(c.x - s * 0.7f, c.y - s * 0.7f)
                                                  : ImVec2(c.x - s * 0.1f, c.y - s * 0.1f);
            draw->AddLine(b, ImVec2(b.x + d * s * 0.55f, b.y), color, t);
            draw->AddLine(b, ImVec2(b.x, b.y + d * s * 0.55f), color, t);
            break;
        }
        case Icon::LIST:
            for (int i = -1; i <= 1; ++i)
            {
                const float y = c.y + static_cast<float>(i) * s * 0.55f;
                draw->AddCircleFilled(ImVec2(c.x - s * 0.7f, y), t * 0.8f, color);
                draw->AddLine(ImVec2(c.x - s * 0.3f, y), ImVec2(c.x + s * 0.8f, y), color, t);
            }
            break;
        case Icon::COMMIT:
            draw->AddCircle(c, s * 0.4f, color, 12, t);
            draw->AddLine(ImVec2(c.x - s * 0.9f, c.y), ImVec2(c.x - s * 0.4f, c.y), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.4f, c.y), ImVec2(c.x + s * 0.9f, c.y), color, t);
            break;
        case Icon::HEADPHONES:
            draw->PathArcTo(c, s * 0.75f, 3.14159f, 6.28318f, 14);
            draw->PathStroke(color, 0, t);
            draw->AddRectFilled(ImVec2(c.x - s * 0.9f, c.y), ImVec2(c.x - s * 0.5f, c.y + s * 0.6f), color, px(1.f));
            draw->AddRectFilled(ImVec2(c.x + s * 0.5f, c.y), ImVec2(c.x + s * 0.9f, c.y + s * 0.6f), color, px(1.f));
            break;
        case Icon::PLUG:
            draw->AddLine(ImVec2(c.x - s * 0.3f, c.y - s * 0.9f), ImVec2(c.x - s * 0.3f, c.y - s * 0.3f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.3f, c.y - s * 0.9f), ImVec2(c.x + s * 0.3f, c.y - s * 0.3f), color, t);
            draw->AddRect(ImVec2(c.x - s * 0.7f, c.y - s * 0.3f), ImVec2(c.x + s * 0.7f, c.y + s * 0.3f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x, c.y + s * 0.3f), ImVec2(c.x, c.y + s * 0.9f), color, t);
            break;
        case Icon::USER:
            draw->AddCircle(ImVec2(c.x, c.y - s * 0.35f), s * 0.35f, color, 12, t);
            draw->PathArcTo(ImVec2(c.x, c.y + s * 0.9f), s * 0.75f, 3.4f, 6.0f, 12);
            draw->PathStroke(color, 0, t);
            break;
        case Icon::WORKTREE:
            draw->AddRect(ImVec2(c.x - s * 0.8f, c.y - s * 0.8f), ImVec2(c.x + s * 0.8f, c.y + s * 0.8f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x - s * 0.8f, c.y), ImVec2(c.x + s * 0.8f, c.y), color, t);
            draw->AddLine(ImVec2(c.x, c.y), ImVec2(c.x, c.y + s * 0.8f), color, t);
            break;
        case Icon::SUBMODULE:
            draw->AddRect(ImVec2(c.x - s * 0.8f, c.y - s * 0.8f), ImVec2(c.x + s * 0.2f, c.y + s * 0.2f), color,
                          px(1.f), 0, t);
            draw->AddRect(ImVec2(c.x - s * 0.2f, c.y - s * 0.2f), ImVec2(c.x + s * 0.8f, c.y + s * 0.8f), color,
                          px(1.f), 0, t);
            break;
        case Icon::PATCH:
            draw->AddRect(ImVec2(c.x - s * 0.6f, c.y - s * 0.8f), ImVec2(c.x + s * 0.6f, c.y + s * 0.8f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x - s * 0.3f, c.y - s * 0.3f), ImVec2(c.x + s * 0.3f, c.y - s * 0.3f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.3f, c.y + s * 0.1f), ImVec2(c.x + s * 0.3f, c.y + s * 0.1f), color, t);
            break;
        case Icon::ISSUE:
            draw->AddCircle(c, s * 0.8f, color, 16, t);
            draw->AddLine(ImVec2(c.x, c.y - s * 0.4f), ImVec2(c.x, c.y + s * 0.1f), color, t);
            draw->AddCircleFilled(ImVec2(c.x, c.y + s * 0.45f), t * 0.8f, color);
            break;
        case Icon::TEAM:
            draw->AddCircle(ImVec2(c.x - s * 0.35f, c.y - s * 0.3f), s * 0.3f, color, 10, t);
            draw->AddCircle(ImVec2(c.x + s * 0.4f, c.y - s * 0.3f), s * 0.3f, color, 10, t);
            draw->PathArcTo(ImVec2(c.x - s * 0.35f, c.y + s * 0.9f), s * 0.65f, 3.4f, 6.0f, 10);
            draw->PathStroke(color, 0, t);
            draw->PathArcTo(ImVec2(c.x + s * 0.4f, c.y + s * 0.9f), s * 0.65f, 3.4f, 6.0f, 10);
            draw->PathStroke(color, 0, t);
            break;
        case Icon::LFS:
            draw->AddRect(ImVec2(c.x - s * 0.8f, c.y - s * 0.6f), ImVec2(c.x + s * 0.8f, c.y + s * 0.6f), color,
                          px(2.f), 0, t);
            draw->AddLine(ImVec2(c.x - s * 0.5f, c.y - s * 0.2f), ImVec2(c.x - s * 0.5f, c.y + s * 0.3f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.5f, c.y + s * 0.3f), ImVec2(c.x - s * 0.2f, c.y + s * 0.3f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.1f, c.y - s * 0.2f), ImVec2(c.x + s * 0.1f, c.y + s * 0.3f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.1f, c.y - s * 0.2f), ImVec2(c.x + s * 0.5f, c.y - s * 0.2f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.1f, c.y + s * 0.05f), ImVec2(c.x + s * 0.4f, c.y + s * 0.05f), color, t);
            break;
        case Icon::SPINNER:
        {
            const float a = static_cast<float>(std::fmod(ImGui::GetTime() * 5.0, 6.28318));
            draw->PathArcTo(c, s * 0.75f, a, a + 4.2f, 16);
            draw->PathStroke(color, 0, t);
            break;
        }
        case Icon::EXTERNAL:
            draw->AddRect(ImVec2(c.x - s * 0.8f, c.y - s * 0.4f), ImVec2(c.x + s * 0.4f, c.y + s * 0.8f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x, c.y), ImVec2(c.x + s * 0.8f, c.y - s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.2f, c.y - s * 0.8f), ImVec2(c.x + s * 0.8f, c.y - s * 0.8f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.8f, c.y - s * 0.8f), ImVec2(c.x + s * 0.8f, c.y - s * 0.2f), color, t);
            break;
        case Icon::FILE:
            draw->AddRect(ImVec2(c.x - s * 0.6f, c.y - s * 0.85f), ImVec2(c.x + s * 0.6f, c.y + s * 0.85f), color,
                          px(1.f), 0, t);
            draw->AddLine(ImVec2(c.x - s * 0.3f, c.y - s * 0.2f), ImVec2(c.x + s * 0.3f, c.y - s * 0.2f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.3f, c.y + s * 0.2f), ImVec2(c.x + s * 0.3f, c.y + s * 0.2f), color, t);
            break;
        case Icon::CHEVRON_DOWN:
            draw->AddLine(ImVec2(c.x - s * 0.5f, c.y - s * 0.25f), ImVec2(c.x, c.y + s * 0.25f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.5f, c.y - s * 0.25f), ImVec2(c.x, c.y + s * 0.25f), color, t);
            break;
        case Icon::CHEVRON_UP:
            draw->AddLine(ImVec2(c.x - s * 0.5f, c.y + s * 0.25f), ImVec2(c.x, c.y - s * 0.25f), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.5f, c.y + s * 0.25f), ImVec2(c.x, c.y - s * 0.25f), color, t);
            break;
        case Icon::CHEVRON_RIGHT:
            draw->AddLine(ImVec2(c.x - s * 0.25f, c.y - s * 0.5f), ImVec2(c.x + s * 0.25f, c.y), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.25f, c.y + s * 0.5f), ImVec2(c.x + s * 0.25f, c.y), color, t);
            break;
        case Icon::CHEVRON_LEFT:
            draw->AddLine(ImVec2(c.x + s * 0.25f, c.y - s * 0.5f), ImVec2(c.x - s * 0.25f, c.y), color, t);
            draw->AddLine(ImVec2(c.x + s * 0.25f, c.y + s * 0.5f), ImVec2(c.x - s * 0.25f, c.y), color, t);
            break;
        case Icon::CHECK:
            draw->AddLine(ImVec2(c.x - s * 0.6f, c.y), ImVec2(c.x - s * 0.15f, c.y + s * 0.5f), color, t);
            draw->AddLine(ImVec2(c.x - s * 0.15f, c.y + s * 0.5f), ImVec2(c.x + s * 0.65f, c.y - s * 0.5f), color, t);
            break;
        case Icon::WARNING:
            draw->AddTriangle(ImVec2(c.x, c.y - s * 0.85f), ImVec2(c.x + s * 0.9f, c.y + s * 0.75f),
                              ImVec2(c.x - s * 0.9f, c.y + s * 0.75f), color, t);
            draw->AddLine(ImVec2(c.x, c.y - s * 0.3f), ImVec2(c.x, c.y + s * 0.2f), color, t);
            draw->AddCircleFilled(ImVec2(c.x, c.y + s * 0.5f), t * 0.8f, color);
            break;
        case Icon::COPY:
            draw->AddRect(ImVec2(c.x - s * 0.75f, c.y - s * 0.75f), ImVec2(c.x + s * 0.25f, c.y + s * 0.25f), color,
                          px(1.f), 0, t);
            draw->AddRect(ImVec2(c.x - s * 0.25f, c.y - s * 0.25f), ImVec2(c.x + s * 0.75f, c.y + s * 0.75f), color,
                          px(1.f), 0, t);
            break;
        default:
            break; // drawn from their Font Awesome paths only
        }
    }

    bool icon_button(const char* id, Icon icon, float size, ImVec4 color, const char* tip, bool active)
    {
        const float box = px(size + 10.f);
        const ImVec2 pos = ImGui::GetCursorScreenPos();
        const bool pressed = ImGui::InvisibleButton(id, ImVec2(box, box));
        const bool hovered = ImGui::IsItemHovered();
        ImDrawList* draw = ImGui::GetWindowDrawList();
        if (hovered || active)
        {
            draw->AddRectFilled(pos, ImVec2(pos.x + box, pos.y + box),
                                theme::u32(active ? theme::palette().selected_row : theme::palette().hover_row),
                                px(3.f));
        }
        const ImVec4 final_color = hovered || active ? theme::palette().text_selected : color;
        draw_icon(draw, icon, ImVec2(pos.x + box / 2.f, pos.y + box / 2.f), px(size), theme::u32(final_color));
        if (tip != nullptr)
        {
            tooltip(tip);
        }
        return pressed;
    }
} // namespace imgit::ui
