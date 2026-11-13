//! The grid, its scrollback and everything the escape sequences do to them.

use std::collections::VecDeque;

use bitflags::bitflags;

use crate::cell::{Attributes, Cell, Color, Line};
use crate::parser::{Csi, Parser, Perform};

bitflags! {
    /// The modes a program can switch (`CSI ? Pm h` / `CSI ? Pm l`, and `CSI 4 h` for insert).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Modes: u8 {
        /// DECOM (`?6`): cursor addressing is relative to, and confined to, the scroll region.
        const ORIGIN = 1;
        /// DECAWM (`?7`): printing past the last column continues on the next line.
        const AUTO_WRAP = 1 << 1;
        /// IRM (`4`): printed characters push the rest of the line right instead of overwriting it.
        const INSERT = 1 << 2;
        /// DECTCEM (`?25`).
        const CURSOR_VISIBLE = 1 << 3;
        /// DECCKM (`?1`): the arrow keys send `ESC O A` rather than `ESC [ A`.
        const APPLICATION_CURSOR_KEYS = 1 << 4;
        /// `?2004`: pasted text is to be wrapped in `ESC [ 200 ~` / `ESC [ 201 ~`.
        const BRACKETED_PASTE = 1 << 5;
        /// `?1004`: the program wants `ESC [ I` / `ESC [ O` when the terminal gains or loses focus.
        const FOCUS_REPORTING = 1 << 6;
        /// `?47`, `?1047`, `?1049`: the alternate screen (no scrollback) is showing.
        const ALTERNATE_SCREEN = 1 << 7;
    }
}

impl Default for Modes {
    /// A fresh terminal: auto-wrap on, cursor visible.
    fn default() -> Self {
        Self::AUTO_WRAP | Self::CURSOR_VISIBLE
    }
}

/// Where the cursor is on the screen, both zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CursorPosition {
    pub column: usize,
    pub row: usize,
}

/// A cell boundary in the buffer, by line id (see [`Screen::first_line_id`]) so that it survives scrolling.
/// Ordered by line, then column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Point {
    pub line: u64,
    pub column: usize,
}

/// The colours and attributes the next printed character gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Pen {
    fg: Color,
    bg: Color,
    attributes: Attributes,
}

/// What DECSC saves and DECRC restores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SavedCursor {
    position: CursorPosition,
    pen: Pen,
    pending_wrap: bool,
    origin_mode: bool,
}

/// A terminal's screen: feed it the program's output, read back the lines.
#[derive(Debug, Clone)]
pub struct Screen {
    columns: usize,
    rows: usize,
    max_scrollback: usize,
    /// Scrollback + screen.
    main: VecDeque<Line>,
    /// The alternate screen (no scrollback).
    alt: VecDeque<Line>,
    scrolled: u64,
    dropped: u64,

    cursor: CursorPosition,
    pen: Pen,
    /// The cursor sits on the last column with a character printed there: the next one wraps first.
    pending_wrap: bool,
    modes: Modes,
    scroll_top: usize,
    /// Inclusive.
    scroll_bottom: usize,
    saved: SavedCursor,
    saved_alt: SavedCursor,
    tab_stops: Vec<bool>,

    parser: Parser,
    title: String,
    replies: String,
}

impl Default for Screen {
    /// 80 x 24 with 5000 lines of scrollback.
    fn default() -> Self {
        Self::new(80, 24, 5000)
    }
}

/// Queries.
impl Screen {
    /// A blank screen of at least 1 x 1 keeping up to `scrollback` lines above it.
    #[must_use]
    pub fn new(columns: usize, rows: usize, scrollback: usize) -> Self {
        let mut screen = Self {
            columns: columns.max(1),
            rows: rows.max(1),
            max_scrollback: scrollback,
            main: VecDeque::new(),
            alt: VecDeque::new(),
            scrolled: 0,
            dropped: 0,
            cursor: CursorPosition::default(),
            pen: Pen::default(),
            pending_wrap: false,
            modes: Modes::default(),
            scroll_top: 0,
            scroll_bottom: 0,
            saved: SavedCursor::default(),
            saved_alt: SavedCursor::default(),
            tab_stops: Vec::new(),
            parser: Parser::default(),
            title: String::new(),
            replies: String::new(),
        };
        screen.reset();
        screen
    }

    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }

    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// How many lines the active buffer holds, scrollback included.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.buffer().len()
    }

    /// Line `index` of the active buffer, scrollback first; the last [`rows`](Self::rows) of them are the screen.
    #[must_use]
    pub fn line(&self, index: usize) -> Option<&Line> {
        self.buffer().get(index)
    }

    /// The lines of the active buffer, scrollback first; the last [`rows`](Self::rows) of them are the screen.
    pub fn lines(&self) -> impl DoubleEndedIterator<Item = &Line> + ExactSizeIterator {
        self.buffer().iter()
    }

    /// Row `row` of the visible screen.
    ///
    /// # Panics
    ///
    /// When `row` is not below [`rows`](Self::rows).
    #[must_use]
    pub fn screen_line(&self, row: usize) -> &Line {
        assert!(row < self.rows, "row {row} of a {}-row screen", self.rows);
        &self.buffer()[self.base() + row]
    }

    /// The rows of the visible screen, top to bottom.
    pub fn screen_lines(&self) -> impl DoubleEndedIterator<Item = &Line> + ExactSizeIterator {
        self.buffer().range(self.base()..)
    }

    /// How many lines have ever scrolled into the scrollback (a scrolled-up view stays put by following it).
    #[must_use]
    pub const fn scrolled_lines(&self) -> u64 {
        self.scrolled
    }

    /// The id of `line(0)`: lines keep their id while older ones are dropped from the scrollback.
    #[must_use]
    pub const fn first_line_id(&self) -> u64 {
        if self.alternate() { 0 } else { self.dropped }
    }

    /// The line with the given id, if it is still in the buffer.
    #[must_use]
    pub fn line_by_id(&self, id: u64) -> Option<&Line> {
        let index = id.checked_sub(self.first_line_id())?;
        self.line(usize::try_from(index).ok()?)
    }

    #[must_use]
    pub const fn cursor(&self) -> CursorPosition {
        self.cursor
    }

    #[must_use]
    pub const fn modes(&self) -> Modes {
        self.modes
    }

    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.modes.contains(Modes::CURSOR_VISIBLE)
    }

    #[must_use]
    pub const fn application_cursor_keys(&self) -> bool {
        self.modes.contains(Modes::APPLICATION_CURSOR_KEYS)
    }

    #[must_use]
    pub const fn bracketed_paste(&self) -> bool {
        self.modes.contains(Modes::BRACKETED_PASTE)
    }

    #[must_use]
    pub const fn focus_reporting(&self) -> bool {
        self.modes.contains(Modes::FOCUS_REPORTING)
    }

    #[must_use]
    pub const fn alternate_screen(&self) -> bool {
        self.alternate()
    }

    /// The window title the program set (OSC 0 / OSC 2); empty until it does.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Replies the program asked for (device attributes, cursor position), to write back to it.
    #[must_use]
    pub fn take_replies(&mut self) -> String {
        std::mem::take(&mut self.replies)
    }

    /// The text from `start` up to (not including) `end`, as a selection is copied: trailing blanks go (those of
    /// a soft-wrapped line stay, they separate its words from the next line's), hard line breaks become CR LF
    /// and soft-wrapped lines join without a break. Lines no longer in the buffer are skipped.
    #[must_use]
    pub fn text_between(&self, start: Point, end: Point) -> String {
        let first = self.first_line_id();
        let mut text = String::new();
        for id in start.line.max(first)..=end.line {
            let Some(line) = self.line_by_id(id) else {
                break;
            };
            let cells = line.cells();
            let from = if id == start.line { start.column } else { 0 };
            let to = if id == end.line {
                end.column
            } else {
                cells.len()
            };
            let mut row: String = cells
                .get(from..to.min(cells.len()))
                .unwrap_or_default()
                .iter()
                .filter_map(|cell| cell.ch)
                .collect();
            // The blanks of a soft-wrapped line are text (the space between words that wrapped).
            if (id != end.line || to >= cells.len()) && !line.is_wrapped() {
                row.truncate(row.trim_end_matches(' ').len());
            }
            text += &row;
            if id != end.line && !line.is_wrapped() {
                text += "\r\n";
            }
        }
        text
    }
}

/// Changes from the outside.
impl Screen {
    /// Applies the program's output. Escape sequences and UTF-8 characters may be split across calls.
    pub fn feed(&mut self, bytes: &[u8]) {
        // The parser steps aside so that it can call back into the screen.
        let mut parser = std::mem::take(&mut self.parser);
        for &byte in bytes {
            parser.advance(byte, self);
        }
        self.parser = parser;
    }

    /// Changes the size (at least 1 x 1). Lines are cut or padded on the right, not reflowed; with fewer rows the
    /// blank lines under the cursor go first, then the top lines scroll into the scrollback. The scroll region
    /// and the tab stops go back to their defaults.
    pub fn resize(&mut self, columns: usize, rows: usize) {
        let columns = columns.max(1);
        let rows = rows.max(1);
        if columns == self.columns && rows == self.rows {
            return;
        }
        for line in self.main.iter_mut().chain(&mut self.alt) {
            line.cells.resize(columns, Cell::default());
        }
        self.columns = columns;

        let empty_line = || Line::filled(columns, Cell::default());
        let mut current = self.rows;
        while current > rows
            && current - 1 > self.cursor.row
            && self.main.back().is_some_and(Line::is_blank)
        {
            self.main.pop_back();
            current -= 1;
        }
        if current > rows {
            self.cursor.row = self.cursor.row.saturating_sub(current - rows);
            self.scrolled += (current - rows) as u64;
        }
        while current < rows {
            self.main.push_back(empty_line());
            current += 1;
        }
        while self.alt.len() > rows {
            self.alt.pop_front();
        }
        while self.alt.len() < rows {
            self.alt.push_back(empty_line());
        }
        self.rows = rows;
        while self.main.len() < rows {
            self.main.push_front(empty_line());
        }
        self.cursor.column = self.cursor.column.min(columns - 1);
        self.cursor.row = self.cursor.row.min(rows - 1);
        self.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.reset_tab_stops();
        self.trim_scrollback();
    }

    /// xterm's `clear()`: the scrollback goes and the cursor's line becomes the first line. Nothing happens on
    /// the alternate screen.
    pub fn clear(&mut self) {
        if self.alternate() {
            return;
        }
        let current = self.row(self.cursor.row).clone();
        self.dropped += self.main.len() as u64;
        self.main.clear();
        self.main.push_back(current);
        let blank = self.blank_line();
        self.main.resize(self.rows, blank);
        self.cursor.row = 0;
    }
}

/// The buffer and its primitives.
impl Screen {
    const fn alternate(&self) -> bool {
        self.modes.contains(Modes::ALTERNATE_SCREEN)
    }

    const fn buffer(&self) -> &VecDeque<Line> {
        if self.alternate() {
            &self.alt
        } else {
            &self.main
        }
    }

    const fn buffer_mut(&mut self) -> &mut VecDeque<Line> {
        if self.alternate() {
            &mut self.alt
        } else {
            &mut self.main
        }
    }

    /// The buffer index of the screen's first row.
    fn base(&self) -> usize {
        self.buffer().len() - self.rows
    }

    fn row(&mut self, y: usize) -> &mut Line {
        let index = self.base() + y;
        &mut self.buffer_mut()[index]
    }

    /// Erased cells keep the current background, like xterm.
    const fn blank(&self) -> Cell {
        Cell::blank(self.pen.bg)
    }

    fn blank_line(&self) -> Line {
        Line::filled(self.columns, self.blank())
    }

    /// Removes screen row `remove` and puts a blank line at screen row `insert` (of the buffer without the removed
    /// row): how a region scrolls without feeding the scrollback.
    fn rotate_rows(&mut self, remove: usize, insert: usize) {
        let base = self.base();
        let blank = self.blank_line();
        let lines = self.buffer_mut();
        lines.remove(base + remove);
        lines.insert(base + insert, blank);
    }

    fn reset_tab_stops(&mut self) {
        self.tab_stops = (0..self.columns).map(|x| x != 0 && x % 8 == 0).collect();
    }

    /// RIS: everything but the title back to the initial state.
    fn reset(&mut self) {
        // The line ids (first_line_id) stay unique across the lines thrown away, as in clear().
        self.dropped += self.main.len() as u64;
        self.pen = Pen::default();
        self.modes = Modes::default();
        let blank = self.blank_line();
        self.main.clear();
        self.main.resize(self.rows, blank.clone());
        self.alt.clear();
        self.alt.resize(self.rows, blank);
        self.cursor = CursorPosition::default();
        self.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.saved = SavedCursor::default();
        self.saved_alt = SavedCursor::default();
        self.reset_tab_stops();
    }

    fn line_feed(&mut self) {
        self.pending_wrap = false;
        if self.cursor.row == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.cursor.row < self.rows - 1 {
            self.cursor.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.pending_wrap = false;
        if self.cursor.row == self.scroll_top {
            self.scroll_down(1);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
    }

    fn tab(&mut self) {
        let last = self.columns - 1;
        let mut x = self.cursor.column + 1;
        while x < last && !self.tab_stops[x] {
            x += 1;
        }
        self.cursor.column = x.min(last);
        self.pending_wrap = false;
    }

    fn back_tab(&mut self, count: usize) {
        for _ in 0..count {
            if self.cursor.column == 0 {
                break;
            }
            self.cursor.column -= 1;
            while self.cursor.column > 0 && !self.tab_stops[self.cursor.column] {
                self.cursor.column -= 1;
            }
        }
        self.pending_wrap = false;
    }

    fn scroll_up(&mut self, count: usize) {
        let count = count.min(self.scroll_bottom - self.scroll_top + 1);
        for _ in 0..count {
            if !self.alternate() && self.scroll_top == 0 {
                // The top line moves into the scrollback.
                let below = self.base() + self.scroll_bottom + 1;
                let blank = self.blank_line();
                self.main.insert(below, blank);
                self.scrolled += 1;
            } else {
                self.rotate_rows(self.scroll_top, self.scroll_bottom);
            }
        }
        self.trim_scrollback();
    }

    fn scroll_down(&mut self, count: usize) {
        let count = count.min(self.scroll_bottom - self.scroll_top + 1);
        for _ in 0..count {
            self.rotate_rows(self.scroll_bottom, self.scroll_top);
        }
    }

    fn trim_scrollback(&mut self) {
        while self.main.len() > self.rows + self.max_scrollback {
            self.main.pop_front();
            self.dropped += 1;
        }
    }

    /// Blanks columns `from..to` (clamped to the line) of screen row `y`.
    fn erase_cells(&mut self, y: usize, from: usize, to: usize) {
        let blank = self.blank();
        let to = to.min(self.columns);
        let cells = &mut self.row(y).cells;
        if let Some(erased) = cells.get_mut(from..to) {
            erased.fill(blank);
        }
        blank_split_wide(cells, blank);
    }

    fn erase_row(&mut self, y: usize) {
        self.erase_cells(y, 0, self.columns);
        self.row(y).wrapped = false;
    }

    /// ED.
    fn erase_in_display(&mut self, mode: usize) {
        match mode {
            0 => {
                self.erase_in_line(0);
                for y in self.cursor.row + 1..self.rows {
                    self.erase_row(y);
                }
            }
            1 => {
                for y in 0..self.cursor.row {
                    self.erase_row(y);
                }
                self.erase_in_line(1);
            }
            2 => {
                for y in 0..self.rows {
                    self.erase_row(y);
                }
            }
            // The scrollback.
            3 if !self.alternate() => {
                let scrollback = self.base();
                self.dropped += scrollback as u64;
                self.main.drain(..scrollback);
            }
            _ => {}
        }
    }

    /// EL.
    fn erase_in_line(&mut self, mode: usize) {
        self.pending_wrap = false;
        let CursorPosition { column, row } = self.cursor;
        match mode {
            0 => {
                self.erase_cells(row, column, self.columns);
                self.row(row).wrapped = false;
            }
            1 => self.erase_cells(row, 0, column + 1),
            2 => self.erase_row(row),
            _ => {}
        }
    }

    /// IL (`insert`) and DL: only inside the scroll region, whose lower part shifts.
    fn insert_or_delete_lines(&mut self, count: usize, insert: bool) {
        let y = self.cursor.row;
        if y < self.scroll_top || y > self.scroll_bottom {
            return;
        }
        for _ in 0..count.min(self.scroll_bottom - y + 1) {
            if insert {
                self.rotate_rows(self.scroll_bottom, y);
            } else {
                self.rotate_rows(y, self.scroll_bottom);
            }
        }
        self.cursor.column = 0;
        self.pending_wrap = false;
    }

    /// ICH: blanks at the cursor, the rest of the line moves right and falls off the end.
    fn insert_chars(&mut self, count: usize) {
        let blank = self.blank();
        let x = self.cursor.column;
        let count = count.min(self.columns - x);
        let tail = &mut self.row(self.cursor.row).cells[x..];
        tail.rotate_right(count);
        tail[..count].fill(blank);
        blank_split_wide(&mut self.row(self.cursor.row).cells, blank);
        self.pending_wrap = false;
    }

    /// DCH: the rest of the line moves left over the cursor, blanks come in at the end.
    fn delete_chars(&mut self, count: usize) {
        let blank = self.blank();
        let x = self.cursor.column;
        let count = count.min(self.columns - x);
        let tail = &mut self.row(self.cursor.row).cells[x..];
        tail.rotate_left(count);
        let kept = tail.len() - count;
        tail[kept..].fill(blank);
        blank_split_wide(&mut self.row(self.cursor.row).cells, blank);
        self.pending_wrap = false;
    }

    /// Cursor addressing, zero-based: relative to and confined to the scroll region in origin mode.
    fn move_to(&mut self, x: usize, y: usize) {
        let (top, bottom) = if self.modes.contains(Modes::ORIGIN) {
            (self.scroll_top, self.scroll_bottom)
        } else {
            (0, self.rows - 1)
        };
        self.cursor.column = x.min(self.columns - 1);
        self.cursor.row = (y + top).min(bottom);
        self.pending_wrap = false;
    }

    const fn saved_cursor(&mut self) -> &mut SavedCursor {
        if self.alternate() {
            &mut self.saved_alt
        } else {
            &mut self.saved
        }
    }

    /// DECSC; each of the two screens has its own.
    fn save_cursor(&mut self) {
        *self.saved_cursor() = SavedCursor {
            position: self.cursor,
            pen: self.pen,
            pending_wrap: self.pending_wrap,
            origin_mode: self.modes.contains(Modes::ORIGIN),
        };
    }

    /// DECRC.
    fn restore_cursor(&mut self) {
        let saved = *self.saved_cursor();
        self.cursor.column = saved.position.column.min(self.columns - 1);
        self.cursor.row = saved.position.row.min(self.rows - 1);
        self.pen = saved.pen;
        self.pending_wrap = saved.pending_wrap;
        self.modes.set(Modes::ORIGIN, saved.origin_mode);
    }

    /// Enters (onto a blank screen) or leaves the alternate screen; `save_restore` is 1049's cursor save on the
    /// way in and restore on the way out.
    fn switch_alternate(&mut self, enable: bool, save_restore: bool) {
        if enable == self.alternate() {
            return;
        }
        if enable {
            if save_restore {
                self.save_cursor();
            }
            self.modes.insert(Modes::ALTERNATE_SCREEN);
            let blank = self.blank_line();
            self.alt.clear();
            self.alt.resize(self.rows, blank);
        } else {
            self.modes.remove(Modes::ALTERNATE_SCREEN);
            if save_restore {
                self.restore_cursor();
            }
        }
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
    }

    /// SM / RM.
    fn set_modes(&mut self, csi: &Csi, enable: bool) {
        for index in 0..csi.param_count().max(1) {
            let mode = csi.param(index, 0);
            match (csi.private(), mode) {
                (Some(b'?'), 1) => self.modes.set(Modes::APPLICATION_CURSOR_KEYS, enable),
                (Some(b'?'), 6) => {
                    self.modes.set(Modes::ORIGIN, enable);
                    self.move_to(0, 0);
                }
                (Some(b'?'), 7) => self.modes.set(Modes::AUTO_WRAP, enable),
                (Some(b'?'), 25) => self.modes.set(Modes::CURSOR_VISIBLE, enable),
                (Some(b'?'), 47 | 1047) => self.switch_alternate(enable, false),
                (Some(b'?'), 1048) => {
                    if enable {
                        self.save_cursor();
                    } else {
                        self.restore_cursor();
                    }
                }
                (Some(b'?'), 1049) => self.switch_alternate(enable, true),
                (Some(b'?'), 1004) => self.modes.set(Modes::FOCUS_REPORTING, enable),
                (Some(b'?'), 2004) => self.modes.set(Modes::BRACKETED_PASTE, enable),
                (None, 4) => self.modes.set(Modes::INSERT, enable),
                _ => {}
            }
        }
    }

    /// DECSTR: the pen, the modes that affect drawing and the scroll region; the contents stay.
    fn soft_reset(&mut self) {
        self.pen = Pen::default();
        self.modes
            .remove(Modes::INSERT | Modes::ORIGIN | Modes::APPLICATION_CURSOR_KEYS);
        self.modes.insert(Modes::AUTO_WRAP | Modes::CURSOR_VISIBLE);
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
    }

    /// SGR.
    fn select_graphic_rendition(&mut self, csi: &Csi) {
        if csi.param_count() == 0 {
            self.pen = Pen::default();
            return;
        }
        let mut index = 0;
        while index < csi.param_count() {
            let pen = &mut self.pen;
            match csi.param(index, 0) {
                0 => *pen = Pen::default(),
                1 => pen.attributes |= Attributes::BOLD,
                2 => pen.attributes |= Attributes::DIM,
                3 => pen.attributes |= Attributes::ITALIC,
                4 | 21 => pen.attributes |= Attributes::UNDERLINE,
                7 => pen.attributes |= Attributes::INVERSE,
                8 => pen.attributes |= Attributes::INVISIBLE,
                9 => pen.attributes |= Attributes::STRIKETHROUGH,
                22 => pen.attributes -= Attributes::BOLD | Attributes::DIM,
                23 => pen.attributes -= Attributes::ITALIC,
                24 => pen.attributes -= Attributes::UNDERLINE,
                27 => pen.attributes -= Attributes::INVERSE,
                28 => pen.attributes -= Attributes::INVISIBLE,
                29 => pen.attributes -= Attributes::STRIKETHROUGH,
                code @ 30..=37 => pen.fg = Color::Indexed((code - 30) as u8),
                38 => pen.fg = extended_color(csi, &mut index).unwrap_or(pen.fg),
                39 => pen.fg = Color::Default,
                code @ 40..=47 => pen.bg = Color::Indexed((code - 40) as u8),
                48 => pen.bg = extended_color(csi, &mut index).unwrap_or(pen.bg),
                49 => pen.bg = Color::Default,
                // Underline colour: not drawn, but its parameters are not codes.
                58 => {
                    extended_color(csi, &mut index);
                }
                code @ 90..=97 => pen.fg = Color::Indexed((code - 90 + 8) as u8),
                code @ 100..=107 => pen.bg = Color::Indexed((code - 100 + 8) as u8),
                _ => {}
            }
            // A code's sub-parameters (4:3 is a curly underline) are never codes of their own.
            while csi.is_sub_param(index + 1) {
                index += 1;
            }
            index += 1;
        }
    }
}

/// The colour of an SGR 38 / 48 / 58 at parameter `*index`. `38;5;n` and `38;2;r;g;b` take the parameters that
/// follow (`*index` moves past them); `38:5:n` and `38:2:[colour space]:r:g:b` only their own sub-parameters.
/// `None` when the kind is neither 5 nor 2.
fn extended_color(csi: &Csi, index: &mut usize) -> Option<Color> {
    let at = *index;
    let sub_params = (1..)
        .take_while(|offset| csi.is_sub_param(at + offset))
        .count();
    let colon = sub_params > 0;
    let value = |offset: usize| {
        if colon && offset > sub_params {
            0
        } else {
            csi.param(at + offset, 0)
        }
    };
    let channel = |offset: usize| value(offset).min(255) as u8;
    match value(1) {
        5 => {
            if !colon {
                *index += 2;
            }
            Some(Color::Indexed(channel(2)))
        }
        2 => {
            // 38:2::r:g:b carries an (empty) colour-space slot that 38:2:r:g:b and the ; form do not.
            let first = if colon && sub_params >= 5 { 3 } else { 2 };
            if !colon {
                *index += 4;
            }
            Some(Color::Rgb(
                channel(first),
                channel(first + 1),
                channel(first + 2),
            ))
        }
        _ => None,
    }
}

/// East Asian wide and emoji code points take two cells (wcwidth), combining marks none.
const fn char_width(ch: char) -> usize {
    match ch as u32 {
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0x20D0..=0x20FF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Blanks both halves of every wide character that an edit cut: a continuation cell with no wide character
/// before it, and a wide character with no continuation after it.
fn blank_split_wide(cells: &mut [Cell], blank: Cell) {
    let wide = |cell: &Cell| cell.ch.is_some_and(|ch| char_width(ch) == 2);
    for x in 0..cells.len() {
        let orphan_continuation = cells[x].ch.is_none() && (x == 0 || !wide(&cells[x - 1]));
        let orphan_lead = wide(&cells[x]) && cells.get(x + 1).is_none_or(|next| next.ch.is_some());
        if orphan_continuation || orphan_lead {
            cells[x] = blank;
        }
    }
}

impl Perform for Screen {
    fn print(&mut self, ch: char) {
        let width = char_width(ch);
        if width == 0 {
            return;
        }
        let auto_wrap = self.modes.contains(Modes::AUTO_WRAP);
        if self.pending_wrap && auto_wrap {
            self.row(self.cursor.row).wrapped = true;
            self.cursor.column = 0;
            self.line_feed();
        }
        self.pending_wrap = false;
        if width == 2 && self.cursor.column == self.columns - 1 {
            // A wide character does not fit in the last column: it goes to the next line.
            if !auto_wrap {
                return;
            }
            let CursorPosition { column, row } = self.cursor;
            self.erase_cells(row, column, column + 1);
            self.row(row).wrapped = true;
            self.cursor.column = 0;
            self.line_feed();
        }
        if self.modes.contains(Modes::INSERT) {
            self.insert_chars(width);
        }
        let Pen { fg, bg, attributes } = self.pen;
        let cell = |ch| Cell {
            ch,
            fg,
            bg,
            attributes,
        };
        let x = self.cursor.column;
        let cells = &mut self.row(self.cursor.row).cells;
        // Overwriting half of a wide character blanks the other half.
        if x > 0 && cells[x].ch.is_none() {
            cells[x - 1].ch = Some(' ');
        }
        cells[x] = cell(Some(ch));
        if let Some(next) = cells.get_mut(x + 1) {
            if width == 2 {
                *next = cell(None);
            } else if next.ch.is_none() {
                next.ch = Some(' ');
            }
        }
        self.cursor.column += width;
        if self.cursor.column >= self.columns {
            self.cursor.column = self.columns - 1;
            self.pending_wrap = auto_wrap;
        }
    }

    fn execute(&mut self, control: u8) {
        match control {
            // BS
            0x08 => {
                self.cursor.column = self.cursor.column.saturating_sub(1);
                self.pending_wrap = false;
            }
            // HT
            0x09 => self.tab(),
            // LF, VT, FF
            0x0A..=0x0C => self.line_feed(),
            // CR
            0x0D => {
                self.cursor.column = 0;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, final_byte: u8) {
        match final_byte {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            // IND
            b'D' => self.line_feed(),
            // NEL
            b'E' => {
                self.cursor.column = 0;
                self.line_feed();
            }
            // HTS
            b'H' => self.tab_stops[self.cursor.column] = true,
            // RI
            b'M' => self.reverse_index(),
            // RIS
            b'c' => self.reset(),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, csi: &Csi, final_byte: u8) {
        if csi.private().is_some() && !matches!(final_byte, b'h' | b'l' | b'c' | b'n') {
            return;
        }
        if !csi.intermediates().is_empty() {
            if csi.intermediates() == b"!" && final_byte == b'p' {
                self.soft_reset();
            }
            return; // DECSCUSR (cursor style) and others
        }
        // The count most sequences take: 1 when absent or 0.
        let n = csi.param(0, 1).max(1);
        let last_column = self.columns - 1;
        let last_row = self.rows - 1;
        let CursorPosition { column, row } = self.cursor;
        match final_byte {
            b'@' => self.insert_chars(n),
            // CUU, CUD (and VPR): stop at the scroll region's edge when starting inside it.
            b'A' => {
                let limit = if row >= self.scroll_top {
                    self.scroll_top
                } else {
                    0
                };
                self.cursor.row = row.saturating_sub(n).max(limit);
                self.pending_wrap = false;
            }
            b'B' | b'e' => {
                let limit = if row <= self.scroll_bottom {
                    self.scroll_bottom
                } else {
                    last_row
                };
                self.cursor.row = (row + n).min(limit);
                self.pending_wrap = false;
            }
            // CUF (and HPR), CUB
            b'C' | b'a' => {
                self.cursor.column = (column + n).min(last_column);
                self.pending_wrap = false;
            }
            b'D' => {
                self.cursor.column = column.saturating_sub(n);
                self.pending_wrap = false;
            }
            // CNL, CPL
            b'E' => {
                self.cursor = CursorPosition {
                    column: 0,
                    row: (row + n).min(last_row),
                };
                self.pending_wrap = false;
            }
            b'F' => {
                self.cursor = CursorPosition {
                    column: 0,
                    row: row.saturating_sub(n),
                };
                self.pending_wrap = false;
            }
            // CHA (and HPA)
            b'G' | b'`' => {
                self.cursor.column = (n - 1).min(last_column);
                self.pending_wrap = false;
            }
            // CUP (and HVP)
            b'H' | b'f' => self.move_to(csi.param(1, 1).saturating_sub(1), n - 1),
            // CHT
            b'I' => {
                for _ in 0..n {
                    self.tab();
                }
            }
            b'J' => self.erase_in_display(csi.param(0, 0)),
            b'K' => self.erase_in_line(csi.param(0, 0)),
            b'L' => self.insert_or_delete_lines(n, true),
            b'M' => self.insert_or_delete_lines(n, false),
            b'P' => self.delete_chars(n),
            b'S' => self.scroll_up(n),
            b'T' => self.scroll_down(n),
            // ECH
            b'X' => {
                self.erase_cells(row, column, column + n);
                self.pending_wrap = false;
            }
            // CBT
            b'Z' => self.back_tab(n),
            // DA1, DA2
            b'c' => match csi.private() {
                None if csi.param(0, 0) == 0 => self.replies += "\x1b[?1;2c",
                Some(b'>') => self.replies += "\x1b[>0;276;0c",
                _ => {}
            },
            // VPA
            b'd' => self.move_to(column, n - 1),
            // TBC
            b'g' => match csi.param(0, 0) {
                0 => self.tab_stops[column] = false,
                3 => self.tab_stops.fill(false),
                _ => {}
            },
            b'h' => self.set_modes(csi, true),
            b'l' => self.set_modes(csi, false),
            b'm' => self.select_graphic_rendition(csi),
            // DSR: status, cursor position
            b'n' => match csi.param(0, 0) {
                5 if csi.private().is_none() => self.replies += "\x1b[0n",
                6 => self.replies += &format!("\x1b[{};{}R", row + 1, column + 1),
                _ => {}
            },
            // DECSTBM
            b'r' => {
                // An explicit 0 means the default for either end, as in xterm.
                let top = csi.param(0, 0).max(1) - 1;
                let bottom = match csi.param(1, 0) {
                    0 => self.rows,
                    requested => requested.min(self.rows),
                } - 1;
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    self.move_to(0, 0);
                }
            }
            b's' => self.save_cursor(),
            b'u' => self.restore_cursor(),
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, payload: &[u8]) {
        // OSC 0 (icon name and title) and OSC 2 (title).
        let Some(separator) = payload.iter().position(|&byte| byte == b';') else {
            return;
        };
        if matches!(&payload[..separator], b"0" | b"2") {
            self.title = String::from_utf8_lossy(&payload[separator + 1..]).into_owned();
        }
    }
}
