//! The screen model of a terminal emulator: a VT/xterm byte stream (what a pseudo console emits) applied to a grid
//! of cells with scrollback, like xterm.js's buffer.
//!
//! [`Screen::feed`] takes the program's output; the terminal panel then reads the [`Line`]s (scrollback first, the
//! last [`Screen::rows`] of them being the visible screen), the cursor, the [`Modes`] that change how input is
//! encoded, the window title, and the replies the program asked for ([`Screen::take_replies`]).
//!
//! ```
//! use imgit_vt::{Color, Screen};
//!
//! let mut screen = Screen::new(80, 24, 1000);
//! screen.feed(b"\x1b[31mred\x1b[0m\x1b[6n");
//! assert_eq!(screen.screen_line(0).text().trim_end(), "red");
//! assert_eq!(screen.screen_line(0).cells()[0].fg, Color::Indexed(1));
//! assert_eq!(screen.take_replies(), "\x1b[1;4R");
//! ```

mod cell;
mod parser;
mod screen;
#[cfg(test)]
mod tests;

pub use cell::{Attributes, Cell, Color, Line};
pub use screen::{CursorPosition, Modes, Point, Screen};
