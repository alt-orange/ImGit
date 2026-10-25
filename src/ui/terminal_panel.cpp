#include "app/application.hpp"
#include "app/theme.hpp"
#include "platform/system.hpp"
#include "ui/panels.hpp"
#include "ui/vector_icons.hpp"
#include "ui/widgets.hpp"
#include <algorithm>
#include <cmath>
#include <imgui.h>
#include <string>
#include <utility>

// The terminal panel: a 30px toolbar with the shell tabs, +, and Minimize / Kill All /
// Hide, over xterm.js (Consolas 12px, line height 1.2: 6x16 cells, 1.2rem padding) running PowerShell on a
// pseudo console. The content dims to 60% while the terminal does not have focus.
namespace imgit::ui
{
    namespace
    {

        constexpr float TOOLBAR_HEIGHT = 30.f;
        constexpr float PADDING = 12.f; // .terminal-panel { padding: 1.2rem 0 1.2rem 1.2rem }
        constexpr float FONT_SIZE = 12.f;
        constexpr float CELL_WIDTH = 6.f; // floor(Consolas 12px advance)
        constexpr float CELL_HEIGHT = 16.f; // floor(14 * 1.2)
        constexpr float BASELINE = 12.f; // from the cell top
        constexpr float RULER_WIDTH = 10.f; // overviewRuler / scrollbar

        // The terminal colour theme.
        constexpr unsigned ANSI[16] = {0x1A1A1A, 0xF0716A, 0x5CCB7A, 0xEBC45A, 0x5A9CF0, 0xC77DE8, 0x4FC3D2, 0xDCDCDC,
                                       0x6C6C6C, 0xF79A94, 0x86DE9E, 0xF4D78A, 0x8BBBF7, 0xDDA6F2, 0x7FD7E2, 0xF5F5F5};
        constexpr unsigned FOREGROUND = 0xDCDCDC;

        ImU32 rgb(unsigned value, float alpha)
        {
            return IM_COL32((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF,
                            static_cast<int>(std::lround(alpha * 255.f)));
        }

        unsigned hex(ImVec4 color)
        {
            const auto channel = [](float v)
            {
                return static_cast<unsigned>(std::lround(std::clamp(v, 0.f, 1.f) * 255.f));
            };
            return channel(color.x) << 16 | channel(color.y) << 8 | channel(color.z);
        }

        // The theme's terminal background and its accent as the cursor.
        unsigned background_color()
        {
            return hex(theme::palette().terminal_bg);
        }

        unsigned cursor_color()
        {
            return hex(theme::palette().accent);
        }

        unsigned resolve(std::uint32_t color, unsigned fallback)
        {
            const std::uint32_t kind = color & app::vt_color::KIND_MASK;
            if (kind == app::vt_color::RGB)
            {
                return color & 0xFFFFFF;
            }
            if (kind != app::vt_color::PALETTE)
            {
                return fallback;
            }
            const unsigned index = color & 0xFF;
            if (index < 16)
            {
                return ANSI[index];
            }
            if (index < 232)
            {
                static constexpr unsigned LEVELS[6] = {0, 95, 135, 175, 215, 255};
                const unsigned i = index - 16;
                return (LEVELS[i / 36] << 16) | (LEVELS[(i / 6) % 6] << 8) | LEVELS[i % 6];
            }
            const unsigned gray = 8 + (index - 232) * 10;
            return (gray << 16) | (gray << 8) | gray;
        }

        app::TerminalSession* current(app::Application& app)
        {
            return app.active_terminal(app.active());
        }

        // The selection as (line id, column) pairs, start before end.
        struct Range
        {
            std::uint64_t start_line;
            int start_column;
            std::uint64_t end_line;
            int end_column; // exclusive
        };

        Range selection_range(const app::TerminalSession& session)
        {
            const bool forward =
                    session.anchor_line < session.head_line
                    || (session.anchor_line == session.head_line && session.anchor_column <= session.head_column);
            return forward ? Range{session.anchor_line, session.anchor_column, session.head_line, session.head_column}
                           : Range{session.head_line, session.head_column, session.anchor_line, session.anchor_column};
        }

        void append_utf8(std::string& out, char32_t ch)
        {
            if (ch < 0x80)
            {
                out += static_cast<char>(ch);
            }
            else if (ch < 0x800)
            {
                out += static_cast<char>(0xC0 | (ch >> 6));
                out += static_cast<char>(0x80 | (ch & 0x3F));
            }
            else if (ch < 0x10000)
            {
                out += static_cast<char>(0xE0 | (ch >> 12));
                out += static_cast<char>(0x80 | ((ch >> 6) & 0x3F));
                out += static_cast<char>(0x80 | (ch & 0x3F));
            }
            else
            {
                out += static_cast<char>(0xF0 | (ch >> 18));
                out += static_cast<char>(0x80 | ((ch >> 12) & 0x3F));
                out += static_cast<char>(0x80 | ((ch >> 6) & 0x3F));
                out += static_cast<char>(0x80 | (ch & 0x3F));
            }
        }

        std::string selection_text(const app::TerminalSession& session)
        {
            if (!session.has_selection)
            {
                return {};
            }
            const app::VtScreen& screen = session.screen;
            const Range range = selection_range(session);
            const std::uint64_t first = screen.first_line_id();
            std::string text;
            for (std::uint64_t id = std::max(range.start_line, first); id <= range.end_line; ++id)
            {
                const std::size_t index = static_cast<std::size_t>(id - first);
                if (index >= screen.line_count())
                {
                    break;
                }
                const app::VtLine& line = screen.line(index);
                const int from = id == range.start_line ? range.start_column : 0;
                const int to = id == range.end_line ? range.end_column : static_cast<int>(line.cells.size());
                std::string row;
                for (int x = std::max(0, from); x < std::min(to, static_cast<int>(line.cells.size())); ++x)
                {
                    if (const char32_t ch = line.cells[static_cast<std::size_t>(x)].ch; ch != 0)
                    {
                        append_utf8(row, ch);
                    }
                }
                // Trailing blanks go, and soft-wrapped lines join without a break.
                if (id != range.end_line || to >= static_cast<int>(line.cells.size()))
                {
                    while (!row.empty() && row.back() == ' ')
                    {
                        row.pop_back();
                    }
                }
                text += row;
                if (id != range.end_line && !line.wrapped)
                {
                    text += "\r\n";
                }
            }
            return text;
        }

        void paste(app::TerminalSession& session, std::string text)
        {
            // xterm normalises line breaks to CR and brackets the paste when the program asked for it.
            std::string normalized;
            for (std::size_t i = 0; i < text.size(); ++i)
            {
                if (text[i] == '\r' && i + 1 < text.size() && text[i + 1] == '\n')
                {
                    continue;
                }
                normalized += text[i] == '\n' ? '\r' : text[i];
            }
            if (session.screen.bracketed_paste())
            {
                normalized = "\x1b[200~" + normalized + "\x1b[201~";
            }
            session.console.write(normalized);
            session.scroll = 0;
        }

        // The VT bytes for a key, xterm's way (application cursor mode, modifier parameters).
        std::string key_sequence(ImGuiKey key, bool ctrl, bool shift, bool alt, bool application_cursor)
        {
            const int modifiers = 1 + (shift ? 1 : 0) + (alt ? 2 : 0) + (ctrl ? 4 : 0);
            const auto cursor = [&](char final)
            {
                if (modifiers > 1)
                {
                    return "\x1b[1;" + std::to_string(modifiers) + final;
                }
                return std::string(application_cursor ? "\x1bO" : "\x1b[") + final;
            };
            const auto tilde = [&](int code)
            {
                return "\x1b[" + std::to_string(code) + (modifiers > 1 ? ";" + std::to_string(modifiers) : "") + "~";
            };
            const auto function = [&](char final)
            {
                return modifiers > 1 ? "\x1b[1;" + std::to_string(modifiers) + final : std::string("\x1bO") + final;
            };
            switch (key)
            {
            case ImGuiKey_Enter:
            case ImGuiKey_KeypadEnter:
                return alt ? "\x1b\r" : "\r";
            case ImGuiKey_Backspace:
                return std::string(alt ? "\x1b" : "") + (ctrl ? "\x08" : "\x7f");
            case ImGuiKey_Tab:
                return shift ? "\x1b[Z" : "\t";
            case ImGuiKey_Escape:
                return "\x1b";
            case ImGuiKey_UpArrow:
                return cursor('A');
            case ImGuiKey_DownArrow:
                return cursor('B');
            case ImGuiKey_RightArrow:
                return cursor('C');
            case ImGuiKey_LeftArrow:
                return cursor('D');
            case ImGuiKey_Home:
                return cursor('H');
            case ImGuiKey_End:
                return cursor('F');
            case ImGuiKey_Insert:
                return tilde(2);
            case ImGuiKey_Delete:
                return tilde(3);
            case ImGuiKey_PageUp:
                return tilde(5);
            case ImGuiKey_PageDown:
                return tilde(6);
            case ImGuiKey_F1:
                return function('P');
            case ImGuiKey_F2:
                return function('Q');
            case ImGuiKey_F3:
                return function('R');
            case ImGuiKey_F4:
                return function('S');
            case ImGuiKey_F5:
                return tilde(15);
            case ImGuiKey_F6:
                return tilde(17);
            case ImGuiKey_F7:
                return tilde(18);
            case ImGuiKey_F8:
                return tilde(19);
            case ImGuiKey_F9:
                return tilde(20);
            case ImGuiKey_F10:
                return tilde(21);
            case ImGuiKey_F11:
                return tilde(23);
            case ImGuiKey_F12:
                return tilde(24);
            default:
                break;
            }
            if (ctrl && !alt)
            {
                if (key >= ImGuiKey_A && key <= ImGuiKey_Z)
                {
                    return std::string(1, static_cast<char>(1 + (key - ImGuiKey_A)));
                }
                switch (key)
                {
                case ImGuiKey_Space:
                case ImGuiKey_2:
                    return std::string(1, '\0');
                case ImGuiKey_LeftBracket:
                    return "\x1b";
                case ImGuiKey_Backslash:
                    return "\x1c";
                case ImGuiKey_RightBracket:
                    return "\x1d";
                default:
                    break;
                }
            }
            return {};
        }

        void handle_keyboard(app::Application& app, app::TerminalSession& session)
        {
            ImGuiIO& io = ImGui::GetIO();
            const bool ctrl = io.KeyCtrl;
            const bool shift = io.KeyShift;
            const bool alt = io.KeyAlt;
            std::string out;
            for (int k = ImGuiKey_NamedKey_BEGIN; k < ImGuiKey_NamedKey_END; ++k)
            {
                const auto key = static_cast<ImGuiKey>(k);
                if (key >= ImGuiKey_ReservedForModCtrl || !ImGui::IsKeyPressed(key, true))
                {
                    continue;
                }
                // The app's own chords (command palette, zoom, tabs, terminal tabs) are not typed.
                const bool app_chord =
                        ctrl
                        && (key == ImGuiKey_P || key == ImGuiKey_Tab || key == ImGuiKey_GraveAccent
                            || key == ImGuiKey_PageUp || key == ImGuiKey_PageDown || key == ImGuiKey_Equal
                            || key == ImGuiKey_Minus || (!shift && key == ImGuiKey_0)
                            || (shift
                                && (key == ImGuiKey_N || key == ImGuiKey_W
                                    || (key >= ImGuiKey_1 && key <= ImGuiKey_9))));
                if (app_chord)
                {
                    continue;
                }
                // Ctrl+C copies a selection (else interrupts); Ctrl+V pastes.
                if (ctrl && !shift && key == ImGuiKey_C && session.has_selection)
                {
                    app.copy_to_clipboard(selection_text(session));
                    session.has_selection = false;
                    continue;
                }
                if (ctrl && key == ImGuiKey_V)
                {
                    if (const char* text = ImGui::GetClipboardText(); text != nullptr)
                    {
                        paste(session, text);
                    }
                    continue;
                }
                out += key_sequence(key, ctrl, shift, alt, session.screen.application_cursor_keys());
            }
            if (!ctrl)
            {
                for (const ImWchar ch : io.InputQueueCharacters)
                {
                    if (ch >= 0x20 && ch != 0x7F)
                    {
                        if (alt)
                        {
                            out += '\x1b';
                        }
                        append_utf8(out, ch);
                    }
                }
            }
            io.InputQueueCharacters.resize(0);
            if (!out.empty())
            {
                session.console.write(out);
                session.scroll = 0;
            }
        }
    } // namespace

    bool terminal_has_selection(app::Application& app)
    {
        const app::TerminalSession* session = current(app);
        return session != nullptr && session->has_selection;
    }

    void terminal_copy(app::Application& app)
    {
        if (const app::TerminalSession* session = current(app); session != nullptr && session->has_selection)
        {
            app.copy_to_clipboard(selection_text(*session));
        }
    }

    void terminal_paste(app::Application& app)
    {
        app::TerminalSession* session = current(app);
        if (const char* text = ImGui::GetClipboardText(); session != nullptr && text != nullptr)
        {
            paste(*session, text);
        }
        app.terminal.focus_requested = true;
    }

    void terminal_select_all(app::Application& app)
    {
        if (app::TerminalSession* session = current(app); session != nullptr)
        {
            const app::VtScreen& screen = session->screen;
            session->has_selection = true;
            session->anchor_line = screen.first_line_id();
            session->anchor_column = 0;
            session->head_line = screen.first_line_id() + screen.line_count() - 1;
            session->head_column = screen.columns();
        }
    }

    void terminal_clear(app::Application& app)
    {
        if (app::TerminalSession* session = current(app); session != nullptr)
        {
            session->screen.clear();
            session->scroll = 0;
            session->has_selection = false;
        }
    }

    void terminal_write(app::Application& app, std::string_view bytes)
    {
        if (app::TerminalSession* session = current(app); session != nullptr)
        {
            session->console.write(bytes);
            session->scroll = 0;
        }
        app.terminal.focus_requested = true;
    }

    void draw_terminal_panel(app::Application& app, app::RepoState* state, ImVec2 size)
    {
        const theme::Palette& p = theme::palette();
        const ImVec2 origin = ImGui::GetCursorScreenPos();
        app::TerminalPanel& panel = app.terminal;
        app::TerminalPanel::Tabs& tabs = app.terminal_tabs(state);
        // Opening the panel starts a shell; while it closes (the last shell exited) it stays empty.
        if (tabs.sessions.empty() && app.show_terminal_panel)
        {
            app.new_terminal_tab(state);
        }
        ImGui::BeginChild("##terminal_panel", size, ImGuiChildFlags_None,
                          ImGuiWindowFlags_NoScrollbar | ImGuiWindowFlags_NoScrollWithMouse);
        ImDrawList* draw = ImGui::GetWindowDrawList();
        const ImU32 border = IM_COL32(255, 255, 255, 20); // --section-border
        const ImU32 background = rgb(background_color(), 1.f);
        const float right = origin.x + size.x;
        draw->AddRectFilled(origin, ImVec2(right, origin.y + size.y), background);

        // Toolbar: 1px borders top and bottom, 10px padding.
        const float bar_h = px(TOOLBAR_HEIGHT);
        draw->AddRectFilled(origin, ImVec2(right, origin.y + 1.f), border);
        draw->AddRectFilled(ImVec2(origin.x, origin.y + bar_h - 1.f), ImVec2(right, origin.y + bar_h), border);
        const auto text_at = [&](float x, float box_top, ImU32 color, const char* text)
        {
            const float ascent = 12.f * 2189.f / 2048.f * dpi_scale();
            draw_text(draw, fonts().regular, 12.f,
                      ImVec2(x, std::round(box_top + std::round(ascent)) - std::ceil(ascent)), color, text);
        };
        if (panel.minimized)
        {
            draw_fa_icon(draw, "far:terminal", ImVec2(origin.x + px(17.5f), origin.y + px(14.92f)), px(12.f),
                         rgb(FOREGROUND, 1.f));
            text_at(origin.x + px(30.f), origin.y + px(6.42f), rgb(FOREGROUND, 1.f), "Terminal");
        }
        else
        {
            // Tabs: 21.14px, 4px inline padding, 2px apart; active white 30%, hovered 10%.
            float x = origin.x + px(10.f);
            const float tab_y = origin.y + px(4.42f);
            const float tab_h = px(21.14f);
            int close_index = -1;
            for (std::size_t i = 0; i < tabs.sessions.size(); ++i)
            {
                ImGui::PushID(static_cast<int>(i));
                static const std::string shell_label = platform::default_shell().label;
                const char* label = shell_label.c_str();
                const float label_w = text_size(fonts().regular, 12.f, label).x;
                const float w = std::max(px(48.f), px(4.f + 12.f + 2.f) + label_w + px(4.f + 12.f + 4.f));
                const ImVec2 tmin(x, tab_y);
                const ImVec2 tmax(x + w, tab_y + tab_h);
                const bool active = static_cast<int>(i) == tabs.active;
                ImGui::SetCursorScreenPos(tmin);
                const bool clicked = ImGui::InvisibleButton("##term_tab", ImVec2(w - px(16.f), tab_h));
                const bool hovered = ImGui::IsItemHovered();
                ImGui::SetCursorScreenPos(ImVec2(tmax.x - px(16.f), tmin.y));
                const bool closed = ImGui::InvisibleButton("##term_tab_close", ImVec2(px(16.f), tab_h));
                const bool close_hovered = ImGui::IsItemHovered();
                if (active || hovered || close_hovered)
                {
                    draw->AddRectFilled(tmin, tmax, IM_COL32(255, 255, 255, active ? 77 : 26), px(2.f));
                }
                const ImU32 color = active ? IM_COL32(255, 255, 255, 255) : rgb(FOREGROUND, 1.f);
                draw_fa_icon(draw, "far:terminal", ImVec2(x + px(10.f), tab_y + px(10.56f)), px(9.6f), color);
                text_at(x + px(18.f), tab_y + px(2.f), color, label);
                const float cx = x + px(18.f) + label_w + px(4.f);
                draw_svg_path(draw, "close", svg_path("close"), 24.f, 24.f, ImVec2(cx, tab_y + px(4.56f)),
                              ImVec2(cx + px(12.f), tab_y + px(16.56f)), rgb(FOREGROUND, 1.f));
                if (clicked)
                {
                    tabs.active = static_cast<int>(i);
                    panel.focus_requested = true;
                }
                if (closed)
                {
                    close_index = static_cast<int>(i);
                }
                ImGui::PopID();
                x += w + px(2.f);
            }
            // New tab: 16px button, 2px padding, 12px icon.
            const ImVec2 plus_min(x, origin.y + px(6.98f));
            ImGui::SetCursorScreenPos(plus_min);
            if (ImGui::InvisibleButton("##term_new_tab", ImVec2(px(16.f), px(16.f))))
            {
                app.new_terminal_tab(state);
            }
            draw_svg_path(draw, "plus", svg_path("plus"), 24.f, 24.f,
                          ImVec2(plus_min.x + px(2.f), plus_min.y + px(2.f)),
                          ImVec2(plus_min.x + px(14.f), plus_min.y + px(14.f)), rgb(FOREGROUND, 1.f));
            if (close_index >= 0)
            {
                app.close_terminal_tab(state, close_index);
            }
        }
        // Right: Minimize (Expand), Kill All Terminals, Hide; 5px padding, 12px icons, 60% white.
        {
            const float icon_y = origin.y + px(14.92f);
            const auto action = [&](const char* id, const char* icon, float x, float icon_w, const char* tip)
            {
                const float w = px(5.f + 5.f + 5.f) + icon_w;
                ImGui::SetCursorScreenPos(ImVec2(x, origin.y + px(6.42f)));
                const bool pressed = ImGui::InvisibleButton(id, ImVec2(w, px(17.14f)));
                const bool hovered = ImGui::IsItemHovered();
                draw_fa_icon(draw, icon, ImVec2(x + px(10.f) + icon_w / 2.f, icon_y), px(12.f),
                             theme::u32(hovered ? p.text_selected : p.text_secondary));
                if (hovered && tip != nullptr)
                {
                    tooltip_on_rect(ImGui::GetItemRectMin(), ImGui::GetItemRectMax(), tip, true);
                }
                return pressed;
            };
            const float hide_x = right - px(10.f) - px(22.5f);
            const float kill_x = hide_x - px(25.5f);
            const float minimize_x = kill_x - px(25.5f);
            if (action("##term_minimize", panel.minimized ? "far:chevron-up" : "far:chevron-down", minimize_x,
                       px(10.5f), panel.minimized ? "Expand Terminal" : "Minimize Terminal"))
            {
                panel.minimized = !panel.minimized;
                panel.focus_requested = !panel.minimized;
            }
            if (action("##term_kill", "far:trash", kill_x, px(10.5f), "Kill All Terminals"))
            {
                app.kill_terminals(state);
            }
            if (action("##term_hide", "far:times", hide_x, px(7.5f), nullptr))
            {
                app.show_terminal_panel = false;
            }
        }
        app::TerminalSession* session = app.active_terminal(state);
        if (panel.minimized || session == nullptr)
        {
            ImGui::EndChild();
            return;
        }

        // The terminal: a 1px top / left border, 12px padding, then the cell grid and a 10px ruler on the right.
        const float body_top = origin.y + bar_h;
        draw->AddRectFilled(ImVec2(origin.x, body_top), ImVec2(right, body_top + 1.f), border);
        draw->AddRectFilled(ImVec2(origin.x, body_top), ImVec2(origin.x + 1.f, origin.y + size.y), border);
        const ImVec2 grid(origin.x + 1.f + px(PADDING), body_top + 1.f + px(PADDING));
        const float cell_w = std::floor(px(CELL_WIDTH));
        const float cell_h = std::floor(px(CELL_HEIGHT));
        const float grid_w = right - grid.x - px(RULER_WIDTH);
        const float grid_h = origin.y + size.y - px(PADDING) - grid.y;
        const int columns = std::max(2, static_cast<int>(grid_w / cell_w));
        const int rows = std::max(1, static_cast<int>(grid_h / cell_h));
        app::VtScreen& screen = session->screen;
        if (!session->started && !session->failed)
        {
            screen.resize(columns, rows);
            session->started = session->console.start(platform::default_shell().args, session->workdir, columns, rows);
            session->failed = !session->started;
            session->console.write(std::exchange(session->pending_input, {}));
        }
        else if (session->started && (columns != screen.columns() || rows != screen.rows()))
        {
            screen.resize(columns, rows);
            session->console.resize(columns, rows);
            session->has_selection = false;
        }
        if (session->failed)
        {
            text_at(grid.x, grid.y, rgb(ANSI[1], 1.f), "Failed to start shell process");
            ImGui::EndChild();
            return;
        }

        // Focus: a click in the panel takes it, a click anywhere else (or another text field) gives it up.
        const ImVec2 area_min(origin.x, body_top);
        const ImVec2 area_max(right, origin.y + size.y);
        const bool mouse_inside = ImGui::IsMouseHoveringRect(area_min, area_max) && ImGui::IsWindowHovered();
        const bool was_focused = panel.focused;
        if (panel.focus_requested)
        {
            panel.focused = true;
            panel.focus_requested = false;
        }
        else if (ImGui::IsMouseClicked(ImGuiMouseButton_Left) || ImGui::IsMouseClicked(ImGuiMouseButton_Right))
        {
            panel.focused = ImGui::IsWindowHovered(ImGuiHoveredFlags_ChildWindows);
        }
        else if (ImGui::GetIO().WantTextInput)
        {
            panel.focused = false;
        }
        if (panel.focused != was_focused && screen.focus_reporting())
        {
            session->console.write(panel.focused ? "\x1b[I" : "\x1b[O");
        }

        // Scrolling: the wheel moves through the scrollback (arrow keys on the alternate screen); output that
        // arrives while scrolled up does not move the view.
        const int scrollback = static_cast<int>(screen.line_count()) - screen.rows();
        if (session->scroll > 0)
        {
            session->scroll += static_cast<int>(screen.scrolled_lines() - session->scrolled_seen);
        }
        session->scrolled_seen = screen.scrolled_lines();
        if (mouse_inside && ImGui::GetIO().MouseWheel != 0.f)
        {
            const int lines = static_cast<int>(std::lround(ImGui::GetIO().MouseWheel * 100.f / CELL_HEIGHT));
            if (screen.alternate_screen())
            {
                std::string keys;
                for (int i = 0; i < std::abs(lines); ++i)
                {
                    keys += lines > 0 ? (screen.application_cursor_keys() ? "\x1bOA" : "\x1b[A")
                                      : (screen.application_cursor_keys() ? "\x1bOB" : "\x1b[B");
                }
                session->console.write(keys);
            }
            else
            {
                session->scroll += lines;
            }
        }
        session->scroll = std::clamp(session->scroll, 0, std::max(0, scrollback));
        const std::size_t top_index = static_cast<std::size_t>(scrollback - session->scroll);

        // Selection: drag to select, double-click selects a word; a plain click clears it.
        ImGui::SetCursorScreenPos(grid);
        ImGui::InvisibleButton("##terminal_grid", ImVec2(grid_w + px(RULER_WIDTH), cell_h * static_cast<float>(rows)));
        const auto cell_at = [&](ImVec2 mouse)
        {
            const int x =
                    std::clamp(static_cast<int>(std::floor((mouse.x - grid.x) / cell_w + 0.5f)), 0, screen.columns());
            const int y = std::clamp(static_cast<int>(std::floor((mouse.y - grid.y) / cell_h)), 0, rows - 1);
            return std::pair<std::uint64_t, int>(screen.first_line_id() + top_index + static_cast<std::size_t>(y), x);
        };
        if (ImGui::IsItemActivated())
        {
            const auto [line, column] = cell_at(ImGui::GetMousePos());
            session->anchor_line = session->head_line = line;
            session->anchor_column = session->head_column = column;
            session->selecting = true;
            session->has_selection = false;
            if (ImGui::IsMouseDoubleClicked(ImGuiMouseButton_Left))
            {
                const std::size_t index = static_cast<std::size_t>(line - screen.first_line_id());
                const std::vector<app::VtCell>& cells = screen.line(index).cells;
                const auto word = [&](int x)
                {
                    const char32_t ch = cells[static_cast<std::size_t>(x)].ch;
                    return ch != U' ' && ch != 0 && ch != U'(' && ch != U')' && ch != U'"' && ch != U'\'';
                };
                int from = std::min(column, static_cast<int>(cells.size()) - 1);
                int to = from;
                if (word(from))
                {
                    while (from > 0 && word(from - 1))
                    {
                        --from;
                    }
                    while (to + 1 < static_cast<int>(cells.size()) && word(to + 1))
                    {
                        ++to;
                    }
                    session->anchor_column = from;
                    session->head_column = to + 1;
                    session->has_selection = true;
                    session->selecting = false;
                }
            }
        }
        if (session->selecting && ImGui::IsItemActive())
        {
            const auto [line, column] = cell_at(ImGui::GetMousePos());
            session->head_line = line;
            session->head_column = column;
            session->has_selection = line != session->anchor_line || column != session->anchor_column;
        }
        if (!ImGui::IsItemActive())
        {
            session->selecting = false;
        }
        if (ImGui::IsItemHovered())
        {
            ImGui::SetMouseCursor(ImGuiMouseCursor_TextInput);
        }
        if (ImGui::IsWindowHovered() && ImGui::IsMouseReleased(ImGuiMouseButton_Right) && mouse_inside)
        {
            popup_menu(terminal_menu(app));
        }

        if (panel.focused)
        {
            handle_keyboard(app, *session);
        }

        // The cells: backgrounds, the selection, glyphs, underlines, then the cursor; 60% while unfocused.
        const float opacity = panel.focused ? 1.f : 0.6f;
        draw->PushClipRect(grid,
                           ImVec2(grid.x + cell_w * static_cast<float>(screen.columns()),
                                  grid.y + cell_h * static_cast<float>(rows)),
                           true);
        ImFont* mono = fonts().mono;
        const float glyph_size = font_px(mono, FONT_SIZE);
        const float ascent = mono->GetFontBaked(glyph_size)->Ascent;
        const Range range = selection_range(*session);
        for (int y = 0; y < rows && top_index + static_cast<std::size_t>(y) < screen.line_count(); ++y)
        {
            const std::size_t index = top_index + static_cast<std::size_t>(y);
            const app::VtLine& line = screen.line(index);
            const std::uint64_t id = screen.first_line_id() + index;
            const float cy = grid.y + cell_h * static_cast<float>(y);
            const float baseline = std::round(cy + px(BASELINE));
            for (int x = 0; x < static_cast<int>(line.cells.size()); ++x)
            {
                const app::VtCell& cell = line.cells[static_cast<std::size_t>(x)];
                std::uint32_t fg_color = cell.fg;
                // drawBoldTextInBrightColors
                if ((cell.attributes & app::BOLD) != 0
                    && (fg_color & app::vt_color::KIND_MASK) == app::vt_color::PALETTE && (fg_color & 0xFF) < 8)
                {
                    fg_color += 8;
                }
                unsigned fg = resolve(fg_color, FOREGROUND);
                unsigned bg = resolve(cell.bg, background_color());
                const bool default_bg = cell.bg == app::vt_color::DEFAULT;
                bool paint_bg = !default_bg;
                if ((cell.attributes & app::INVERSE) != 0)
                {
                    std::swap(fg, bg);
                    paint_bg = true;
                }
                const float cx = grid.x + cell_w * static_cast<float>(x);
                const bool wide = x + 1 < static_cast<int>(line.cells.size())
                                  && line.cells[static_cast<std::size_t>(x) + 1].ch == 0 && cell.ch != 0;
                const float w = wide ? cell_w * 2.f : cell_w;
                if (paint_bg && cell.ch != 0)
                {
                    draw->AddRectFilled(ImVec2(cx, cy), ImVec2(cx + w, cy + cell_h), rgb(bg, opacity));
                }
                if (session->has_selection)
                {
                    const bool after_start =
                            id > range.start_line || (id == range.start_line && x >= range.start_column);
                    const bool before_end = id < range.end_line || (id == range.end_line && x < range.end_column);
                    if (after_start && before_end)
                    {
                        draw->AddRectFilled(ImVec2(cx, cy), ImVec2(cx + cell_w, cy + cell_h),
                                            rgb(cursor_color(), 64.f / 255.f * opacity));
                    }
                }
                if (cell.ch == 0 || cell.ch == U' ' || (cell.attributes & app::INVISIBLE) != 0)
                {
                    continue;
                }
                const float alpha = opacity * ((cell.attributes & app::DIM) != 0 ? 0.5f : 1.f);
                const ImWchar glyph = cell.ch <= 0xFFFF ? static_cast<ImWchar>(cell.ch) : static_cast<ImWchar>(0xFFFD);
                mono->RenderChar(draw, glyph_size, ImVec2(cx, baseline - ascent), rgb(fg, alpha), glyph);
                if ((cell.attributes & app::UNDERLINE) != 0)
                {
                    draw->AddRectFilled(ImVec2(cx, baseline + 1.f), ImVec2(cx + w, baseline + 2.f), rgb(fg, alpha));
                }
                if ((cell.attributes & app::STRIKETHROUGH) != 0)
                {
                    const float sy = std::round(cy + cell_h / 2.f);
                    draw->AddRectFilled(ImVec2(cx, sy), ImVec2(cx + w, sy + 1.f), rgb(fg, alpha));
                }
            }
        }
        // xterm's bar cursor while focused, an outlined cell otherwise.
        const int cursor_row = screen.cursor_y() + session->scroll;
        if (screen.cursor_visible() && cursor_row < rows)
        {
            const ImVec2 cmin(grid.x + cell_w * static_cast<float>(screen.cursor_x()),
                              grid.y + cell_h * static_cast<float>(cursor_row));
            if (panel.focused)
            {
                draw->AddRectFilled(cmin, ImVec2(cmin.x + 1.f, cmin.y + cell_h), rgb(cursor_color(), 1.f));
            }
            else
            {
                draw->AddRect(cmin, ImVec2(cmin.x + cell_w, cmin.y + cell_h), rgb(cursor_color(), opacity), 0.f, 0, 1.f);
            }
        }
        draw->PopClipRect();

        // The scrollbar: a 10px slider in the ruler column while there is scrollback.
        if (scrollback > 0)
        {
            const float track_top = grid.y;
            const float track_h = cell_h * static_cast<float>(rows);
            const float total = static_cast<float>(screen.line_count());
            const float slider_h = std::max(px(20.f), track_h * static_cast<float>(rows) / total);
            const float slider_top =
                    track_top + (track_h - slider_h) * static_cast<float>(top_index) / static_cast<float>(scrollback);
            const float sx = right - px(RULER_WIDTH) - 1.f;
            draw->AddRectFilled(ImVec2(sx, std::round(slider_top)),
                                ImVec2(sx + px(RULER_WIDTH), std::round(slider_top + slider_h)), rgb(FOREGROUND, 0.2f));
        }
        ImGui::EndChild();
    }
} // namespace imgit::ui
