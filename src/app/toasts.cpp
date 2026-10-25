#include "app/toasts.hpp"
#include "app/theme.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <chrono>
#include <cmath>
#include <imgui.h>
#include <optional>
#include <string_view>
#include <utility>

namespace imgit::app
{
    void Toasts::push(ToastKind kind, std::string title, std::string detail)
    {
        const double lifetime = 10.0; // TOAST_DURATION_DEFAULT
        const std::int64_t now =
                std::chrono::duration_cast<std::chrono::seconds>(std::chrono::system_clock::now().time_since_epoch())
                        .count();
        m_history.insert(m_history.begin(), Entry{kind, title, detail, now, false});
        if (m_history.size() > 50)
        {
            m_history.pop_back();
        }
        const double shown_at = ImGui::GetTime();
        m_toasts.push_back(
                Toast{kind, std::move(title), std::move(detail), shown_at, shown_at + lifetime, std::nullopt});
    }

    int Toasts::unread() const noexcept
    {
        int count = 0;
        for (const Entry& entry : m_history)
        {
            count += entry.read ? 0 : 1;
        }
        return count;
    }

    void Toasts::mark_all_read() noexcept
    {
        for (Entry& entry : m_history)
        {
            entry.read = true;
        }
    }

    // The toast cards: bottom-left, 16px in and 32px above the window bottom,
    // 352px wide, at least 80px tall, newest at the bottom, 8px apart. A 36px status column with the white
    // icon, the 16px title and 14px secondary text with 16px padding, and the × in a 31px column. Clicking a
    // card dismisses it. A card slides in from the left edge over SLIDE_SECONDS and back out when it expires or is
    // dismissed; its slot in the stack grows and shrinks with it, so the cards above settle instead of jumping.
    void Toasts::draw()
    {
        constexpr double SLIDE_SECONDS = 0.3;
        const double now = ImGui::GetTime();
        for (Toast& toast : m_toasts)
        {
            if (!toast.hidden_at && toast.expires_at < now)
            {
                toast.hidden_at = now;
            }
        }
        std::erase_if(m_toasts,
                      [&](const Toast& t)
                      {
                          return t.hidden_at && *t.hidden_at + SLIDE_SECONDS <= now;
                      });
        if (m_toasts.empty())
        {
            return;
        }
        // How far in a card is: 0 fully off the left edge, 1 in place.
        const auto slide = [&](const Toast& toast)
        {
            const double in = (now - toast.shown_at) / SLIDE_SECONDS;
            float visible = ui::ease_in_out(static_cast<float>(in));
            if (toast.hidden_at)
            {
                const double out = (now - *toast.hidden_at) / SLIDE_SECONDS;
                visible = std::min(visible, 1.f - ui::ease_in_out(static_cast<float>(out)));
            }
            if (in < 1.0 || toast.hidden_at)
            {
                ui::keep_animating();
            }
            return visible;
        };

        const theme::Palette& p = theme::palette();
        const ImGuiViewport* viewport = ImGui::GetMainViewport();
        const float width = ui::px(352.f);
        const float text_w = ui::px(252.7f);
        const float x = viewport->Pos.x + ui::px(16.f);
        float bottom = viewport->Pos.y + viewport->Size.y - ui::px(32.f);
        const auto line_count = [&](const std::string& text, float size, float line_h)
        {
            if (text.empty())
            {
                return 0.f;
            }
            ImFont* font = ui::fonts().regular;
            const float font_size = ui::font_px(font, size);
            float lines = 0.f;
            const char* cursor = text.c_str();
            const char* end = cursor + text.size();
            while (cursor < end)
            {
                const char* wrap = font->CalcWordWrapPosition(font_size, cursor, end, text_w);
                if (wrap == cursor)
                {
                    wrap = cursor + 1;
                }
                cursor = wrap;
                while (cursor < end && (*cursor == ' ' || *cursor == '\n'))
                {
                    ++cursor;
                }
                lines += 1.f;
            }
            return lines * ui::px(line_h);
        };
        const auto wrapped =
                [&](ImDrawList* draw, const std::string& text, float size, float line_h, ImVec2 pos, ImU32 color)
        {
            ImFont* font = ui::fonts().regular;
            const float font_size = ui::font_px(font, size);
            const float ascent = size * 2189.f / 2048.f * ui::dpi_scale();
            const float content = std::round(ascent) + std::round(size * 600.f / 2048.f * ui::dpi_scale());
            const char* cursor = text.c_str();
            const char* end = cursor + text.size();
            float y = pos.y;
            while (cursor < end)
            {
                const char* wrap = font->CalcWordWrapPosition(font_size, cursor, end, text_w);
                if (wrap == cursor)
                {
                    wrap = cursor + 1;
                }
                const std::string line(cursor, wrap);
                const float box_top = y + (ui::px(line_h) - content) / 2.f;
                ui::draw_text(draw, font, size,
                              ImVec2(pos.x, std::round(box_top + std::round(ascent)) - std::ceil(ascent)), color,
                              line.c_str());
                y += ui::px(line_h);
                cursor = wrap;
                while (cursor < end && (*cursor == ' ' || *cursor == '\n'))
                {
                    ++cursor;
                }
            }
        };

        std::optional<std::size_t> dismiss;
        for (std::size_t i = m_toasts.size(); i-- > 0;)
        {
            const Toast& toast = m_toasts[i];
            const float title_h = line_count(toast.title, 16.f, 20.8f);
            const float body_h = line_count(toast.detail, 14.f, 18.2f);
            const float height =
                    std::max(ui::px(80.f), ui::px(16.f) + title_h + ui::px(10.f) + body_h + ui::px(5.f) + ui::px(16.f));
            const float visible = slide(toast);
            const float card_x = x - (1.f - visible) * (width + ui::px(16.f));
            const ImVec2 min(std::round(card_x), bottom - height);
            const ImVec2 max(min.x + width, bottom);
            bottom = min.y - ui::px(8.f) + (1.f - visible) * (height + ui::px(8.f));

            ImGui::SetNextWindowPos(min);
            ImGui::SetNextWindowSize(ImVec2(width, height));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            char name[32];
            snprintf(name, sizeof(name), "##toast%zu", i);
            ImGui::Begin(name, nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                 | ImGuiWindowFlags_NoFocusOnAppearing | ImGuiWindowFlags_NoNav
                                 | ImGuiWindowFlags_NoBackground);
            ImGui::PopStyleVar(2);
            ImDrawList* draw = ImGui::GetWindowDrawList();
            ImVec4 accent = p.accent;
            std::string_view icon = ui::svg_path("circle-info");
            const char* key = "toast-info";
            if (toast.kind == ToastKind::SUCCESS)
            {
                accent = p.green;
                icon = ui::svg_path("circle-check");
                key = "toast-success";
            }
            else if (toast.kind == ToastKind::ERROR)
            {
                accent = p.red;
                icon = ui::svg_path("circle-xmark");
                key = "toast-error";
            }
            draw->AddRectFilled(min, max, theme::u32(p.panel_bg1), ui::px(4.f));
            draw->AddRectFilled(min, ImVec2(min.x + ui::px(36.f), max.y), theme::u32(accent), ui::px(4.f),
                                ImDrawFlags_RoundCornersLeft);
            const float icon_y = std::round((min.y + max.y) / 2.f - ui::px(10.f));
            ui::draw_svg_path(draw, key, icon, 512.f, 512.f, ImVec2(min.x + ui::px(8.f), icon_y),
                              ImVec2(min.x + ui::px(28.f), icon_y + ui::px(20.f)), IM_COL32(255, 255, 255, 255));
            wrapped(draw, toast.title, 16.f, 20.8f, ImVec2(min.x + ui::px(52.f), min.y + ui::px(16.f)),
                    theme::u32(p.text_selected));
            if (!toast.detail.empty())
            {
                wrapped(draw, toast.detail, 14.f, 18.2f,
                        ImVec2(min.x + ui::px(52.f), min.y + ui::px(16.f) + title_h + ui::px(10.f)),
                        theme::u32(p.text_secondary));
            }
            // The ×: far:times at 18px in the dismiss column.
            const ImVec2 close_min(max.x - ui::px(31.3f), min.y + ui::px(8.f));
            ImGui::SetCursorScreenPos(close_min);
            const bool close = ImGui::InvisibleButton("##dismiss", ImVec2(ui::px(21.3f), ui::px(28.7f)));
            const bool close_hovered = ImGui::IsItemHovered();
            ui::draw_fa_icon(draw, "far:times", ImVec2(close_min.x + ui::px(10.6f), min.y + ui::px(22.f)), ui::px(18.f),
                             theme::u32(close_hovered ? p.text_selected : p.text_secondary));
            ImGui::SetCursorScreenPos(min);
            const bool clicked = ImGui::InvisibleButton("##card", ImVec2(width, height));
            if ((close || clicked) && !toast.hidden_at)
            {
                dismiss = i;
            }
            ImGui::End();
        }
        if (dismiss)
        {
            m_toasts[*dismiss].hidden_at = now;
        }
    }
} // namespace imgit::app
