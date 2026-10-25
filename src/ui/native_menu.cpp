#include "ui/native_menu.hpp"
#include "platform/system.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cctype>
#include <cmath>
#include <imgui_internal.h>

// Geometry and colours of Electron 41's menus on Windows 11, 100% scale:
//   Segoe UI 9pt, rows 28px, 12px above the first and below the last row, separators 8 + 1 + 8,
//   labels 20px from the left (48 when the menu has check marks), 8px corner radius,
//   #1F1F1F background, #363635 hot row, #E3E3E3 text, #C7C7C7 accelerators, #9AA0A6 disabled text,
//   #5E5E5E full-width separators and a DWM style shadow offset downwards.
namespace imgit::ui
{
    namespace
    {
        constexpr float ROW_HEIGHT = 28.f;
        constexpr float VERTICAL_PADDING = 12.f;
        constexpr float SEPARATOR_HEIGHT = 17.f;
        constexpr float LABEL_START = 20.f;
        constexpr float CHECK_COLUMN = 28.f;
        constexpr float RIGHT_MARGIN = 22.f;
        constexpr float ARROW_COLUMN = 33.f;
        constexpr float ACCELERATOR_PADDING = 6.f;
        constexpr float CORNER_RADIUS = 8.f;
        constexpr float FONT_SIZE = 12.f;

        constexpr ImU32 BACKGROUND = IM_COL32(0x1F, 0x1F, 0x1F, 0xFF);
        constexpr ImU32 HOT_ROW = IM_COL32(0x36, 0x36, 0x35, 0xFF);
        constexpr ImU32 TEXT = IM_COL32(0xE3, 0xE3, 0xE3, 0xFF);
        constexpr ImU32 ACCELERATOR = IM_COL32(0xC7, 0xC7, 0xC7, 0xFF);
        constexpr ImU32 DISABLED = IM_COL32(0x9A, 0xA0, 0xA6, 0xFF);
        constexpr ImU32 SEPARATOR = IM_COL32(0x5E, 0x5E, 0x5E, 0xFF);

        // Shadow opacity per pixel of distance outside each edge (the corners combine them). Above the menu it is
        // below one grey level.
        constexpr float SHADOW_SIDE[] = {0.151f, 0.14f,  0.108f, 0.097f, 0.065f, 0.065f,
                                         0.043f, 0.032f, 0.032f, 0.032f, 0.011f};
        constexpr float SHADOW_BOTTOM[] = {0.258f, 0.247f, 0.237f, 0.226f, 0.204f, 0.194f, 0.172f, 0.172f,
                                           0.172f, 0.14f,  0.14f,  0.129f, 0.108f, 0.097f, 0.086f, 0.075f,
                                           0.065f, 0.065f, 0.043f, 0.032f, 0.032f, 0.032f, 0.011f};
        constexpr float SHADOW_PEAK = 0.36f;

        struct Level
        {
            MenuItems items;
            ImVec2 pos;
            ImVec2 size;
            float label_start = 0.f;
            float accelerator_right = 0.f;
            int hot = -1;
            int child_item = -1; // item whose submenu is the next level
            int pending_item = -1; // item whose submenu opens / closes when the delay elapses
            double pending_since = 0.0;
        };

        struct MenuBarState
        {
            int index = -1;
            ImVec2 button_min;
            ImVec2 button_max;
        };

        std::vector<Level> g_levels;
        MenuBarState g_bar;
        bool g_keyboard = false;
        bool g_take_focus = false;
        std::function<void()> g_pending_click;
        int g_opened_frame = -1;
        // A menu opens on a press; the release of that same click must not pick the item that happens to land
        // under the cursor (a tall menu is moved up over it). A release counts once the mouse was dragged away
        // from where the menu opened, or after a fresh press.
        bool g_release_armed = false;
        ImVec2 g_open_mouse;

        bool selectable(const MenuItem& item)
        {
            return item.type != MenuItem::Type::SEPARATOR && item.enabled;
        }

        // A label as shown: '&' marks the mnemonic in the menu bar's menus; a context menu (its labels built from
        // branch, tag and file names) shows it as it is.
        std::string menu_text(const std::string& label)
        {
            return g_bar.index >= 0 ? strip_mnemonic(label) : label;
        }

        float label_width(const std::string& label)
        {
            const std::string text = menu_text(label);
            return text_size(fonts().system, FONT_SIZE, text.c_str()).x;
        }

        void layout(Level& level)
        {
            bool checks = false;
            bool submenus = false;
            float max_label = 0.f;
            float max_accelerator = 0.f;
            float height = px(VERTICAL_PADDING) * 2.f;
            for (const MenuItem& item : level.items)
            {
                if (item.type == MenuItem::Type::SEPARATOR)
                {
                    height += px(SEPARATOR_HEIGHT);
                    continue;
                }
                height += px(ROW_HEIGHT);
                checks |= item.type == MenuItem::Type::CHECKBOX;
                submenus |= !item.submenu.empty();
                max_label = std::max(max_label, label_width(item.label));
                if (!item.accelerator.empty())
                {
                    max_accelerator =
                            std::max(max_accelerator, text_size(fonts().system, FONT_SIZE, item.accelerator.c_str()).x);
                }
            }
            level.label_start = px(LABEL_START + (checks ? CHECK_COLUMN : 0.f));
            // The check column moves the labels 28px but widens the menu by 25 (Chromium's check icon overlaps the
            // left padding).
            float width = level.label_start - (checks ? px(3.f) : 0.f) + std::ceil(max_label) + px(RIGHT_MARGIN);
            if (submenus)
            {
                width += px(ARROW_COLUMN);
            }
            if (max_accelerator > 0.f)
            {
                width += std::ceil(max_accelerator) + px(ACCELERATOR_PADDING);
            }
            level.accelerator_right = width - px(LABEL_START + 1.f);
            level.size = ImVec2(std::round(width), std::round(height));
        }

        // Places a menu inside the viewport like Chromium: below / right of the anchor, flipped when it would
        // not fit.
        void place(Level& level, ImVec2 anchor, float flip_x, float flip_y)
        {
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            const ImVec2 min = viewport->Pos;
            const ImVec2 max(viewport->Pos.x + viewport->Size.x, viewport->Pos.y + viewport->Size.y);
            ImVec2 pos = anchor;
            if (pos.x + level.size.x > max.x)
            {
                pos.x = flip_x - level.size.x;
            }
            if (pos.y + level.size.y > max.y)
            {
                pos.y = std::max(min.y, std::min(flip_y - level.size.y, max.y - level.size.y));
            }
            pos.x = std::max(min.x, pos.x);
            pos.y = std::max(min.y, pos.y);
            level.pos = ImVec2(std::round(pos.x), std::round(pos.y));
        }

        float item_top(const Level& level, int index)
        {
            float y = level.pos.y + px(VERTICAL_PADDING);
            for (int i = 0; i < index; ++i)
            {
                y += px(level.items[static_cast<std::size_t>(i)].type == MenuItem::Type::SEPARATOR ? SEPARATOR_HEIGHT
                                                                                                   : ROW_HEIGHT);
            }
            return y;
        }

        int item_at(const Level& level, ImVec2 mouse)
        {
            if (mouse.x < level.pos.x || mouse.x >= level.pos.x + level.size.x || mouse.y < level.pos.y
                || mouse.y >= level.pos.y + level.size.y)
            {
                return -1;
            }
            float y = level.pos.y + px(VERTICAL_PADDING);
            for (int i = 0; i < static_cast<int>(level.items.size()); ++i)
            {
                const MenuItem& item = level.items[static_cast<std::size_t>(i)];
                const float h = px(item.type == MenuItem::Type::SEPARATOR ? SEPARATOR_HEIGHT : ROW_HEIGHT);
                if (mouse.y >= y && mouse.y < y + h)
                {
                    return item.type == MenuItem::Type::SEPARATOR ? -1 : i;
                }
                y += h;
            }
            return -1;
        }

        int level_at(ImVec2 mouse)
        {
            for (int i = static_cast<int>(g_levels.size()) - 1; i >= 0; --i)
            {
                const Level& level = g_levels[static_cast<std::size_t>(i)];
                if (mouse.x >= level.pos.x && mouse.x < level.pos.x + level.size.x && mouse.y >= level.pos.y
                    && mouse.y < level.pos.y + level.size.y)
                {
                    return i;
                }
            }
            return -1;
        }

        void close_children(std::size_t depth)
        {
            if (g_levels.size() > depth + 1)
            {
                g_levels.resize(depth + 1);
            }
            g_levels[depth].child_item = -1;
        }

        void open_child(std::size_t depth, int index, bool select_first)
        {
            close_children(depth);
            Level& parent = g_levels[depth];
            const MenuItem& item = parent.items[static_cast<std::size_t>(index)];
            if (item.submenu.empty() || !item.enabled)
            {
                return;
            }
            parent.child_item = index;
            parent.pending_item = -1;
            Level child;
            child.items = item.submenu;
            layout(child);
            const float top = item_top(parent, index);
            place(child, ImVec2(parent.pos.x + parent.size.x, top - px(VERTICAL_PADDING)), parent.pos.x,
                  top + px(ROW_HEIGHT) + px(VERTICAL_PADDING));
            if (select_first)
            {
                for (int i = 0; i < static_cast<int>(child.items.size()); ++i)
                {
                    if (selectable(child.items[static_cast<std::size_t>(i)]))
                    {
                        child.hot = i;
                        break;
                    }
                }
            }
            g_levels.push_back(std::move(child));
        }

        void activate(std::size_t depth, int index)
        {
            const MenuItem& item = g_levels[depth].items[static_cast<std::size_t>(index)];
            if (!selectable(item))
            {
                return;
            }
            if (!item.submenu.empty())
            {
                open_child(depth, index, false);
                return;
            }
            g_pending_click = item.click;
            close_menus();
        }

        void move_hot(Level& level, int direction)
        {
            const int count = static_cast<int>(level.items.size());
            if (count == 0)
            {
                return;
            }
            int index = level.hot;
            for (int step = 0; step < count; ++step)
            {
                index = index < 0 ? (direction > 0 ? 0 : count - 1) : (index + direction + count) % count;
                if (selectable(level.items[static_cast<std::size_t>(index)]))
                {
                    level.hot = index;
                    return;
                }
            }
        }

        // Menu bar wiring, set by the application each frame it draws the bar.
        std::function<MenuItems(int)> g_bar_builder;
        std::function<bool(int, ImVec2*, ImVec2*)> g_bar_rect;
        int g_bar_count = 0;

        void switch_bar_menu(int index, bool keyboard)
        {
            if (!g_bar_builder || g_bar_count <= 0)
            {
                return;
            }
            index = (index + g_bar_count) % g_bar_count;
            ImVec2 min;
            ImVec2 max;
            if (!g_bar_rect(index, &min, &max))
            {
                return;
            }
            popup_bar_menu(g_bar_builder(index), index, min, max, keyboard);
        }

        void handle_keyboard()
        {
            if (g_levels.empty())
            {
                return;
            }
            Level& level = g_levels.back();
            const std::size_t depth = g_levels.size() - 1;
            if (ImGui::IsKeyPressed(ImGuiKey_DownArrow))
            {
                move_hot(level, 1);
                g_keyboard = true;
            }
            else if (ImGui::IsKeyPressed(ImGuiKey_UpArrow))
            {
                move_hot(level, -1);
                g_keyboard = true;
            }
            else if (ImGui::IsKeyPressed(ImGuiKey_RightArrow))
            {
                if (level.hot >= 0 && !level.items[static_cast<std::size_t>(level.hot)].submenu.empty())
                {
                    open_child(depth, level.hot, true);
                }
                else if (g_bar.index >= 0)
                {
                    switch_bar_menu(g_bar.index + 1, true);
                }
            }
            else if (ImGui::IsKeyPressed(ImGuiKey_LeftArrow))
            {
                if (depth > 0)
                {
                    g_levels.pop_back();
                    g_levels.back().child_item = -1;
                }
                else if (g_bar.index >= 0)
                {
                    switch_bar_menu(g_bar.index - 1, true);
                }
            }
            else if (ImGui::IsKeyPressed(ImGuiKey_Escape))
            {
                if (depth > 0)
                {
                    g_levels.pop_back();
                    g_levels.back().child_item = -1;
                }
                else
                {
                    close_menus();
                }
            }
            else if (ImGui::IsKeyPressed(ImGuiKey_Enter) || ImGui::IsKeyPressed(ImGuiKey_KeypadEnter)
                     || ImGui::IsKeyPressed(ImGuiKey_Space))
            {
                if (level.hot >= 0)
                {
                    activate(depth, level.hot);
                }
            }
            else
            {
                // Mnemonics (menu bar menus only): the underlined letter of an item activates it.
                for (int key = ImGuiKey_A; key <= ImGuiKey_Z && g_bar.index >= 0; ++key)
                {
                    if (!ImGui::IsKeyPressed(static_cast<ImGuiKey>(key), false))
                    {
                        continue;
                    }
                    const char letter = static_cast<char>('a' + (key - ImGuiKey_A));
                    for (int i = 0; i < static_cast<int>(level.items.size()); ++i)
                    {
                        const std::string& label = level.items[static_cast<std::size_t>(i)].label;
                        const auto amp = label.find('&');
                        if (amp != std::string::npos && amp + 1 < label.size()
                            && std::tolower(static_cast<unsigned char>(label[amp + 1])) == letter)
                        {
                            level.hot = i;
                            activate(depth, i);
                            return;
                        }
                    }
                }
            }
        }

        void handle_mouse()
        {
            const ImVec2 mouse = ImGui::GetMousePos();
            const double now = ImGui::GetTime();
            const int over_level = level_at(mouse);

            // A press outside every menu closes them; the press is then replayed so that it also reaches what
            // is under the mouse, as Chromium reposts it to the window below.
            for (int button = 0; button < 3; ++button)
            {
                if (!ImGui::IsMouseClicked(button) || over_level >= 0 || g_opened_frame == ImGui::GetFrameCount())
                {
                    continue;
                }
                const bool on_bar_button = g_bar.index >= 0 && mouse.x >= g_bar.button_min.x
                                           && mouse.x < g_bar.button_max.x && mouse.y >= g_bar.button_min.y
                                           && mouse.y < g_bar.button_max.y;
                close_menus();
                if (!on_bar_button)
                {
                    ImGuiIO& io = ImGui::GetIO();
                    io.AddMouseButtonEvent(button, false);
                    io.AddMouseButtonEvent(button, true);
                }
                return;
            }

            // Hovering another menu bar entry while a bar menu is open switches to it.
            if (g_bar.index >= 0 && g_bar_rect && over_level < 0)
            {
                for (int i = 0; i < g_bar_count; ++i)
                {
                    ImVec2 min;
                    ImVec2 max;
                    if (i != g_bar.index && g_bar_rect(i, &min, &max) && mouse.x >= min.x && mouse.x < max.x
                        && mouse.y >= min.y && mouse.y < max.y)
                    {
                        switch_bar_menu(i, false);
                        return;
                    }
                }
            }

            const bool moved = ImGui::GetIO().MouseDelta.x != 0.f || ImGui::GetIO().MouseDelta.y != 0.f;
            if (over_level >= 0)
            {
                const auto depth = static_cast<std::size_t>(over_level);
                Level& level = g_levels[depth];
                const int index = item_at(level, mouse);
                if (moved || index != level.hot)
                {
                    if (moved)
                    {
                        g_keyboard = false;
                    }
                    const bool enabled_item = index >= 0 && level.items[static_cast<std::size_t>(index)].enabled;
                    if (moved || enabled_item)
                    {
                        level.hot = enabled_item ? index : -1;
                    }
                    // Submenus open, and other items' submenus close, after the system menu delay.
                    if (index != level.child_item && level.pending_item != index)
                    {
                        level.pending_item = index;
                        level.pending_since = now;
                    }
                }
                const ImVec2 travel(mouse.x - g_open_mouse.x, mouse.y - g_open_mouse.y);
                if ((ImGui::GetFrameCount() != g_opened_frame
                     && (ImGui::IsMouseClicked(ImGuiMouseButton_Left) || ImGui::IsMouseClicked(ImGuiMouseButton_Right)))
                    || travel.x * travel.x + travel.y * travel.y > px(4.f) * px(4.f))
                {
                    g_release_armed = true;
                }
                if (ImGui::IsMouseReleased(ImGuiMouseButton_Left) || ImGui::IsMouseReleased(ImGuiMouseButton_Right))
                {
                    if (index >= 0 && g_release_armed)
                    {
                        activate(depth, index);
                        return;
                    }
                }
                // Deeper levels keep their hot item only while the mouse is over them.
                for (std::size_t d = depth + 1; d < g_levels.size(); ++d)
                {
                    if (moved && g_levels[d].child_item < 0)
                    {
                        g_levels[d].hot = -1;
                    }
                }
            }
            else if (moved && !g_levels.empty())
            {
                // Leaving the menu clears the hot row unless it shows its submenu.
                Level& top = g_levels.back();
                top.hot = top.child_item >= 0 ? top.child_item : -1;
                top.pending_item = -1;
            }

            for (std::size_t depth = 0; depth < g_levels.size(); ++depth)
            {
                Level& level = g_levels[depth];
                if (level.pending_item == -1 && level.child_item == -1)
                {
                    continue;
                }
                if (level.pending_item != level.child_item && now - level.pending_since >= platform::menu_show_delay())
                {
                    const int pending = level.pending_item;
                    level.pending_item = level.child_item;
                    if (pending >= 0 && !level.items[static_cast<std::size_t>(pending)].submenu.empty())
                    {
                        open_child(depth, pending, false);
                    }
                    else if (pending >= 0)
                    {
                        close_children(depth);
                    }
                    break;
                }
            }
            if (ImGui::GetIO().MouseWheel != 0.f)
            {
                ImGui::GetIO().MouseWheel = 0.f;
            }
        }

        void draw_shadow(ImDrawList* draw, ImVec2 min, ImVec2 max)
        {
            const auto shadow = [](float alpha)
            {
                return IM_COL32(0, 0, 0, static_cast<int>(alpha * 255.f + 0.5f));
            };
            const int side = static_cast<int>(std::size(SHADOW_SIDE));
            const int bottom = static_cast<int>(std::size(SHADOW_BOTTOM));
            for (int d = 0; d < side; ++d)
            {
                const float a = SHADOW_SIDE[d];
                draw->AddRectFilled(ImVec2(min.x - 1.f - d, min.y), ImVec2(min.x - d, max.y), shadow(a));
                draw->AddRectFilled(ImVec2(max.x + d, min.y), ImVec2(max.x + d + 1.f, max.y), shadow(a));
            }
            for (int d = 0; d < bottom; ++d)
            {
                draw->AddRectFilled(ImVec2(min.x, max.y + d), ImVec2(max.x, max.y + d + 1.f), shadow(SHADOW_BOTTOM[d]));
            }
            for (int dx = 0; dx < side; ++dx)
            {
                for (int dy = 0; dy < bottom; ++dy)
                {
                    const ImU32 c = shadow(SHADOW_SIDE[dx] * SHADOW_BOTTOM[dy] / SHADOW_PEAK);
                    draw->AddRectFilled(ImVec2(min.x - 1.f - dx, max.y + dy), ImVec2(min.x - dx, max.y + dy + 1.f), c);
                    draw->AddRectFilled(ImVec2(max.x + dx, max.y + dy), ImVec2(max.x + dx + 1.f, max.y + dy + 1.f), c);
                }
            }
            // Under the rounded corners, where the menu does not cover its bounding box.
            const float r = px(CORNER_RADIUS);
            draw->AddRectFilled(ImVec2(min.x, max.y - r), ImVec2(min.x + r, max.y),
                                shadow(SHADOW_SIDE[0] * SHADOW_BOTTOM[0] / SHADOW_PEAK));
            draw->AddRectFilled(ImVec2(max.x - r, max.y - r), max,
                                shadow(SHADOW_SIDE[0] * SHADOW_BOTTOM[0] / SHADOW_PEAK));
        }

        void draw_label(ImDrawList* draw, ImVec2 pos, ImU32 color, const std::string& label, bool underline)
        {
            ImFont* font = fonts().system;
            const std::string text = menu_text(label);
            draw_text(draw, font, FONT_SIZE, pos, color, text.c_str());
            if (!underline || g_bar.index < 0)
            {
                return;
            }
            // Position of the mnemonic letter in the stripped text.
            std::size_t stripped = 0;
            for (std::size_t i = 0; i < label.size(); ++i)
            {
                if (label[i] == '&' && i + 1 < label.size())
                {
                    if (label[i + 1] == '&')
                    {
                        ++i;
                        ++stripped;
                        continue;
                    }
                    const float x0 = text_size(font, FONT_SIZE, text.c_str(), text.c_str() + stripped).x;
                    const float x1 = text_size(font, FONT_SIZE, text.c_str(), text.c_str() + stripped + 1).x;
                    const float y = std::round(pos.y + font_px(font, FONT_SIZE) * 0.86f);
                    draw->AddRectFilled(ImVec2(std::round(pos.x + x0), y), ImVec2(std::round(pos.x + x1), y + 1.f),
                                        color);
                    return;
                }
                ++stripped;
            }
        }

        void draw_level(ImDrawList* draw, const Level& level)
        {
            const ImVec2 min = level.pos;
            const ImVec2 max(level.pos.x + level.size.x, level.pos.y + level.size.y);
            draw_shadow(draw, min, max);
            draw->AddRectFilled(min, max, BACKGROUND, px(CORNER_RADIUS));

            ImFont* font = fonts().system;
            const float line = font_px(font, FONT_SIZE);
            float y = min.y + px(VERTICAL_PADDING);
            for (int i = 0; i < static_cast<int>(level.items.size()); ++i)
            {
                const MenuItem& item = level.items[static_cast<std::size_t>(i)];
                if (item.type == MenuItem::Type::SEPARATOR)
                {
                    const float sy = y + px(8.f);
                    draw->AddRectFilled(ImVec2(min.x, sy), ImVec2(max.x, sy + px(1.f)), SEPARATOR);
                    y += px(SEPARATOR_HEIGHT);
                    continue;
                }
                const float row = px(ROW_HEIGHT);
                if (i == level.hot && item.enabled)
                {
                    draw->AddRectFilled(ImVec2(min.x, y), ImVec2(max.x, y + row), HOT_ROW);
                }
                const ImU32 color = item.enabled ? TEXT : DISABLED;
                const float text_y = std::round(y + (row - line) / 2.f);
                draw_label(draw, ImVec2(min.x + level.label_start, text_y), color, item.label, g_keyboard);
                if (!item.accelerator.empty())
                {
                    const float w = text_size(font, FONT_SIZE, item.accelerator.c_str()).x;
                    draw_text(draw, font, FONT_SIZE, ImVec2(std::round(min.x + level.accelerator_right - w), text_y),
                              item.enabled ? ACCELERATOR : DISABLED, item.accelerator.c_str());
                }
                if (item.type == MenuItem::Type::CHECKBOX && item.checked)
                {
                    // Chromium's check icon: a 1.6px stroke at about 75% of the text colour.
                    const ImVec2 points[] = {ImVec2(min.x + px(22.6f), y + px(15.2f)),
                                             ImVec2(min.x + px(25.9f), y + px(18.4f)),
                                             ImVec2(min.x + px(33.4f), y + px(10.4f))};
                    draw->AddPolyline(points, 3, (color & 0x00FFFFFFu) | (static_cast<ImU32>(0xC0) << IM_COL32_A_SHIFT),
                                      ImDrawFlags_None, px(1.6f));
                }
                if (!item.submenu.empty())
                {
                    const float ax = max.x - px(30.f);
                    const float ay = y + row / 2.f;
                    const ImVec2 points[] = {ImVec2(ax + px(0.25f), ay - px(3.25f)), ImVec2(ax + px(3.5f), ay),
                                             ImVec2(ax + px(0.25f), ay + px(3.25f))};
                    draw->AddPolyline(points, 3, color, ImDrawFlags_None, px(1.f));
                }
                y += row;
            }
        }
    } // namespace

    MenuItem menu_item(std::string label, std::function<void()> click, bool enabled)
    {
        MenuItem item;
        item.label = std::move(label);
        item.click = std::move(click);
        item.enabled = enabled;
        return item;
    }

    MenuItem menu_checkbox(std::string label, bool checked, std::function<void()> click, bool enabled)
    {
        MenuItem item = menu_item(std::move(label), std::move(click), enabled);
        item.type = MenuItem::Type::CHECKBOX;
        item.checked = checked;
        return item;
    }

    MenuItem menu_submenu(std::string label, MenuItems submenu)
    {
        MenuItem item;
        item.label = std::move(label);
        item.submenu = std::move(submenu);
        return item;
    }

    MenuItems separated_menu(std::vector<MenuItems> groups)
    {
        MenuItems result;
        for (MenuItems& group : groups)
        {
            if (group.empty())
            {
                continue;
            }
            if (!result.empty())
            {
                result.push_back(MenuItem::separator());
            }
            for (MenuItem& item : group)
            {
                result.push_back(std::move(item));
            }
        }
        return result;
    }

    std::string strip_mnemonic(const std::string& label)
    {
        std::string text;
        text.reserve(label.size());
        for (std::size_t i = 0; i < label.size(); ++i)
        {
            if (label[i] == '&')
            {
                if (i + 1 < label.size() && label[i + 1] == '&')
                {
                    text += '&';
                    ++i;
                }
                continue;
            }
            text += label[i];
        }
        return text;
    }

    void popup_menu(MenuItems items, std::optional<ImVec2> pos)
    {
        close_menus();
        if (items.empty())
        {
            return;
        }
        Level level;
        level.items = std::move(items);
        layout(level);
        const ImVec2 anchor = pos.value_or(ImGui::GetMousePos());
        place(level, ImVec2(std::round(anchor.x), std::round(anchor.y)), anchor.x, anchor.y);
        g_levels.push_back(std::move(level));
        g_keyboard = false;
        g_take_focus = true;
        g_opened_frame = ImGui::GetFrameCount();
        g_release_armed = false;
        g_open_mouse = ImGui::GetMousePos();
    }

    void popup_bar_menu(MenuItems items, int bar_index, ImVec2 button_min, ImVec2 button_max, bool keyboard)
    {
        close_menus();
        g_bar = {bar_index, button_min, button_max}; // before the layout: it measures the labels without their '&'
        Level level;
        level.items = std::move(items);
        layout(level);
        // The dropdown overlaps the bar's last pixel row, like Chromium's.
        place(level, ImVec2(button_min.x, button_max.y - px(1.f)), button_max.x, button_min.y);
        if (keyboard)
        {
            move_hot(level, 1);
        }
        g_levels.push_back(std::move(level));
        g_keyboard = keyboard;
        g_take_focus = true;
        g_opened_frame = ImGui::GetFrameCount();
        g_release_armed = false;
        g_open_mouse = ImGui::GetMousePos();
    }

    void close_menus()
    {
        g_levels.clear();
        g_bar = {};
    }

    bool menu_open() noexcept
    {
        return !g_levels.empty();
    }

    int open_bar_menu() noexcept
    {
        return g_levels.empty() ? -1 : g_bar.index;
    }

    bool menu_keyboard_mode() noexcept
    {
        return g_keyboard;
    }

    void set_menu_bar(int count, std::function<MenuItems(int)> build, std::function<bool(int, ImVec2*, ImVec2*)> rect)
    {
        g_bar_count = count;
        g_bar_builder = std::move(build);
        g_bar_rect = std::move(rect);
    }

    void draw_menus()
    {
        if (!g_levels.empty())
        {
            handle_keyboard();
        }
        if (!g_levels.empty())
        {
            handle_mouse();
        }
        if (!g_levels.empty())
        {
            // A window over the whole viewport: while a menu is open, the rest of the UI gets no hover or input.
            const ImGuiViewport* viewport = ImGui::GetMainViewport();
            ImGui::SetNextWindowPos(viewport->Pos);
            ImGui::SetNextWindowSize(viewport->Size);
            if (g_take_focus)
            {
                ImGui::SetNextWindowFocus();
                g_take_focus = false;
            }
            ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(0.f, 0.f));
            ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.f);
            ImGui::Begin("##native_menus", nullptr,
                         ImGuiWindowFlags_NoDecoration | ImGuiWindowFlags_NoBackground | ImGuiWindowFlags_NoMove
                                 | ImGuiWindowFlags_NoSavedSettings | ImGuiWindowFlags_NoNav
                                 | ImGuiWindowFlags_NoScrollWithMouse);
            ImGui::BringWindowToDisplayFront(ImGui::GetCurrentWindow());
            ImDrawList* draw = ImGui::GetWindowDrawList();
            draw->PushClipRectFullScreen();
            for (const Level& level : g_levels)
            {
                draw_level(draw, level);
            }
            draw->PopClipRect();
            ImGui::End();
            ImGui::PopStyleVar(2);
        }
        if (g_pending_click)
        {
            std::function<void()> click = std::move(g_pending_click);
            g_pending_click = nullptr;
            click();
        }
    }
} // namespace imgit::ui
