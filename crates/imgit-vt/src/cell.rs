//! What the grid is made of: colours, attributes, cells and lines.

use bitflags::bitflags;

/// A colour as the program selected it; the renderer resolves it against its theme and palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// The terminal's default foreground or background.
    #[default]
    Default,
    /// An xterm palette index: 0..=7 normal, 8..=15 bright, 16..=231 the colour cube, 232..=255 greys.
    Indexed(u8),
    /// 24-bit red, green, blue.
    Rgb(u8, u8, u8),
}

bitflags! {
    /// The rendition of a cell (SGR).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Attributes: u8 {
        const BOLD = 1;
        const DIM = 1 << 1;
        const ITALIC = 1 << 2;
        const UNDERLINE = 1 << 3;
        const INVERSE = 1 << 4;
        const INVISIBLE = 1 << 5;
        const STRIKETHROUGH = 1 << 6;
    }
}

/// One cell of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    /// The character, or `None` for the second half of a wide character (which lives in the cell before).
    pub ch: Option<char>,
    pub fg: Color,
    pub bg: Color,
    pub attributes: Attributes,
}

impl Cell {
    /// Whether this is the second half of a wide character.
    #[must_use]
    pub const fn is_wide_continuation(&self) -> bool {
        self.ch.is_none()
    }

    /// An erased cell: a space on the given background.
    pub(crate) const fn blank(bg: Color) -> Self {
        Self {
            ch: Some(' '),
            fg: Color::Default,
            bg,
            attributes: Attributes::empty(),
        }
    }
}

impl Default for Cell {
    /// A space in the default colours.
    fn default() -> Self {
        Self::blank(Color::Default)
    }
}

/// A line of the buffer: exactly [`Screen::columns`](crate::Screen::columns) cells.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    pub(crate) cells: Vec<Cell>,
    pub(crate) wrapped: bool,
}

impl Line {
    pub(crate) fn filled(columns: usize, cell: Cell) -> Self {
        Self {
            cells: vec![cell; columns],
            wrapped: false,
        }
    }

    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Whether the line continues onto the next one (a soft wrap).
    #[must_use]
    pub const fn is_wrapped(&self) -> bool {
        self.wrapped
    }

    /// The characters of the line, trailing blanks included.
    #[must_use]
    pub fn text(&self) -> String {
        self.cells.iter().filter_map(|cell| cell.ch).collect()
    }

    /// Whether nothing was drawn on the line: only spaces on the default background.
    pub(crate) fn is_blank(&self) -> bool {
        self.cells
            .iter()
            .all(|cell| matches!(cell.ch, Some(' ') | None) && cell.bg == Color::Default)
    }
}
