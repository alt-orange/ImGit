#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/shell.hpp"
#include "platform/system.hpp"
#include "ui/avatars.hpp"
#include "ui/popover.hpp"
#include "ui/native_menu.hpp"
#include "ui/panels.hpp"
#include "ui/profile_widgets.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <ctime>
#include <filesystem>
#include <fstream>
#include <functional>
#include <imgui.h>
#include <imgui_internal.h>
#include <imgui_stdlib.h>
#include <map>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

// The Preferences view: a 300px sidebar with the global and repo-specific pages, and the selected page in a
// scrolling panel. Positions are CSS pixels (fractions are Chromium's LayoutUnits, 1/64 px, so heights such as
// 19.796875 add up to its pixel rounding): most pages are Bootstrap form-horizontal rows (a 285px label column,
// controls 15px into the next one), Sparse Checkout uses a 248px label column. Values ImGit implements are bound
// to its settings; the others are kept in preferences.txt.
namespace imgit::ui
{
    namespace
    {

        constexpr ImVec4 rgb(float r, float g, float b, float a = 1.f)
        {
            return ImVec4(r / 255.f, g / 255.f, b / 255.f, a);
        }

        constexpr ImVec4 white(float alpha)
        {
            return ImVec4(1.f, 1.f, 1.f, alpha);
        }

        // The current theme's colours, by reference so a theme switch reaches them.
        const ImVec4& ACCENT = theme::palette().link; // links, checked boxes
        const ImVec4& BLUE = theme::palette().accent;
        const ImVec4& GREEN = theme::palette().green;
        const ImVec4& RED = theme::palette().red;
        const ImVec4& PANEL_BG0 = theme::palette().panel_bg0;
        const ImVec4& PANEL_BG1 = theme::palette().panel_bg1;
        constexpr ImVec4 FIELD_BG = ImVec4(0.f, 0.f, 0.f, 0.2f);
        constexpr ImVec4 OUTLINE = rgb(119, 120, 123); // design-system outline buttons
        const ImVec4& CODE_TEXT = theme::palette().text_accent;
        constexpr ImVec4 CLEAR = ImVec4(0.f, 0.f, 0.f, 0.f);

        ImU32 u32(ImVec4 color, float opacity = 1.f)
        {
            color.w *= opacity;
            return ImGui::ColorConvertFloat4ToU32(color);
        }

        // ---------------------------------------------------------------------------------------------------------
        // Text

        enum class Face
        {
            REGULAR,
            SEMIBOLD,
            ITALIC,
            MONO,
        };

        struct Style
        {
            float size = 14.f;
            ImVec4 color = white(0.75f);
            Face face = Face::REGULAR;
        };

        constexpr Style LABEL{14.f, white(0.75f)};
        constexpr Style HELP{12.f, white(0.6f)};

        ImFont* font_of(Face face)
        {
            switch (face)
            {
            case Face::SEMIBOLD:
                return fonts().semibold;
            case Face::ITALIC:
                return fonts().italic != nullptr ? fonts().italic : fonts().regular;
            case Face::MONO:
                return fonts().mono;
            case Face::REGULAR:
                break;
            }
            return fonts().regular;
        }

        // hhea ascent / descent over the em (Open Sans, Consolas).
        float ascent_of(Face face)
        {
            return face == Face::MONO ? 1884.f / 2048.f : 2189.f / 2048.f;
        }

        float descent_of(Face face)
        {
            return face == Face::MONO ? 514.f / 2048.f : 600.f / 2048.f;
        }

        // CSS px.
        float measure(const Style& style, std::string_view s)
        {
            return text_size(font_of(style.face), style.size, s.data(), s.data() + s.size()).x / dpi_scale();
        }

        // Where Chromium puts the text box in a line box `line_height` tall: half the leading, floored
        // (NGLineHeightMetrics::CalculateLeadingSpace), over the rounded ascent + descent.
        float leading(const Style& style, float line_height)
        {
            const float content =
                    std::round(style.size * ascent_of(style.face)) + std::round(style.size * descent_of(style.face));
            return std::floor((line_height - content) / 2.f);
        }

        // Plain text broken like Chromium: at spaces (dropped) and after a hyphen inside a word; '\n' forces a
        // break.
        std::vector<std::string_view> wrap(const Style& style, std::string_view s, float max_width)
        {
            std::vector<std::string_view> lines;
            std::size_t start = 0;
            while (true)
            {
                const std::size_t newline = s.find('\n', start);
                const std::size_t end = newline == std::string_view::npos ? s.size() : newline;
                std::size_t line = start;
                while (true)
                {
                    std::size_t cut = end;
                    std::size_t next = end;
                    if (measure(style, s.substr(line, end - line)) > max_width)
                    {
                        std::size_t best = std::string_view::npos;
                        std::size_t best_next = end;
                        for (std::size_t i = line + 1; i < end; ++i)
                        {
                            const bool space = s[i] == ' ';
                            const bool hyphen = s[i - 1] == '-' && s[i] != ' ' && i >= line + 2 && s[i - 2] != ' ';
                            if (!space && !hyphen)
                            {
                                continue;
                            }
                            if (measure(style, s.substr(line, i - line)) > max_width)
                            {
                                break;
                            }
                            best = i;
                            best_next = space ? i + 1 : i;
                        }
                        if (best != std::string_view::npos)
                        {
                            cut = best;
                            next = best_next;
                        }
                    }
                    lines.push_back(s.substr(line, cut - line));
                    if (cut >= end)
                    {
                        break;
                    }
                    line = next;
                }
                if (newline == std::string_view::npos)
                {
                    break;
                }
                start = newline + 1;
            }
            return lines;
        }

        // ---------------------------------------------------------------------------------------------------------
        // The page being drawn: positions are CSS px from its top-left corner.

        struct Page
        {
            app::Application& app;
            ImDrawList* draw;
            ImVec2 origin; // screen position of (0, 0), scrolled
            float width = 0.f; // CSS px
            float extent = 0.f; // content height, for the scroll range
            float scrollbar = 0.f; // the page's scrollbar, when it scrolls (CSS px)
            bool stacked = false; // a window under Bootstrap's md breakpoint (992px): labels above their controls

            [[nodiscard]] ImVec2 at(float x, float y) const
            {
                return ImVec2(origin.x + px(x), origin.y + px(y));
            }

            [[nodiscard]] ImVec2 at(ImVec2 p) const
            {
                return at(p.x, p.y);
            }

            void reach(float y)
            {
                extent = std::max(extent, y);
            }
        };

        // Text with its Chromium text box (the inline box's content area) at (x, y). x keeps its fraction (the LCD
        // renderer places glyphs at 1/3 px); the baseline is rounded to a pixel like Chromium's. Returns the width.
        float text(Page& page, float x, float y, const Style& style, std::string_view s, float opacity = 1.f)
        {
            const float ascent = style.size * ascent_of(style.face) * dpi_scale();
            const ImVec2 pos(page.origin.x + x * dpi_scale(),
                             page.origin.y + px(y) + std::round(ascent) - std::ceil(ascent));
            draw_text(page.draw, font_of(style.face), style.size, pos, u32(style.color, opacity), s.data(),
                      s.data() + s.size());
            return measure(style, s);
        }

        // text() cut with an ellipsis at `max_width` CSS px (.truncate).
        void text_truncated(Page& page, float x, float y, const Style& style, const std::string& s, float max_width)
        {
            const float ascent = style.size * ascent_of(style.face) * dpi_scale();
            const ImVec2 pos(page.origin.x + x * dpi_scale(),
                             page.origin.y + px(y) + std::round(ascent) - std::ceil(ascent));
            draw_text_ellipsis(page.draw, font_of(style.face), style.size, pos, px(max_width), u32(style.color),
                               s.c_str());
        }

        // A Font Awesome icon whose box (CSS px) is at (x, y), `size` its em.
        void icon(Page& page, std::string_view name, float x, float y, float w, float h, float size, ImVec4 color)
        {
            const ImVec2 min = page.at(x, y);
            draw_fa_icon(page.draw, name, ImVec2(min.x + px(w) / 2.f, min.y + px(h) / 2.f), px(size), u32(color));
        }

        // fad:check-circle: a white check on a blue circle (the current profile, the chosen profile icon).
        void check_badge(Page& page, float x, float y)
        {
            const ImVec2 min = page.at(x, y);
            draw_fa_duotone(page.draw, "fad:check-circle", ImVec2(min.x + px(7.f), min.y + px(7.f)), px(14.f),
                            u32(white(1.f)), u32(BLUE));
        }

        void svg(Page& page, std::string_view key, std::string_view path, float view_w, float view_h, float x, float y,
                 float w, float h, ImVec4 color, float opacity = 1.f)
        {
            const ImVec2 min = page.at(x, y);
            draw_svg_path(page.draw, key, path, view_w, view_h, min, ImVec2(min.x + px(w), min.y + px(h)),
                          u32(color, opacity));
        }

        bool hovering(ImVec2 min, ImVec2 max)
        {
            const ImVec2 mouse = ImGui::GetIO().MousePos;
            return ImGui::IsWindowHovered() && mouse.x >= min.x && mouse.x < max.x && mouse.y >= min.y
                   && mouse.y < max.y;
        }

        // A clickable rectangle (CSS px); shows the hand cursor.
        bool hit(Page& page, const char* id, float x, float y, float w, float h, bool enabled = true)
        {
            ImGui::SetCursorScreenPos(page.at(x, y));
            ImGui::SetNextItemAllowOverlap();
            const bool clicked =
                    ImGui::InvisibleButton(id, ImVec2(std::max(px(w), 1.f), std::max(px(h), 1.f))) && enabled;
            if (ImGui::IsItemHovered() && enabled)
            {
                ImGui::SetMouseCursor(ImGuiMouseCursor_Hand);
            }
            return clicked;
        }

        // ---------------------------------------------------------------------------------------------------------
        // Boxes

        struct BoxStyle
        {
            ImVec4 fill = CLEAR;
            ImVec4 border = CLEAR;
            float radius = 0.f;
            bool inset_shadow = false; // Bootstrap's form-control: rgba(0, 0, 0, .075) 0 1px 1px inset
            float border_width = 1.f;
        };

        void box(Page& page, float x, float y, float w, float h, const BoxStyle& style, float opacity = 1.f)
        {
            ImDrawList* draw = page.draw;
            const ImVec2 min = page.at(x, y);
            const ImVec2 max(min.x + px(w), min.y + px(h));
            const float rounding = px(style.radius);
            if (style.fill.w > 0.f)
            {
                draw->AddRectFilled(min, max, u32(style.fill, opacity), rounding);
            }
            if (style.inset_shadow)
            {
                draw->AddRectFilled(ImVec2(min.x + rounding, min.y + px(1.f)),
                                    ImVec2(max.x - rounding, min.y + px(2.f)),
                                    u32(ImVec4(0.f, 0.f, 0.f, 0.075f), opacity));
            }
            if (style.border.w <= 0.f)
            {
                return;
            }
            // Crisp straight edges like Chromium's (ImGui's anti-aliased 1px lines spread over two pixels); only the
            // rounded corners are anti-aliased.
            const float b = px(style.border_width);
            const ImU32 color = u32(style.border, opacity);
            const float r = rounding >= b ? rounding : 0.f;
            draw->AddRectFilled(ImVec2(min.x + r, min.y), ImVec2(max.x - r, min.y + b), color);
            draw->AddRectFilled(ImVec2(min.x + r, max.y - b), ImVec2(max.x - r, max.y), color);
            draw->AddRectFilled(ImVec2(min.x, min.y + std::max(r, b)), ImVec2(min.x + b, max.y - std::max(r, b)),
                                color);
            draw->AddRectFilled(ImVec2(max.x - b, min.y + std::max(r, b)), ImVec2(max.x, max.y - std::max(r, b)),
                                color);
            if (r <= 0.f)
            {
                return;
            }
            const float arc = r - b / 2.f;
            const ImVec2 centers[] = {ImVec2(min.x + r, min.y + r), ImVec2(max.x - r, min.y + r),
                                      ImVec2(max.x - r, max.y - r), ImVec2(min.x + r, max.y - r)};
            const float starts[] = {IM_PI, IM_PI * 1.5f, 0.f, IM_PI * 0.5f};
            for (int c = 0; c < 4; ++c)
            {
                draw->PathArcTo(centers[c], arc, starts[c], starts[c] + IM_PI * 0.5f, 6);
                draw->PathStroke(color, ImDrawFlags_None, b);
            }
        }

        // Bootstrap's .form-control: 20% black, an 8% white border (the accent when focused), 4px corners.
        void field_box(Page& page, float x, float y, float w, float h, bool focused = false)
        {
            box(page, x, y, w, h, {FIELD_BG, focused ? BLUE : white(0.08f), 4.f, true});
        }

        // ---------------------------------------------------------------------------------------------------------
        // Rich text: a paragraph's runs with **bold**, [links](url), `code` and {warning} icons, wrapped
        // like Chromium. A link keeps its 2px padding + 1px border at both ends.

        struct Atom
        {
            enum class Kind
            {
                WORD,
                SPACE,
                BREAK,
                WARNING, // fas:exclamation-triangle with mr1
                CODE,
            };
            Kind kind = Kind::WORD;
            std::string_view text;
            Face face = Face::REGULAR;
            std::string_view url;
            float lead = 0.f;
            float trail = 0.f;
        };

        std::vector<Atom> parse_markup(std::string_view markup, Face base)
        {
            std::vector<Atom> atoms;
            const auto words = [&](std::string_view run, Face face, std::string_view url)
            {
                std::size_t a = 0;
                while (a < run.size())
                {
                    if (run[a] == '\n')
                    {
                        atoms.push_back({Atom::Kind::BREAK});
                        ++a;
                        continue;
                    }
                    if (run[a] == ' ')
                    {
                        atoms.push_back({Atom::Kind::SPACE, run.substr(a, 1), face, url});
                        ++a;
                        continue;
                    }
                    std::size_t b = run.find_first_of(" \n", a);
                    b = b == std::string_view::npos ? run.size() : b;
                    atoms.push_back({Atom::Kind::WORD, run.substr(a, b - a), face, url});
                    a = b;
                }
            };
            constexpr std::size_t npos = std::string_view::npos;
            std::size_t i = 0;
            // An unmatched marker: the rest is plain text.
            const auto rest = [&]
            {
                words(markup.substr(i), base, {});
                i = markup.size();
            };
            while (i < markup.size())
            {
                if (markup.compare(i, 2, "**") == 0)
                {
                    const std::size_t close = markup.find("**", i + 2);
                    if (close == npos)
                    {
                        rest();
                        continue;
                    }
                    words(markup.substr(i + 2, close - i - 2), Face::SEMIBOLD, {});
                    i = close + 2;
                    continue;
                }
                if (markup[i] == '[')
                {
                    const std::size_t middle = markup.find("](", i);
                    const std::size_t close = middle == npos ? npos : markup.find(')', middle);
                    if (close == npos)
                    {
                        rest();
                        continue;
                    }
                    const std::size_t first = atoms.size();
                    words(markup.substr(i + 1, middle - i - 1), base, markup.substr(middle + 2, close - middle - 2));
                    atoms[first].lead = 3.f;
                    atoms.back().trail = 3.f;
                    i = close + 1;
                    continue;
                }
                if (markup[i] == '`')
                {
                    const std::size_t close = markup.find('`', i + 1);
                    if (close == npos)
                    {
                        rest();
                        continue;
                    }
                    atoms.push_back({Atom::Kind::CODE, markup.substr(i + 1, close - i - 1), Face::MONO});
                    i = close + 1;
                    continue;
                }
                if (markup[i] == '{')
                {
                    const std::size_t close = markup.find('}', i);
                    if (close == npos)
                    {
                        rest();
                        continue;
                    }
                    atoms.push_back({Atom::Kind::WARNING});
                    i = close + 1;
                    continue;
                }
                std::size_t next = markup.find_first_of("*[`{", i + 1);
                next = next == npos ? markup.size() : next;
                words(markup.substr(i, next - i), base, {});
                i = next;
            }
            return atoms;
        }

        // Draws the markup in a box `width` wide from (x, y); returns its height.
        float rich(Page& page, float x, float y, float width, float line_height, const Style& style,
                   std::string_view markup)
        {
            const std::vector<Atom> atoms = parse_markup(markup, style.face);
            const auto style_of = [&](const Atom& atom)
            {
                Style s = style;
                s.face = atom.face;
                if (atom.face == Face::MONO)
                {
                    s.color = CODE_TEXT;
                }
                if (!atom.url.empty())
                {
                    s.color = ACCENT;
                }
                return s;
            };
            const auto width_of = [&](const Atom& atom)
            {
                switch (atom.kind)
                {
                case Atom::Kind::WORD:
                case Atom::Kind::SPACE:
                    return measure(style_of(atom), atom.text) + atom.lead + atom.trail;
                case Atom::Kind::WARNING:
                    return style.size * 1.125f + 5.f;
                case Atom::Kind::CODE:
                    return measure(style_of(atom), atom.text) + 8.f;
                case Atom::Kind::BREAK:
                    break;
                }
                return 0.f;
            };

            // Lay out: units of atoms with no space between them can't be broken.
            struct Placed
            {
                std::size_t atom;
                float x;
                int line;
            };
            std::vector<Placed> placed;
            std::vector<float> extra(1, 0.f); // per line: what a raised inline box adds under it
            int line = 0;
            float cx = 0.f;
            float pending = 0.f; // spaces not yet placed
            std::vector<std::size_t> pending_spaces;
            std::size_t i = 0;
            while (i < atoms.size())
            {
                const Atom& atom = atoms[i];
                if (atom.kind == Atom::Kind::BREAK)
                {
                    ++line;
                    extra.push_back(0.f);
                    cx = 0.f;
                    pending = 0.f;
                    pending_spaces.clear();
                    ++i;
                    continue;
                }
                if (atom.kind == Atom::Kind::SPACE)
                {
                    // Spaces at the start of a wrapped line collapse.
                    if (cx > 0.f || line == 0)
                    {
                        pending += width_of(atom);
                        pending_spaces.push_back(i);
                    }
                    ++i;
                    continue;
                }
                std::size_t end = i;
                float unit = 0.f;
                while (end < atoms.size() && atoms[end].kind != Atom::Kind::SPACE
                       && atoms[end].kind != Atom::Kind::BREAK)
                {
                    unit += width_of(atoms[end]);
                    ++end;
                }
                if (cx > 0.f && cx + pending + unit > width)
                {
                    ++line;
                    extra.push_back(0.f);
                    cx = 0.f;
                }
                else
                {
                    for (const std::size_t space : pending_spaces)
                    {
                        placed.push_back({space, cx, line});
                        cx += width_of(atoms[space]);
                    }
                }
                pending = 0.f;
                pending_spaces.clear();
                for (std::size_t k = i; k < end; ++k)
                {
                    placed.push_back({k, cx, line});
                    cx += width_of(atoms[k]);
                }
                i = end;
            }

            // Link IDs: one scope per paragraph position.
            ImGui::PushID(static_cast<int>(std::lround(x * 16.f)));
            ImGui::PushID(static_cast<int>(std::lround(y * 16.f)));
            std::vector<float> tops(extra.size(), y);
            for (std::size_t l = 1; l < tops.size(); ++l)
            {
                tops[l] = tops[l - 1] + line_height + extra[l - 1];
            }
            for (const Placed& p : placed)
            {
                const Atom& atom = atoms[p.atom];
                const Style s = style_of(atom);
                const float top = tops[static_cast<std::size_t>(p.line)];
                const float text_top = top + leading(style, line_height);
                const float ax = x + p.x;
                // Font Awesome's svg-inline sits 0.125em under the baseline.
                const float baseline = text_top + std::round(style.size * ascent_of(style.face));
                switch (atom.kind)
                {
                case Atom::Kind::WORD:
                case Atom::Kind::SPACE:
                {
                    const float w = text(page, ax + atom.lead, text_top, s, atom.text);
                    if (!atom.url.empty())
                    {
                        ImGui::PushID(static_cast<int>(p.atom));
                        if (hit(page, "##link", ax, top, w + atom.lead + atom.trail, line_height))
                        {
                            platform::shell::open_url(std::string(atom.url));
                        }
                        ImGui::PopID();
                    }
                    break;
                }
                case Atom::Kind::WARNING:
                    icon(page, "fas:exclamation-triangle", ax, baseline + style.size * 0.125f - style.size,
                         style.size * 1.125f, style.size, style.size, style.color);
                    break;
                case Atom::Kind::CODE:
                {
                    const float w = measure(s, atom.text);
                    box(page, ax, top, w + 8.f, 21.f, {FIELD_BG, CLEAR, 2.f});
                    text(page, ax + 4.f, top + 2.f, s, atom.text);
                    break;
                }
                case Atom::Kind::BREAK:
                    break;
                }
            }
            ImGui::PopID();
            ImGui::PopID();
            return tops.back() - y + line_height + extra.back();
        }

        // ---------------------------------------------------------------------------------------------------------
        // Controls

        // Chromium's checkbox: a 12px box whose corner pixels are blended (a ~1.5px radius); checked = the accent
        // with a dark tick, unchecked = a 40% white border around 20% black. `label_width` extends the click area
        // over a <label> next to it. Returns true when clicked.
        bool checkbox(Page& page, const char* id, float x, float y, bool checked, bool disabled = false,
                      float label_width = 0.f)
        {
            ImDrawList* draw = page.draw;
            const ImVec2 min = page.at(x, y);
            const float one = px(1.f);
            const ImVec2 max(min.x + px(12.f), min.y + px(12.f));
            const float opacity = disabled ? 0.5f : 1.f;
            const auto corners = [&](ImVec4 color, float alpha)
            {
                color.w *= alpha;
                const ImU32 c = u32(color, opacity);
                draw->AddRectFilled(min, ImVec2(min.x + one, min.y + one), c);
                draw->AddRectFilled(ImVec2(max.x - one, min.y), ImVec2(max.x, min.y + one), c);
                draw->AddRectFilled(ImVec2(min.x, max.y - one), ImVec2(min.x + one, max.y), c);
                draw->AddRectFilled(ImVec2(max.x - one, max.y - one), max, c);
            };
            if (checked)
            {
                draw->AddRectFilled(ImVec2(min.x + one, min.y), ImVec2(max.x - one, max.y), u32(ACCENT, opacity));
                draw->AddRectFilled(ImVec2(min.x, min.y + one), ImVec2(min.x + one, max.y - one), u32(ACCENT, opacity));
                draw->AddRectFilled(ImVec2(max.x - one, min.y + one), ImVec2(max.x, max.y - one), u32(ACCENT, opacity));
                corners(ACCENT, 0.85f);
                const ImVec2 points[] = {ImVec2(min.x + px(2.7f), min.y + px(5.9f)),
                                         ImVec2(min.x + px(5.6f), min.y + px(8.7f)),
                                         ImVec2(min.x + px(9.9f), min.y + px(2.9f))};
                draw->AddPolyline(points, 3, u32(rgb(34, 34, 34), opacity), 0, px(1.9f));
            }
            else
            {
                const ImVec4 border = white(0.4f);
                draw->AddRectFilled(ImVec2(min.x + one, min.y + one), ImVec2(max.x - one, max.y - one),
                                    u32(FIELD_BG, opacity));
                draw->AddRectFilled(ImVec2(min.x + one, min.y), ImVec2(max.x - one, min.y + one), u32(border, opacity));
                draw->AddRectFilled(ImVec2(min.x + one, max.y - one), ImVec2(max.x - one, max.y), u32(border, opacity));
                draw->AddRectFilled(ImVec2(min.x, min.y + one), ImVec2(min.x + one, max.y - one), u32(border, opacity));
                draw->AddRectFilled(ImVec2(max.x - one, min.y + one), ImVec2(max.x, max.y - one), u32(border, opacity));
                corners(border, 0.5f);
            }
            return hit(page, id, x - 2.f, y - 4.f, 16.f + label_width, 20.f, !disabled);
        }

        struct Field
        {
            const char* placeholder = "";
            float pad_left = 13.f; // padding + border
            float pad_right = 13.f;
            Style style{14.f, white(0.75f)};
            ImGuiInputTextFlags flags = 0;
            float inset = 0.f; // a text input: the <input> sits 1px inside its form-control box
        };

        // The text of an <input> drawn by ImGui where Chromium puts it: its text box centred in the field.
        bool input_text_at(Page& page, const char* id, float x, float y, float w, float h, std::string& value,
                           const Field& field)
        {
            ImFont* font = font_of(field.style.face);
            const float size = field.style.size;
            const float font_line =
                    std::round(size * ascent_of(field.style.face)) + std::round(size * descent_of(field.style.face));
            const float ascent = size * ascent_of(field.style.face) * dpi_scale();
            const float exact_top = page.origin.y + (y + field.inset) * dpi_scale();
            const float baseline = std::round(exact_top + (h - font_line) / 2.f * dpi_scale() + std::round(ascent));
            const ImVec2 min = page.at(x, y);
            const ImVec2 text_pos(min.x + px(field.pad_left), baseline - std::ceil(ascent));
            ImGui::SetCursorScreenPos(text_pos);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, CLEAR);
            ImGui::PushStyleColor(ImGuiCol_Text, field.style.color);
            ImGui::PushStyleColor(ImGuiCol_TextDisabled, white(0.4f));
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            push_font(font, size);
            ImGui::SetNextItemWidth(min.x + px(w) - text_pos.x - px(field.pad_right));
            const bool changed = ImGui::InputTextWithHint(id, field.placeholder, &value, field.flags);
            text_field_menu(value);
            ImGui::PopFont();
            ImGui::PopStyleVar(2);
            ImGui::PopStyleColor(3);
            return changed;
        }

        bool focused(const char* id)
        {
            return ImGui::GetActiveID() == ImGui::GetID(id);
        }

        // A Bootstrap text field (form-control).
        bool text_field(Page& page, const char* id, float x, float y, float w, std::string& value,
                        const Field& field = {}, float h = 34.f)
        {
            field_box(page, x, y, w, h, focused(id));
            return input_text_at(page, id, x, y, w, h, value, field);
        }

        // The text input: a form-control box without the inset shadow, ringed by another 1px of the accent
        // while focused.
        bool form_text_field(Page& page, const char* id, float x, float y, float w, std::string& value,
                           const Field& field)
        {
            const bool focus = focused(id);
            box(page, x, y, w, 34.f, {FIELD_BG, focus ? BLUE : white(0.08f), 4.f});
            if (focus)
            {
                box(page, x - 1.f, y - 1.f, w + 2.f, 36.f, {CLEAR, BLUE, 5.f});
            }
            return input_text_at(page, id, x, y, w, 34.f, value, field);
        }

        // A <textarea>: ImGui's multiline input at Chromium's first line.
        bool text_area(Page& page, const char* id, float x, float y, float w, float h, std::string& value,
                       const char* placeholder, float pad, float line_height, const Style& style)
        {
            ImFont* font = font_of(style.face);
            const float font_line =
                    std::round(style.size * ascent_of(style.face)) + std::round(style.size * descent_of(style.face));
            const float ascent = style.size * ascent_of(style.face) * dpi_scale();
            const float exact_top = page.origin.y + y * dpi_scale();
            const float baseline =
                    std::round(exact_top + (pad + (line_height - font_line) / 2.f) * dpi_scale() + std::round(ascent));
            const ImVec2 min = page.at(x, y);
            const ImVec2 max(min.x + px(w), min.y + px(h));
            const ImVec2 text_pos(min.x + px(pad), baseline - std::ceil(ascent));
            ImGui::SetCursorScreenPos(text_pos);
            ImGui::PushStyleColor(ImGuiCol_FrameBg, CLEAR);
            ImGui::PushStyleColor(ImGuiCol_Text, style.color);
            ImGui::PushStyleVar(ImGuiStyleVar_FramePadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize, 0.f);
            push_font(font, style.size);
            const bool changed = ImGui::InputTextMultiline(
                    id, &value, ImVec2(max.x - text_pos.x - px(pad), max.y - text_pos.y - px(6.f)));
            const bool active = ImGui::IsItemActive();
            text_field_menu(value);
            ImGui::PopFont();
            ImGui::PopStyleVar(2);
            ImGui::PopStyleColor(2);
            if (value.empty() && !active && placeholder != nullptr)
            {
                float line_y = text_pos.y;
                for (const std::string_view line : wrap(style, placeholder, 1e9f))
                {
                    draw_text(page.draw, font, style.size, ImVec2(text_pos.x, line_y), IM_COL32(255, 255, 255, 102),
                              line.data(), line.data() + line.size());
                    line_y += px(line_height);
                }
            }
            return changed;
        }

        void open_options(Page& page, float x, float y, const std::vector<std::string>& options,
                          const std::function<void(const std::string&)>& pick)
        {
            MenuItems items;
            for (const std::string& option : options)
            {
                items.push_back(menu_item(option,
                                          [pick, option]
                                          {
                                              pick(option);
                                          }));
            }
            popup_menu(std::move(items), page.at(x, y));
        }

        // A native <select class="form-control">: the value 17px in, Chromium's bold chevron 9px from the right;
        // it opens its options as a native menu.
        void native_select(Page& page, const char* id, float x, float y, float w, const std::string& value,
                           const std::vector<std::string>& options, const std::function<void(const std::string&)>& pick)
        {
            field_box(page, x, y, w, 34.f);
            const Style style{14.f, white(1.f)};
            const float ascent = 14.f * ascent_of(Face::REGULAR) * dpi_scale();
            const float exact_top = page.origin.y + y * dpi_scale();
            const float baseline = std::round(exact_top + (34.f - 19.f) / 2.f * dpi_scale() + std::round(ascent));
            const ImVec2 min = page.at(x, y);
            const float right = min.x + px(w);
            draw_text(page.draw, font_of(Face::REGULAR), 14.f, ImVec2(min.x + px(17.f), baseline - std::ceil(ascent)),
                      u32(style.color), value.c_str());
            const float top = std::floor(exact_top);
            const ImVec2 chevron[] = {ImVec2(right - px(12.5f), top + px(15.f)),
                                      ImVec2(right - px(9.f), top + px(18.6f)),
                                      ImVec2(right - px(5.4f), top + px(15.f))};
            page.draw->AddPolyline(chevron, 3, u32(white(1.f)), 0, px(1.8f));
            if (hit(page, id, x, y, w, 34.f))
            {
                open_options(page, x, y + 34.f, options, pick);
            }
        }

        // The design system's select: a 31px button with the value, an optional muted suffix and Material's
        // expand chevron.
        void ds_select(Page& page, const char* id, float x, float y, std::string_view value, std::string_view suffix,
                       const std::vector<std::string>& options, const std::function<void(const std::string&)>& pick)
        {
            const Style style{14.f, white(0.8f)};
            float w = 9.f + measure(style, value) + 8.f + 16.f + 9.f;
            if (!suffix.empty())
            {
                w += 5.f + measure(style, suffix);
            }
            box(page, x, y, w, 31.f, {FIELD_BG, white(0.2f), 2.f});
            float cx = x + 9.f + text(page, x + 9.f, y + 6.f, style, value);
            if (!suffix.empty())
            {
                cx += 5.f + text(page, cx + 5.f, y + 6.f, Style{14.f, white(0.4f)}, suffix);
            }
            svg(page, "chevron-down", svg_path("chevron-down"), 20.f, 20.f, cx + 8.f, y + 7.5f, 16.f, 16.f,
                white(0.6f));
            if (hit(page, id, x, y, w, 31.f))
            {
                open_options(page, x, y + 31.f, options, pick);
            }
        }

        // react-select's control: the value 11px in, an 8% separator and the chevron
        // 25px from the right.
        void combo(Page& page, const char* id, float x, float y, float w, float h, std::string_view value,
                   bool placeholder, const std::vector<std::string>& options,
                   const std::function<void(const std::string&)>& pick, Face face = Face::REGULAR)
        {
            box(page, x, y, w, h, {FIELD_BG, white(0.08f), 4.f});
            if (face == Face::MONO)
            {
                text(page, x + 11.f, y + 8.5f, Style{14.f, white(0.75f), Face::MONO}, value);
            }
            else
            {
                text(page, x + 11.f, y + 7.f, Style{14.f, white(placeholder ? 0.4f : 0.75f)}, value);
            }
            box(page, x + w - 30.f, y + 7.f, 1.f, 20.f, {white(0.08f)});
            svg(page, "chevron-down", svg_path("chevron-down"), 20.f, 20.f, x + w - 25.f, y + 7.f, 20.f, 20.f,
                white(0.75f));
            if (hit(page, id, x, y, w, h))
            {
                open_options(page, x, y + h, options, pick);
            }
        }

        enum class Tone
        {
            PRIMARY,
            SUCCESS,
            DANGER,
            DEFAULT,
        };

        ImVec4 tone_color(Tone tone)
        {
            switch (tone)
            {
            case Tone::PRIMARY:
                return BLUE;
            case Tone::SUCCESS:
                return GREEN;
            case Tone::DANGER:
                return RED;
            case Tone::DEFAULT:
                break;
            }
            return white(0.75f);
        }

        struct ButtonShape
        {
            float height = 31.140625f; // .btn: 12px with a 17.14px line, 6px padding, 1px border
            float line = 17.140625f;
            float pad = 13.f; // padding + border
        };

        constexpr ButtonShape BTN{};
        constexpr ButtonShape BTN_34{34.f, 17.140625f, 13.f}; // next to a 34px field

        // Bootstrap's .btn: the tone at 20% inside its solid border (btn-default: a 75% white border only).
        bool button(Page& page, const char* id, float x, float y, std::string_view label, Tone tone,
                    const ButtonShape& shape = BTN, bool disabled = false)
        {
            const Style style{12.f, white(0.75f)};
            const float w = shape.pad * 2.f + measure(style, label);
            const float opacity = disabled ? 0.5f : 1.f;
            ImVec4 fill = tone_color(tone);
            fill.w = tone == Tone::DEFAULT ? 0.f : 0.2f;
            box(page, x, y, w, shape.height, {fill, tone_color(tone), 2.f}, opacity);
            text(page, x + shape.pad, y + (shape.height - shape.line) / 2.f + leading(style, shape.line), style, label,
                 opacity);
            const bool clicked = hit(page, id, x, y, w, shape.height, !disabled);
            if (ImGui::IsItemHovered() && !disabled)
            {
                const ImVec2 min = page.at(x, y);
                page.draw->AddRectFilled(min, ImVec2(min.x + px(w), min.y + px(shape.height)),
                                         IM_COL32(255, 255, 255, 12), px(2.f));
            }
            return clicked;
        }

        enum class DsTone
        {
            OUTLINE,
            PRIMARY,
            SUCCESS,
        };

        // The design system's button (12px, 70% white): 23px chips with 5px padding, 27px ones with 9px.
        bool ds_button(Page& page, const char* id, float x, float y, std::string_view label, DsTone tone, bool large,
                       bool disabled = false, float* width = nullptr)
        {
            const Style style{12.f, white(0.7f)};
            const float pad = large ? 9.f : 5.f;
            const float h = large ? 27.140625f : 23.140625f;
            const float w = pad * 2.f + measure(style, label);
            BoxStyle shape{CLEAR, OUTLINE, 2.f};
            if (tone != DsTone::OUTLINE)
            {
                const ImVec4 color = tone == DsTone::PRIMARY ? BLUE : GREEN;
                shape.border = color;
                shape.fill = ImVec4(color.x, color.y, color.z, 0.2f);
            }
            const float opacity = disabled ? 0.5f : 1.f;
            box(page, x, y, w, h, shape, opacity);
            text(page, x + pad, y + (large ? 5.f : 3.f), style, label, opacity);
            if (width != nullptr)
            {
                *width = w;
            }
            const bool clicked = hit(page, id, x, y, w, h, !disabled);
            if (ImGui::IsItemHovered() && !disabled)
            {
                const ImVec2 min = page.at(x, y);
                page.draw->AddRectFilled(min, ImVec2(min.x + px(w), min.y + px(h)), IM_COL32(255, 255, 255, 12),
                                         px(2.f));
            }
            return clicked;
        }

        // ---------------------------------------------------------------------------------------------------------
        // Stored values: controls ImGit has no setting for keep what the user chose in preferences.txt.

        std::map<std::string, std::string>& stored(app::Application& app)
        {
            static std::map<std::string, std::string> values;
            static std::string loaded_for; // the profile whose preferences.txt `values` holds
            if (loaded_for != app.profiles.current().guid)
            {
                loaded_for = app.profiles.current().guid;
                values.clear();
                std::ifstream file(app.config_path("preferences.txt"));
                std::string line;
                while (std::getline(file, line))
                {
                    const auto tab = line.find('\t');
                    if (tab != std::string::npos)
                    {
                        values[line.substr(0, tab)] = line.substr(tab + 1);
                    }
                }
            }
            return values;
        }

        void save_stored(app::Application& app)
        {
            std::ofstream file(app.config_path("preferences.txt"));
            for (const auto& [key, value] : stored(app))
            {
                file << key << '\t' << value << '\n';
            }
        }

        std::string& value_of(app::Application& app, const std::string& key, const char* fallback)
        {
            auto& values = stored(app);
            const auto found = values.find(key);
            return found != values.end() ? found->second : values.emplace(key, fallback).first->second;
        }

        bool flag_of(app::Application& app, const std::string& key, bool fallback)
        {
            return value_of(app, key, fallback ? "1" : "0") == "1";
        }

        void set_value(app::Application& app, const std::string& key, std::string value)
        {
            stored(app)[key] = std::move(value);
            save_stored(app);
        }

        void save_settings(app::Application& app)
        {
            app.settings.save(app.config_path("settings.txt"));
        }

        void not_available(app::Application& app, std::string_view what)
        {
            app.toasts.push(app::ToastKind::INFO, std::string(what),
                            "Not available in ImGit yet.");
        }

        // The user and repository the pages describe.
        struct Context
        {
            git::Author author;
            std::string repo_name;
            std::string hooks_path;
            std::string home;
        };

        const Context& context(app::Application& app)
        {
            static Context cached;
            static std::string cached_for = "\x01";
            app::RepoState* state = app.active();
            const std::string key = state != nullptr ? state->repo.workdir() : std::string();
            if (key == cached_for)
            {
                return cached;
            }
            cached_for = key;
            cached = {};
            if (state != nullptr)
            {
                cached.author = state->repo.default_author().value_or(git::Author{});
                cached.repo_name = state->repo.name();
                cached.hooks_path = state->repo.workdir() + ".git/hooks";
                std::replace(cached.hooks_path.begin(), cached.hooks_path.end(), '/',
                             static_cast<char>(std::filesystem::path::preferred_separator));
            }
            if (cached.author.email.empty())
            {
                cached.author.email = app.profiles.current().user_email;
            }
            cached.home = platform::home_directory().string();
            return cached;
        }

        // ---------------------------------------------------------------------------------------------------------
        // Bootstrap's form-horizontal (the classic pages)

        struct Form
        {
            Page& page;
            float left = 55.f; // .container padding
            // The .container: 1170px, or what the page leaves inside its 40px padding and scrollbar; 15px padding.
            float width = std::min(1140.f, page.width - 80.f - page.scrollbar - 30.f);
            float y = 0.f; // bottom of the last block
            float margin = 0.f; // its bottom margin, collapsing with the next block's top margin

            // col-md-3: the label column (the whole row when stacked).
            [[nodiscard]] float column() const
            {
                return page.stacked ? width : width / 4.f;
            }

            // col-md-9: the control column.
            [[nodiscard]] float control_x() const
            {
                return page.stacked ? left + 15.f : left + column() + 15.f;
            }

            [[nodiscard]] float control_width() const
            {
                return page.stacked ? width : width - column();
            }

            [[nodiscard]] float place(float margin_top) const
            {
                return y + std::max(margin, margin_top);
            }

            void advance(float top, float height, float margin_bottom)
            {
                y = top + height;
                margin = margin_bottom;
                page.reach(y + margin + 20.f); // the form's padding-bottom
            }

            // A clearfix / border: the last block's margin stays inside its parent.
            void contain()
            {
                y += margin;
                margin = 0.f;
            }
        };

        // h3.bold: 24px semibold on a 26.4px line, margins 20 / 10.
        void title(Form& form, std::string_view s)
        {
            const float top = form.place(20.f);
            const Style style{24.f, white(0.75f), Face::SEMIBOLD};
            text(form.page, form.left, top + leading(style, 26.390625f), style, s);
            form.advance(top, 26.390625f, 10.f);
        }

        // h4: 18px on a 19.8px line, margins 10 / 10.
        void heading(Form& form, std::string_view s, Face face = Face::SEMIBOLD)
        {
            const float top = form.place(10.f);
            const Style style{18.f, white(0.75f), face};
            text(form.page, form.left, top + leading(style, 19.796875f), style, s);
            form.advance(top, 19.796875f, 10.f);
        }

        // p: 14px on 20px lines.
        void paragraph(Form& form, std::string_view markup, float margin_bottom = 10.f)
        {
            const float top = form.place(0.f);
            form.advance(top, rich(form.page, form.left, top, form.width, 20.f, LABEL, markup), margin_bottom);
        }

        struct Row
        {
            Form& form;
            float top = 0.f;
            float label = 0.f; // the label's height
            float column = 0.f; // the control column's height
            bool help = false; // the help block is open

            [[nodiscard]] Page& page() const
            {
                return form.page;
            }

            [[nodiscard]] float x() const
            {
                return form.control_x();
            }
        };

        // A setting row: the label right-aligned in its column (7px padding-top, 20px lines), `body` draws the
        // control column; 15px under the taller of the two. Stacked, the label sits left-aligned above the column.
        template<class Body>
        void row(Form& form, std::string_view label, Body&& body)
        {
            const float top = form.place(0.f);
            Row r{form, top};
            const float right = form.left + form.column() - 15.f;
            const std::vector<std::string_view> lines = wrap(LABEL, label, form.column() - 30.f);
            for (std::size_t i = 0; i < lines.size(); ++i)
            {
                const float x = form.page.stacked ? form.left + 15.f : right - measure(LABEL, lines[i]);
                text(form.page, x, top + 7.f + 20.f * static_cast<float>(i), LABEL, lines[i]);
            }
            r.label = 7.f + 20.f * static_cast<float>(lines.size());
            if (form.page.stacked)
            {
                r.top += r.label;
                r.label = 0.f;
            }
            body(r);
            if (r.help)
            {
                r.column += 10.f;
            }
            form.advance(top, r.top - top + std::max(r.label, r.column), 15.f);
        }

        // One div.mt1 of the row's help block under the control (12px, 60% white).
        void help(Row& r, std::string_view markup, float line_height = 17.140625f, const Style& style = HELP)
        {
            if (!r.help)
            {
                r.column += 5.f;
                r.help = true;
            }
            r.column += 5.f;
            r.column += rich(r.page(), r.x(), r.top + r.column, std::min(525.f, r.form.control_width() - 30.f),
                             line_height, style, markup);
        }

        // A checkbox in its div.checkbox (7px padding-top, a 20px line), with an optional 12px label after it.
        bool row_checkbox(Row& r, const char* id, bool checked, std::string_view label = {}, bool disabled = false)
        {
            float label_width = 0.f;
            if (!label.empty())
            {
                label_width = 8.f + text(r.page(), r.x() + 20.f, r.top + 7.f + leading(HELP, 20.f), HELP, label);
            }
            r.column = 27.f;
            return checkbox(r.page(), id, r.x(), r.top + 11.f, checked, disabled, label_width);
        }

        // A checkbox row backed by a stored value.
        void stored_checkbox(Row& r, const char* id, const std::string& key, bool fallback, std::string_view label = {},
                             bool disabled = false)
        {
            const bool value = flag_of(r.page().app, key, fallback);
            if (row_checkbox(r, id, value, label, disabled))
            {
                set_value(r.page().app, key, value ? "0" : "1");
            }
        }

        // A checkbox row bound to a setting.
        void setting_checkbox(Row& r, const char* id, bool& setting)
        {
            if (row_checkbox(r, id, setting))
            {
                setting = !setting;
                save_settings(r.page().app);
            }
        }

        // A form-control text field backed by a stored value.
        void stored_field(Row& r, const char* id, const std::string& key, const char* fallback, const Field& field = {},
                          float width = 300.f)
        {
            std::string& value = value_of(r.page().app, key, fallback);
            if (text_field(r.page(), id, r.x(), r.top, width, value, field))
            {
                save_stored(r.page().app);
            }
            r.column = 34.f;
        }

        // A number field bound to a setting: edited as text, applied when the field is left.
        void setting_number(Row& r, const char* id, int value, const std::function<void(int)>& apply)
        {
            static std::map<std::string, std::string> buffers;
            std::string& buffer = buffers[id];
            if (!focused(id))
            {
                buffer = std::to_string(value);
            }
            Field field;
            field.flags = ImGuiInputTextFlags_CharsDecimal;
            text_field(r.page(), id, r.x(), r.top, 300.f, buffer, field);
            if (ImGui::IsItemDeactivatedAfterEdit())
            {
                apply(std::atoi(buffer.c_str()));
                save_settings(r.page().app);
            }
            r.column = 34.f;
        }

        void stored_select(Row& r, const char* id, const std::string& key, const char* fallback,
                           const std::vector<std::string>& options, float width = 300.f)
        {
            app::Application& app = r.page().app;
            native_select(r.page(), id, r.x(), r.top, width, value_of(app, key, fallback), options,
                          [&app, key](const std::string& option)
                          {
                              set_value(app, key, option);
                          });
            r.column = 34.f;
        }

        // A Browse button with the chosen path after it (5px apart, the path on the button's text line).
        void browse_row(Row& r, const std::string& key, const char* fallback, const char* extension)
        {
            app::Application& app = r.page().app;
            if (button(r.page(), "##browse", r.x(), r.top, "Browse", Tone::PRIMARY))
            {
                if (const auto path = platform::shell::open_file_dialog(extension); path)
                {
                    set_value(app, key, *path);
                }
            }
            const float browse_w = 26.f + measure(Style{12.f}, "Browse");
            text(r.page(), r.x() + browse_w + 5.f, r.top + 4.32f, LABEL, value_of(app, key, fallback));
            r.column = 31.140625f;
        }

        // ---------------------------------------------------------------------------------------------------------
        // Pages

        void general_page(Page& page)
        {
            app::Application& app = page.app;
            app::Settings& s = app.settings;
            Form form{page};
            title(form, "General");
            row(form, "Auto-Fetch Interval",
                [&](Row& r)
                {
                    setting_number(r, "##auto_fetch", s.auto_fetch_minutes,
                                   [&](int v)
                                   {
                                       s.auto_fetch_minutes = std::clamp(v, 0, 60);
                                   });
                    help(r, "In minutes, between 0 and 60 (0 disables auto-fetch). Fetches all visible remotes.");
                    help(r, "{warning}If you have many visible remotes, this may impact performance. LFS objects will "
                            "not be automatically fetched.");
                });
            row(form, "Auto-Prune",
                [&](Row& r)
                {
                    stored_checkbox(r, "##prune", "General/Auto-Prune", true);
                });
            row(form, "Keep submodules up to date",
                [&](Row& r)
                {
                    stored_checkbox(r, "##submodules", "General/Keep submodules up to date", true,
                                    "Automatically update all submodules after performing a Git action");
                });
            row(form, "Default Branch Name",
                [&](Row& r)
                {
                    static std::string buffer;
                    if (!focused("##default_branch"))
                    {
                        buffer = s.default_branch == "main" ? std::string() : s.default_branch;
                    }
                    Field field;
                    field.placeholder = "main";
                    if (text_field(page, "##default_branch", r.x(), r.top, 300.f, buffer, field))
                    {
                        s.default_branch = buffer.empty() ? "main" : buffer;
                        save_settings(app);
                    }
                    r.column = 34.f;
                    help(r, "Sets the default branch name when initializing new repositories.");
                });
            row(form, "Delete \".orig\" files after merging",
                [&](Row& r)
                {
                    stored_checkbox(r, "##orig", "General/Delete .orig files", false);
                });
            row(form, "Show All Commits in Graph",
                [&](Row& r)
                {
                    stored_checkbox(r, "##all_commits", "General/Show All Commits in Graph", false);
                    help(r, "{warning}Enabling this option may adversely affect performance.");
                });
            row(form, "Initial Commits in Graph",
                [&](Row& r)
                {
                    setting_number(r, "##initial_commits", s.initial_commits,
                                   [&](int v)
                                   {
                                       s.initial_commits = std::max(500, v);
                                       app.apply_settings();
                                   });
                    help(r, "Minimum 500. If commit lazy loading is enabled, ImGit Desktop will load additional "
                            "commits if you reach the earliest commit in the Graph.");
                    help(r, "{warning}Increasing this value may adversely affect performance.");
                });
            row(form, "Lazy Load Commits in Graph",
                [&](Row& r)
                {
                    stored_checkbox(r, "##lazy", "General/Lazy Load Commits in Graph", true);
                });
            row(form, "Remember tabs",
                [&](Row& r)
                {
                    setting_checkbox(r, "##remember_tabs", s.remember_tabs);
                });
            row(form, "Path to sh.exe",
                [&](Row& r)
                {
                    browse_row(r, "General/Path to sh.exe", "C:\\Program Files\\Git\\bin\\sh.exe", "exe");
                    help(r, "Pointing to sh.exe is only required if you're using Git hooks.\nsh.exe can be found "
                            "wherever you've installed Git for Windows,\nwhich is typically in Program Files\\Git\\bin."
                            "\nIf you can't find it, you may need to install Git for Windows.");
                });
            row(form, "Longpaths",
                [&](Row& r)
                {
                    stored_checkbox(r, "##longpaths", "General/Longpaths", false);
                    help(r, "Sets core.longpaths in your global Git Config. By default, Windows only supports file "
                            "paths less than 260 characters. Enable support for longer file paths.");
                });
            row(form, "AutoCRLF",
                [&](Row& r)
                {
                    stored_checkbox(r, "##autocrlf", "General/AutoCRLF", true);
                    help(r, "Sets core.autocrlf in your global Git Config.");
                });
            row(form, "Use extended logging in activity log",
                [&](Row& r)
                {
                    stored_checkbox(r, "##extended_logging", "General/Extended logging", false);
                });
            row(form, "Forget all Usernames and Passwords",
                [&](Row& r)
                {
                    if (button(page, "##forget_all", r.x(), r.top, "Forget All", Tone::DANGER))
                    {
                        // The ones entered this session; "Remember me" left the others to the credential helper.
                        app.credentials.clear();
                        app.toasts.push(app::ToastKind::INFO, "Forget All",
                                        "Forgot the usernames and passwords entered this session.");
                    }
                    r.column = 31.140625f;
                });
        }

        void open_add_profile(app::Application& app)
        {
            // openAddProfileModal: the current profile's author as the placeholders.
            const app::Profile& current = app.profiles.current();
            app::ProfileDialog& d = app.profile_dialog;
            d.close();
            d.kind = app::ProfileDialog::Kind::ADD_EDIT;
            d.just_opened = true;
            d.defaults = {"", "New Profile", current.user_name, current.user_email};
        }

        void open_edit_profile(app::Application& app, const app::Profile& profile)
        {
            app::ProfileDialog& d = app.profile_dialog;
            d.close();
            d.kind = app::ProfileDialog::Kind::ADD_EDIT;
            d.editing = true;
            d.just_opened = true;
            d.values = profile;
            d.defaults = profile;
            d.gravatar_email = profile.user_email;
        }

        void open_delete_profile(app::Application& app, const app::Profile& profile)
        {
            app::ProfileDialog& d = app.profile_dialog;
            d.close();
            d.kind = app::ProfileDialog::Kind::DELETE;
            d.just_opened = true;
            d.values = profile;
        }

        // The profile row's DropdownButton menu (pullRight, 160px): Switch to this profile, Edit Profile, Delete.
        void profile_row_menu(app::Application& app, const std::string& guid, ImVec2 button_min, ImVec2 button_max)
        {
            const app::Profile* profile = app.profiles.find(guid);
            if (!ImGui::IsPopupOpen("##profile_row_menu"))
            {
                return;
            }
            constexpr float ITEM = 37.140625f; // 12px text, 17.14px line, 10px padding
            const ImVec2 min(button_max.x - px(160.f), button_max.y);
            const ImVec2 max(button_max.x, min.y + px(ITEM * 3.f));
            ImGui::SetNextWindowPos(min);
            ImGui::SetNextWindowSize(ImVec2(max.x - min.x, max.y - min.y));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_PopupBorderSize, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(0.f, 0.f));
            ImGui::PushStyleColor(ImGuiCol_PopupBg, CLEAR);
            const bool open = ImGui::BeginPopup("##profile_row_menu", ImGuiWindowFlags_NoMove);
            ImGui::PopStyleColor();
            ImGui::PopStyleVar(3);
            if (!open)
            {
                return;
            }
            (void)button_min;
            ImDrawList* draw = ImGui::GetWindowDrawList();
            draw->PushClipRectFullScreen();
            draw_box_shadow(draw, min, max, ImVec2(0.f, px(6.f)), px(12.f), 0.f, ImVec4(0.f, 0.f, 0.f, 0.176f));
            draw->AddRectFilled(min, max, u32(PANEL_BG1));
            // Bootstrap's 1px rgba(0, 0, 0, .15) border shows at the sides.
            draw->AddRectFilled(min, ImVec2(min.x + px(1.f), max.y), u32(ImVec4(0.f, 0.f, 0.f, 0.15f)));
            draw->AddRectFilled(ImVec2(max.x - px(1.f), min.y), max, u32(ImVec4(0.f, 0.f, 0.f, 0.15f)));
            draw->PopClipRect();
            const bool is_current = profile != nullptr && guid == app.profiles.current().guid;
            Page menu{app, draw, min, 160.f};
            const struct
            {
                const char* label;
                bool disabled;
                ImVec4 color;
            } items[] = {
                    {"Switch to this profile", is_current, white(0.75f)},
                    {"Edit Profile", false, white(0.75f)},
                    {"Delete", is_current, theme::palette().red},
            };
            int chosen = -1;
            for (int i = 0; i < 3; ++i)
            {
                const float top = ITEM * static_cast<float>(i);
                const bool clicked = hit(menu, items[i].label, 1.f, top, 158.f, ITEM, !items[i].disabled);
                if (ImGui::IsItemHovered() && !items[i].disabled)
                {
                    box(menu, 1.f, top, 158.f, ITEM, {theme::palette().hover_row});
                }
                const Style style{12.f, items[i].disabled && i == 0 ? white(0.4f) : items[i].color};
                text(menu, 21.f, top + 10.f + leading(style, 17.140625f), style, items[i].label);
                if (clicked)
                {
                    chosen = i;
                }
            }
            if (ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                ImGui::CloseCurrentPopup();
            }
            if (chosen >= 0)
            {
                ImGui::CloseCurrentPopup();
            }
            ImGui::EndPopup();
            if (profile == nullptr || chosen < 0)
            {
                return;
            }
            const app::Profile copy = *profile;
            if (chosen == 0)
            {
                app.switch_profile(copy.guid);
            }
            else if (chosen == 1)
            {
                open_edit_profile(app, copy);
            }
            else
            {
                open_delete_profile(app, copy);
            }
        }

        void profiles_page(Page& page)
        {
            app::Application& app = page.app;
            Form form{page};
            title(form, "Profiles");
            paragraph(form, "ImGit Desktop uses profiles to store your application preferences as well as git "
                            "config information. The preferences you set after installing are set as your default "
                            "profile. Create additional profiles to quickly switch between them for different work "
                            "environments.");
            // div.panel-bg1.py1.px3.my3: as wide as its text.
            const Style heading_style{18.f, white(0.75f)};
            const char* keep = "Keep my .gitconfig updated with my ImGit Desktop Profile preferences";
            const char* note = "When checked, ImGit Desktop will update your global .gitconfig to match ImGit "
                               "Preferences, including commit author name and email, and any GPG settings.";
            const float panel_top = form.place(20.f);
            const float panel_w = 40.f + std::max(measure(LABEL, keep) + 20.f, measure(HELP, note));
            box(page, 55.f, panel_top, panel_w, 96.94f, {PANEL_BG1, CLEAR, 2.f});
            text(page, 75.f, panel_top + 12.f, heading_style, "My Profiles");
            const bool keep_config = app.profiles.keep_git_config();
            if (checkbox(page, "##keep_config", 75.f, panel_top + 48.8f, keep_config, false,
                         8.f + measure(LABEL, keep)))
            {
                app.set_keep_git_config(!keep_config);
            }
            text(page, 95.f, panel_top + 44.8f, LABEL, keep);
            text(page, 75.f, panel_top + 64.8f, HELP, note);
            form.advance(panel_top, 96.94f, 20.f);

            const float add_top = form.place(0.f);
            box(page, 55.f, add_top, 112.45f, 30.f, {ImVec4(GREEN.x, GREEN.y, GREEN.z, 0.2f), GREEN, 2.f});
            icon(page, "far:plus", 66.f, add_top + 9.f, 9.f, 12.f, 12.f, white(0.75f));
            text(page, 85.f, add_top + 6.f, Style{12.f, white(0.75f)}, "Add a Profile");
            if (hit(page, "##add_profile", 55.f, add_top, 112.45f, 30.f))
            {
                open_add_profile(app);
            }

            // The profiles table: a 41px header with a bottom border, then a panel-bg1 row per profile, sorted by
            // name; the current one is outlined and checked. Its five cells (20% wide, 10px margins) are spread
            // with justify-content: space-between.
            // The table is 1140px wide in a wide window and narrows with it (the cells shrink, the names
            // truncate), so the row menu at its right end stays on screen; the page does not scroll sideways.
            const float table_w = std::clamp(page.width - page.scrollbar - 55.f - 40.f, 400.f, 1140.f);
            const float scale = std::clamp((table_w - 70.f) / 992.f, 0.3f, 1.f);
            const float cell_w = 248.f * scale;
            const float header = add_top + 40.f;
            box(page, 55.f, header + 40.f, table_w, 1.f, {white(0.08f)});
            const float columns[] = {55.f + 10.f * scale, 55.f + 285.f * scale, 55.f + 560.f * scale};
            const char* titles[] = {"Profile Name", "Author Name", "Author Email"};
            for (int i = 0; i < 3; ++i)
            {
                text(page, columns[i], header + 10.f, LABEL, titles[i]);
            }
            icon(page, "fas:caret-up", 65.f + measure(LABEL, titles[0]) + 5.f, header + 12.75f, 8.75f, 14.f, 14.f,
                 white(0.75f));

            static std::string menu_guid; // the row whose menu is open
            static ImVec2 menu_button_min;
            static ImVec2 menu_button_max;
            float row_top = header + 51.f;
            for (const app::Profile& profile : app.profiles.all())
            {
                const bool current = profile.guid == app.profiles.current().guid;
                const float border = current ? 2.f : 0.f;
                const float height = 50.f + border * 2.f;
                const float gap = (table_w - border * 2.f - (4.f * cell_w + 66.f)) / 4.f;
                const auto cell = [&](int i)
                {
                    return 55.f + border + 10.f + static_cast<float>(i) * (cell_w + gap);
                };
                const bool menu_open = ImGui::IsPopupOpen("##profile_row_menu") && menu_guid == profile.guid;
                const bool hovered =
                        hovering(page.at(55.f, row_top), page.at(55.f + table_w, row_top + height)) || menu_open;
                if (current)
                {
                    box(page, 55.f, row_top, table_w, height,
                        {ImVec4(BLUE.x, BLUE.y, BLUE.z, 0.1f), BLUE, 2.f, false, 2.f});
                }
                else
                {
                    box(page, 55.f, row_top, table_w, height,
                        {hovered ? ImVec4(BLUE.x, BLUE.y, BLUE.z, 0.1f) : PANEL_BG1, CLEAR, 2.f});
                }
                if (current)
                {
                    check_badge(page, 49.f, row_top - 6.f);
                }
                const float content = row_top + border + 10.f;
                draw_profile_icon(page.draw, page.at(cell(0), content), px(30.f), profile);
                text_truncated(page, cell(0) + 40.f, content + 5.f, LABEL, profile.name, cell_w - 60.f);
                text_truncated(page, cell(1), content + 5.f, Style{14.f, white(0.4f)}, profile.user_name,
                               cell_w - 20.f);
                text_truncated(page, cell(2), content + 5.f, Style{14.f, white(0.4f)}, profile.user_email,
                               cell_w - 20.f);
                const float menu_x = cell(3) + cell_w + gap;
                ImGui::PushID(profile.guid.c_str());
                const bool open_menu = hit(page, "##profile_menu", menu_x, content, 46.f, 30.f);
                const bool button_hot = ImGui::IsItemHovered() || menu_open;
                ImGui::PopID();
                if (open_menu)
                {
                    menu_guid = profile.guid;
                    menu_button_min = page.at(menu_x, content);
                    menu_button_max = page.at(menu_x + 46.f, content + 30.f);
                    ImGui::OpenPopup("##profile_row_menu");
                }
                icon(page, "far:ellipsis-v", menu_x + 13.f, content + 7.f, 20.f, 16.f, 16.f,
                     button_hot ? white(1.f) : white(0.75f));
                row_top += height + 10.f;
            }
            profile_row_menu(app, menu_guid, menu_button_min, menu_button_max);
            page.reach(row_top + 10.f);
        }

        // ---------------------------------------------------------------------------------------------------------
        // The profile modals (react-bootstrap Modal.Dialog): 82px from the top over a 50% black backdrop, panel-bg1
        // with a 1px rgba(0, 0, 0, .2) border and Bootstrap's 0 5px 15px rgba(0, 0, 0, .5) shadow, a 51.56px header.

        // Draws the frame of a modal `width` x `height` CSS px; returns its page (positions relative to its top-left)
        // and sets `close` when its × is clicked.
        Page modal_frame(app::Application& app, float width, float height, std::string_view title_icon,
                         float icon_width, std::string_view title, bool& close)
        {
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            ImDrawList* draw = ImGui::GetWindowDrawList();
            draw->AddRectFilled(
                    viewport->WorkPos,
                    ImVec2(viewport->WorkPos.x + viewport->WorkSize.x, viewport->WorkPos.y + viewport->WorkSize.y),
                    IM_COL32(0, 0, 0, 128));
            const ImVec2 min(std::round(viewport->WorkPos.x + (viewport->WorkSize.x - px(width)) / 2.f),
                             viewport->WorkPos.y + px(82.f));
            Page page{app, draw, min, width};
            const ImVec2 max = page.at(width, height);
            draw_box_shadow(draw, min, max, ImVec2(0.f, px(5.f)), px(15.f), 0.f, ImVec4(0.f, 0.f, 0.f, 0.5f));
            box(page, 0.f, 0.f, width, height, {PANEL_BG1, ImVec4(0.f, 0.f, 0.f, 0.2f)});
            box(page, 1.f, 1.f, width - 2.f, 51.5625f, {theme::palette().panel_bg2});
            const Style title_style{18.f, white(0.75f)};
            icon(page, title_icon, 21.f, 17.265625f, icon_width, 18.f, 18.f, white(0.75f));
            text(page, 21.f + icon_width + 10.f, 13.421875f + leading(title_style, 25.71875f), title_style, title);
            close = hit(page, "##close", width - 39.5f, 11.f, 18.5f, 30.5625f);
            icon(page, "far:times", width - 36.5f, 15.5f, 12.5f, 20.f, 20.f,
                 ImGui::IsItemHovered() ? white(1.f) : white(0.75f));
            return page;
        }

        bool is_email(const std::string& email)
        {
            const auto at = email.find('@');
            if (at == std::string::npos || at == 0 || email.find('@', at + 1) != std::string::npos
                || email.find(' ') != std::string::npos)
            {
                return false;
            }
            const std::string domain = email.substr(at + 1);
            const auto dot = domain.rfind('.');
            return dot != std::string::npos && dot > 0 && domain.size() - dot - 1 >= 2;
        }

        void add_edit_profile_modal(app::Application& app)
        {
            app::ProfileDialog& d = app.profile_dialog;
            // gravatarEmail follows the email field 500ms after typing stops.
            if (d.gravatar_email != d.values.user_email && ImGui::GetTime() - d.email_changed_at > 0.5)
            {
                d.gravatar_email = d.values.user_email;
            }
            const bool valid_email = d.values.user_email.empty() || is_email(d.values.user_email);
            const float error_h = valid_email ? 0.f : 17.140625f;
            bool close = false;
            Page page = modal_frame(app, 750.f, 336.703125f + error_h, "far:user", 15.75f,
                                    d.editing ? "Edit Profile" : "Add Profile", close);

            // The preview (the author's Gravatar or initials), then the fields with their placeholders: what an
            // empty field falls back to.
            const float controls = 67.5625f;
            const app::Profile preview{"", "", d.values.user_name.empty() ? d.defaults.user_name : d.values.user_name,
                                       d.gravatar_email.empty() ? d.defaults.user_email : d.gravatar_email};
            draw_profile_icon(page.draw, page.at(21.f, controls), px(55.f), preview);
            text(page, 86.f, controls, LABEL, "Profile Name");
            Field name_field;
            name_field.placeholder = d.defaults.name.c_str();
            name_field.inset = 1.f;
            if (d.just_opened)
            {
                ImGui::SetKeyboardFocusHere();
            }
            form_text_field(page, "##profile_name", 86.f, controls + 20.f, 640.f, d.values.name, name_field);
            text(page, 21.f, controls + 69.f, LABEL, "Author Name");
            Field user_field;
            user_field.placeholder = d.defaults.user_name.c_str();
            user_field.inset = 1.f;
            form_text_field(page, "##profile_user_name", 21.f, controls + 89.f, 710.f, d.values.user_name, user_field);
            text(page, 21.f, controls + 138.f, LABEL, "Author Email");
            Field email_field;
            email_field.placeholder = d.defaults.user_email.c_str();
            email_field.inset = 1.f;
            if (!valid_email)
            {
                // .has-error
                box(page, 21.f, controls + 158.f, 710.f, 34.f, {CLEAR, RED, 4.f});
            }
            if (form_text_field(page, "##profile_user_email", 21.f, controls + 158.f, 710.f, d.values.user_email,
                              email_field))
            {
                d.email_changed_at = ImGui::GetTime();
            }
            if (!valid_email)
            {
                const Style error{12.f, RED};
                const char* message = "Must use a valid email.";
                text(page, 731.f - 20.f - measure(error, message), controls + 192.f, error, message);
            }

            // The footer: "Copy settings from current profile" (adding only), Cancel and the submit button,
            // aligned to the right on their baseline.
            const float footer = controls + 222.f + error_h;
            const char* submit_label = d.editing ? "Save Changes" : "Add Profile";
            const Style button_style{12.f, white(0.75f)};
            const float submit_w = 26.f + measure(button_style, submit_label);
            const float cancel_w = 26.f + measure(button_style, "Cancel");
            const float submit_x = 729.f - submit_w;
            const float cancel_x = submit_x - 10.f - cancel_w;
            if (!d.editing)
            {
                const char* copy = "Copy settings from current profile";
                const float copy_x = cancel_x - 10.f - (20.f + measure(LABEL, copy));
                if (checkbox(page, "##copy_profile", copy_x, footer + 7.33f, d.copy_current, false,
                             8.f + measure(LABEL, copy)))
                {
                    d.copy_current = !d.copy_current;
                }
                text(page, copy_x + 20.f, footer + 3.33f, LABEL, copy);
            }
            const bool can_submit = !(d.defaults.name.empty() && d.values.name.empty())
                                    && !(d.defaults.user_name.empty() && d.values.user_name.empty())
                                    && !(d.defaults.user_email.empty() && d.values.user_email.empty()) && valid_email;
            if (button(page, "##cancel_profile", cancel_x, footer, "Cancel", Tone::DEFAULT))
            {
                close = true;
            }
            bool submit =
                    button(page, "##submit_profile", submit_x, footer, submit_label, Tone::SUCCESS, BTN, !can_submit);
            submit = submit || (can_submit && ImGui::IsKeyPressed(ImGuiKey_Enter));
            d.just_opened = false;
            if (ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                close = true;
            }
            if (submit)
            {
                // The defaults stand in for the fields left empty.
                app::Profile profile = d.values;
                profile.guid = d.values.guid;
                for (auto [field, fallback] :
                     {std::pair{&profile.name, &d.defaults.name}, std::pair{&profile.user_name, &d.defaults.user_name},
                      std::pair{&profile.user_email, &d.defaults.user_email}})
                {
                    if (field->empty())
                    {
                        *field = *fallback;
                    }
                }
                if (d.editing)
                {
                    app.edit_profile(profile);
                }
                else
                {
                    app.add_profile(profile, d.copy_current);
                }
                close = true;
            }
            if (close)
            {
                d.close();
            }
        }

        void delete_profile_modal(app::Application& app)
        {
            app::ProfileDialog& d = app.profile_dialog;
            bool close = false;
            Page page = modal_frame(app, 600.f, 244.703125f, "far:users", 22.5f, "Delete profile", close);
            // "Are you sure you want to **delete** this profile?"
            float x = 41.f;
            x += text(page, x, 82.5625f, LABEL, "Are you sure you want to ");
            x += text(page, x, 82.5625f, Style{14.f, white(0.75f), Face::SEMIBOLD}, "delete");
            text(page, x, 82.5625f, LABEL, " this profile?");
            draw_profile_icon(page.draw, page.at(41.f, 112.5625f), px(30.f), d.values);
            text_truncated(page, 81.f, 117.5625f, LABEL, d.values.name, 478.f);
            const char* label = "Delete profile";
            const float w = 26.f + measure(Style{12.f, white(0.75f)}, label);
            const bool confirm = button(page, "##delete_profile", 559.f - w, 152.5625f, label, Tone::DANGER);
            const char* warning = "This action cannot be undone";
            text(page, 559.f - measure(LABEL, warning), 193.703125f, Style{14.f, white(0.4f)}, warning);
            d.just_opened = false;
            if (confirm)
            {
                app.delete_profile(d.values.guid);
                close = true;
            }
            if (close || ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                d.close();
            }
        }

        // Over everything, in its own window like the command palette.
        void draw_profile_dialogs(app::Application& app)
        {
            if (app.profile_dialog.kind == app::ProfileDialog::Kind::NONE)
            {
                return;
            }
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            ImGui::SetNextWindowPos(viewport->WorkPos);
            ImGui::SetNextWindowSize(viewport->WorkSize);
            ImGui::SetNextWindowFocus();
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing, ImVec2(0.f, 0.f));
            ImGui::Begin("##profile_modal", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoSavedSettings
                                 | ImGuiWindowFlags_NoBackground | ImGuiWindowFlags_NoNav
                                 | ImGuiWindowFlags_NoScrollWithMouse);
            if (app.profile_dialog.kind == app::ProfileDialog::Kind::ADD_EDIT)
            {
                add_edit_profile_modal(app);
            }
            else
            {
                delete_profile_modal(app);
            }
            ImGui::End();
            ImGui::PopStyleVar(3);
        }

        void ssh_page(Page& page)
        {
            app::Application& app = page.app;
            const std::string& home = context(app).home;
            Form form{page};
            title(form, "SSH");
            row(form, "Use local SSH agent",
                [&](Row& r)
                {
                    stored_checkbox(r, "##ssh_agent", "SSH/Use local SSH agent", false);
                });
            // The path after the button sits on an inline line box half a pixel taller than the button.
            row(form, "SSH Private Key",
                [&](Row& r)
                {
                    browse_row(r, "SSH/Private key", (home + "/.ssh/id_rsa").c_str(), "");
                    r.column = 31.64f;
                });
            row(form, "SSH Public Key",
                [&](Row& r)
                {
                    browse_row(r, "SSH/Public key", (home + "/.ssh/id_rsa.pub").c_str(), "pub");
                    const std::string& path = value_of(app, "SSH/Public key", (home + "/.ssh/id_rsa.pub").c_str());
                    // The copy button: btn-default with a clipboard (18px).
                    const float copy_x = r.x() + 31.f + measure(Style{12.f}, "Browse") + measure(LABEL, path) + 5.f;
                    box(page, copy_x, r.top - 0.5f, 40.f, 32.140625f, {CLEAR, white(0.75f), 2.f});
                    svg(page, "clipboard", svg_path("clipboard"), 384.f, 512.f, copy_x + 11.f, r.top + 6.5f,
                        13.5f, 18.f, white(0.75f));
                    if (hit(page, "##copy_public_key", copy_x, r.top - 0.5f, 40.f, 32.140625f))
                    {
                        std::ifstream file(path);
                        std::string key((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
                        ImGui::SetClipboardText(key.c_str());
                    }
                    r.column = 31.64f;
                });
            row(form, "Generate new Private/Public key",
                [&](Row& r)
                {
                    if (button(page, "##generate_ssh", r.x(), r.top, "Generate", Tone::SUCCESS))
                    {
                        not_available(app, "Generate SSH key");
                    }
                    r.column = 31.140625f;
                    help(r, "This will create new keys and configure ImGit Desktop to use them");
                });
            row(form, "Use default Git Credential Manager",
                [&](Row& r)
                {
                    stored_checkbox(r, "##credential_manager", "SSH/Use Git Credential Manager", false);
                    help(r, "Select this to have ImGit use the credential manager in your Git config to access "
                            "Git repositories");
                });
        }

        void ds_title(Page& page, std::string_view s)
        {
            text(page, 40.f, 19.f, Style{24.f, white(0.8f)}, s);
        }

        const Style DS_TEXT{14.f, white(0.8f)};
        const Style DS_HINT{12.f, white(0.6f)};

        // A form-horizontal select over a stored value (External Tools' design-system selects).
        void tool_select(Row& r, const char* id, const std::string& key, const char* fallback,
                         const std::vector<std::string>& options)
        {
            app::Application& app = r.page().app;
            ds_select(r.page(), id, r.x(), r.top, value_of(app, key, fallback), "", options,
                      [&app, key](const std::string& option)
                      {
                          set_value(app, key, option);
                      });
            r.column = 31.f;
        }

        // div.sub-setting-group (a clearfix) with its header: the h4's margins stay inside, 5px under the header.
        template<class Body>
        void group(Form& form, std::string_view name, Body&& body)
        {
            form.advance(form.place(0.f), 0.f, 0.f);
            heading(form, name);
            body();
            form.contain();
            form.advance(form.y, 0.f, 15.f);
        }

        void external_tools_page(Page& page)
        {
            Form form{page};
            title(form, "External Tools");
            group(form, "Merge Tool",
                  [&]
                  {
                      row(form, "External Merge Tool",
                          [&](Row& r)
                          {
                              tool_select(r, "##merge_tool", "External Tools/Merge Tool", "Git Config Default",
                                          {"<None>", "Git Config Default"});
                          });
                  });
            group(form, "Diff Tool",
                  [&]
                  {
                      row(form, "External Diff Tool",
                          [&](Row& r)
                          {
                              tool_select(r, "##diff_tool", "External Tools/Diff Tool", "<Use Merge Tool>",
                                          {"<Use Merge Tool>", "<None>", "Git Config Default"});
                          });
                  });
            group(form, "Editor",
                  [&]
                  {
                      row(form, "External Editor",
                          [&](Row& r)
                          {
                              tool_select(r, "##editor", "External Tools/Editor", "<None>",
                                          {"<None>", "<Custom>", "Visual Studio Code", "Rider"});
                          });
                  });
            group(form, "Terminal",
                  [&]
                  {
                      row(form, "External Terminal",
                          [&](Row& r)
                          {
                              tool_select(r, "##terminal", "External Tools/Terminal", "<None>",
                                          {"<None>", "ImGit Desktop Integrated Terminal", "<Custom>", "cmd",
                                           "Git Bash", "PowerShell"});
                          });
                  });
        }

        void notifications_page(Page& page)
        {
            Form form{page};
            title(form, "Notifications");
            row(form, "Enable Desktop Notifications",
                [&](Row& r)
                {
                    stored_checkbox(r, "##desktop", "Notifications/Desktop", false);
                    help(r, "Allow ImGit to send desktop notifications.");
                });
            row(form, "Receive Marketing Notifications",
                [&](Row& r)
                {
                    stored_checkbox(r, "##marketing", "Notifications/Marketing", true);
                    help(r, "Receive marketing related notifications.");
                });
        }

        // moment.js format tokens the date fields take (the subset its defaults use).
        std::string moment_format(std::string_view format, const std::tm& t)
        {
            static const char* months[] = {"January", "February", "March",     "April",   "May",      "June",
                                           "July",    "August",   "September", "October", "November", "December"};
            const auto two = [](int v)
            {
                return (v < 10 ? "0" : "") + std::to_string(v);
            };
            std::string out;
            std::size_t i = 0;
            while (i < format.size())
            {
                const auto take = [&](std::string_view token)
                {
                    if (format.compare(i, token.size(), token) == 0)
                    {
                        i += token.size();
                        return true;
                    }
                    return false;
                };
                if (take("YYYY"))
                {
                    out += std::to_string(t.tm_year + 1900);
                }
                else if (take("MMMM"))
                {
                    out += months[t.tm_mon];
                }
                else if (take("MM"))
                {
                    out += two(t.tm_mon + 1);
                }
                else if (take("DD"))
                {
                    out += two(t.tm_mday);
                }
                else if (take("D"))
                {
                    out += std::to_string(t.tm_mday);
                }
                else if (take("hh"))
                {
                    out += two(t.tm_hour % 12 == 0 ? 12 : t.tm_hour % 12);
                }
                else if (take("h"))
                {
                    out += std::to_string(t.tm_hour % 12 == 0 ? 12 : t.tm_hour % 12);
                }
                else if (take("HH"))
                {
                    out += two(t.tm_hour);
                }
                else if (take("mm"))
                {
                    out += two(t.tm_min);
                }
                else if (take("A"))
                {
                    out += t.tm_hour < 12 ? "AM" : "PM";
                }
                else
                {
                    out += format[i++];
                }
            }
            return out;
        }

        // A react-select colour picker (200px) with a reset button 10px after it.
        void color_row(Row& r, const char* id, const ImVec4& swatch, const char* value, const char* reset)
        {
            Page& page = r.page();
            box(page, r.x(), r.top, 200.f, 34.f, {FIELD_BG, white(0.08f), 4.f});
            box(page, r.x() + 11.f, r.top + 7.f, 20.f, 20.f, {swatch});
            text(page, r.x() + 36.f, r.top + 7.f, LABEL, value);
            box(page, r.x() + 170.f, r.top + 7.f, 1.f, 20.f, {white(0.08f)});
            svg(page, "chevron-down", svg_path("chevron-down"), 20.f, 20.f, r.x() + 175.f, r.top + 7.f, 20.f, 20.f,
                white(0.75f));
            ImGui::PushID(id);
            if (hit(page, "##color", r.x(), r.top, 200.f, 34.f))
            {
                not_available(page.app, "Workspace colors");
            }
            // btn-default.btn-custom with a far:sync icon (fa-fw, 12px) and a 5px caption margin.
            const Style style{12.f, white(0.75f)};
            const float bx = r.x() + 210.f;
            const float w = 13.f + 15.f + 5.f + measure(style, reset) + 5.f + 13.f;
            box(page, bx, r.top, w, 31.140625f, {CLEAR, white(0.75f), 2.f});
            icon(page, "far:sync", bx + 13.f, r.top + 9.5f, 15.f, 12.f, 12.f, white(0.75f));
            text(page, bx + 33.f, r.top + 7.f, style, reset);
            if (hit(page, "##reset", bx, r.top, w, 31.140625f))
            {
                not_available(page.app, reset);
            }
            ImGui::PopID();
            r.column = 34.f;
        }

        void ui_customization_page(Page& page)
        {
            app::Application& app = page.app;
            app::Settings& s = app.settings;
            Form form{page};
            title(form, "UI Customization");
            row(form, "Theme",
                [&](Row& r)
                {
                    const auto names = theme::theme_names();
                    native_select(r.page(), "##theme", r.x(), r.top, 300.f, s.theme,
                                  std::vector<std::string>(names.begin(), names.end()),
                                  [&app](const std::string& option)
                                  {
                                      app.settings.theme = option;
                                      app.apply_settings();
                                  });
                    r.column = 34.f;
                });
            row(form, "Notification Location",
                [&](Row& r)
                {
                    stored_select(r, "##notification_location", "UI Customization/Notification Location", "Bottom Left",
                                  {"Top Left", "Top Right", "Bottom Left", "Bottom Right"});
                });

            // Date/Time: a sub-setting group with a bottom border, its header underlined.
            const float group_top = form.place(0.f);
            form.advance(group_top, 0.f, 0.f);
            const float header_top = form.place(10.f);
            text(page, 55.f, header_top + leading(Style{18.f}, 19.796875f), Style{18.f, white(0.75f), Face::SEMIBOLD},
                 "Date/Time");
            box(page, 55.f, header_top + 29.8f, 1140.f, 1.f, {white(0.6f)});
            form.advance(header_top, 30.8f, 5.f);
            paragraph(form, "See [here](https://momentjs.com/docs/#/displaying/format/) for formatting options. "
                            "Restart ImGit Desktop for changes to take effect.");
            row(form, "Date/Time Locale",
                [&](Row& r)
                {
                    stored_select(r, "##locale", "UI Customization/Locale", "System [system]",
                                  {"System [system]", "English [en]", "English (United Kingdom) [en-gb]",
                                   "English (United States) [en-us]", "French [fr]", "German [de]", "Spanish [es]",
                                   "Italian [it]", "Japanese [ja]", "Russian [ru]", "Chinese (China) [zh-cn]"});
                });
            const std::tm local = platform::local_time(std::time(nullptr));
            const std::pair<const char*, const char*> formats[] = {{"Date Format", "MM/DD/YYYY"},
                                                                   {"Date word Format", "MMMM D, YYYY"},
                                                                   {"Date/Time Format", "MM/DD/YYYY @ h:mm A"},
                                                                   {"Date verbose Format", "MMMM D, YYYY h:mm A"}};
            for (const auto& [label, fallback] : formats)
            {
                row(form, label,
                    [&](Row& r)
                    {
                        ImGui::PushID(label);
                        Field field;
                        field.placeholder = fallback;
                        field.inset = 1.f;
                        const std::string key = std::string("UI Customization/") + label;
                        stored_field(r, "##format", key, "", field);
                        const std::string& value = value_of(app, key, "");
                        text(page, r.x() + 305.f, r.top + 7.f, HELP,
                             moment_format(value.empty() ? fallback : value, local));
                        ImGui::PopID();
                    });
            }
            form.contain();
            box(page, 55.f, form.y, 1140.f, 1.f, {white(0.6f)});
            form.advance(form.y, 1.f, 15.f);

            row(form, "Default color for default groups",
                [&](Row& r)
                {
                    color_row(r, "groups", white(0.1f), "Gray", "Reset all default groups to default color");
                });
            row(form, "Automatically group worktree tabs together",
                [&](Row& r)
                {
                    stored_checkbox(r, "##group_worktrees", "UI Customization/Group worktree tabs", true);
                });
            row(form, "Show toolbar icon labels",
                [&](Row& r)
                {
                    setting_checkbox(r, "##toolbar_labels", s.show_toolbar_labels);
                });
            row(form, "Enable spell checking",
                [&](Row& r)
                {
                    stored_checkbox(r, "##spell", "UI Customization/Spell checking", true);
                });
            row(form, "Display author initials and generic remote icons instead of avatars",
                [&](Row& r)
                {
                    setting_checkbox(r, "##initials", s.author_initials);
                });
            row(form, "Show ghost branch/tag when hovering over or selecting a commit",
                [&](Row& r)
                {
                    setting_checkbox(r, "##ghost", s.show_ghost_refs);
                });
            row(form, "Highlight associated rows when hovering over a branch",
                [&](Row& r)
                {
                    setting_checkbox(r, "##highlight", s.highlight_rows_on_ref_hover);
                });
            row(form, "Branch visibility in commit graph",
                [&](Row& r)
                {
                    stored_select(r, "##branch_visibility", "UI Customization/Branch visibility", "All",
                                  {"All", "Smart Branch Visibility"});
                });
            row(form, "Show branches and tags in graph",
                [&](Row& r)
                {
                    stored_checkbox(r, "##refs", "UI Customization/Show refs", true);
                });
            row(form, "Show commit author in graph",
                [&](Row& r)
                {
                    setting_checkbox(r, "##author", s.show_author);
                });
            row(form, "Show commit date/time in graph",
                [&](Row& r)
                {
                    setting_checkbox(r, "##date", s.show_date);
                });
            row(form, "Show commit message in graph",
                [&](Row& r)
                {
                    stored_checkbox(r, "##message", "UI Customization/Show message", true);
                });
            row(form, "Show commit description in graph",
                [&](Row& r)
                {
                    const std::string key = "UI Customization/Show description";
                    std::string value = s.show_description ? value_of(app, key, "Always") : std::string("Never");
                    if (value == "Never" && s.show_description)
                    {
                        value = "Always";
                    }
                    native_select(page, "##description", r.x(), r.top, 300.f, value, {"Always", "On hover", "Never"},
                                  [&app, key](const std::string& option)
                                  {
                                      set_value(app, key, option);
                                      app.settings.show_description = option != "Never";
                                      save_settings(app);
                                  });
                    r.column = 34.f;
                });
            row(form, "Show commit sha in graph",
                [&](Row& r)
                {
                    setting_checkbox(r, "##sha", s.show_sha);
                });
            row(form, "Show commit tree in graph",
                [&](Row& r)
                {
                    stored_checkbox(r, "##tree", "UI Customization/Show tree", true);
                });
        }

        void commit_signing_page(Page& page)
        {
            app::Application& app = page.app;
            Form form{page};
            title(form, "Commit Signing");
            row(form, "GPG Format",
                [&](Row& r)
                {
                    stored_select(r, "##gpg_format", "Commit Signing/GPG Format", "OPENPGP", {"OPENPGP", "SSH"});
                });
            row(form, "GPG Program",
                [&](Row& r)
                {
                    stored_field(r, "##gpg_program", "Commit Signing/GPG Program", "gpg", {}, 227.28f);
                    if (button(page, "##browse_gpg", r.x() + 232.28f, r.top, "Browse", Tone::PRIMARY, BTN_34))
                    {
                        if (const auto path = platform::shell::open_file_dialog("exe"); path)
                        {
                            set_value(app, "Commit Signing/GPG Program", *path);
                        }
                    }
                    help(r,
                         "This is usually \"gpg\" or \"gpg2\" depending on the\nversion you are using.\nYou can also "
                         "browse for the path directly.");
                });
            row(form, "Signing Key",
                [&](Row& r)
                {
                    stored_select(r, "##signing_key", "Commit Signing/Signing Key", "<None>", {"<None>"}, 257.f);
                    // The refresh button: btn-primary with a 12px refresh icon.
                    box(page, r.x() + 262.f, r.top, 38.f, 34.f, {ImVec4(BLUE.x, BLUE.y, BLUE.z, 0.2f), BLUE, 2.f});
                    svg(page, "arrows-rotate", svg_path("arrows-rotate"), 512.f, 512.f, r.x() + 275.f,
                        r.top + 11.f, 12.f, 12.f, white(0.75f));
                    (void)hit(page, "##refresh_keys", r.x() + 262.f, r.top, 38.f, 34.f);
                });
            row(form, "Generate new GPG Key",
                [&](Row& r)
                {
                    static std::string passphrase;
                    Field field;
                    field.placeholder = "Passphrase (optional)";
                    field.flags = ImGuiInputTextFlags_Password;
                    field.inset = 1.f;
                    (void)text_field(page, "##passphrase", r.x(), r.top, 216.89f, passphrase, field);
                    r.column = 34.f;
                    if (button(page, "##generate_gpg", r.x() + 221.89f, r.top, "Generate", Tone::SUCCESS, BTN_34))
                    {
                        not_available(app, "Generate GPG key");
                    }
                    help(r, "This will create a 4096 bit key with the name and email\nin your current profile that "
                            "expires in 2 years.\nThis will also be set as your active signing key.");
                });
            row(form, "Sign Commits by Default",
                [&](Row& r)
                {
                    stored_checkbox(r, "##sign_commits", "Commit Signing/Sign Commits", false);
                });
            row(form, "Sign Tags by Default",
                [&](Row& r)
                {
                    stored_checkbox(r, "##sign_tags", "Commit Signing/Sign Tags", false);
                });
        }

        const std::vector<std::string>& monospace_fonts()
        {
            static const std::vector<std::string> list = {"Cascadia Code", "Cascadia Mono", "Consolas", "Courier New",
                                                          "Lucida Console"};
            return list;
        }

        void font_row(Row& r, const char* key_prefix)
        {
            app::Application& app = r.page().app;
            const std::string key = std::string(key_prefix) + "/Font";
            combo(
                    r.page(), "##font", r.x(), r.top, 300.f, 34.f, value_of(app, key, "Consolas"), false,
                    monospace_fonts(),
                    [&app, key](const std::string& option)
                    {
                        set_value(app, key, option);
                    },
                    Face::MONO);
            r.column = 34.f;
        }

        void editor_page(Page& page)
        {
            app::Application& app = page.app;
            Form form{page};
            title(form, "Editor");
            row(form, "Font",
                [&](Row& r)
                {
                    font_row(r, "Editor");
                    help(r, "");
                    // The help line is the "Show Only Monospace Fonts" checkbox.
                    const float line = r.top + r.column - 17.140625f;
                    const bool only_mono = flag_of(app, "Editor/Only monospace", true);
                    if (checkbox(page, "##only_mono", r.x(), line + 2.56f, only_mono, false,
                                 8.f + measure(HELP, "Show Only Monospace Fonts")))
                    {
                        set_value(app, "Editor/Only monospace", only_mono ? "0" : "1");
                    }
                    text(page, r.x() + 20.f, line, HELP, "Show Only Monospace Fonts");
                });
            row(form, "Font Size",
                [&](Row& r)
                {
                    stored_field(r, "##font_size", "Editor/Font Size", "12",
                                 Field{"", 13.f, 13.f, {}, ImGuiInputTextFlags_CharsDecimal});
                });
            row(form, "Tab Size",
                [&](Row& r)
                {
                    stored_field(r, "##tab_size", "Editor/Tab Size", "4",
                                 Field{"", 13.f, 13.f, {}, ImGuiInputTextFlags_CharsDecimal});
                });
            row(form, "End of Line Character",
                [&](Row& r)
                {
                    stored_select(r, "##eol", "Editor/End of Line", "CRLF", {"LF", "CRLF"});
                });
            row(form, "Syntax Highlighting",
                [&](Row& r)
                {
                    stored_checkbox(r, "##syntax", "Editor/Syntax Highlighting", true);
                });
            row(form, "Show Line Numbers",
                [&](Row& r)
                {
                    stored_checkbox(r, "##line_numbers", "Editor/Show Line Numbers", true);
                });
            row(form, "Word Wrap",
                [&](Row& r)
                {
                    stored_checkbox(r, "##word_wrap", "Editor/Word Wrap", false);
                });
        }

        void terminal_page(Page& page)
        {
            app::Application& app = page.app;
            Form form{page};
            title(form, "In-App Terminal");
            row(form, "Font",
                [&](Row& r)
                {
                    font_row(r, "In-App Terminal");
                    // The help block: the monospace checkbox in its div.checkbox, then the note.
                    const float top = r.top + 44.f;
                    const bool only_mono = flag_of(app, "In-App Terminal/Only monospace", true);
                    if (checkbox(page, "##only_mono", r.x(), top + 11.f, only_mono, false,
                                 8.f + measure(HELP, "Show Only Monospace Fonts")))
                    {
                        set_value(app, "In-App Terminal/Only monospace", only_mono ? "0" : "1");
                    }
                    text(page, r.x() + 20.f, top + 8.42f, HELP, "Show Only Monospace Fonts");
                    text(page, r.x(), top + 27.f, HELP,
                         "Changes to this setting will only apply to new tabs or after restarting ImGit Desktop");
                    r.column = 103.14f;
                });
            row(form, "Font Size",
                [&](Row& r)
                {
                    stored_field(r, "##font_size", "In-App Terminal/Font Size", "12",
                                 Field{"", 13.f, 13.f, {}, ImGuiInputTextFlags_CharsDecimal});
                });
            row(form, "Line Height",
                [&](Row& r)
                {
                    stored_field(r, "##line_height", "In-App Terminal/Line Height", "1.2",
                                 Field{"", 13.f, 13.f, {}, ImGuiInputTextFlags_CharsDecimal});
                });
            row(form, "Cursor Style",
                [&](Row& r)
                {
                    stored_select(r, "##cursor", "In-App Terminal/Cursor Style", "Bar", {"Block", "Underline", "Bar"});
                });
            row(form, "Dim terminal when unfocused",
                [&](Row& r)
                {
                    stored_checkbox(r, "##dim", "In-App Terminal/Dim when unfocused", true);
                });
            row(form, "Default In-App Terminal",
                [&](Row& r)
                {
                    stored_select(r, "##shell", "In-App Terminal/Default", "PowerShell", {"Git Bash", "PowerShell"});
                    help(r,
                         "Changes to this setting will only apply to new tabs or after restarting ImGit Desktop");
                });
            heading(form, "Autocomplete options");
            row(form, "Tab Behavior",
                [&](Row& r)
                {
                    stored_select(r, "##tab_behavior", "In-App Terminal/Tab Behavior", "Ignore",
                                  {"Navigation", "Enter", "Ignore"});
                });
            row(form, "Enable Autocomplete Suggestions",
                [&](Row& r)
                {
                    stored_checkbox(r, "##autocomplete", "In-App Terminal/Autocomplete", true);
                });
        }

        void encoding_page(Page& page)
        {
            Form form{page};
            title(form, "Encoding");
            row(form, "Default encoding",
                [&](Row& r)
                {
                    stored_select(r, "##encoding", "Encoding/Default", "UTF-8",
                                  {"GUESS ENCODING", "UTF-8",        "UTF-8 BOM",    "UTF-16LE",     "UTF-16BE",
                                   "UTF-32LE",       "UTF-32BE",     "windows-1250", "windows-1251", "windows-1252",
                                   "windows-1253",   "windows-1254", "windows-1255", "windows-1256", "windows-1257",
                                   "windows-1258",   "Windows 874",  "ISO-8859-1",   "ISO-8859-2",   "ISO-8859-3",
                                   "ISO-8859-4",     "ISO-8859-5",   "ISO-8859-6",   "ISO-8859-7",   "ISO-8859-8",
                                   "ISO-8859-9",     "ISO-8859-10",  "ISO-8859-11",  "ISO-8859-13",  "ISO-8859-14",
                                   "ISO-8859-15",    "ISO-8859-16",  "Mac Roman",    "CP437",        "CP850",
                                   "CP852",          "CP865",        "CP866",        "KOI8-R",       "KOI8-U",
                                   "KOI8-RU",        "KOI8-T",       "GBK",          "GB18030",      "GB2312",
                                   "Big5",           "Big5-HKSCS",   "Shift_JIS",    "EUC-JP",       "EUC-KR"});
                    help(r, "Default encoding when reading or writing files in this repo.");
                });
        }

        void gitflow_page(Page& page)
        {
            app::Application& app = page.app;
            Form form{page};
            title(form, "Gitflow");
            // section-header h4s (medium weight); the fields are text inputs: a form-control div around an input.
            const auto field_row = [&](const char* label, const char* fallback)
            {
                row(form, label,
                    [&](Row& r)
                    {
                        ImGui::PushID(label);
                        Field field;
                        field.inset = 1.f;
                        stored_field(r, "##field", std::string("Gitflow/") + label, fallback, field);
                        ImGui::PopID();
                    });
            };
            heading(form, "Branches", Face::REGULAR);
            field_row("Main/Master", "main");
            field_row("Develop", "develop");
            heading(form, "Prefixes", Face::REGULAR);
            field_row("Feature", "feature/");
            field_row("Release", "release/");
            field_row("Hotfix", "hotfix/");
            field_row("Version Tag", "");
            const float top = form.place(0.f);
            if (button(page, "##init_gitflow", form.control_x(), top, "Initialize Gitflow", Tone::SUCCESS))
            {
                not_available(app, "Initialize Gitflow");
            }
            form.advance(top, 31.140625f, 0.f);
        }

        void git_hooks_page(Page& page)
        {
            app::Application& app = page.app;
            Form form{page};
            title(form, "Git Hooks");
            paragraph(form,
                      "Git hooks are shell scripts that execute after an event such as a commit or push. For more "
                      "details, see the [Git hooks documentation](https://git-scm.com/docs/githooks).",
                      20.f);
            row(form, "Git Hooks directory",
                [&](Row& r)
                {
                    Field field;
                    field.placeholder = context(app).hooks_path.c_str();
                    stored_field(r, "##hooks_dir", "Git Hooks/Directory", "", field, 227.28f);
                    if (button(page, "##browse_hooks", r.x() + 232.28f, r.top, "Browse", Tone::PRIMARY, BTN_34))
                    {
                        if (const auto path = platform::shell::open_file_dialog(""); path)
                        {
                            set_value(app, "Git Hooks/Directory", *path);
                        }
                    }
                    help(r, "The directory that Git hooks will use for this repository and its linked worktrees.");
                });
            row(form, "Skip git hooks by default when committing",
                [&](Row& r)
                {
                    stored_checkbox(r, "##skip_hooks", "Commit/Skip hooks", false);
                });
        }

        void commit_page(Page& page)
        {
            app::Application& app = page.app;
            app::Settings& s = app.settings;
            Form form{page};
            title(form, "Commit");
            row(form, "Push after each commit",
                [&](Row& r)
                {
                    if (row_checkbox(r, "##push_after_commit", s.push_after_commit))
                    {
                        s.push_after_commit = !s.push_after_commit;
                        for (auto& tab : app.tabs)
                        {
                            tab->push_after_commit = s.push_after_commit;
                        }
                        save_settings(app);
                    }
                });
            row(form, "Skip git hooks by default when committing",
                [&](Row& r)
                {
                    stored_checkbox(r, "##skip_hooks", "Commit/Skip hooks", false);
                });
            heading(form, "Merge Behavior");
            row(form, "Squash",
                [&](Row& r)
                {
                    stored_checkbox(r, "##squash", "Commit/Squash merges", false);
                    help(r,
                         "When enabled, merging branches locally will stage all changes without creating a merge "
                         "commit automatically. You will need to commit manually.",
                         18.f);
                });
            heading(form, "Commit Template");
            paragraph(form, "Create and apply a default initial message for commits to maintain consistency in commit "
                            "summaries and descriptions. For more information see the [Git commit template "
                            "documentation](https://git-scm.com/docs/git-commit#Documentation/git-commit.txt---"
                            "templateltfilegt)");
            row(form, "Apply this template to commit messages",
                [&](Row& r)
                {
                    stored_checkbox(r, "##apply_template", "Commit/Apply template", true);
                });
            row(form, "Remove comments from commit messages",
                [&](Row& r)
                {
                    stored_checkbox(r, "##strip_comments", "Commit/Remove comments", false);
                });
            // The template being edited; Save stores it.
            static std::string summary;
            static std::string description;
            static bool loaded = false;
            if (!loaded)
            {
                loaded = true;
                summary = value_of(app, "Commit/Template summary", "");
                description = value_of(app, "Commit/Template description", "");
            }
            row(form, "Summary",
                [&](Row& r)
                {
                    Field field;
                    field.pad_right = 21.f;
                    (void)text_field(page, "##template_summary", r.x(), r.top, 300.f, summary, field);
                    const std::string left = std::to_string(72 - static_cast<int>(summary.size()));
                    text(page, r.x() + 294.f - measure(LABEL, left), r.top + 6.f, Style{14.f, white(0.4f)}, left);
                    r.column = 34.f;
                });
            row(form, "Description",
                [&](Row& r)
                {
                    field_box(page, r.x(), r.top, 825.f, 400.f, focused("##template_description"));
                    (void)text_area(page, "##template_description", r.x(), r.top, 825.f, 400.f, description, nullptr,
                                    7.f, 20.f, LABEL);
                    r.column = 400.f;
                });
            const bool changed = summary != value_of(app, "Commit/Template summary", "")
                                 || description != value_of(app, "Commit/Template description", "");
            const float top = form.place(0.f);
            if (button(page, "##save_template", form.control_x(), top, "Save Commit Template", Tone::SUCCESS, BTN,
                       !changed))
            {
                stored(app)["Commit/Template summary"] = summary;
                set_value(app, "Commit/Template description", description);
            }
            form.advance(top, 31.140625f, 0.f);
        }

        void lfs_page(Page& page)
        {
            app::Application& app = page.app;
            Form form{page};
            title(form, "LFS");
            form.contain();
            heading(form, "Initialize LFS", Face::REGULAR);
            // Plain text, not markup: the repository's name may hold markup characters.
            {
                const float line_top = form.place(0.f);
                text(page, form.left, line_top + leading(LABEL, 20.f), LABEL,
                     "LFS has not been initialized on " + context(app).repo_name);
                form.advance(line_top, 20.f, 10.f);
            }
            const float top = form.place(0.f);
            if (button(page, "##init_lfs", form.left, top, "Initialize LFS on This Repo", Tone::SUCCESS))
            {
                if (app::RepoState* state = app.active(); state != nullptr)
                {
                    app.run_git_cli(*state, "Initialize LFS", {"lfs", "install", "--local"});
                }
            }
            form.advance(top, 31.140625f, 0.f);
        }

        void sparse_checkout_page(Page& page)
        {
            app::Application& app = page.app;
            ds_title(page, "Sparse checkout");
            text(page, 40.f, 64.2f, DS_TEXT, "Paths to include");
            const float right = std::min(857.69f, page.width - 40.f);
            std::string& paths = value_of(app, "Sparse Checkout/Paths", "");
            box(page, 40.f, 96.2f, right - 40.f, 238.f, {FIELD_BG, white(0.2f), 2.f});
            if (text_area(page, "##paths", 40.f, 96.2f, right - 40.f, 238.f, paths, "e.g.\nsrc/\ndocs/\nREADME.md", 9.f,
                          21.f, DS_TEXT))
            {
                save_stored(app);
            }
            text(page, 40.f, 346.2f, DS_HINT,
                 "Enter one path per line. Only these paths will be checked out in your working directory. If no paths "
                 "are provided, only root files will be checked out.");
            app::RepoState* state = app.active();
            const bool enabled = flag_of(app, "Sparse Checkout/Enabled", false);
            float w = 0.f;
            float x = 40.f;
            if (ds_button(page, "##enable", x, 376.2f, "Enable sparse checkout", DsTone::OUTLINE, true,
                          state == nullptr, &w)
                && state != nullptr)
            {
                std::vector<std::string> args = {"sparse-checkout", "set"};
                for (const std::string_view line : wrap(DS_TEXT, paths, 1e9f))
                {
                    if (!line.empty())
                    {
                        args.emplace_back(line);
                    }
                }
                app.run_git_cli(*state, "Enable sparse checkout", args);
                set_value(app, "Sparse Checkout/Enabled", "1");
            }
            x += w + 8.f;
            if (ds_button(page, "##disable", x, 376.2f, "Disable sparse checkout", DsTone::OUTLINE, true, !enabled, &w)
                && state != nullptr)
            {
                app.run_git_cli(*state, "Disable sparse checkout", {"sparse-checkout", "disable"});
                set_value(app, "Sparse Checkout/Enabled", "0");
            }
            x += w + 8.f;
            if (ds_button(page, "##reapply", x, 376.2f, "Reapply sparse checkout", DsTone::OUTLINE, true, !enabled)
                && state != nullptr)
            {
                app.run_git_cli(*state, "Reapply sparse checkout", {"sparse-checkout", "reapply"});
            }
            page.reach(376.2f + 27.14f + 20.f);
        }

        // ---------------------------------------------------------------------------------------------------------
        // The sidebar

        struct PageEntry
        {
            const char* name;
            const char* icon;
            void (*draw)(Page&);
        };

        constexpr PageEntry GLOBAL_PAGES[] = {
                {"General", "fas:cog", general_page},
                {"Profiles", "fas:user-friends", profiles_page},
                {"SSH", "fas:shield-check", ssh_page},
                {"External Tools", "fas:tools", external_tools_page},
                {"Notifications", "fas:bell", notifications_page},
                {"UI Customization", "fas:paint-brush", ui_customization_page},
                {"Commit Signing", "fas:file-signature", commit_signing_page},
                {"Editor", "fas:code", editor_page},
                {"In-App Terminal", "far:terminal", terminal_page},
        };

        constexpr PageEntry REPO_PAGES[] = {
                {"Encoding", "far:text-size", encoding_page},
                {"Gitflow", "fas:exchange", gitflow_page},
                {"Git Hooks", "fas:brackets-curly", git_hooks_page},
                {"Commit", "fas:code-commit", commit_page},
                {"LFS", "fas:file", lfs_page},
                {"Sparse Checkout", "fas:folder-tree", sparse_checkout_page},
        };

        // A page row: 45.7px (py2 around an 18px label on a 25.7px line), the fa-fw icon 30px in.
        void sidebar_row(Page& page, float top, const PageEntry& entry)
        {
            app::Application& app = page.app;
            const bool selected = app.preferences_page == entry.name;
            const bool clicked = hit(page, entry.name, 0.f, top, page.width, 45.7f);
            const bool hovered = ImGui::IsItemHovered();
            if (clicked)
            {
                app.preferences_page = entry.name;
            }
            if (selected || hovered)
            {
                box(page, 0.f, top, page.width, 45.7f,
                    {selected ? theme::palette().selected_row : theme::palette().hover_row});
            }
            const ImVec4 color = selected || hovered ? white(1.f) : white(0.6f);
            icon(page, entry.icon, 30.f, top + 13.25f, 22.5f, 18.f, 18.f, color);
            text(page, 72.5f, top + 10.f, Style{18.f, color}, entry.name);
        }

        void draw_sidebar(app::Application& app, float height)
        {
            // A short window scrolls the sidebar; its 8px scrollbar narrows the rows (the border-right stays
            // at 299px, drawn by draw_preferences).
            const float w = 299.f - (ImGui::GetScrollMaxY() > 0.f ? 8.f : 0.f);
            Page page{app, ImGui::GetWindowDrawList(), ImGui::GetCursorScreenPos(), w};
            box(page, 0.f, 0.f, 299.f, std::max(height, 1317.f), {PANEL_BG1});
            const Style secondary{14.f, white(0.6f)};

            // Exit Preferences.
            if (hit(page, "##exit", 0.f, -0.855f, w, 45.7f))
            {
                app.preferences_open = false;
            }
            if (ImGui::IsItemHovered())
            {
                box(page, 0.f, -0.855f, w, 45.7f, {theme::palette().hover_row});
            }
            icon(page, "fas:sign-out", 20.f, 13.25f, 18.f, 18.f, 18.f, white(0.75f));
            text(page, 43.f, 10.f, Style{18.f, white(0.75f)}, "Exit Preferences");

            // The profile picker: a react-select showing the current profile as its placeholder; its menu switches.
            const app::Profile& profile = app.profiles.current();
            text(page, 10.f, 45.7f, secondary, "Current profile");
            box(page, 10.f, 75.7f, w - 10.f, 42.f, {PANEL_BG0, CLEAR, 2.f});
            box(page, 10.f, 75.7f, w - 10.f, 42.f, {FIELD_BG, white(0.08f), 4.f});
            if (hit(page, "##profile_picker", 10.f, 75.7f, w - 10.f, 42.f))
            {
                ImGui::OpenPopup("##sidebar_profile_select");
            }
            if (ImGui::IsPopupOpen("##sidebar_profile_select"))
            {
                box(page, 10.f, 75.7f, w - 10.f, 42.f, {CLEAR, BLUE, 4.f});
                draw_select_focus(page.draw, page.at(10.f, 75.7f), page.at(w, 117.7f), px(4.f));
            }
            draw_profile_icon(page.draw, page.at(21.f, 82.7f), px(28.f), profile);
            text_truncated(page, 59.f, 86.7f, Style{14.f, white(0.4f)}, profile.name, 200.f);
            box(page, w - 30.f, 82.7f, 1.f, 28.f, {white(0.08f)});
            svg(page, "chevron-down", svg_path("chevron-down"), 20.f, 20.f, w - 25.f, 86.7f, 20.f, 20.f,
                white(0.75f));
            if (const std::string picked = profile_select_menu(app, "##sidebar_profile_select", page.at(10.f, 75.7f),
                                                               page.at(w, 117.7f), SIDEBAR_PROFILE_MENU);
                !picked.empty())
            {
                app.switch_profile(picked);
            }

            text(page, 10.f, 137.7f, secondary, "Preferences");
            float top = 162.7f;
            for (const PageEntry& entry : GLOBAL_PAGES)
            {
                sidebar_row(page, top, entry);
                top += 45.703125f;
            }
            text(page, 10.f, top + 30.f, secondary, "Repo-Specific Preferences");
            icon(page, "far:folder", 10.f, top + 57.75f, 14.f, 14.f, 14.f, white(0.6f));
            text(page, 29.f, top + 55.f, secondary, "Repo: " + context(app).repo_name);
            top += 80.f;
            for (const PageEntry& entry : REPO_PAGES)
            {
                sidebar_row(page, top, entry);
                top += 45.703125f;
            }
            ImGui::SetCursorScreenPos(page.origin);
            ImGui::Dummy(ImVec2(px(w), px(top)));
        }
    } // namespace

    void draw_preferences(app::Application& app, ImVec2 size)
    {
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        // panel-bg0 under everything.
        ImGui::GetWindowDrawList()->AddRectFilled(origin, ImVec2(origin.x + size.x, origin.y + size.y), u32(PANEL_BG0));

        ImGui::GetWindowDrawList()->AddRectFilled(ImVec2(origin.x + px(299.f), origin.y),
                                                  ImVec2(origin.x + px(300.f), origin.y + size.y), u32(white(0.08f)));
        ImGui::PushStyleVar(ImGuiStyleVar_ScrollbarSize, px(8.f));
        ImGui::BeginChild("##prefs_sidebar", ImVec2(px(299.f), size.y), ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoBackground);
        ImGui::PopStyleVar();
        draw_sidebar(app, size.y / dpi_scale());
        ImGui::EndChild();

        ImGui::SetCursorScreenPos(ImVec2(origin.x + px(300.f), origin.y));
        ImGui::BeginChild("##prefs_page", ImVec2(size.x - px(300.f), size.y), ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoBackground);
        Page page{app, ImGui::GetWindowDrawList(), ImGui::GetCursorScreenPos(), (size.x - px(300.f)) / dpi_scale()};
        // The page scrollbar is 8px wide; the grid's breakpoint follows the window (a media query).
        page.scrollbar = ImGui::GetScrollMaxY() > 0.f ? 8.f : 0.f;
        page.stacked = ImGui::GetIO().DisplaySize.x / dpi_scale() < 992.f;
        ImGui::PushID(app.preferences_page.c_str());
        bool drawn = false;
        for (const PageEntry& entry : GLOBAL_PAGES)
        {
            if (app.preferences_page == entry.name)
            {
                entry.draw(page);
                drawn = true;
            }
        }
        for (const PageEntry& entry : REPO_PAGES)
        {
            if (!drawn && app.preferences_page == entry.name)
            {
                entry.draw(page);
            }
        }
        ImGui::PopID();
        // Reserve the content for scrolling.
        ImGui::SetCursorScreenPos(page.origin);
        ImGui::Dummy(ImVec2(px(page.width), px(page.extent)));
        ImGui::EndChild();
        draw_profile_dialogs(app);
    }
} // namespace imgit::ui
