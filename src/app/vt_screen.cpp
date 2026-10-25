#include "app/vt_screen.hpp"
#include <algorithm>
#include <utility>

namespace imgit::app
{
    namespace
    {
        // East Asian wide and emoji code points take two cells (wcwidth), combining marks none.
        int char_width(char32_t ch)
        {
            if ((ch >= 0x0300 && ch <= 0x036F) || (ch >= 0x200B && ch <= 0x200F) || (ch >= 0xFE00 && ch <= 0xFE0F)
                || (ch >= 0x20D0 && ch <= 0x20FF))
            {
                return 0;
            }
            if ((ch >= 0x1100 && ch <= 0x115F) || (ch >= 0x2E80 && ch <= 0x303E) || (ch >= 0x3041 && ch <= 0x33FF)
                || (ch >= 0x3400 && ch <= 0x4DBF) || (ch >= 0x4E00 && ch <= 0x9FFF) || (ch >= 0xA000 && ch <= 0xA4CF)
                || (ch >= 0xAC00 && ch <= 0xD7A3) || (ch >= 0xF900 && ch <= 0xFAFF) || (ch >= 0xFE30 && ch <= 0xFE4F)
                || (ch >= 0xFF00 && ch <= 0xFF60) || (ch >= 0xFFE0 && ch <= 0xFFE6) || (ch >= 0x1F300 && ch <= 0x1F64F)
                || (ch >= 0x1F900 && ch <= 0x1F9FF) || (ch >= 0x20000 && ch <= 0x3FFFD))
            {
                return 2;
            }
            return 1;
        }

        constexpr std::size_t MAX_OSC = 4096;
    } // namespace

    VtScreen::VtScreen(int columns, int rows, std::size_t scrollback)
        : m_columns(std::max(1, columns)), m_rows(std::max(1, rows)), m_max_scrollback(scrollback)
    {
        reset();
    }

    void VtScreen::reset()
    {
        m_main.clear();
        m_alt.clear();
        m_alternate = false;
        m_fg = vt_color::DEFAULT;
        m_bg = vt_color::DEFAULT;
        m_attributes = 0;
        for (int i = 0; i < m_rows; ++i)
        {
            m_main.push_back(blank_line());
            m_alt.push_back(blank_line());
        }
        m_cursor_x = 0;
        m_cursor_y = 0;
        m_pending_wrap = false;
        m_origin_mode = false;
        m_auto_wrap = true;
        m_insert_mode = false;
        m_cursor_visible = true;
        m_application_cursor = false;
        m_bracketed_paste = false;
        m_focus_reporting = false;
        m_scroll_top = 0;
        m_scroll_bottom = m_rows - 1;
        m_saved = {};
        m_saved_alt = {};
        m_tab_stops.assign(static_cast<std::size_t>(m_columns), false);
        for (int x = 8; x < m_columns; x += 8)
        {
            m_tab_stops[static_cast<std::size_t>(x)] = true;
        }
    }

    std::deque<VtLine>& VtScreen::buffer() noexcept
    {
        return m_alternate ? m_alt : m_main;
    }

    std::size_t VtScreen::line_count() const noexcept
    {
        return m_alternate ? m_alt.size() : m_main.size();
    }

    const VtLine& VtScreen::line(std::size_t index) const
    {
        return m_alternate ? m_alt[index] : m_main[index];
    }

    VtLine& VtScreen::row(int y)
    {
        std::deque<VtLine>& lines = buffer();
        return lines[lines.size() - static_cast<std::size_t>(m_rows) + static_cast<std::size_t>(y)];
    }

    VtCell VtScreen::blank() const
    {
        // Erased cells keep the current background, like xterm.
        VtCell cell;
        cell.bg = m_bg;
        return cell;
    }

    VtLine VtScreen::blank_line() const
    {
        VtLine line;
        line.cells.assign(static_cast<std::size_t>(m_columns), blank());
        return line;
    }

    std::string VtScreen::take_replies()
    {
        return std::exchange(m_replies, {});
    }

    void VtScreen::feed(std::string_view bytes)
    {
        for (std::size_t i = 0; i < bytes.size(); ++i)
        {
            const auto b = static_cast<unsigned char>(bytes[i]);
            if (b == 0x1B)
            {
                if (m_state == State::OSC)
                {
                    m_state = State::OSC_ESCAPE;
                }
                else if (m_state == State::STRING)
                {
                    m_state = State::STRING_ESCAPE;
                }
                else
                {
                    m_utf8_remaining = 0;
                    m_intermediates.clear();
                    m_state = State::ESCAPE;
                }
                continue;
            }
            if (b == 0x18 || b == 0x1A) // CAN, SUB
            {
                m_state = State::GROUND;
                continue;
            }
            switch (m_state)
            {
            case State::GROUND:
                if (m_utf8_remaining > 0)
                {
                    if ((b & 0xC0) == 0x80)
                    {
                        m_utf8 = (m_utf8 << 6) | (b & 0x3F);
                        if (--m_utf8_remaining == 0)
                        {
                            print(m_utf8);
                        }
                        break;
                    }
                    m_utf8_remaining = 0;
                    print(U'\xFFFD');
                }
                if (b < 0x20)
                {
                    execute(b);
                }
                else if (b < 0x7F)
                {
                    print(b);
                }
                else if (b == 0x7F)
                {
                }
                else if ((b & 0xE0) == 0xC0)
                {
                    m_utf8 = b & 0x1F;
                    m_utf8_remaining = 1;
                }
                else if ((b & 0xF0) == 0xE0)
                {
                    m_utf8 = b & 0x0F;
                    m_utf8_remaining = 2;
                }
                else if ((b & 0xF8) == 0xF0)
                {
                    m_utf8 = b & 0x07;
                    m_utf8_remaining = 3;
                }
                else
                {
                    print(U'\xFFFD');
                }
                break;
            case State::ESCAPE:
                if (b < 0x20)
                {
                    execute(b);
                }
                else if (b <= 0x2F)
                {
                    m_intermediates += static_cast<char>(b);
                    m_state = State::ESCAPE_INTERMEDIATE;
                }
                else if (b == '[')
                {
                    m_params.clear();
                    m_param_started = false;
                    m_private = 0;
                    m_state = State::CSI;
                }
                else if (b == ']')
                {
                    m_osc.clear();
                    m_state = State::OSC;
                }
                else if (b == 'P' || b == 'X' || b == '^' || b == '_')
                {
                    m_state = State::STRING;
                }
                else
                {
                    m_state = State::GROUND;
                    escape_dispatch(static_cast<char>(b));
                }
                break;
            case State::ESCAPE_INTERMEDIATE:
                if (b < 0x20)
                {
                    execute(b);
                }
                else if (b <= 0x2F)
                {
                    m_intermediates += static_cast<char>(b);
                }
                else
                {
                    // Character set designations and the like: nothing to draw.
                    m_state = State::GROUND;
                }
                break;
            case State::CSI:
                if (b < 0x20)
                {
                    execute(b);
                }
                else if (b >= '0' && b <= '9')
                {
                    if (!m_param_started)
                    {
                        m_params.push_back(-1);
                        m_param_started = true;
                    }
                    int& value = m_params.back();
                    value = std::min(99999, (value < 0 ? 0 : value) * 10 + (b - '0'));
                }
                else if (b == ';' || b == ':')
                {
                    if (!m_param_started)
                    {
                        m_params.push_back(-1);
                    }
                    m_param_started = false;
                }
                else if (b >= 0x3C && b <= 0x3F)
                {
                    m_private = static_cast<char>(b);
                }
                else if (b <= 0x2F)
                {
                    m_intermediates += static_cast<char>(b);
                }
                else if (b <= 0x7E)
                {
                    m_state = State::GROUND;
                    csi_dispatch(static_cast<char>(b));
                    m_intermediates.clear();
                }
                break;
            case State::OSC:
                if (b == 0x07)
                {
                    osc_dispatch();
                    m_state = State::GROUND;
                }
                else if (m_osc.size() < MAX_OSC)
                {
                    m_osc += static_cast<char>(b);
                }
                break;
            case State::OSC_ESCAPE:
                osc_dispatch();
                m_state = State::GROUND;
                if (b != '\\')
                {
                    m_state = State::ESCAPE;
                    --i; // the ESC started a new sequence
                }
                break;
            case State::STRING:
                if (b == 0x07)
                {
                    m_state = State::GROUND;
                }
                break;
            case State::STRING_ESCAPE:
                m_state = State::GROUND;
                if (b != '\\')
                {
                    m_state = State::ESCAPE;
                    --i;
                }
                break;
            }
        }
    }

    void VtScreen::execute(char32_t control)
    {
        switch (control)
        {
        case 0x08: // BS
            if (m_cursor_x > 0)
            {
                --m_cursor_x;
            }
            m_pending_wrap = false;
            break;
        case 0x09: // HT
        {
            int x = m_cursor_x + 1;
            while (x < m_columns - 1 && !m_tab_stops[static_cast<std::size_t>(x)])
            {
                ++x;
            }
            m_cursor_x = std::min(x, m_columns - 1);
            m_pending_wrap = false;
            break;
        }
        case 0x0A: // LF, VT, FF
        case 0x0B:
        case 0x0C:
            line_feed();
            break;
        case 0x0D: // CR
            m_cursor_x = 0;
            m_pending_wrap = false;
            break;
        default:
            break;
        }
    }

    void VtScreen::print(char32_t ch)
    {
        const int width = char_width(ch);
        if (width == 0)
        {
            return;
        }
        if (m_pending_wrap && m_auto_wrap)
        {
            row(m_cursor_y).wrapped = true;
            m_cursor_x = 0;
            line_feed();
        }
        m_pending_wrap = false;
        if (width == 2 && m_cursor_x == m_columns - 1)
        {
            if (!m_auto_wrap)
            {
                return;
            }
            erase_cells(m_cursor_y, m_cursor_x, m_cursor_x + 1);
            row(m_cursor_y).wrapped = true;
            m_cursor_x = 0;
            line_feed();
        }
        if (m_insert_mode)
        {
            insert_chars(width);
        }
        std::vector<VtCell>& cells = row(m_cursor_y).cells;
        const auto x = static_cast<std::size_t>(m_cursor_x);
        // Overwriting half of a wide character blanks the other half.
        if (cells[x].ch == 0 && x > 0)
        {
            cells[x - 1].ch = U' ';
        }
        cells[x] = VtCell{ch, m_fg, m_bg, m_attributes};
        if (width == 2 && x + 1 < cells.size())
        {
            cells[x + 1] = VtCell{0, m_fg, m_bg, m_attributes};
        }
        else if (x + 1 < cells.size() && cells[x + 1].ch == 0)
        {
            cells[x + 1].ch = U' ';
        }
        m_cursor_x += width;
        if (m_cursor_x >= m_columns)
        {
            m_cursor_x = m_columns - 1;
            m_pending_wrap = m_auto_wrap;
        }
    }

    void VtScreen::line_feed()
    {
        m_pending_wrap = false;
        if (m_cursor_y == m_scroll_bottom)
        {
            scroll_up(1);
        }
        else if (m_cursor_y < m_rows - 1)
        {
            ++m_cursor_y;
        }
    }

    void VtScreen::reverse_index()
    {
        m_pending_wrap = false;
        if (m_cursor_y == m_scroll_top)
        {
            scroll_down(1);
        }
        else if (m_cursor_y > 0)
        {
            --m_cursor_y;
        }
    }

    void VtScreen::scroll_up(int count)
    {
        std::deque<VtLine>& lines = buffer();
        count = std::clamp(count, 0, m_scroll_bottom - m_scroll_top + 1);
        for (int i = 0; i < count; ++i)
        {
            const std::size_t base = lines.size() - static_cast<std::size_t>(m_rows);
            if (!m_alternate && m_scroll_top == 0)
            {
                // The top line moves into the scrollback.
                if (m_scroll_bottom == m_rows - 1)
                {
                    lines.push_back(blank_line());
                }
                else
                {
                    lines.insert(lines.begin() + static_cast<std::ptrdiff_t>(base + m_scroll_bottom + 1), blank_line());
                }
                ++m_scrolled;
            }
            else
            {
                lines.erase(lines.begin() + static_cast<std::ptrdiff_t>(base + m_scroll_top));
                lines.insert(lines.begin() + static_cast<std::ptrdiff_t>(base + m_scroll_bottom), blank_line());
            }
        }
        trim_scrollback();
    }

    void VtScreen::scroll_down(int count)
    {
        std::deque<VtLine>& lines = buffer();
        count = std::clamp(count, 0, m_scroll_bottom - m_scroll_top + 1);
        for (int i = 0; i < count; ++i)
        {
            const std::size_t base = lines.size() - static_cast<std::size_t>(m_rows);
            lines.erase(lines.begin() + static_cast<std::ptrdiff_t>(base + m_scroll_bottom));
            lines.insert(lines.begin() + static_cast<std::ptrdiff_t>(base + m_scroll_top), blank_line());
        }
    }

    void VtScreen::trim_scrollback()
    {
        while (m_main.size() > static_cast<std::size_t>(m_rows) + m_max_scrollback)
        {
            m_main.pop_front();
            ++m_dropped;
        }
    }

    void VtScreen::erase_cells(int y, int from, int to)
    {
        std::vector<VtCell>& cells = row(y).cells;
        from = std::clamp(from, 0, m_columns);
        to = std::clamp(to, 0, m_columns);
        std::fill(cells.begin() + from, cells.begin() + to, blank());
    }

    void VtScreen::erase_in_display(int mode)
    {
        switch (mode)
        {
        case 0:
            erase_in_line(0);
            for (int y = m_cursor_y + 1; y < m_rows; ++y)
            {
                erase_cells(y, 0, m_columns);
                row(y).wrapped = false;
            }
            break;
        case 1:
            for (int y = 0; y < m_cursor_y; ++y)
            {
                erase_cells(y, 0, m_columns);
                row(y).wrapped = false;
            }
            erase_in_line(1);
            break;
        case 2:
            for (int y = 0; y < m_rows; ++y)
            {
                erase_cells(y, 0, m_columns);
                row(y).wrapped = false;
            }
            break;
        case 3:
            if (!m_alternate)
            {
                m_dropped += m_main.size() - static_cast<std::size_t>(m_rows);
                m_main.erase(m_main.begin(), m_main.end() - m_rows);
            }
            break;
        default:
            break;
        }
    }

    void VtScreen::erase_in_line(int mode)
    {
        m_pending_wrap = false;
        switch (mode)
        {
        case 0:
            erase_cells(m_cursor_y, m_cursor_x, m_columns);
            row(m_cursor_y).wrapped = false;
            break;
        case 1:
            erase_cells(m_cursor_y, 0, m_cursor_x + 1);
            break;
        case 2:
            erase_cells(m_cursor_y, 0, m_columns);
            row(m_cursor_y).wrapped = false;
            break;
        default:
            break;
        }
    }

    void VtScreen::insert_lines(int count)
    {
        if (m_cursor_y < m_scroll_top || m_cursor_y > m_scroll_bottom)
        {
            return;
        }
        std::deque<VtLine>& lines = buffer();
        count = std::clamp(count, 0, m_scroll_bottom - m_cursor_y + 1);
        for (int i = 0; i < count; ++i)
        {
            const std::size_t base = lines.size() - static_cast<std::size_t>(m_rows);
            lines.erase(lines.begin() + static_cast<std::ptrdiff_t>(base + m_scroll_bottom));
            lines.insert(lines.begin() + static_cast<std::ptrdiff_t>(base + m_cursor_y), blank_line());
        }
        m_cursor_x = 0;
        m_pending_wrap = false;
    }

    void VtScreen::delete_lines(int count)
    {
        if (m_cursor_y < m_scroll_top || m_cursor_y > m_scroll_bottom)
        {
            return;
        }
        std::deque<VtLine>& lines = buffer();
        count = std::clamp(count, 0, m_scroll_bottom - m_cursor_y + 1);
        for (int i = 0; i < count; ++i)
        {
            const std::size_t base = lines.size() - static_cast<std::size_t>(m_rows);
            lines.erase(lines.begin() + static_cast<std::ptrdiff_t>(base + m_cursor_y));
            lines.insert(lines.begin() + static_cast<std::ptrdiff_t>(base + m_scroll_bottom), blank_line());
        }
        m_cursor_x = 0;
        m_pending_wrap = false;
    }

    void VtScreen::insert_chars(int count)
    {
        std::vector<VtCell>& cells = row(m_cursor_y).cells;
        count = std::clamp(count, 0, m_columns - m_cursor_x);
        cells.insert(cells.begin() + m_cursor_x, static_cast<std::size_t>(count), blank());
        cells.resize(static_cast<std::size_t>(m_columns));
        m_pending_wrap = false;
    }

    void VtScreen::delete_chars(int count)
    {
        std::vector<VtCell>& cells = row(m_cursor_y).cells;
        count = std::clamp(count, 0, m_columns - m_cursor_x);
        cells.erase(cells.begin() + m_cursor_x, cells.begin() + m_cursor_x + count);
        cells.resize(static_cast<std::size_t>(m_columns), blank());
        m_pending_wrap = false;
    }

    void VtScreen::move_to(int x, int y)
    {
        const int top = m_origin_mode ? m_scroll_top : 0;
        const int bottom = m_origin_mode ? m_scroll_bottom : m_rows - 1;
        m_cursor_x = std::clamp(x, 0, m_columns - 1);
        m_cursor_y = std::clamp(y + top, top, bottom);
        m_pending_wrap = false;
    }

    void VtScreen::save_cursor()
    {
        Cursor& saved = m_alternate ? m_saved_alt : m_saved;
        saved = {m_cursor_x, m_cursor_y, m_fg, m_bg, m_attributes, m_pending_wrap, m_origin_mode};
    }

    void VtScreen::restore_cursor()
    {
        const Cursor& saved = m_alternate ? m_saved_alt : m_saved;
        m_cursor_x = std::clamp(saved.x, 0, m_columns - 1);
        m_cursor_y = std::clamp(saved.y, 0, m_rows - 1);
        m_fg = saved.fg;
        m_bg = saved.bg;
        m_attributes = saved.attributes;
        m_pending_wrap = saved.pending_wrap;
        m_origin_mode = saved.origin_mode;
    }

    void VtScreen::switch_alternate(bool enable, bool save_restore)
    {
        if (enable == m_alternate)
        {
            return;
        }
        if (enable)
        {
            if (save_restore)
            {
                save_cursor();
            }
            m_alternate = true;
            m_alt.clear();
            for (int i = 0; i < m_rows; ++i)
            {
                m_alt.push_back(blank_line());
            }
        }
        else
        {
            m_alternate = false;
            if (save_restore)
            {
                restore_cursor();
            }
        }
        m_scroll_top = 0;
        m_scroll_bottom = m_rows - 1;
    }

    int VtScreen::param(std::size_t index, int fallback) const
    {
        return index < m_params.size() && m_params[index] >= 0 ? m_params[index] : fallback;
    }

    void VtScreen::escape_dispatch(char final)
    {
        if (!m_intermediates.empty())
        {
            return;
        }
        switch (final)
        {
        case '7':
            save_cursor();
            break;
        case '8':
            restore_cursor();
            break;
        case 'D':
            line_feed();
            break;
        case 'E':
            m_cursor_x = 0;
            line_feed();
            break;
        case 'H':
            m_tab_stops[static_cast<std::size_t>(m_cursor_x)] = true;
            break;
        case 'M':
            reverse_index();
            break;
        case 'c':
            reset();
            break;
        default:
            break;
        }
    }

    void VtScreen::csi_dispatch(char final)
    {
        const int n = std::max(1, param(0, 1));
        if (m_private != 0 && final != 'h' && final != 'l' && final != 'c' && final != 'n')
        {
            return;
        }
        if (!m_intermediates.empty())
        {
            if (m_intermediates == "!" && final == 'p') // DECSTR
            {
                m_fg = vt_color::DEFAULT;
                m_bg = vt_color::DEFAULT;
                m_attributes = 0;
                m_insert_mode = false;
                m_origin_mode = false;
                m_auto_wrap = true;
                m_cursor_visible = true;
                m_application_cursor = false;
                m_scroll_top = 0;
                m_scroll_bottom = m_rows - 1;
            }
            return; // DECSCUSR (cursor style) and others
        }
        switch (final)
        {
        case '@':
            insert_chars(n);
            break;
        case 'A':
            m_cursor_y = std::max(m_cursor_y >= m_scroll_top ? m_scroll_top : 0, m_cursor_y - n);
            m_pending_wrap = false;
            break;
        case 'B':
        case 'e':
            m_cursor_y = std::min(m_cursor_y <= m_scroll_bottom ? m_scroll_bottom : m_rows - 1, m_cursor_y + n);
            m_pending_wrap = false;
            break;
        case 'C':
        case 'a':
            m_cursor_x = std::min(m_columns - 1, m_cursor_x + n);
            m_pending_wrap = false;
            break;
        case 'D':
            m_cursor_x = std::max(0, std::min(m_cursor_x, m_columns - 1) - n);
            m_pending_wrap = false;
            break;
        case 'E':
            m_cursor_y = std::min(m_rows - 1, m_cursor_y + n);
            m_cursor_x = 0;
            m_pending_wrap = false;
            break;
        case 'F':
            m_cursor_y = std::max(0, m_cursor_y - n);
            m_cursor_x = 0;
            m_pending_wrap = false;
            break;
        case 'G':
        case '`':
            m_cursor_x = std::clamp(n - 1, 0, m_columns - 1);
            m_pending_wrap = false;
            break;
        case 'H':
        case 'f':
            move_to(param(1, 1) - 1, std::max(1, param(0, 1)) - 1);
            break;
        case 'I':
            for (int i = 0; i < n; ++i)
            {
                execute(0x09);
            }
            break;
        case 'J':
            erase_in_display(param(0, 0));
            break;
        case 'K':
            erase_in_line(param(0, 0));
            break;
        case 'L':
            insert_lines(n);
            break;
        case 'M':
            delete_lines(n);
            break;
        case 'P':
            delete_chars(n);
            break;
        case 'S':
            scroll_up(n);
            break;
        case 'T':
            scroll_down(n);
            break;
        case 'X':
            erase_cells(m_cursor_y, m_cursor_x, m_cursor_x + n);
            m_pending_wrap = false;
            break;
        case 'Z':
            for (int i = 0; i < n && m_cursor_x > 0; ++i)
            {
                do
                {
                    --m_cursor_x;
                }
                while (m_cursor_x > 0 && !m_tab_stops[static_cast<std::size_t>(m_cursor_x)]);
            }
            m_pending_wrap = false;
            break;
        case 'c':
            if (m_private == 0 && param(0, 0) == 0)
            {
                m_replies += "\x1b[?1;2c";
            }
            else if (m_private == '>')
            {
                m_replies += "\x1b[>0;276;0c";
            }
            break;
        case 'd':
            move_to(m_cursor_x, n - 1);
            break;
        case 'g':
            if (param(0, 0) == 0)
            {
                m_tab_stops[static_cast<std::size_t>(m_cursor_x)] = false;
            }
            else if (param(0, 0) == 3)
            {
                std::fill(m_tab_stops.begin(), m_tab_stops.end(), false);
            }
            break;
        case 'h':
            set_mode(true);
            break;
        case 'l':
            set_mode(false);
            break;
        case 'm':
            select_graphic_rendition();
            break;
        case 'n':
            if (m_private == 0 && param(0, 0) == 5)
            {
                m_replies += "\x1b[0n";
            }
            else if (param(0, 0) == 6)
            {
                m_replies += "\x1b[" + std::to_string(m_cursor_y + 1) + ";" + std::to_string(m_cursor_x + 1) + "R";
            }
            break;
        case 'r':
        {
            const int top = std::max(1, param(0, 1)) - 1;
            const int bottom = std::min(m_rows, param(1, m_rows)) - 1;
            if (top < bottom)
            {
                m_scroll_top = top;
                m_scroll_bottom = bottom;
                move_to(0, 0);
            }
            break;
        }
        case 's':
            save_cursor();
            break;
        case 'u':
            restore_cursor();
            break;
        default:
            break;
        }
    }

    void VtScreen::set_mode(bool enable)
    {
        for (std::size_t i = 0; i < std::max<std::size_t>(1, m_params.size()); ++i)
        {
            const int mode = param(i, 0);
            if (m_private == '?')
            {
                switch (mode)
                {
                case 1:
                    m_application_cursor = enable;
                    break;
                case 6:
                    m_origin_mode = enable;
                    move_to(0, 0);
                    break;
                case 7:
                    m_auto_wrap = enable;
                    break;
                case 25:
                    m_cursor_visible = enable;
                    break;
                case 47:
                case 1047:
                    switch_alternate(enable, false);
                    break;
                case 1048:
                    if (enable)
                    {
                        save_cursor();
                    }
                    else
                    {
                        restore_cursor();
                    }
                    break;
                case 1049:
                    switch_alternate(enable, true);
                    break;
                case 1004:
                    m_focus_reporting = enable;
                    break;
                case 2004:
                    m_bracketed_paste = enable;
                    break;
                default:
                    break;
                }
            }
            else if (m_private == 0 && mode == 4)
            {
                m_insert_mode = enable;
            }
        }
    }

    void VtScreen::select_graphic_rendition()
    {
        if (m_params.empty())
        {
            m_fg = vt_color::DEFAULT;
            m_bg = vt_color::DEFAULT;
            m_attributes = 0;
            return;
        }
        for (std::size_t i = 0; i < m_params.size(); ++i)
        {
            const int code = param(i, 0);
            // 38;5;n / 38;2;r;g;b (and 48 for the background)
            const auto extended = [&](std::uint32_t& target)
            {
                const int kind = param(i + 1, 0);
                if (kind == 5)
                {
                    target = vt_color::PALETTE | static_cast<std::uint32_t>(std::clamp(param(i + 2, 0), 0, 255));
                    i += 2;
                }
                else if (kind == 2)
                {
                    const auto channel = [&](std::size_t k)
                    {
                        return static_cast<std::uint32_t>(std::clamp(param(k, 0), 0, 255));
                    };
                    target = vt_color::RGB | (channel(i + 2) << 16) | (channel(i + 3) << 8) | channel(i + 4);
                    i += 4;
                }
            };
            switch (code)
            {
            case 0:
                m_fg = vt_color::DEFAULT;
                m_bg = vt_color::DEFAULT;
                m_attributes = 0;
                break;
            case 1:
                m_attributes |= BOLD;
                break;
            case 2:
                m_attributes |= DIM;
                break;
            case 3:
                m_attributes |= ITALIC;
                break;
            case 4:
            case 21:
                m_attributes |= UNDERLINE;
                break;
            case 7:
                m_attributes |= INVERSE;
                break;
            case 8:
                m_attributes |= INVISIBLE;
                break;
            case 9:
                m_attributes |= STRIKETHROUGH;
                break;
            case 22:
                m_attributes &= static_cast<std::uint8_t>(~(BOLD | DIM));
                break;
            case 23:
                m_attributes &= static_cast<std::uint8_t>(~ITALIC);
                break;
            case 24:
                m_attributes &= static_cast<std::uint8_t>(~UNDERLINE);
                break;
            case 27:
                m_attributes &= static_cast<std::uint8_t>(~INVERSE);
                break;
            case 28:
                m_attributes &= static_cast<std::uint8_t>(~INVISIBLE);
                break;
            case 29:
                m_attributes &= static_cast<std::uint8_t>(~STRIKETHROUGH);
                break;
            case 38:
                extended(m_fg);
                break;
            case 39:
                m_fg = vt_color::DEFAULT;
                break;
            case 48:
                extended(m_bg);
                break;
            case 49:
                m_bg = vt_color::DEFAULT;
                break;
            default:
                if (code >= 30 && code <= 37)
                {
                    m_fg = vt_color::PALETTE | static_cast<std::uint32_t>(code - 30);
                }
                else if (code >= 40 && code <= 47)
                {
                    m_bg = vt_color::PALETTE | static_cast<std::uint32_t>(code - 40);
                }
                else if (code >= 90 && code <= 97)
                {
                    m_fg = vt_color::PALETTE | static_cast<std::uint32_t>(code - 90 + 8);
                }
                else if (code >= 100 && code <= 107)
                {
                    m_bg = vt_color::PALETTE | static_cast<std::uint32_t>(code - 100 + 8);
                }
                break;
            }
        }
    }

    void VtScreen::osc_dispatch()
    {
        const std::size_t separator = m_osc.find(';');
        if (separator == std::string::npos)
        {
            return;
        }
        const std::string code = m_osc.substr(0, separator);
        if (code == "0" || code == "2")
        {
            m_title = m_osc.substr(separator + 1);
        }
    }

    void VtScreen::resize(int columns, int rows)
    {
        columns = std::max(1, columns);
        rows = std::max(1, rows);
        if (columns == m_columns && rows == m_rows)
        {
            return;
        }
        const auto fit = [&](std::deque<VtLine>& lines)
        {
            for (VtLine& line : lines)
            {
                line.cells.resize(static_cast<std::size_t>(columns), VtCell{});
            }
        };
        m_columns = columns;
        fit(m_main);
        fit(m_alt);

        // Fewer rows: blank lines under the cursor go first, then the top lines scroll into the scrollback.
        int current = m_rows;
        const auto is_blank = [](const VtLine& line)
        {
            return std::all_of(line.cells.begin(), line.cells.end(),
                               [](const VtCell& cell)
                               {
                                   return (cell.ch == U' ' || cell.ch == 0) && cell.bg == vt_color::DEFAULT;
                               });
        };
        while (current > rows && current - 1 > m_cursor_y && is_blank(m_main.back()))
        {
            m_main.pop_back();
            --current;
        }
        if (current > rows)
        {
            m_cursor_y -= current - rows;
            m_scrolled += static_cast<std::uint64_t>(current - rows);
        }
        while (current < rows)
        {
            m_main.push_back(VtLine{std::vector<VtCell>(static_cast<std::size_t>(columns)), false});
            ++current;
        }
        while (m_alt.size() > static_cast<std::size_t>(rows))
        {
            m_alt.pop_front();
        }
        while (m_alt.size() < static_cast<std::size_t>(rows))
        {
            m_alt.push_back(VtLine{std::vector<VtCell>(static_cast<std::size_t>(columns)), false});
        }
        m_rows = rows;
        while (m_main.size() < static_cast<std::size_t>(m_rows))
        {
            m_main.push_front(VtLine{std::vector<VtCell>(static_cast<std::size_t>(columns)), false});
        }
        m_cursor_x = std::clamp(m_cursor_x, 0, m_columns - 1);
        m_cursor_y = std::clamp(m_cursor_y, 0, m_rows - 1);
        m_pending_wrap = false;
        m_scroll_top = 0;
        m_scroll_bottom = m_rows - 1;
        m_tab_stops.assign(static_cast<std::size_t>(m_columns), false);
        for (int x = 8; x < m_columns; x += 8)
        {
            m_tab_stops[static_cast<std::size_t>(x)] = true;
        }
        trim_scrollback();
    }

    void VtScreen::clear()
    {
        if (m_alternate)
        {
            return;
        }
        VtLine current = row(m_cursor_y);
        m_dropped += m_main.size();
        m_main.clear();
        m_main.push_back(std::move(current));
        for (int i = 1; i < m_rows; ++i)
        {
            m_main.push_back(blank_line());
        }
        m_cursor_y = 0;
    }
} // namespace imgit::app
