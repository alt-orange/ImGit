use crate::{Attributes, Cell, Color, CursorPosition, Modes, Point, Screen};

fn fed(columns: usize, rows: usize, input: &str) -> Screen {
    let mut screen = Screen::new(columns, rows, 100);
    screen.feed(input.as_bytes());
    screen
}

/// The visible rows without their trailing blanks.
fn rows(screen: &Screen) -> Vec<String> {
    screen
        .screen_lines()
        .map(|line| line.text().trim_end().to_owned())
        .collect()
}

/// Every line of the buffer, scrollback first, without trailing blanks.
fn lines(screen: &Screen) -> Vec<String> {
    screen
        .lines()
        .map(|line| line.text().trim_end().to_owned())
        .collect()
}

/// The cursor as (column, row).
fn at(screen: &Screen) -> (usize, usize) {
    let CursorPosition { column, row } = screen.cursor();
    (column, row)
}

fn cell(screen: &Screen, column: usize, row: usize) -> Cell {
    screen.screen_line(row).cells()[column]
}

/// The cell a character printed after `input` ends up as.
fn pen(input: &str) -> Cell {
    let screen = fed(20, 3, &format!("{input}x"));
    cell(&screen, 0, 0)
}

const FIVE_ROWS: &str = "A\r\nB\r\nC\r\nD\r\nE";

// Printing and the C0 controls.

#[test]
fn a_new_screen_is_blank() {
    let screen = Screen::new(7, 3, 10);
    assert_eq!((screen.columns(), screen.rows()), (7, 3));
    assert_eq!(screen.line_count(), 3);
    assert_eq!(screen.first_line_id(), 0);
    assert_eq!(screen.scrolled_lines(), 0);
    assert_eq!(at(&screen), (0, 0));
    assert_eq!(screen.modes(), Modes::AUTO_WRAP | Modes::CURSOR_VISIBLE);
    assert_eq!(screen.title(), "");
    assert!(
        screen
            .lines()
            .all(|line| line.cells() == [Cell::default(); 7] && !line.is_wrapped())
    );
    let default = Screen::default();
    assert_eq!((default.columns(), default.rows()), (80, 24));
}

#[test]
fn plain_text_advances_the_cursor() {
    let screen = fed(10, 3, "hello");
    assert_eq!(rows(&screen), ["hello", "", ""]);
    assert_eq!(at(&screen), (5, 0));
    assert_eq!(
        cell(&screen, 0, 0),
        Cell {
            ch: Some('h'),
            fg: Color::Default,
            bg: Color::Default,
            attributes: Attributes::empty()
        }
    );
}

#[test]
fn the_wrap_is_deferred_until_the_next_character() {
    let mut screen = fed(5, 3, "abcde");
    assert_eq!(at(&screen), (4, 0));
    assert!(!screen.screen_line(0).is_wrapped());
    screen.feed(b"fg");
    assert_eq!(rows(&screen), ["abcde", "fg", ""]);
    assert!(screen.screen_line(0).is_wrapped());
    assert!(!screen.screen_line(1).is_wrapped());
    assert_eq!(at(&screen), (2, 1));
}

#[test]
fn a_carriage_return_cancels_the_pending_wrap() {
    let screen = fed(5, 3, "abcde\r\nf");
    assert_eq!(rows(&screen), ["abcde", "f", ""]);
    assert!(!screen.screen_line(0).is_wrapped());
}

#[test]
fn without_auto_wrap_the_last_column_is_overwritten() {
    let screen = fed(5, 3, "\x1b[?7labcdefg");
    assert_eq!(rows(&screen), ["abcdg", "", ""]);
    assert_eq!(at(&screen), (4, 0));
    assert!(!screen.modes().contains(Modes::AUTO_WRAP));
    let screen = fed(5, 3, "\x1b[?7l\x1b[?7habcdefg");
    assert_eq!(rows(&screen), ["abcde", "fg", ""]);
}

#[test]
fn line_feeds_keep_the_column() {
    assert_eq!(rows(&fed(10, 3, "ab\r\ncd")), ["ab", "cd", ""]);
    for feed in ['\n', '\x0b', '\x0c'] {
        let screen = fed(10, 3, &format!("ab{feed}cd"));
        assert_eq!(rows(&screen), ["ab", "  cd", ""]);
        assert_eq!(at(&screen), (4, 1));
    }
}

#[test]
fn backspace_moves_left_without_erasing() {
    let screen = fed(10, 3, "abc\x08\x08X");
    assert_eq!(rows(&screen)[0], "aXc");
    assert_eq!(at(&fed(10, 3, "\x08\x08")), (0, 0));
    // From a pending wrap it steps off the last column.
    let screen = fed(5, 3, "abcde\x08X");
    assert_eq!(rows(&screen), ["abcXe", "", ""]);
    assert_eq!(at(&screen), (4, 0));
}

#[test]
fn tabs_stop_every_eight_columns() {
    let screen = fed(20, 3, "a\tb");
    assert_eq!(cell(&screen, 8, 0).ch, Some('b'));
    assert_eq!(at(&fed(20, 3, "\t\t")), (16, 0));
    // Past the last stop: the last column.
    assert_eq!(at(&fed(20, 3, "\t\t\t")), (19, 0));
    assert_eq!(at(&fed(20, 3, "\t\t\t\t")), (19, 0));
    // Tabbing does not erase.
    assert_eq!(rows(&fed(20, 3, "abcdefghij\r\tX"))[0], "abcdefghXj");
}

#[test]
fn tab_stops_can_be_set_and_cleared() {
    // HTS at column 3.
    assert_eq!(at(&fed(20, 3, "abc\x1bH\r\t")), (3, 0));
    // TBC at the cursor: the stop at 8 goes, 16 is next.
    assert_eq!(at(&fed(20, 3, "\t\x1b[g\r\t")), (16, 0));
    assert_eq!(at(&fed(20, 3, "\t\x1b[0g\r\t")), (16, 0));
    // TBC 3: all of them.
    assert_eq!(at(&fed(20, 3, "\x1b[3g\t")), (19, 0));
    // Other TBC modes do nothing.
    assert_eq!(at(&fed(20, 3, "\x1b[1g\t")), (8, 0));
}

#[test]
fn forward_and_backward_tabulation() {
    assert_eq!(at(&fed(40, 3, "\x1b[2I")), (16, 0));
    assert_eq!(at(&fed(40, 3, "\x1b[I")), (8, 0));
    assert_eq!(at(&fed(40, 3, "\x1b[1;21H\x1b[Z")), (16, 0));
    assert_eq!(at(&fed(40, 3, "\x1b[1;21H\x1b[2Z")), (8, 0));
    assert_eq!(at(&fed(40, 3, "\x1b[1;17H\x1b[Z")), (8, 0));
    assert_eq!(at(&fed(40, 3, "\x1b[1;4H\x1b[Z")), (0, 0));
    assert_eq!(at(&fed(40, 3, "\x1b[1;21H\x1b[99Z")), (0, 0));
}

#[test]
fn delete_and_other_controls_are_ignored() {
    let screen = fed(10, 3, "a\x7fb\x00\x07\x0e\x0fc");
    assert_eq!(rows(&screen)[0], "abc");
    assert_eq!(at(&screen), (3, 0));
}

// Cursor movement.

#[test]
fn cursor_position() {
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H")), (4, 2));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5f")), (4, 2));
    assert_eq!(at(&fed(10, 5, "abc\x1b[H")), (0, 0));
    assert_eq!(at(&fed(10, 5, "abc\x1b[0;0H")), (0, 0));
    assert_eq!(at(&fed(10, 5, "\x1b[99;99H")), (9, 4));
    assert_eq!(at(&fed(10, 5, "\x1b[;5H")), (4, 0));
    assert_eq!(at(&fed(10, 5, "\x1b[3H")), (0, 2));
    assert_eq!(at(&fed(10, 5, "\x1b[3;H")), (0, 2));
    // Parameters saturate rather than overflow.
    assert_eq!(
        at(&fed(
            10,
            5,
            "\x1b[99999999999999999999;99999999999999999999H"
        )),
        (9, 4)
    );
}

#[test]
fn relative_cursor_movement() {
    let home = "\x1b[3;5H";
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[A"))), (4, 1));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[0A"))), (4, 1));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[2A"))), (4, 0));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[9A"))), (4, 0));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[B"))), (4, 3));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[9B"))), (4, 4));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[2e"))), (4, 4));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[C"))), (5, 2));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[3a"))), (7, 2));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[99C"))), (9, 2));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[D"))), (3, 2));
    assert_eq!(at(&fed(10, 5, &format!("{home}\x1b[99D"))), (0, 2));
}

#[test]
fn cursor_next_and_previous_line() {
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[E")), (0, 3));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[9E")), (0, 4));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[2F")), (0, 0));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[9F")), (0, 0));
}

#[test]
fn absolute_column_and_row() {
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[7G")), (6, 2));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[G")), (0, 2));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[99`")), (9, 2));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[d")), (4, 0));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[5d")), (4, 4));
    assert_eq!(at(&fed(10, 5, "\x1b[3;5H\x1b[99d")), (4, 4));
}

#[test]
fn cursor_movement_cancels_the_pending_wrap() {
    for movement in [
        "\x1b[D",
        "\x1b[C",
        "\x1b[A",
        "\x1b[5G",
        "\x1b[1;5H",
        "\x1b[K",
        "\x1b[X",
    ] {
        let screen = fed(5, 3, &format!("\n\nabcde{movement}Z"));
        assert_eq!(
            screen.scrolled_lines(),
            0,
            "{movement:?} left the wrap pending"
        );
    }
    // Cursor back from the pending wrap: one column left of the last.
    assert_eq!(rows(&fed(5, 3, "abcde\x1b[DZ"))[0], "abcZe");
}

#[test]
fn vertical_movement_stops_at_the_scroll_region() {
    let region = "\x1b[3;6r";
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[5;1H\x1b[9A"))),
        (0, 2)
    );
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[5;1H\x1b[9B"))),
        (0, 5)
    );
    // From below the region up, and from above it down, the region's edge still stops the cursor.
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[10;1H\x1b[9A"))),
        (0, 2)
    );
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[1;1H\x1b[9B"))),
        (0, 5)
    );
    // Moving away from it, the screen's edge does.
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[2;1H\x1b[9A"))),
        (0, 0)
    );
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[8;1H\x1b[9B"))),
        (0, 9)
    );
    // CNL and CPL only know the screen.
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[5;1H\x1b[9E"))),
        (0, 9)
    );
    assert_eq!(
        at(&fed(10, 10, &format!("{region}\x1b[5;1H\x1b[9F"))),
        (0, 0)
    );
}

#[test]
fn save_and_restore_cursor() {
    for (save, restore) in [
        ("\x1b7", "\x1b8"),
        ("\x1b[s", "\x1b[u"),
        ("\x1b[?1048h", "\x1b[?1048l"),
    ] {
        let mut screen = fed(
            10,
            5,
            &format!("ab\x1b[31;44;1m{save}\x1b[5;5H\x1b[0m{restore}"),
        );
        assert_eq!(at(&screen), (2, 0));
        screen.feed(b"X");
        assert_eq!(
            cell(&screen, 2, 0),
            Cell {
                ch: Some('X'),
                fg: Color::Indexed(1),
                bg: Color::Indexed(4),
                attributes: Attributes::BOLD
            }
        );
    }
}

#[test]
fn restoring_without_a_save_goes_home_with_the_default_pen() {
    let mut screen = fed(10, 5, "\x1b[3;3H\x1b[31m\x1b8");
    assert_eq!(at(&screen), (0, 0));
    screen.feed(b"X");
    assert_eq!(cell(&screen, 0, 0).fg, Color::Default);
}

#[test]
fn the_saved_cursor_carries_origin_mode_and_the_pending_wrap() {
    let screen = fed(10, 10, "\x1b[3;6r\x1b[?6h\x1b7\x1b[?6l\x1b8\x1b[1;1H");
    assert!(screen.modes().contains(Modes::ORIGIN));
    assert_eq!(at(&screen), (0, 2));
    let screen = fed(5, 3, "abcde\x1b7\x1b[1;1H\x1b8f");
    assert_eq!(rows(&screen), ["abcde", "f", ""]);
}

#[test]
fn the_restored_cursor_is_clamped_to_a_smaller_screen() {
    let mut screen = fed(10, 5, "\x1b[5;10H\x1b7");
    screen.resize(4, 2);
    screen.feed(b"\x1b8");
    assert_eq!(at(&screen), (3, 1));
}

// Erasing.

#[test]
fn erase_in_line() {
    let at_c = "abcdef\x1b[1;3H";
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[K")))[0], "ab");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[0K")))[0], "ab");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[1K")))[0], "   def");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[2K")))[0], "");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[3K")))[0], "abcdef");
    assert_eq!(at(&fed(8, 2, &format!("{at_c}\x1b[2K"))), (2, 0));
}

#[test]
fn erased_cells_keep_the_background_only() {
    let screen = fed(8, 2, "abcdef\x1b[1;31;41m\x1b[1;3H\x1b[K");
    let erased = Cell {
        ch: Some(' '),
        fg: Color::Default,
        bg: Color::Indexed(1),
        attributes: Attributes::empty(),
    };
    assert_eq!(screen.screen_line(0).cells()[2..], [erased; 6]);
    assert_eq!(cell(&screen, 1, 0).bg, Color::Default);
    // Lines that scroll in do too.
    let screen = fed(4, 2, "\x1b[44m\n\n");
    assert_eq!(
        screen.screen_line(1).cells(),
        [Cell {
            bg: Color::Indexed(4),
            ..Cell::default()
        }; 4]
    );
}

#[test]
fn erasing_to_the_end_of_a_line_unwraps_it() {
    let mut screen = fed(5, 3, "abcdefg");
    assert!(screen.screen_line(0).is_wrapped());
    screen.feed(b"\x1b[1;3H\x1b[1K");
    assert!(screen.screen_line(0).is_wrapped());
    screen.feed(b"\x1b[K");
    assert!(!screen.screen_line(0).is_wrapped());
}

#[test]
fn erase_in_display() {
    let filled = "aaaaa\r\nbbbbb\r\nccccc\x1b[2;3H";
    assert_eq!(
        rows(&fed(5, 3, &format!("{filled}\x1b[J"))),
        ["aaaaa", "bb", ""]
    );
    assert_eq!(
        rows(&fed(5, 3, &format!("{filled}\x1b[0J"))),
        ["aaaaa", "bb", ""]
    );
    assert_eq!(
        rows(&fed(5, 3, &format!("{filled}\x1b[1J"))),
        ["", "   bb", "ccccc"]
    );
    let screen = fed(5, 3, &format!("{filled}\x1b[2J"));
    assert_eq!(rows(&screen), ["", "", ""]);
    assert_eq!(at(&screen), (2, 1));
    assert_eq!(screen.line_count(), 3);
}

#[test]
fn erase_scrollback() {
    let mut screen = fed(5, 3, "1\r\n2\r\n3\r\n4\r\n5");
    assert_eq!(lines(&screen), ["1", "2", "3", "4", "5"]);
    screen.feed(b"\x1b[3J");
    assert_eq!(lines(&screen), ["3", "4", "5"]);
    assert_eq!(screen.first_line_id(), 2);
    assert_eq!(screen.scrolled_lines(), 2);
    assert_eq!(at(&screen), (1, 2));
}

#[test]
fn insert_delete_and_erase_characters() {
    let at_c = "abcdef\x1b[1;3H";
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[2@")))[0], "ab  cdef");
    assert_eq!(rows(&fed(6, 2, &format!("{at_c}\x1b[2@")))[0], "ab  cd");
    assert_eq!(rows(&fed(6, 2, &format!("{at_c}\x1b[@")))[0], "ab cde");
    assert_eq!(rows(&fed(6, 2, &format!("{at_c}\x1b[99@")))[0], "ab");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[2P")))[0], "abef");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[P")))[0], "abdef");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[99P")))[0], "ab");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[2X")))[0], "ab  ef");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[X")))[0], "ab def");
    assert_eq!(rows(&fed(8, 2, &format!("{at_c}\x1b[99X")))[0], "ab");
    for edit in ["\x1b[2@", "\x1b[2P", "\x1b[2X"] {
        assert_eq!(at(&fed(8, 2, &format!("{at_c}{edit}"))), (2, 0));
    }
    // What comes in is blank on the current background.
    let screen = fed(6, 2, &format!("{at_c}\x1b[1;31;42m\x1b[P"));
    assert_eq!(cell(&screen, 5, 0), Cell::blank(Color::Indexed(2)));
    let screen = fed(6, 2, &format!("{at_c}\x1b[1;31;42m\x1b[@"));
    assert_eq!(cell(&screen, 2, 0), Cell::blank(Color::Indexed(2)));
    let screen = fed(6, 2, &format!("{at_c}\x1b[1;31;42m\x1b[X"));
    assert_eq!(cell(&screen, 2, 0), Cell::blank(Color::Indexed(2)));
}

#[test]
fn insert_and_delete_lines() {
    let screen = fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;2H\x1b[L"));
    assert_eq!(rows(&screen), ["A", "", "B", "C", "D"]);
    assert_eq!(at(&screen), (0, 1));
    assert_eq!(screen.line_count(), 5);
    assert_eq!(
        rows(&fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;2H\x1b[99L"))),
        ["A", "", "", "", ""]
    );
    let screen = fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;2H\x1b[2M"));
    assert_eq!(rows(&screen), ["A", "D", "E", "", ""]);
    assert_eq!(at(&screen), (0, 1));
    assert_eq!(
        rows(&fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;2H\x1b[99M"))),
        ["A", "", "", "", ""]
    );
}

#[test]
fn insert_and_delete_lines_stay_inside_the_scroll_region() {
    let region = format!("{FIVE_ROWS}\x1b[2;4r");
    assert_eq!(
        rows(&fed(5, 5, &format!("{region}\x1b[3;1H\x1b[L"))),
        ["A", "B", "", "C", "E"]
    );
    assert_eq!(
        rows(&fed(5, 5, &format!("{region}\x1b[2;1H\x1b[M"))),
        ["A", "C", "D", "", "E"]
    );
    assert_eq!(
        rows(&fed(5, 5, &format!("{region}\x1b[2;1H\x1b[99M"))),
        ["A", "", "", "", "E"]
    );
    // Outside the region nothing happens, not even the carriage return.
    for edit in ["\x1b[L", "\x1b[M"] {
        let screen = fed(5, 5, &format!("{region}\x1b[5;3H{edit}"));
        assert_eq!(rows(&screen), ["A", "B", "C", "D", "E"]);
        assert_eq!(at(&screen), (2, 4));
    }
}

// Scrolling.

#[test]
fn lines_scroll_into_the_scrollback() {
    let screen = fed(5, 3, "1\r\n2\r\n3\r\n4");
    assert_eq!(lines(&screen), ["1", "2", "3", "4"]);
    assert_eq!(rows(&screen), ["2", "3", "4"]);
    assert_eq!(screen.line_count(), 4);
    assert_eq!(screen.scrolled_lines(), 1);
    assert_eq!(screen.first_line_id(), 0);
    assert_eq!(at(&screen), (1, 2));
    assert_eq!(
        screen.line(0).map(|line| line.text()),
        Some("1    ".to_owned())
    );
    assert!(screen.line(4).is_none());
}

#[test]
fn the_scrollback_is_limited_and_line_ids_stay_stable() {
    let mut screen = Screen::new(10, 2, 3);
    screen.feed(b"0\r\n1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9");
    assert_eq!(lines(&screen), ["5", "6", "7", "8", "9"]);
    assert_eq!(screen.scrolled_lines(), 8);
    assert_eq!(screen.first_line_id(), 5);
    assert_eq!(
        screen
            .line_by_id(7)
            .map(|line| line.text().trim_end().to_owned()),
        Some("7".to_owned())
    );
    assert!(screen.line_by_id(4).is_none());
    assert!(screen.line_by_id(10).is_none());

    let mut screen = Screen::new(5, 2, 0);
    screen.feed(b"1\r\n2\r\n3");
    assert_eq!(lines(&screen), ["2", "3"]);
    assert_eq!(screen.first_line_id(), 1);
}

#[test]
fn a_wrapped_line_scrolls_like_any_other() {
    let screen = fed(3, 2, "abcdefgh");
    assert_eq!(lines(&screen), ["abc", "def", "gh"]);
    assert!(screen.line(0).is_some_and(|line| line.is_wrapped()));
    assert!(screen.line(1).is_some_and(|line| line.is_wrapped()));
}

#[test]
fn a_scroll_region_scrolls_alone() {
    let screen = fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;4r\x1b[4;1H\n"));
    assert_eq!(rows(&screen), ["A", "C", "D", "", "E"]);
    assert_eq!(screen.line_count(), 5);
    assert_eq!(screen.scrolled_lines(), 0);
    assert_eq!(at(&screen), (0, 3));
}

#[test]
fn a_region_at_the_top_still_feeds_the_scrollback() {
    let screen = fed(5, 5, &format!("{FIVE_ROWS}\x1b[1;3r\x1b[3;1H\n"));
    assert_eq!(lines(&screen), ["A", "B", "C", "", "D", "E"]);
    assert_eq!(rows(&screen), ["B", "C", "", "D", "E"]);
    assert_eq!(screen.scrolled_lines(), 1);
}

#[test]
fn a_line_feed_below_the_region_does_not_scroll() {
    let screen = fed(5, 5, &format!("{FIVE_ROWS}\x1b[1;3r\x1b[5;1H\n\n"));
    assert_eq!(rows(&screen), ["A", "B", "C", "D", "E"]);
    assert_eq!(at(&screen), (0, 4));
    assert_eq!(screen.scrolled_lines(), 0);
}

#[test]
fn setting_the_scroll_region() {
    // It homes the cursor.
    assert_eq!(at(&fed(5, 5, "\x1b[3;3H\x1b[2;4r")), (0, 0));
    // Invalid ones are ignored altogether.
    for region in ["\x1b[3;3r", "\x1b[4;2r", "\x1b[9;10r"] {
        let screen = fed(
            5,
            5,
            &format!("{FIVE_ROWS}\x1b[2;4r\x1b[3;3H{region}\x1b[4;1H\n"),
        );
        assert_eq!(rows(&screen), ["A", "C", "D", "", "E"], "{region:?}");
    }
    // No parameters, zeros, or a bottom past the screen: down to the last row.
    for region in ["\x1b[r", "\x1b[0;0r", "\x1b[1;99r", "\x1b[;5r"] {
        let screen = fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;4r{region}\x1b[5;1H\n"));
        assert_eq!(rows(&screen), ["B", "C", "D", "E", ""], "{region:?}");
        assert_eq!(screen.scrolled_lines(), 1);
    }
    let screen = fed(5, 5, &format!("{FIVE_ROWS}\x1b[3;99r\x1b[5;1H\n"));
    assert_eq!(rows(&screen), ["A", "B", "D", "E", ""]);
}

#[test]
fn scroll_up_and_down() {
    let three = "A\r\nB\r\nC";
    let screen = fed(5, 3, &format!("{three}\x1b[S"));
    assert_eq!(lines(&screen), ["A", "B", "C", ""]);
    assert_eq!(screen.scrolled_lines(), 1);
    assert_eq!(at(&screen), (1, 2));
    let screen = fed(5, 3, &format!("{three}\x1b[99S"));
    assert_eq!(lines(&screen), ["A", "B", "C", "", "", ""]);
    let screen = fed(5, 3, &format!("{three}\x1b[T"));
    assert_eq!(lines(&screen), ["", "A", "B"]);
    assert_eq!(screen.scrolled_lines(), 0);
    assert_eq!(lines(&fed(5, 3, &format!("{three}\x1b[2T"))), ["", "", "A"]);
    assert_eq!(lines(&fed(5, 3, &format!("{three}\x1b[99T"))), ["", "", ""]);
    // Within a region.
    assert_eq!(
        rows(&fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;4r\x1b[S"))),
        ["A", "C", "D", "", "E"]
    );
    assert_eq!(
        rows(&fed(5, 5, &format!("{FIVE_ROWS}\x1b[2;4r\x1b[T"))),
        ["A", "", "B", "C", "E"]
    );
}

#[test]
fn index_next_line_and_reverse_index() {
    assert_eq!(rows(&fed(5, 3, "ab\x1bDcd")), ["ab", "  cd", ""]);
    assert_eq!(rows(&fed(5, 3, "ab\x1bEcd")), ["ab", "cd", ""]);
    assert_eq!(at(&fed(5, 3, "\n\n\x1bM")), (0, 1));
    let screen = fed(5, 3, "A\r\nB\r\nC\x1b[1;2H\x1bM");
    assert_eq!(lines(&screen), ["", "A", "B"]);
    assert_eq!(at(&screen), (1, 0));
    // At the top of a region the region scrolls; above it nothing does.
    assert_eq!(
        rows(&fed(5, 3, "A\r\nB\r\nC\x1b[2;3r\x1b[2;1H\x1bM")),
        ["A", "", "B"]
    );
    assert_eq!(
        rows(&fed(5, 3, "A\r\nB\r\nC\x1b[2;3r\x1bM")),
        ["A", "B", "C"]
    );
    // IND and NEL scroll at the bottom.
    assert_eq!(fed(5, 3, "\n\n\x1bD\x1bE").scrolled_lines(), 2);
}

#[test]
fn origin_mode_addresses_the_scroll_region() {
    let mut screen = fed(10, 10, "\x1b[3;6r\x1b[?6h");
    assert!(screen.modes().contains(Modes::ORIGIN));
    assert_eq!(at(&screen), (0, 2));
    screen.feed(b"\x1b[2;4H");
    assert_eq!(at(&screen), (3, 3));
    // The report is in screen coordinates.
    screen.feed(b"\x1b[6n");
    assert_eq!(screen.take_replies(), "\x1b[4;4R");
    screen.feed(b"\x1b[99;1H");
    assert_eq!(at(&screen), (0, 5));
    screen.feed(b"\x1b[1d");
    assert_eq!(at(&screen), (0, 2));
    screen.feed(b"\x1b[5C\x1b[?6l");
    assert_eq!(at(&screen), (0, 0));
    screen.feed(b"\x1b[2;4H");
    assert_eq!(at(&screen), (3, 1));
}

// Colours and attributes.

#[test]
fn sgr_basic_colours() {
    assert_eq!(pen("\x1b[31;42m").fg, Color::Indexed(1));
    assert_eq!(pen("\x1b[31;42m").bg, Color::Indexed(2));
    assert_eq!(pen("\x1b[30m").fg, Color::Indexed(0));
    assert_eq!(pen("\x1b[37;47m").bg, Color::Indexed(7));
    assert_eq!(pen("\x1b[90m").fg, Color::Indexed(8));
    assert_eq!(pen("\x1b[97m").fg, Color::Indexed(15));
    assert_eq!(pen("\x1b[100m").bg, Color::Indexed(8));
    assert_eq!(pen("\x1b[107m").bg, Color::Indexed(15));
    assert_eq!(pen("\x1b[31;42m\x1b[39m").fg, Color::Default);
    assert_eq!(pen("\x1b[31;42m\x1b[39m").bg, Color::Indexed(2));
    assert_eq!(pen("\x1b[31;42m\x1b[49m").bg, Color::Default);
    assert_eq!(pen("\x1b[31;42m\x1b[49m").fg, Color::Indexed(1));
}

#[test]
fn sgr_256_colours() {
    assert_eq!(pen("\x1b[38;5;200m").fg, Color::Indexed(200));
    assert_eq!(pen("\x1b[48;5;17m").bg, Color::Indexed(17));
    assert_eq!(pen("\x1b[38;5;999m").fg, Color::Indexed(255));
    // The index missing: 0.
    assert_eq!(pen("\x1b[38;5m").fg, Color::Indexed(0));
    // The codes after it still apply, the colour's parameters do not.
    let cell = pen("\x1b[38;5;4;1m");
    assert_eq!(
        (cell.fg, cell.attributes),
        (Color::Indexed(4), Attributes::BOLD)
    );
    let cell = pen("\x1b[1;38;5;4;48;5;7;9m");
    assert_eq!(
        (cell.fg, cell.bg, cell.attributes),
        (
            Color::Indexed(4),
            Color::Indexed(7),
            Attributes::BOLD | Attributes::STRIKETHROUGH
        )
    );
}

#[test]
fn sgr_true_colour() {
    assert_eq!(pen("\x1b[38;2;1;2;3m").fg, Color::Rgb(1, 2, 3));
    assert_eq!(pen("\x1b[48;2;255;128;0m").bg, Color::Rgb(255, 128, 0));
    assert_eq!(pen("\x1b[38;2;300;0;999m").fg, Color::Rgb(255, 0, 255));
    assert_eq!(pen("\x1b[38;2;1m").fg, Color::Rgb(1, 0, 0));
    let cell = pen("\x1b[38;2;1;2;3;4m");
    assert_eq!(
        (cell.fg, cell.attributes),
        (Color::Rgb(1, 2, 3), Attributes::UNDERLINE)
    );
    // Neither 5 nor 2: nothing changes, and the kind is read as a code of its own.
    let cell = pen("\x1b[31m\x1b[38;9m");
    assert_eq!(
        (cell.fg, cell.attributes),
        (Color::Indexed(1), Attributes::STRIKETHROUGH)
    );
    assert_eq!(pen("\x1b[31m\x1b[38m").fg, Color::Indexed(1));
}

#[test]
fn sgr_colon_sub_parameters() {
    assert_eq!(pen("\x1b[38:5:200m").fg, Color::Indexed(200));
    assert_eq!(pen("\x1b[38:2:1:2:3m").fg, Color::Rgb(1, 2, 3));
    // With the colour-space slot, empty or not.
    assert_eq!(pen("\x1b[38:2::1:2:3m").fg, Color::Rgb(1, 2, 3));
    assert_eq!(pen("\x1b[48:2:0:1:2:3m").bg, Color::Rgb(1, 2, 3));
    // A colon form takes only its own sub-parameters.
    let cell = pen("\x1b[38:5:4;1m");
    assert_eq!(
        (cell.fg, cell.attributes),
        (Color::Indexed(4), Attributes::BOLD)
    );
    let cell = pen("\x1b[38:2::1:2:3;3m");
    assert_eq!(
        (cell.fg, cell.attributes),
        (Color::Rgb(1, 2, 3), Attributes::ITALIC)
    );
    let cell = pen("\x1b[38:5;1m");
    assert_eq!(
        (cell.fg, cell.attributes),
        (Color::Indexed(0), Attributes::BOLD)
    );
    // 4:3 is a curly underline, not underline and italic.
    assert_eq!(pen("\x1b[4:3m").attributes, Attributes::UNDERLINE);
    assert_eq!(
        pen("\x1b[4:3;1m").attributes,
        Attributes::UNDERLINE | Attributes::BOLD
    );
}

#[test]
fn sgr_underline_colour_is_skipped() {
    let cell = pen("\x1b[58;2;1;3;4;9m");
    assert_eq!(
        (cell.fg, cell.bg, cell.attributes),
        (Color::Default, Color::Default, Attributes::STRIKETHROUGH)
    );
    assert_eq!(pen("\x1b[58;5;1;9m").attributes, Attributes::STRIKETHROUGH);
    assert_eq!(
        pen("\x1b[58:2::1:3:4;9m").attributes,
        Attributes::STRIKETHROUGH
    );
}

#[test]
fn sgr_attributes() {
    for (code, attribute) in [
        (1, Attributes::BOLD),
        (2, Attributes::DIM),
        (3, Attributes::ITALIC),
        (4, Attributes::UNDERLINE),
        (21, Attributes::UNDERLINE),
        (7, Attributes::INVERSE),
        (8, Attributes::INVISIBLE),
        (9, Attributes::STRIKETHROUGH),
    ] {
        assert_eq!(
            pen(&format!("\x1b[{code}m")).attributes,
            attribute,
            "SGR {code}"
        );
    }
    let all = "\x1b[1;2;3;4;7;8;9m";
    assert_eq!(pen(all).attributes, Attributes::all());
    for (code, cleared) in [
        (22, Attributes::BOLD | Attributes::DIM),
        (23, Attributes::ITALIC),
        (24, Attributes::UNDERLINE),
        (27, Attributes::INVERSE),
        (28, Attributes::INVISIBLE),
        (29, Attributes::STRIKETHROUGH),
    ] {
        assert_eq!(
            pen(&format!("{all}\x1b[{code}m")).attributes,
            Attributes::all() - cleared,
            "SGR {code}"
        );
    }
    // Blink and the like are not tracked.
    assert_eq!(pen("\x1b[5;6;10;53m").attributes, Attributes::empty());
}

#[test]
fn sgr_resets() {
    let styled = "\x1b[1;4;31;42m";
    for reset in ["\x1b[m", "\x1b[0m", "\x1b[;m"] {
        assert_eq!(
            pen(&format!("{styled}{reset}")),
            Cell {
                ch: Some('x'),
                ..Cell::default()
            },
            "{reset:?}"
        );
    }
    // A reset in the middle keeps what follows; an empty parameter is a 0.
    let cell = pen(&format!("{styled}\x1b[0;3m"));
    assert_eq!(
        (cell.fg, cell.bg, cell.attributes),
        (Color::Default, Color::Default, Attributes::ITALIC)
    );
    let cell = pen(&format!("{styled}\x1b[;3m"));
    assert_eq!(
        (cell.fg, cell.bg, cell.attributes),
        (Color::Default, Color::Default, Attributes::ITALIC)
    );
}

#[test]
fn the_pen_applies_to_what_is_printed_after_it() {
    let screen = fed(10, 2, "a\x1b[1;31mb\x1b[0mc");
    assert_eq!(cell(&screen, 0, 0).attributes, Attributes::empty());
    assert_eq!(
        (cell(&screen, 1, 0).fg, cell(&screen, 1, 0).attributes),
        (Color::Indexed(1), Attributes::BOLD)
    );
    assert_eq!(
        cell(&screen, 2, 0),
        Cell {
            ch: Some('c'),
            ..Cell::default()
        }
    );
}

// The alternate screen.

#[test]
fn the_alternate_screen_and_back() {
    let mut screen = fed(10, 3, "1\r\n2\r\n3\r\n4\r\nmain");
    assert_eq!(screen.line_count(), 5);
    screen.feed(b"\x1b[?1049h");
    assert!(screen.alternate_screen());
    assert!(screen.modes().contains(Modes::ALTERNATE_SCREEN));
    assert_eq!(rows(&screen), ["", "", ""]);
    assert_eq!(screen.line_count(), 3);
    assert_eq!(screen.first_line_id(), 0);
    // The cursor stays where it was.
    assert_eq!(at(&screen), (4, 2));
    screen.feed(b"\x1b[Halt\r\n\n\n\n\n");
    assert_eq!(screen.line_count(), 3);
    assert_eq!(screen.scrolled_lines(), 2);
    screen.feed(b"\x1b[?1049l");
    assert!(!screen.alternate_screen());
    assert_eq!(lines(&screen), ["1", "2", "3", "4", "main"]);
    assert_eq!(at(&screen), (4, 2));
}

#[test]
fn the_alternate_screen_starts_blank_each_time() {
    let mut screen = fed(10, 3, "\x1b[?1049hfirst");
    // Already there: nothing happens.
    screen.feed(b"\x1b[?1049h\x1b[?47h\x1b[?1047h");
    assert_eq!(rows(&screen)[0], "first");
    screen.feed(b"\x1b[?1049l\x1b[?1049l\x1b[?1049h");
    assert_eq!(rows(&screen), ["", "", ""]);
}

#[test]
fn modes_47_and_1047_leave_the_cursor_alone() {
    for mode in [47, 1047] {
        let mut screen = fed(10, 3, &format!("main\x1b[?{mode}h"));
        assert!(screen.alternate_screen());
        assert_eq!(rows(&screen), ["", "", ""]);
        screen.feed(format!("\x1b[3;7H\x1b[?{mode}l").as_bytes());
        assert!(!screen.alternate_screen());
        assert_eq!(rows(&screen)[0], "main");
        assert_eq!(at(&screen), (6, 2));
    }
}

#[test]
fn each_screen_has_its_own_saved_cursor() {
    let mut screen = fed(10, 5, "\x1b[2;2H\x1b7\x1b[?47h\x1b[4;4H\x1b7\x1b[H\x1b8");
    assert_eq!(at(&screen), (3, 3));
    screen.feed(b"\x1b[?47l\x1b8");
    assert_eq!(at(&screen), (1, 1));
}

#[test]
fn switching_screens_resets_the_scroll_region() {
    let screen = fed(5, 4, &format!("\x1b[2;3r\x1b[?1049h\x1b[H{FIVE_ROWS}"));
    assert_eq!(rows(&screen), ["B", "C", "D", "E"]);
    assert_eq!(screen.scrolled_lines(), 0);
    let screen = fed(
        5,
        4,
        &format!("\x1b[?1049h\x1b[2;3r\x1b[?1049l\x1b[H{FIVE_ROWS}"),
    );
    assert_eq!(lines(&screen), ["A", "B", "C", "D", "E"]);
}

#[test]
fn clearing_does_nothing_on_the_alternate_screen() {
    let mut screen = fed(5, 3, "1\r\n2\r\n3\r\n4\x1b[?1049h\x1b[HA\r\nB\x1b[3J");
    screen.clear();
    assert_eq!(rows(&screen), ["A", "B", ""]);
    screen.feed(b"\x1b[?1049l");
    assert_eq!(lines(&screen), ["1", "2", "3", "4"]);
}

// Resizing and clearing.

#[test]
fn resizing_the_width_cuts_or_pads_lines() {
    let mut screen = fed(6, 2, "abcdef\r\ngh");
    screen.resize(8, 2);
    assert_eq!(screen.columns(), 8);
    assert_eq!(rows(&screen), ["abcdef", "gh"]);
    assert!(screen.lines().all(|line| line.cells().len() == 8));
    screen.resize(3, 2);
    assert_eq!(rows(&screen), ["abc", "gh"]);
    assert!(screen.lines().all(|line| line.cells().len() == 3));
    assert_eq!(at(&screen), (2, 1));
    screen.resize(8, 2);
    assert_eq!(rows(&screen), ["abc", "gh"]);
}

#[test]
fn fewer_rows_drop_the_blank_lines_under_the_cursor_first() {
    let mut screen = fed(5, 5, "a\r\nb");
    screen.resize(5, 3);
    assert_eq!(screen.rows(), 3);
    assert_eq!(lines(&screen), ["a", "b", ""]);
    assert_eq!(at(&screen), (1, 1));
    assert_eq!(screen.scrolled_lines(), 0);
    // Then the top lines scroll into the scrollback.
    screen.resize(5, 1);
    assert_eq!(lines(&screen), ["a", "b"]);
    assert_eq!(rows(&screen), ["b"]);
    assert_eq!(at(&screen), (1, 0));
    assert_eq!(screen.scrolled_lines(), 1);
}

#[test]
fn fewer_rows_scroll_a_full_screen_into_the_scrollback() {
    let mut screen = fed(5, 5, "1\r\n2\r\n3\r\n4\r\n5");
    screen.resize(5, 3);
    assert_eq!(lines(&screen), ["1", "2", "3", "4", "5"]);
    assert_eq!(rows(&screen), ["3", "4", "5"]);
    assert_eq!(at(&screen), (1, 2));
    assert_eq!(screen.scrolled_lines(), 2);
    assert_eq!(screen.first_line_id(), 0);
}

#[test]
fn a_coloured_blank_line_counts_as_content() {
    let mut screen = fed(5, 4, "a\x1b[4;1H\x1b[41m\x1b[K\x1b[H");
    screen.resize(5, 2);
    assert_eq!(screen.line_count(), 4);
    assert_eq!(screen.scrolled_lines(), 2);
    assert_eq!(at(&screen), (0, 0));
}

#[test]
fn more_rows_add_blank_lines_at_the_bottom() {
    let mut screen = fed(5, 3, "1\r\n2\r\n3\r\n4");
    screen.resize(5, 5);
    assert_eq!(lines(&screen), ["1", "2", "3", "4", "", ""]);
    assert_eq!(rows(&screen), ["2", "3", "4", "", ""]);
    assert_eq!(at(&screen), (1, 2));
    assert_eq!(screen.scrolled_lines(), 1);
}

#[test]
fn resizing_resets_the_region_the_tab_stops_and_the_pending_wrap() {
    let mut screen = fed(20, 5, "\x1b[2;3r\x1b[3g\x1b[5;1H");
    screen.resize(21, 5);
    screen.feed(b"\t");
    assert_eq!(at(&screen), (8, 4));
    screen.feed(b"\n");
    assert_eq!(screen.scrolled_lines(), 1);

    let mut screen = fed(5, 3, "abcde");
    screen.resize(5, 3);
    screen.feed(b"f");
    assert_eq!(rows(&screen), ["abcde", "f", ""]);
    let mut screen = fed(5, 3, "abcde");
    screen.resize(5, 4);
    screen.feed(b"f");
    assert_eq!(rows(&screen), ["abcdf", "", "", ""]);
}

#[test]
fn resizing_trims_the_scrollback_to_its_limit() {
    let mut screen = Screen::new(5, 4, 1);
    screen.feed(b"1\r\n2\r\n3\r\n4");
    screen.resize(5, 2);
    assert_eq!(lines(&screen), ["2", "3", "4"]);
    assert_eq!(screen.first_line_id(), 1);
}

#[test]
fn resizing_on_the_alternate_screen_keeps_its_bottom_rows() {
    let mut screen = fed(5, 4, "\x1b[?1049h\x1b[HA\r\nB\r\nC\r\nD");
    screen.resize(5, 2);
    assert_eq!(rows(&screen), ["C", "D"]);
    assert_eq!(at(&screen), (1, 1));
    screen.resize(5, 3);
    assert_eq!(rows(&screen), ["C", "D", ""]);
    screen.feed(b"\x1b[?1049l");
    assert_eq!(screen.line_count(), 5);
}

#[test]
fn the_smallest_screen_is_one_cell() {
    let mut screen = Screen::new(0, 0, 0);
    assert_eq!((screen.columns(), screen.rows()), (1, 1));
    screen.feed("abc\t\x1b[Z\x1b[5@\x1b[5P\x1b[L\x1b[M\x1b[r\x1b[2J中\x1bHd".as_bytes());
    assert_eq!(rows(&screen), ["d"]);
    assert_eq!(screen.line_count(), 1);
    screen.resize(0, 0);
    assert_eq!((screen.columns(), screen.rows()), (1, 1));
}

#[test]
fn clear_keeps_only_the_cursor_line() {
    let mut screen = fed(5, 3, "1\r\n2\r\n3\r\n4\r\n5\x1b[2;2H");
    screen.clear();
    assert_eq!(lines(&screen), ["4", "", ""]);
    assert_eq!(at(&screen), (1, 0));
    assert_eq!(screen.first_line_id(), 5);
    assert_eq!(screen.scrolled_lines(), 2);
}

#[test]
fn full_reset() {
    let mut screen = fed(
        20,
        3,
        "\x1b]0;title\x071\r\n2\r\n3\r\n4\r\n5\x1b[31;1m\x1b[?1;6;1004;2004h\x1b[?7;25l\x1b[4h\x1b[3g\x1b7\x1b[1;2r\x1b[?1049h\x1bc",
    );
    assert_eq!(lines(&screen), ["", "", ""]);
    assert_eq!(at(&screen), (0, 0));
    assert_eq!(screen.modes(), Modes::default());
    // The ids of the lines thrown away are not reused.
    assert_eq!(screen.first_line_id(), 5);
    assert_eq!(screen.title(), "title");
    screen.feed(b"\tx");
    assert_eq!(
        cell(&screen, 8, 0),
        Cell {
            ch: Some('x'),
            ..Cell::default()
        }
    );
    screen.feed(b"\x1b8");
    assert_eq!(at(&screen), (0, 0));
    screen.feed(b"\x1b[3;1H\n");
    assert_eq!(screen.scrolled_lines(), 3);
}

#[test]
fn soft_reset() {
    let mut screen = fed(
        10,
        4,
        "text\x1b[31;1m\x1b[2;3r\x1b[?1;6;1004;2004h\x1b[?7;25l\x1b[4h\x1b[2;3H\x1b[!p",
    );
    assert_eq!(
        screen.modes(),
        Modes::AUTO_WRAP | Modes::CURSOR_VISIBLE | Modes::FOCUS_REPORTING | Modes::BRACKETED_PASTE
    );
    // The contents and the cursor stay.
    assert_eq!(rows(&screen)[0], "text");
    assert_eq!(at(&screen), (2, 2));
    screen.feed(b"x\x1b[4;1H\n");
    assert_eq!(
        cell(&screen, 2, 1),
        Cell {
            ch: Some('x'),
            ..Cell::default()
        }
    );
    assert_eq!(screen.scrolled_lines(), 1);
}

// Modes.

#[test]
fn private_modes() {
    type Case = (&'static str, fn(&Screen) -> bool, Modes, bool);
    let cases: [Case; 4] = [
        (
            "?1",
            Screen::application_cursor_keys,
            Modes::APPLICATION_CURSOR_KEYS,
            false,
        ),
        ("?25", Screen::cursor_visible, Modes::CURSOR_VISIBLE, true),
        (
            "?1004",
            Screen::focus_reporting,
            Modes::FOCUS_REPORTING,
            false,
        ),
        (
            "?2004",
            Screen::bracketed_paste,
            Modes::BRACKETED_PASTE,
            false,
        ),
    ];
    for (mode, accessor, flag, initially) in cases {
        let mut screen = Screen::new(10, 3, 0);
        assert_eq!(accessor(&screen), initially, "{mode}");
        screen.feed(format!("\x1b[{mode}h").as_bytes());
        assert!(accessor(&screen), "{mode}");
        assert_eq!(screen.modes(), Modes::default() | flag, "{mode}");
        screen.feed(format!("\x1b[{mode}l").as_bytes());
        assert!(!accessor(&screen), "{mode}");
        assert_eq!(screen.modes(), Modes::default() - flag, "{mode}");
    }
}

#[test]
fn several_modes_in_one_sequence() {
    let mut screen = fed(10, 3, "\x1b[?1;2004;1004h");
    assert!(
        screen.application_cursor_keys() && screen.bracketed_paste() && screen.focus_reporting()
    );
    screen.feed(b"\x1b[?25;1l");
    assert!(
        !screen.cursor_visible() && !screen.application_cursor_keys() && screen.bracketed_paste()
    );
}

#[test]
fn private_and_ansi_modes_are_distinct() {
    // Without the '?' these are other modes, with it 4 is not insert mode.
    assert_eq!(
        fed(10, 3, "\x1b[1;6;25;1004;2004;1049h").modes(),
        Modes::default()
    );
    assert_eq!(fed(10, 3, "\x1b[7;25l").modes(), Modes::default());
    assert_eq!(fed(10, 3, "\x1b[?4h").modes(), Modes::default());
    assert_eq!(
        fed(10, 3, "\x1b[>1h\x1b[=1h\x1b[<1h").modes(),
        Modes::default()
    );
    // Unknown ones are ignored.
    assert_eq!(fed(10, 3, "\x1b[?12;1000;1006h").modes(), Modes::default());
}

#[test]
fn insert_mode_pushes_the_line_right() {
    let mut screen = fed(5, 2, "abc\r\x1b[4hXY");
    assert!(screen.modes().contains(Modes::INSERT));
    assert_eq!(rows(&screen)[0], "XYabc");
    screen.feed(b"\x1b[4lZ");
    assert_eq!(rows(&screen)[0], "XYZbc");
    // What is pushed past the end is lost.
    assert_eq!(rows(&fed(5, 2, "abcde\r\x1b[4hX")), ["Xabcd", ""]);
}

// UTF-8 and character widths.

#[test]
fn utf8_of_every_length() {
    let screen = fed(10, 2, "aé€b");
    assert_eq!(rows(&screen)[0], "aé€b");
    assert_eq!(at(&screen), (4, 0));
    assert_eq!(cell(&screen, 1, 0).ch, Some('é'));
    assert_eq!(cell(&screen, 2, 0).ch, Some('€'));
    let screen = fed(10, 2, "\u{10348}");
    assert_eq!(cell(&screen, 0, 0).ch, Some('\u{10348}'));
    assert_eq!(at(&screen), (1, 0));
}

#[test]
fn utf8_split_across_feeds() {
    for text in ["é", "€", "😀", "a中é😀b"] {
        let mut screen = Screen::new(10, 2, 0);
        for byte in text.as_bytes() {
            screen.feed(&[*byte]);
        }
        assert_eq!(rows(&screen)[0], text);
    }
    let mut screen = Screen::new(10, 2, 0);
    screen.feed(b"\xf0\x9f");
    assert_eq!(at(&screen), (0, 0));
    screen.feed(b"\x98");
    screen.feed(b"\x80!");
    assert_eq!(rows(&screen)[0], "😀!");
    assert_eq!(at(&screen), (3, 0));
}

#[test]
fn invalid_utf8_becomes_the_replacement_character() {
    let fed = |bytes: &[u8]| {
        let mut screen = Screen::new(10, 2, 0);
        screen.feed(bytes);
        rows(&screen).swap_remove(0)
    };
    // A stray continuation byte, and bytes that never start a sequence.
    assert_eq!(fed(b"a\x80b"), "a\u{fffd}b");
    assert_eq!(fed(b"\xff\xf8"), "\u{fffd}\u{fffd}");
    // A sequence cut short: the byte that cut it is handled on its own.
    assert_eq!(fed(b"\xc3a"), "\u{fffd}a");
    assert_eq!(fed(b"\xe2\x82a"), "\u{fffd}a");
    assert_eq!(fed(b"\xc3\xc3\xa9"), "\u{fffd}é");
    assert_eq!(fed(b"\xe2\x82\r\nb"), "\u{fffd}");
    // Surrogates and code points past U+10FFFF.
    assert_eq!(fed(b"\xed\xa0\x80"), "\u{fffd}");
    assert_eq!(fed(b"\xf7\xbf\xbf\xbf"), "\u{fffd}");
    // An escape sequence drops the unfinished character silently.
    assert_eq!(fed(b"\xc3\x1b[mb"), "b");
}

#[test]
fn wide_characters_take_two_cells() {
    let screen = fed(10, 2, "中a");
    assert_eq!(cell(&screen, 0, 0).ch, Some('中'));
    assert_eq!(cell(&screen, 1, 0).ch, None);
    assert!(cell(&screen, 1, 0).is_wide_continuation());
    assert!(!cell(&screen, 0, 0).is_wide_continuation());
    assert_eq!(cell(&screen, 2, 0).ch, Some('a'));
    assert_eq!(at(&screen), (3, 0));
    assert_eq!(screen.screen_line(0).text(), "中a       ");
    // Both halves carry the pen.
    let screen = fed(10, 2, "\x1b[41m한");
    assert_eq!(
        cell(&screen, 1, 0),
        Cell {
            ch: None,
            ..Cell::blank(Color::Indexed(1))
        }
    );
    for wide in [
        'ᄀ', '、', 'あ', '㐀', '一', 'ꀀ', '가', '豈', '︰', 'Ａ', '￠', '🌀', '🤐', '𠀀',
    ] {
        assert_eq!(at(&fed(10, 2, &wide.to_string())), (2, 0), "{wide}");
    }
    for narrow in ['a', 'é', '─', '\u{fffd}', '←'] {
        assert_eq!(at(&fed(10, 2, &narrow.to_string())), (1, 0), "{narrow}");
    }
}

#[test]
fn a_wide_character_wraps_whole() {
    let screen = fed(5, 3, "abcd中");
    assert_eq!(rows(&screen), ["abcd", "中", ""]);
    assert!(screen.screen_line(0).is_wrapped());
    assert_eq!(at(&screen), (2, 1));
    // Filling the line exactly leaves the wrap pending.
    let screen = fed(5, 3, "abc中");
    assert_eq!(rows(&screen), ["abc中", "", ""]);
    assert_eq!(at(&screen), (4, 0));
    let screen = fed(5, 3, "abc中d");
    assert_eq!(rows(&screen), ["abc中", "d", ""]);
    // Without auto-wrap it does not fit and is dropped.
    let screen = fed(5, 3, "\x1b[?7labcd中");
    assert_eq!(rows(&screen), ["abcd", "", ""]);
    assert_eq!(at(&screen), (4, 0));
}

#[test]
fn overwriting_half_a_wide_character_blanks_the_other_half() {
    let screen = fed(10, 2, "中\rx");
    assert_eq!(cell(&screen, 0, 0).ch, Some('x'));
    assert_eq!(cell(&screen, 1, 0).ch, Some(' '));
    let screen = fed(10, 2, "中\x1b[1;2Hx");
    assert_eq!(cell(&screen, 0, 0).ch, Some(' '));
    assert_eq!(cell(&screen, 1, 0).ch, Some('x'));
    let screen = fed(10, 2, "a中\r中");
    assert_eq!(cell(&screen, 0, 0).ch, Some('中'));
    assert_eq!(cell(&screen, 1, 0).ch, None);
    // Only the neighbours of the cells written are looked at: the old second half stays behind.
    assert_eq!(cell(&screen, 2, 0).ch, None);
}

#[test]
fn zero_width_characters_are_dropped() {
    for mark in ['\u{301}', '\u{200b}', '\u{200d}', '\u{fe0f}', '\u{20dd}'] {
        let screen = fed(5, 2, &format!("abcde{mark}"));
        assert_eq!(rows(&screen)[0], "abcde");
        assert_eq!(at(&screen), (4, 0));
        assert_eq!(at(&fed(5, 2, &format!("e{mark}"))), (1, 0));
    }
}

// Sequences that are parsed and ignored, and the parser's edges.

#[test]
fn sequences_split_across_feeds() {
    let input = "ab\x1b[1;31mc\x1b]2;title\x1b\\\x1b[2;3Hd中\x1b[38:2::1:2:3me\x1bP+q\x1b\\\x1b[6n\x1b[?2004h\r\n\x1b(Bf";
    let whole = fed(10, 4, input);
    let mut split = Screen::new(10, 4, 100);
    for byte in input.as_bytes() {
        split.feed(&[*byte]);
    }
    assert_eq!(rows(&whole), ["abc", "  d中e", "f", ""]);
    assert_eq!(whole.title(), "title");
    assert!(whole.screen_lines().eq(split.screen_lines()));
    assert_eq!(at(&whole), at(&split));
    assert_eq!(whole.title(), split.title());
    assert_eq!(whole.modes(), split.modes());
    assert_eq!(fed(10, 4, input).take_replies(), split.take_replies());
}

#[test]
fn cancel_and_substitute_abandon_a_sequence() {
    for abort in ['\x18', '\x1a'] {
        let screen = fed(10, 2, &format!("\x1b[31{abort}mx"));
        assert_eq!(rows(&screen)[0], "mx");
        assert_eq!(cell(&screen, 1, 0).fg, Color::Default);
        let screen = fed(10, 2, &format!("\x1b]0;title{abort}x"));
        assert_eq!((screen.title(), rows(&screen)[0].as_str()), ("", "x"));
        let screen = fed(10, 2, &format!("\x1b]0;title\x1b{abort}x"));
        assert_eq!((screen.title(), rows(&screen)[0].as_str()), ("", "x"));
        assert_eq!(rows(&fed(10, 2, &format!("\x1bPdata{abort}x")))[0], "x");
        assert_eq!(rows(&fed(10, 2, &format!("\x1b{abort}7x")))[0], "7x");
        assert_eq!(rows(&fed(10, 2, &format!("\x1b({abort}Bx")))[0], "Bx");
    }
}

#[test]
fn an_escape_restarts_a_sequence() {
    let screen = fed(10, 2, "\x1b[31\x1b[1mx");
    assert_eq!(
        (cell(&screen, 0, 0).fg, cell(&screen, 0, 0).attributes),
        (Color::Default, Attributes::BOLD)
    );
    assert_eq!(rows(&fed(10, 2, "\x1b\x1b\x1b[1mx"))[0], "x");
}

#[test]
fn controls_inside_a_sequence_are_executed() {
    assert_eq!(at(&fed(10, 3, "ab\x1b[2\rC")), (2, 0));
    assert_eq!(at(&fed(10, 3, "ab\x1b\n[2C")), (4, 1));
    assert_eq!(at(&fed(10, 3, "ab\x1b(\nB")), (2, 1));
}

#[test]
fn sequences_without_an_effect_print_nothing() {
    for ignored in [
        "\x1b(B",
        "\x1b)0",
        "\x1b#8",
        "\x1b%G",
        "\x1b=",
        "\x1b>",
        "\x1bN",
        "\x1b\\",
        "\x1b[2 q",
        "\x1b[0\"q",
        "\x1b[5i",
        "\x1b[?1J",
        "\x1b[?2K",
        "\x1b[>4;2m",
        "\x1b[?25m",
        "\x1b[<5H",
        "\x1b[=5C",
        "\x1b[?3r",
        "\x1b[1;2;3t",
        "\x1b[5\x7fx",
        "\x1b[$p",
        "\x1b[?1$p",
    ] {
        let screen = fed(10, 3, &format!("ab{ignored}c"));
        assert_eq!(rows(&screen), ["abc", "", ""], "{ignored:?}");
        assert_eq!(at(&screen), (3, 0), "{ignored:?}");
        assert_eq!(
            cell(&screen, 2, 0),
            Cell {
                ch: Some('c'),
                ..Cell::default()
            },
            "{ignored:?}"
        );
    }
}

#[test]
fn a_private_marker_anywhere_makes_the_sequence_private() {
    assert_eq!(pen("\x1b[1;?4m").attributes, Attributes::empty());
    assert!(fed(10, 2, "\x1b[2004?h").bracketed_paste());
}

#[test]
fn device_control_and_other_strings_are_skipped() {
    assert_eq!(rows(&fed(10, 2, "a\x1bPq#0;1\r\n[31m\x1b\\b"))[0], "ab");
    assert_eq!(rows(&fed(10, 2, "a\x1b_apc\x07b"))[0], "ab");
    assert_eq!(rows(&fed(10, 2, "a\x1bXsos\x1b\\b"))[0], "ab");
    assert_eq!(rows(&fed(10, 2, "a\x1b^pm\x1b\\b"))[0], "ab");
    // An ESC that is not the start of ST starts a new sequence.
    let screen = fed(10, 2, "\x1bPdata\x1b[31mx");
    assert_eq!(
        (rows(&screen)[0].as_str(), cell(&screen, 0, 0).fg),
        ("x", Color::Indexed(1))
    );
    assert_eq!(rows(&fed(10, 2, "\x1bPdata\x1b\x1b\\x"))[0], "x");
}

// The title and the replies.

#[test]
fn osc_sets_the_title() {
    assert_eq!(fed(10, 2, "\x1b]0;hello world\x07").title(), "hello world");
    assert_eq!(fed(10, 2, "\x1b]2;bell\x07").title(), "bell");
    assert_eq!(
        fed(10, 2, "\x1b]2;string terminator\x1b\\").title(),
        "string terminator"
    );
    assert_eq!(fed(10, 2, "\x1b]0;a;b;c\x07").title(), "a;b;c");
    assert_eq!(fed(10, 2, "\x1b]0;first\x07\x1b]2;\x07").title(), "");
    assert_eq!(fed(10, 2, "\x1b]0;térmïnal 中\x07").title(), "térmïnal 中");
    let mut screen = Screen::new(10, 2, 0);
    screen.feed(b"\x1b]0;\xff\x07");
    assert_eq!(screen.title(), "\u{fffd}");
    // Nothing of it is printed.
    assert_eq!(rows(&fed(10, 2, "a\x1b]0;title\x07b"))[0], "ab");
    assert_eq!(rows(&fed(10, 2, "a\x1b]0;title\x1b\\b"))[0], "ab");
}

#[test]
fn other_osc_are_ignored() {
    for osc in [
        "\x1b]1;icon\x07",
        "\x1b]8;;https://example.com\x07",
        "\x1b]title\x07",
        "\x1b]\x07",
        "\x1b]02;x\x07",
    ] {
        let screen = fed(10, 2, &format!("\x1b]0;kept\x07a{osc}b"));
        assert_eq!(screen.title(), "kept", "{osc:?}");
        assert_eq!(rows(&screen)[0], "ab", "{osc:?}");
    }
}

#[test]
fn an_osc_split_across_feeds() {
    let mut screen = Screen::new(10, 2, 0);
    screen.feed(b"\x1b]0;par");
    assert_eq!(screen.title(), "");
    screen.feed(b"tial\x1b");
    assert_eq!(screen.title(), "");
    screen.feed(b"\\");
    assert_eq!(screen.title(), "partial");
}

#[test]
fn an_osc_ended_by_another_sequence_still_counts() {
    let screen = fed(10, 2, "\x1b]0;title\x1b[31mx");
    assert_eq!(screen.title(), "title");
    assert_eq!(
        (rows(&screen)[0].as_str(), cell(&screen, 0, 0).fg),
        ("x", Color::Indexed(1))
    );
    // Two ESCs in a row drop it.
    assert_eq!(fed(10, 2, "\x1b]0;title\x1b\x1b\\").title(), "");
}

#[test]
fn an_overlong_osc_is_cut() {
    let screen = fed(10, 2, &format!("\x1b]0;{}\x07x", "t".repeat(5000)));
    assert_eq!(screen.title().len(), 4094);
    assert_eq!(rows(&screen)[0], "x");
}

#[test]
fn device_attributes() {
    let mut screen = fed(10, 2, "\x1b[c");
    assert_eq!(screen.take_replies(), "\x1b[?1;2c");
    assert_eq!(screen.take_replies(), "");
    assert_eq!(fed(10, 2, "\x1b[0c").take_replies(), "\x1b[?1;2c");
    assert_eq!(fed(10, 2, "\x1b[>c").take_replies(), "\x1b[>0;276;0c");
    assert_eq!(fed(10, 2, "\x1b[>0c").take_replies(), "\x1b[>0;276;0c");
    assert_eq!(fed(10, 2, "\x1b[1c\x1b[?c\x1b[=c").take_replies(), "");
}

#[test]
fn device_status_reports() {
    assert_eq!(fed(10, 5, "\x1b[5n").take_replies(), "\x1b[0n");
    assert_eq!(fed(10, 5, "\x1b[6n").take_replies(), "\x1b[1;1R");
    assert_eq!(fed(10, 5, "\x1b[3;7H\x1b[6n").take_replies(), "\x1b[3;7R");
    assert_eq!(fed(10, 5, "\x1b[3;7H\x1b[?6n").take_replies(), "\x1b[3;7R");
    // With a wrap pending the cursor is still on the last column.
    assert_eq!(fed(5, 5, "abcde\x1b[6n").take_replies(), "\x1b[1;5R");
    assert_eq!(
        fed(10, 5, "\x1b[n\x1b[0n\x1b[7n\x1b[?5n").take_replies(),
        ""
    );
    // Replies queue up in order until taken.
    let mut screen = fed(10, 5, "\x1b[5n\x1b[c");
    screen.feed(b"ab\x1b[6n");
    assert_eq!(screen.take_replies(), "\x1b[0n\x1b[?1;2c\x1b[1;3R");
    assert_eq!(screen.take_replies(), "");
}

// Selections.

#[test]
fn text_between_joins_wrapped_lines_and_trims_blanks() {
    let screen = fed(5, 5, "abcdefg\r\nhi  \r\n\r\nx");
    let point = |line, column| Point { line, column };
    assert_eq!(
        screen.text_between(point(0, 0), point(4, 5)),
        "abcdefg\r\nhi\r\n\r\nx"
    );
    assert_eq!(screen.text_between(point(0, 1), point(0, 3)), "bc");
    assert_eq!(screen.text_between(point(0, 3), point(1, 1)), "def");
    assert_eq!(screen.text_between(point(1, 1), point(2, 1)), "g\r\nh");
    // An end inside the line keeps the blanks before it; at the line's end they go.
    assert_eq!(screen.text_between(point(1, 0), point(1, 4)), "fg  ");
    assert_eq!(screen.text_between(point(1, 0), point(1, 5)), "fg");
    assert_eq!(screen.text_between(point(1, 0), point(1, 99)), "fg");
    // Empty and out-of-range selections.
    assert_eq!(screen.text_between(point(1, 2), point(1, 2)), "");
    assert_eq!(screen.text_between(point(1, 4), point(1, 2)), "");
    assert_eq!(screen.text_between(point(1, 9), point(2, 1)), "\r\nh");
    assert_eq!(screen.text_between(point(4, 0), point(9, 0)), "x\r\n");
    assert_eq!(screen.text_between(point(7, 0), point(9, 0)), "");
    assert!(point(0, 9) < point(1, 0) && point(1, 0) < point(1, 1));
}

#[test]
fn text_between_follows_line_ids() {
    let mut screen = Screen::new(5, 2, 1);
    screen.feed("0\r\n1\r\n2\r\n中x".as_bytes());
    assert_eq!(screen.first_line_id(), 1);
    let point = |line, column| Point { line, column };
    // Lines that left the scrollback are skipped; wide characters count once.
    assert_eq!(
        screen.text_between(point(0, 0), point(3, 5)),
        "1\r\n2\r\n中x"
    );
    assert_eq!(screen.text_between(point(3, 0), point(3, 2)), "中");
    assert_eq!(screen.text_between(point(3, 1), point(3, 3)), "x");
}

// Whatever comes in, the grid stays well-formed.

#[test]
fn arbitrary_input_keeps_the_invariants() {
    // Bytes weighted towards what makes sequences, from a fixed linear congruential generator.
    const ALPHABET: &[u8] = b"\x1b\x1b\x1b[[[]??;;:0123456789ABCDEFGHJKLMPSTXZcdfghlmnrsu@`a e!p\\\x07\x08\t\n\r\x18\xe4\xb8\xad\xc3\xa9P78DEM";
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as usize
    };
    let mut screen = Screen::new(12, 6, 20);
    for round in 0..4000 {
        let chunk: Vec<u8> = (0..1 + next() % 12)
            .map(|_| ALPHABET[next() % ALPHABET.len()])
            .collect();
        screen.feed(&chunk);
        if round % 97 == 0 {
            screen.resize(1 + next() % 20, 1 + next() % 10);
        }
        if round % 389 == 0 {
            screen.clear();
        }
        let CursorPosition { column, row } = screen.cursor();
        assert!(column < screen.columns() && row < screen.rows());
        assert!(screen.line_count() >= screen.rows());
        assert!(screen.alternate_screen() || screen.line_count() <= screen.rows() + 20);
        assert!(!screen.alternate_screen() || screen.line_count() == screen.rows());
        assert_eq!(screen.screen_lines().len(), screen.rows());
        assert!(
            screen
                .lines()
                .all(|line| line.cells().len() == screen.columns())
        );
        assert!(has_no_control_characters(&screen));
    }
}

// Hardening.

/// No cell of the buffer holds a control character.
fn has_no_control_characters(screen: &Screen) -> bool {
    screen
        .lines()
        .flat_map(|line| line.cells())
        .all(|cell| cell.ch.is_none_or(|ch| !ch.is_control()))
}

#[test]
fn overlong_and_c1_utf8_becomes_a_replacement_character() {
    // C0 / C1 leads, overlong NUL / ESC / space, and the C1 controls U+0085 and U+009B.
    let mut screen = Screen::new(30, 2, 0);
    screen
        .feed(b"a\xc0\x80b\xc1\xbfc\xe0\x80\x80d\xf0\x80\x80\x9be\xe0\x80\xa0f\xc2\x85g\xc2\x9bh");
    assert!(has_no_control_characters(&screen));
    assert_eq!(
        screen.screen_line(0).text().trim_end(),
        "a\u{fffd}\u{fffd}b\u{fffd}\u{fffd}c\u{fffd}d\u{fffd}e\u{fffd}f\u{fffd}g\u{fffd}h"
    );
    // The sequences that are valid still print.
    let screen = fed(10, 1, "\u{e9}\u{4e2d}\u{1f600}");
    assert_eq!(
        screen.screen_line(0).text().trim_end(),
        "\u{e9}\u{4e2d}\u{1f600}"
    );
}

#[test]
fn csi_parameters_and_intermediates_are_capped() {
    // A sequence that never ends is bounded; once it does end the screen carries on.
    let mut input = String::from("\x1b[");
    input.push_str(&"1;".repeat(100_000));
    input.push_str(&" ".repeat(100_000));
    let mut screen = fed(10, 2, &input);
    screen.feed(b"m\x1b[0mok");
    assert_eq!(screen.screen_line(0).text().trim_end(), "ok");
}

#[test]
fn a_wrapped_line_keeps_its_trailing_blank_when_copied() {
    let screen = fed(6, 3, "hello world");
    let point = |line, column| Point { line, column };
    assert_eq!(screen.text_between(point(0, 0), point(1, 5)), "hello world");
    // A line that is not wrapped still loses its blanks.
    let screen = fed(8, 3, "hi\r\nyo");
    assert_eq!(screen.text_between(point(0, 0), point(1, 8)), "hi\r\nyo");
}

/// No continuation cell without a wide character before it, no wide character without its continuation.
fn wide_pairs_are_whole(screen: &Screen) -> bool {
    let wide = |cell: &Cell| cell.ch == Some('\u{4e2d}');
    screen.lines().all(|line| {
        let cells = line.cells();
        cells.iter().enumerate().all(|(x, cell)| {
            (!cell.is_wide_continuation() || (x > 0 && wide(&cells[x - 1])))
                && (!wide(cell) || cells.get(x + 1).is_some_and(Cell::is_wide_continuation))
        })
    })
}

#[test]
fn editing_through_a_wide_character_blanks_both_halves() {
    // "a中b": the pair sits in columns 1 and 2 (cursor column 2 and 3, one-based).
    for (edit, expected) in [
        ("\x1b[3G\x1b[P", "a b"),
        ("\x1b[3G\x1b[@", "a   b"),
        ("\x1b[3G\x1b[X", "a  b"),
        ("\x1b[3G\x1b[1K", "   b"),
        ("\x1b[3G\x1b[K", "a"),
        ("\x1b[3G\x1b[J", "a"),
    ] {
        let screen = fed(6, 1, &format!("a\u{4e2d}b{edit}"));
        assert!(wide_pairs_are_whole(&screen), "{edit:?}");
        let text = screen.screen_line(0).text();
        assert!(!text.contains('\u{4e2d}'), "{edit:?}: {text:?}");
        assert_eq!(text.trim_end(), expected, "{edit:?}");
    }
    // An edit that leaves the pair alone keeps it.
    let screen = fed(6, 1, "a\u{4e2d}b\x1b[4G\x1b[X");
    assert!(screen.screen_line(0).text().contains('\u{4e2d}'));
    assert!(wide_pairs_are_whole(&screen));
}
