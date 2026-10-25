#pragma once

#include <cstddef>
#include <cstdint>
#include <deque>
#include <string>
#include <string_view>
#include <vector>

// The screen model of a terminal emulator: a VT/xterm byte stream (what ConPTY emits) applied to a grid of cells
// with scrollback, like xterm.js's buffer in the terminal.
namespace imgit::app
{
    // A colour: DEFAULT, an xterm palette index (PALETTE | 0..255) or 24-bit (RGB | 0xRRGGBB).
    namespace vt_color
    {
        constexpr std::uint32_t DEFAULT = 0;
        constexpr std::uint32_t PALETTE = 0x01000000;
        constexpr std::uint32_t RGB = 0x02000000;
        constexpr std::uint32_t KIND_MASK = 0xFF000000;
    } // namespace vt_color

    enum VtAttribute : std::uint8_t
    {
        BOLD = 1,
        DIM = 2,
        ITALIC = 4,
        UNDERLINE = 8,
        INVERSE = 16,
        INVISIBLE = 32,
        STRIKETHROUGH = 64,
    };

    struct VtCell
    {
        char32_t ch = U' '; // 0 for the second half of a wide character
        std::uint32_t fg = vt_color::DEFAULT;
        std::uint32_t bg = vt_color::DEFAULT;
        std::uint8_t attributes = 0;
    };

    struct VtLine
    {
        std::vector<VtCell> cells;
        bool wrapped = false; // continues onto the next line (soft wrap)
    };

    class VtScreen
    {
    public:
        explicit VtScreen(int columns = 80, int rows = 24, std::size_t scrollback = 5000);

        void feed(std::string_view bytes);
        void resize(int columns, int rows);
        // xterm's clear(): the scrollback goes and the cursor's line becomes the first line.
        void clear();

        [[nodiscard]] int columns() const noexcept
        {
            return m_columns;
        }
        [[nodiscard]] int rows() const noexcept
        {
            return m_rows;
        }
        // Lines of the active buffer, scrollback first; the last rows() of them are the screen.
        [[nodiscard]] std::size_t line_count() const noexcept;
        [[nodiscard]] const VtLine& line(std::size_t index) const;
        // How many lines have ever scrolled into the scrollback (a scrolled-up view stays put by following it).
        [[nodiscard]] std::uint64_t scrolled_lines() const noexcept
        {
            return m_scrolled;
        }
        // The id of line(0): lines keep their id while older ones are dropped from the scrollback.
        [[nodiscard]] std::uint64_t first_line_id() const noexcept
        {
            return m_alternate ? 0 : m_dropped;
        }
        [[nodiscard]] int cursor_x() const noexcept
        {
            return m_cursor_x;
        }
        [[nodiscard]] int cursor_y() const noexcept
        {
            return m_cursor_y;
        }
        [[nodiscard]] bool cursor_visible() const noexcept
        {
            return m_cursor_visible;
        }
        [[nodiscard]] bool application_cursor_keys() const noexcept
        {
            return m_application_cursor;
        }
        [[nodiscard]] bool bracketed_paste() const noexcept
        {
            return m_bracketed_paste;
        }
        [[nodiscard]] bool focus_reporting() const noexcept
        {
            return m_focus_reporting;
        }
        [[nodiscard]] bool alternate_screen() const noexcept
        {
            return m_alternate;
        }
        [[nodiscard]] const std::string& title() const noexcept
        {
            return m_title;
        }
        // Replies the program asked for (device attributes, cursor position), to write back to it.
        [[nodiscard]] std::string take_replies();

    private:
        enum class State
        {
            GROUND,
            ESCAPE,
            ESCAPE_INTERMEDIATE,
            CSI,
            OSC,
            OSC_ESCAPE,
            STRING, // DCS / SOS / PM / APC: ignored up to ST
            STRING_ESCAPE,
        };

        struct Cursor
        {
            int x = 0;
            int y = 0;
            std::uint32_t fg = vt_color::DEFAULT;
            std::uint32_t bg = vt_color::DEFAULT;
            std::uint8_t attributes = 0;
            bool pending_wrap = false;
            bool origin_mode = false;
        };

        [[nodiscard]] std::deque<VtLine>& buffer() noexcept;
        [[nodiscard]] VtLine& row(int y);
        [[nodiscard]] VtCell blank() const;
        [[nodiscard]] VtLine blank_line() const;

        void execute(char32_t control);
        void print(char32_t ch);
        void escape_dispatch(char final);
        void csi_dispatch(char final);
        void osc_dispatch();
        void select_graphic_rendition();
        void set_mode(bool enable);

        void line_feed();
        void reverse_index();
        void scroll_up(int count);
        void scroll_down(int count);
        void erase_in_display(int mode);
        void erase_in_line(int mode);
        void erase_cells(int y, int from, int to);
        void insert_lines(int count);
        void delete_lines(int count);
        void insert_chars(int count);
        void delete_chars(int count);
        void move_to(int x, int y);
        void save_cursor();
        void restore_cursor();
        void switch_alternate(bool enable, bool save_restore);
        void reset();
        [[nodiscard]] int param(std::size_t index, int fallback) const;
        // Whether parameter `index` followed a ':' (a sub-parameter of the one before it, as in 4:3 or 38:2:r:g:b).
        [[nodiscard]] bool sub_param(std::size_t index) const;
        void trim_scrollback();

        int m_columns;
        int m_rows;
        std::size_t m_max_scrollback;
        std::deque<VtLine> m_main; // scrollback + screen
        std::deque<VtLine> m_alt; // the alternate screen (no scrollback)
        bool m_alternate = false;
        std::uint64_t m_scrolled = 0;
        std::uint64_t m_dropped = 0;

        int m_cursor_x = 0;
        int m_cursor_y = 0;
        std::uint32_t m_fg = vt_color::DEFAULT;
        std::uint32_t m_bg = vt_color::DEFAULT;
        std::uint8_t m_attributes = 0;
        bool m_pending_wrap = false;
        bool m_origin_mode = false;
        bool m_auto_wrap = true;
        bool m_insert_mode = false;
        bool m_cursor_visible = true;
        bool m_application_cursor = false;
        bool m_bracketed_paste = false;
        bool m_focus_reporting = false;
        int m_scroll_top = 0;
        int m_scroll_bottom = 0;
        Cursor m_saved;
        Cursor m_saved_alt;
        std::vector<bool> m_tab_stops;

        State m_state = State::GROUND;
        std::vector<int> m_params;
        std::vector<bool> m_sub_params; // parallel to m_params: introduced by ':' rather than ';'
        bool m_param_started = false;
        bool m_next_sub_param = false; // the last separator was ':'
        char m_private = 0; // '?', '>', '<', '=' before the parameters
        std::string m_intermediates;
        std::string m_osc;
        char32_t m_utf8 = 0;
        int m_utf8_remaining = 0;
        std::string m_title;
        std::string m_replies;
    };
} // namespace imgit::app
