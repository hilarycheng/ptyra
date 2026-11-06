use super::*;
use alacritty_terminal::{
    index::{Column, Line, Point},
    term::TermMode,
    vte::ansi::{Color, NamedColor},
};

fn terminal(tab: u64) -> Terminal {
    Terminal::new(TabId(tab), 4, 16).unwrap()
}

fn feed(term: &mut Terminal, bytes: &[u8]) {
    term.feed(term.tab(), bytes).unwrap();
}

fn cells(term: &Terminal) -> Vec<Cell> {
    term.grid()
        .unwrap()
        .display_iter()
        .map(|c| c.cell.clone())
        .collect()
}

fn row_text(term: &Terminal, line: i32) -> String {
    let grid = term.grid().unwrap();
    (0..grid.columns())
        .map(|column| grid[Line(line)][Column(column)].c)
        .collect()
}

#[test]
fn scrollback_discards_oldest_lines() {
    for limit in [0, 1, 3] {
        let mut term = Terminal::with_scrollback(TabId(1), 2, 4, limit).unwrap();
        assert_eq!(term.grid().unwrap().history_size(), 0);
        // Cross the core's 1,000-row allocation cache as well as the logical cap.
        for number in 0..1105 {
            feed(&mut term, format!("{number:04}\r\n").as_bytes());
            let history = number.min(limit);
            let grid = term.grid().unwrap();
            assert_eq!(grid.history_size(), history);
            assert_eq!(grid.total_lines(), history + 2);
            assert_eq!(row_text(&term, 0), format!("{number:04}"));
            assert_eq!(row_text(&term, 1), "    ");
            for offset in 1..=history {
                assert_eq!(
                    row_text(&term, -(offset as i32)),
                    format!("{:04}", number - offset)
                );
            }
        }
    }
}

#[test]
fn scrollback_defaults_to_five_thousand_lines() {
    assert_eq!(DEFAULT_SCROLLBACK_LINES, 5000);
    let mut term = Terminal::new(TabId(1), 2, 4).unwrap();
    for number in 0..5005 {
        feed(&mut term, format!("{number:04}\r\n").as_bytes());
        assert!(term.grid().unwrap().history_size() <= 5000);
    }
    assert_eq!(term.grid().unwrap().history_size(), 5000);
    assert_eq!(row_text(&term, -5000), "0004");
    assert_eq!(row_text(&term, -1), "5003");
    assert_eq!(row_text(&term, 0), "5004");
}

#[test]
fn scrollback_preserves_unicode_cells_and_soft_wrap() {
    let mut term = Terminal::with_scrollback(TabId(1), 2, 4, 2).unwrap();
    feed(&mut term, "繁e\u{301}ZQ".as_bytes());
    let original: Vec<_> = (0..4)
        .map(|column| term.grid().unwrap()[Line(0)][Column(column)].clone())
        .collect();
    assert_eq!(original[0].flags, Flags::WIDE_CHAR);
    assert_eq!(original[1].flags, Flags::WIDE_CHAR_SPACER);
    assert_eq!(original[2].zerowidth(), Some(&['\u{301}'][..]));
    assert!(original[3].flags.contains(Flags::WRAPLINE));
    for (suffix, history_line) in [(b"\r\nR", -1), (b"\r\nS", -2)] {
        feed(&mut term, suffix);
        for (column, expected) in original.iter().enumerate() {
            assert_eq!(
                &term.grid().unwrap()[Line(history_line)][Column(column)],
                expected
            );
        }
    }
    feed(&mut term, b"\r\nT");
    assert_eq!(term.grid().unwrap().history_size(), 2);
    assert_eq!(row_text(&term, -2), "Q   ");
    assert_eq!(row_text(&term, -1), "R   ");
    assert_eq!(row_text(&term, 0), "S   ");
    assert_eq!(row_text(&term, 1), "T   ");
}

#[test]
fn scrollback_is_per_tab_and_excludes_alternate_screen() {
    let mut first = Terminal::with_scrollback(TabId(1), 2, 4, 2).unwrap();
    let mut second = Terminal::with_scrollback(TabId(2), 2, 4, 1).unwrap();
    feed(&mut first, b"A\r\nB\r\nC");
    feed(&mut second, b"x\r\ny\r\nz\r\nw");
    assert_eq!(first.grid().unwrap().history_size(), 1);
    assert_eq!(row_text(&first, -1), "A   ");
    feed(&mut first, b"\x1b[?1049h\x1b[H1\r\n2\r\n3\r\n4");
    assert!(first.mode().unwrap().contains(TermMode::ALT_SCREEN));
    assert_eq!(first.grid().unwrap().history_size(), 0);
    assert_eq!(first.grid().unwrap().total_lines(), 2);
    assert_eq!(row_text(&first, 0), "3   ");
    feed(&mut first, b"\x1b[?1049l");
    assert_eq!(first.grid().unwrap().history_size(), 1);
    assert_eq!(row_text(&first, -1), "A   ");
    assert_eq!(row_text(&first, 0), "B   ");
    feed(&mut first, b"\r\nD\r\nE");
    assert_eq!(first.grid().unwrap().history_size(), 2);
    assert_eq!(row_text(&first, -2), "B   ");
    assert_eq!(row_text(&first, -1), "C   ");
    feed(&mut first, b"\x1bc");
    assert_eq!(first.grid().unwrap().history_size(), 0);
    feed(&mut first, b"L\r\nM\r\nN\r\nO\r\nP");
    assert_eq!(first.grid().unwrap().history_size(), 2);
    assert_eq!(row_text(&first, -2), "M   ");
    assert_eq!(row_text(&first, -1), "N   ");
    assert_eq!(second.grid().unwrap().history_size(), 1);
    assert_eq!(row_text(&second, -1), "y   ");
    assert_eq!(row_text(&second, 0), "z   ");
    assert_eq!(row_text(&second, 1), "w   ");
}

#[test]
fn scrollback_capacity_rejects_excess_before_allocation() {
    for (rows, columns, limit) in [
        (0, 80, 1),
        (24, 1, 1),
        (2, 4, MAX_SCROLLBACK_LINES + 1),
        (2, 4, usize::MAX),
        (24, 200, 8953),
        (1, 32768, 1),
    ] {
        let error = Terminal::with_scrollback(TabId(7), rows, columns, limit)
            .err()
            .unwrap();
        assert_eq!(error.tab, TabId(7));
        assert_eq!(
            error.error,
            AppError::new(Component::Terminal, std::io::ErrorKind::InvalidInput)
        );
    }
    // 200 columns * (two 24-row screens + 8,952 history + 1,000 cache) = 2,000,000.
    assert!(Terminal::with_scrollback(TabId(1), 24, 200, 8952).is_ok());
    assert!(Terminal::with_scrollback(TabId(1), 2, 4, MAX_SCROLLBACK_LINES).is_ok());
    assert!(Terminal::with_scrollback(TabId(1), 1, 32768, 0).is_ok());
    assert!(Terminal::new(TabId(1), 1, 32768).is_err());
}

#[test]
fn term_feed_handles_split_utf8() {
    let input = "A繁體\x1b[31mZ\x1b[0m\r\nline\x1b[2D!\x1b[K".as_bytes();
    let mut whole = terminal(1);
    feed(&mut whole, input);
    let grid = whole.grid().unwrap();
    assert_eq!(grid[Line(0)][Column(0)].c, 'A');
    assert_eq!(grid[Line(0)][Column(1)].c, '繁');
    assert_eq!(grid[Line(0)][Column(3)].c, '體');
    assert_eq!(grid[Line(0)][Column(5)].c, 'Z');
    assert_eq!(grid[Line(0)][Column(5)].fg, Color::Named(NamedColor::Red));
    assert_eq!(grid[Line(1)][Column(2)].c, '!');
    assert_eq!(grid[Line(1)][Column(3)].c, ' ');
    assert_eq!(grid.cursor.point, Point::new(Line(1), Column(3)));
    for split in 0..=input.len() {
        let mut fragmented = terminal(1);
        feed(&mut fragmented, &input[..split]);
        feed(&mut fragmented, &[]);
        feed(&mut fragmented, &input[split..]);
        assert_eq!(cells(&fragmented), cells(&whole), "split {split}");
        assert_eq!(fragmented.grid().unwrap().cursor, grid.cursor);
    }
    let mut bytewise = terminal(1);
    for byte in input.chunks(1) {
        feed(&mut bytewise, byte);
    }
    assert_eq!(cells(&bytewise), cells(&whole));
    assert_eq!(bytewise.grid().unwrap().cursor, grid.cursor);
}

#[test]
fn cjk_combining_cells_align() {
    let input = "A繁體\u{301}e\u{301}\u{323}Ｘ，　𠀀Z".as_bytes();
    // Fixed column/width expectations, independent of the core's width lookup.
    let expected: &[(usize, char, usize, &[char])] = &[
        (0, 'A', 1, &[]),
        (1, '繁', 2, &[]),
        (3, '體', 2, &['\u{301}']),
        (5, 'e', 1, &['\u{301}', '\u{323}']),
        (6, 'Ｘ', 2, &[]),
        (8, '，', 2, &[]),
        (10, '　', 2, &[]),
        (12, '𠀀', 2, &[]),
        (14, 'Z', 1, &[]),
    ];
    for split in 0..=input.len() {
        let mut term = terminal(1);
        feed(&mut term, &input[..split]);
        feed(&mut term, &input[split..]);
        let grid = term.grid().unwrap();
        for &(column, character, width, marks) in expected {
            let cell = &grid[Line(0)][Column(column)];
            assert_eq!(cell.c, character, "split {split}, column {column}");
            assert_eq!(cell.zerowidth().unwrap_or_default(), marks);
            let flags = if width == 2 {
                Flags::WIDE_CHAR
            } else {
                Flags::empty()
            };
            assert_eq!(cell.flags, flags);
            if width == 2 {
                let spacer = &grid[Line(0)][Column(column + 1)];
                assert_eq!(spacer.c, ' ');
                assert_eq!(spacer.flags, Flags::WIDE_CHAR_SPACER);
                assert!(spacer.zerowidth().unwrap_or_default().is_empty());
            }
        }
        assert_eq!(grid[Line(0)][Column(15)], Cell::default());
        assert_eq!(grid.cursor.point, Point::new(Line(0), Column(15)));
        assert!(!grid.cursor.input_needs_wrap);
    }
}

#[test]
fn cjk_wraps_before_last_column() {
    let mut term = Terminal::new(TabId(1), 3, 4).unwrap();
    for byte in "abc繁\u{301}Z".as_bytes().chunks(1) {
        feed(&mut term, byte);
    }
    let grid = term.grid().unwrap();
    for (column, character) in "abc".chars().enumerate() {
        assert_eq!(grid[Line(0)][Column(column)].c, character);
    }
    let gap = &grid[Line(0)][Column(3)];
    assert_eq!(gap.c, ' ');
    assert_eq!(gap.flags, Flags::LEADING_WIDE_CHAR_SPACER | Flags::WRAPLINE);
    let wide = &grid[Line(1)][Column(0)];
    assert_eq!(wide.c, '繁');
    assert_eq!(wide.flags, Flags::WIDE_CHAR);
    assert_eq!(wide.zerowidth(), Some(&['\u{301}'][..]));
    let spacer = &grid[Line(1)][Column(1)];
    assert_eq!(spacer.c, ' ');
    assert_eq!(spacer.flags, Flags::WIDE_CHAR_SPACER);
    assert!(spacer.zerowidth().unwrap_or_default().is_empty());
    assert_eq!(grid[Line(1)][Column(2)].c, 'Z');
    assert_eq!(grid[Line(1)][Column(2)].flags, Flags::empty());
    assert_eq!(grid.cursor.point, Point::new(Line(1), Column(3)));
    assert!(!grid.cursor.input_needs_wrap);
}

#[test]
fn combining_at_right_margin_does_not_wrap() {
    for (columns, text, base_column) in [(4, "ab繁", 2), (4, "abce", 3), (2, "𠀀", 0)] {
        let mut term = Terminal::new(TabId(1), 3, columns).unwrap();
        feed(&mut term, text.as_bytes());
        let edge = Point::new(Line(0), Column(usize::from(columns - 1)));
        assert_eq!(term.grid().unwrap().cursor.point, edge);
        assert!(term.grid().unwrap().cursor.input_needs_wrap);
        for byte in "\u{301}\u{323}".as_bytes().chunks(1) {
            feed(&mut term, byte);
            assert_eq!(term.grid().unwrap().cursor.point, edge);
            assert!(term.grid().unwrap().cursor.input_needs_wrap);
        }
        let cell = &term.grid().unwrap()[Line(0)][Column(base_column)];
        assert_eq!(cell.c, text.chars().last().unwrap());
        assert_eq!(cell.zerowidth(), Some(&['\u{301}', '\u{323}'][..]));
        if base_column != edge.column.0 {
            let spacer = &term.grid().unwrap()[edge];
            assert_eq!(spacer.flags, Flags::WIDE_CHAR_SPACER);
            assert!(spacer.zerowidth().unwrap_or_default().is_empty());
        }
        assert!(!term.grid().unwrap()[edge].flags.contains(Flags::WRAPLINE));
        feed(&mut term, b"Z");
        let grid = term.grid().unwrap();
        assert!(grid[edge].flags.contains(Flags::WRAPLINE));
        assert_eq!(grid[Line(1)][Column(0)].c, 'Z');
        assert_eq!(grid.cursor.point, Point::new(Line(1), Column(1)));
        assert!(!grid.cursor.input_needs_wrap);
    }
}

#[test]
fn terminal_partial_input_and_tabs_are_isolated() {
    let mut first = terminal(1);
    let mut second = terminal(2);
    feed(&mut first, &[0xe7, 0xb9]);
    assert!(cells(&first).iter().all(|c| c.c == ' '));
    feed(&mut second, b"B\x1b[3");
    feed(&mut first, &[0x81]);
    feed(&mut second, b"2mG");
    assert_eq!(first.grid().unwrap()[Line(0)][Column(0)].c, '繁');
    assert_eq!(
        second.grid().unwrap()[Line(0)][Column(1)].fg,
        Color::Named(NamedColor::Green)
    );
    let saved = cells(&first);
    let error = first.feed(TabId(2), b"wrong tab").unwrap_err();
    assert_eq!(error.tab, TabId(1));
    assert_eq!(error.error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(cells(&first), saved);
    feed(&mut first, b"X");
}

#[test]
fn terminal_invalid_utf8_and_sync_are_deterministic() {
    let input = b"A\xff\xe7X\x1b[?2026hZ\x1b[?2026l";
    let mut whole = terminal(1);
    feed(&mut whole, input);
    let text: String = cells(&whole).iter().map(|c| c.c).collect();
    assert!(text.starts_with("A\u{fffd}\u{fffd}XZ"));
    for split in 0..=input.len() {
        let mut fragmented = terminal(1);
        feed(&mut fragmented, &input[..split]);
        feed(&mut fragmented, &input[split..]);
        assert_eq!(cells(&fragmented), cells(&whole));
    }
    let mut pending = terminal(1);
    feed(&mut pending, b"\x1b[?2026hvisible");
    assert_eq!(pending.grid().unwrap()[Line(0)][Column(0)].c, 'v');
}

#[test]
fn terminal_bounds_reject_without_payload_or_cross_tab_damage() {
    for (rows, columns) in [(0, 80), (24, 0), (24, 1), (u16::MAX, u16::MAX)] {
        assert!(Terminal::new(TabId(1), rows, columns).is_err());
    }
    let mut first = terminal(1);
    let mut second = terminal(2);
    assert!(
        first
            .feed(TabId(1), &vec![b'a'; MAX_FEED_BYTES + 1])
            .is_err()
    );
    feed(&mut first, b"safe");
    feed(&mut first, b"\x1b]2;");
    feed(&mut first, &vec![b'a'; MAX_OSC_BYTES - 2]);
    let error = first.feed(TabId(1), b"private").unwrap_err();
    assert_eq!(error.error.component(), Component::Terminal);
    assert_eq!(error.error.kind(), std::io::ErrorKind::InvalidData);
    assert!(!format!("{error:?}").contains("private"));
    assert!(first.grid().is_none());
    assert_eq!(
        first.feed(TabId(1), b"later").unwrap_err().error.kind(),
        std::io::ErrorKind::NotConnected
    );
    feed(&mut second, b"ok");
    assert_eq!(second.grid().unwrap()[Line(0)][Column(0)].c, 'o');
}

#[test]
fn terminal_combining_limit_includes_wide_cells_and_repeat() {
    for prefix in ["a", "繁", "123456789012345a", "12345678901234繁"] {
        let mut term = terminal(1);
        feed(&mut term, prefix.as_bytes());
        for _ in 0..MAX_COMBINING_MARKS {
            feed(&mut term, "\u{301}".as_bytes());
        }
        assert!(term.feed(TabId(1), "\u{301}".as_bytes()).is_err());
        assert!(term.grid().is_none());
    }
    let mut repeated = terminal(1);
    feed(&mut repeated, "a\u{301}".as_bytes());
    assert!(repeated.feed(TabId(1), b"\x1b[65535b").is_err());
    assert!(repeated.grid().is_none());
}

#[test]
fn oversized_osc_is_rejected_safely() {
    for header in ["2;", "8;;https://example.test/", "999;"] {
        for chunk_size in [1, 31, MAX_FEED_BYTES] {
            let body = format!("{header}{}", "x".repeat(MAX_OSC_BYTES - header.len()));
            let mut bounded = terminal(1);
            let sequence = format!("\x1b]{body}\x1b\\");
            for chunk in sequence.as_bytes().chunks(chunk_size) {
                feed(&mut bounded, chunk);
            }
            assert!(bounded.grid().is_some());
            let mut oversized = terminal(2);
            let sequence = format!("\x1b]{body}SECRET");
            let result = sequence
                .as_bytes()
                .chunks(chunk_size)
                .try_for_each(|chunk| oversized.feed(TabId(2), chunk));
            let error = result.unwrap_err();
            assert_eq!(error.error.kind(), std::io::ErrorKind::InvalidData);
            assert!(!format!("{error:?}").contains("SECRET"));
            assert!(oversized.grid().is_none());
            assert!(oversized.feed(TabId(2), b"\x07after").is_err());
            feed(&mut bounded, b"still works");
        }
    }
}

#[test]
fn terminal_osc_bound_tracks_escape_controls_and_terminators() {
    for terminator in [&b"\x07"[..], &b"\x1b\\"[..], &b"\x18"[..], &b"\x1a"[..]] {
        let mut term = terminal(1);
        for _ in 0..3 {
            feed(&mut term, b"\x1b\x00\x7f]2;");
            feed(&mut term, &vec![b'x'; MAX_OSC_BYTES - 2]);
            feed(&mut term, terminator);
        }
        feed(&mut term, b"Q");
        assert_eq!(term.grid().unwrap()[Line(0)][Column(0)].c, 'Q');
    }
    let mut term = terminal(1);
    feed(&mut term, b"\x1b\x1b]2;");
    for _ in 0..MAX_OSC_BYTES - 2 {
        feed(&mut term, b"x");
    }
    assert!(term.feed(TabId(1), b"x").is_err());
}

#[test]
fn osc52_rejects_oversized_base64() {
    use crate::clipboard::SelectionKind;

    for (selection, kind) in [
        ("", SelectionKind::Clipboard),
        ("cccc", SelectionKind::Clipboard),
        ("pppp", SelectionKind::Primary),
    ] {
        let header = format!("52;{selection};");
        let encoded = "YWFh".repeat((MAX_OSC_BYTES - header.len()) / 4);
        assert_eq!(header.len() + encoded.len(), MAX_OSC_BYTES);
        for terminator in ["\x07", "\x1b\\"] {
            for chunk_size in [1, 31, MAX_FEED_BYTES] {
                for enabled in [false, true] {
                    let mut term = terminal(1);
                    term.set_osc52_write_enabled(enabled).unwrap();
                    let sequence = format!("\x1b]{header}{encoded}{terminator}");
                    for chunk in sequence.as_bytes().chunks(chunk_size) {
                        feed(&mut term, chunk);
                    }
                    let request = term.take_osc52_write(kind);
                    if enabled {
                        let request = request.unwrap();
                        assert_eq!(request.text, "a".repeat(encoded.len() / 4 * 3));
                        assert_eq!(request.selection, kind);
                    } else {
                        assert!(request.is_none());
                    }

                    // Reject before any terminator arrives, including across feed calls.
                    let oversized = format!("\x1b]{header}{encoded}A");
                    let result = oversized
                        .as_bytes()
                        .chunks(chunk_size)
                        .try_for_each(|chunk| term.feed(TabId(1), chunk));
                    assert_eq!(
                        result,
                        Err(TerminalError {
                            tab: TabId(1),
                            error: AppError::new(Component::Terminal, ErrorKind::InvalidData),
                        })
                    );
                    assert!(term.grid().is_none());
                    assert!(term.take_osc52_write(SelectionKind::Clipboard).is_none());
                    assert!(term.take_osc52_write(SelectionKind::Primary).is_none());
                    assert_eq!(
                        term.feed(TabId(1), terminator.as_bytes()),
                        Err(TerminalError {
                            tab: TabId(1),
                            error: AppError::new(Component::Terminal, ErrorKind::NotConnected),
                        })
                    );
                }
            }
        }
    }

    let mut term = terminal(1);
    term.set_osc52_write_enabled(true).unwrap();
    for bad in ["!", "A", "AA=A", "YQ===", "YWJj?", "/w=="] {
        for selection in ["c", "p"] {
            feed(&mut term, b"\x1b]52;c;YQ==\x07\x1b]52;p;Yg==\x07");
            let sequence = format!("\x1b]52;{selection};{bad}\x07");
            for byte in sequence.as_bytes() {
                feed(&mut term, std::slice::from_ref(byte));
            }
            assert_eq!(
                term.take_osc52_write(SelectionKind::Clipboard)
                    .unwrap()
                    .text,
                "a"
            );
            assert_eq!(
                term.take_osc52_write(SelectionKind::Primary).unwrap().text,
                "b"
            );
        }
    }
    let invalid_at_limit = format!("\x1b]52;;{}\x07", "!".repeat(MAX_OSC_BYTES - 4));
    feed(&mut term, invalid_at_limit.as_bytes());
    assert!(term.take_osc52_write(SelectionKind::Clipboard).is_none());
    assert!(term.take_osc52_write(SelectionKind::Primary).is_none());
    feed(&mut term, b"Q");
    assert_eq!(term.grid().unwrap()[Line(0)][Column(0)].c, 'Q');
}

#[test]
fn osc52_write_needs_explicit_opt_in() {
    use crate::clipboard::SelectionKind;

    let mut first = terminal(1);
    let mut second = terminal(2);
    for selection in ["c", "p", "s", ""] {
        let sequence = format!("\x1b]52;{selection};5Lit5paH\x07");
        feed(&mut first, sequence.as_bytes());
        assert!(first.take_osc52_write(SelectionKind::Clipboard).is_none());
        assert!(first.take_osc52_write(SelectionKind::Primary).is_none());
    }

    first.set_osc52_write_enabled(true).unwrap();
    for (selection, kind) in [
        ("c", SelectionKind::Clipboard),
        ("", SelectionKind::Clipboard),
        ("p", SelectionKind::Primary),
        ("s", SelectionKind::Primary),
    ] {
        for terminator in ["\x07", "\x1b\\"] {
            let sequence = format!("\x1b]52;{selection};5Lit5paH{terminator}");
            for byte in sequence.as_bytes() {
                feed(&mut first, std::slice::from_ref(byte));
            }
            let request = first.take_osc52_write(kind).unwrap();
            assert_eq!(request.tab, TabId(1));
            assert_eq!(request.selection, kind);
            assert_eq!(request.text, "中文");
            assert!(first.take_osc52_write(kind).is_none());
            feed(&mut second, sequence.as_bytes());
            assert!(second.take_osc52_write(kind).is_none());
        }
    }
    feed(
        &mut first,
        b"\x1b]52;c;YQ==\x07\x1b]52;p;Yg==\x07\x1b]52;c;Yw==\x07",
    );
    assert_eq!(
        first
            .take_osc52_write(SelectionKind::Clipboard)
            .unwrap()
            .text,
        "c"
    );
    assert_eq!(
        first.take_osc52_write(SelectionKind::Primary).unwrap().text,
        "b"
    );
    feed(&mut first, b"\x1b]52;c;?\x07\x1b]52;p;?\x07");
    assert!(first.take_osc52_write(SelectionKind::Clipboard).is_none());
    assert!(first.take_osc52_write(SelectionKind::Primary).is_none());
    feed(&mut first, b"\x1bc\x1b]52;c;\x07");
    assert_eq!(
        first
            .take_osc52_write(SelectionKind::Clipboard)
            .unwrap()
            .text,
        ""
    );
    feed(&mut first, b"\x1b]52;c;YQ==\x07\x1b]52;p;Yg==\x07");
    first.set_osc52_write_enabled(false).unwrap();
    assert!(first.take_osc52_write(SelectionKind::Clipboard).is_none());
    assert!(first.take_osc52_write(SelectionKind::Primary).is_none());
    feed(&mut first, b"\x1bc\x1b]52;c;YQ==\x07");
    assert!(first.take_osc52_write(SelectionKind::Clipboard).is_none());
    first.set_osc52_write_enabled(true).unwrap();
    feed(&mut first, b"\x1b]52;c;YQ==\x07");
    let oversized = format!("\x1b]{}", "x".repeat(MAX_OSC_BYTES + 1));
    assert!(first.feed(TabId(1), oversized.as_bytes()).is_err());
    assert!(first.take_osc52_write(SelectionKind::Clipboard).is_none());
    assert!(first.set_osc52_write_enabled(true).is_err());
}

#[test]
fn osc52_read_is_denied_by_default() {
    use alacritty_terminal::event::{Event, EventListener};
    use std::{cell::Cell as Counter, rc::Rc};

    struct Probe(Rc<Counter<(usize, usize)>>);
    impl EventListener for Probe {
        fn send_event(&self, event: Event) {
            let (reads, writes) = self.0.get();
            match event {
                Event::ClipboardLoad(..) => self.0.set((reads + 1, writes)),
                Event::PtyWrite(_) => self.0.set((reads, writes + 1)),
                _ => (),
            }
        }
    }

    assert_eq!(terminal_config(0).osc52, Osc52::Disabled);
    for selection in ["", "c", "p", "s", "cp"] {
        for terminator in ["\x07", "\x1b\\"] {
            let query = format!("\x1b]52;{selection};?{terminator}");
            for chunk_size in [1, query.len()] {
                // The enabled control proves these inputs reach the core's Clipboard Read handler.
                for enabled_control in [false, true] {
                    let events = Rc::new(Counter::new((0, 0)));
                    let mut config = terminal_config(0);
                    if enabled_control {
                        config.osc52 = Osc52::OnlyPaste;
                    }
                    let mut core = Term::new(
                        config,
                        &Size {
                            rows: 2,
                            columns: 4,
                        },
                        Probe(events.clone()),
                    );
                    let mut parser = Processor::<NoSync>::new();
                    for round in 1..=2 {
                        for chunk in query.as_bytes().chunks(chunk_size) {
                            parser.advance(&mut core, chunk);
                        }
                        assert_eq!(events.get(), (if enabled_control { round } else { 0 }, 0));
                        parser.advance(&mut core, b"\x1bc");
                    }
                    parser.advance(&mut core, b"Q");
                    assert_eq!(core.grid()[Line(0)][Column(0)].c, 'Q');
                }
            }
        }
    }
}

#[test]
fn terminal_external_events_are_inert_and_kitty_stays_disabled() {
    let mut term = terminal(1);
    // Clipboard requests are denied; title, device query, and hyperlink have no I/O effects.
    feed(&mut term, b"\x1b]52;c;cHJpdmF0ZQ==\x07\x1b]52;c;?\x07\x1b]2;private\x07\x1b[5n\x1b]8;;https://example.invalid\x1b\\");
    for _ in 0..128 {
        feed(&mut term, b"\x1b[>31u");
    }
    assert!(
        !term
            .mode()
            .unwrap()
            .intersects(TermMode::KITTY_KEYBOARD_PROTOCOL)
    );
    feed(&mut term, b"Q");
    assert_eq!(term.grid().unwrap()[Line(0)][Column(0)].c, 'Q');
}

#[test]
fn terminal_modes_track_sequences() {
    let tracked = TermMode::ALT_SCREEN | TermMode::BRACKETED_PASTE;
    let mut term = terminal(1);
    let mut other = terminal(2);
    assert_eq!(*term.mode().unwrap() & tracked, TermMode::empty());
    let steps: &[(&[u8], TermMode)] = &[
        (b"\x1b[?2004h", TermMode::BRACKETED_PASTE),
        (b"\x1b[?1049h", tracked),
        (b"\x1b[?1049h", tracked),
        (b"\x1b[?2004l", TermMode::ALT_SCREEN),
        (b"\x1b[?1049l", TermMode::empty()),
        (b"\x1b[?1049l", TermMode::empty()),
        (b"\x1b[?1049;2004h", tracked),
        (b"\x1b[?1049l", TermMode::BRACKETED_PASTE),
        (b"\x1b[?1049h", tracked),
        (b"\x1b[?1049;2004l", TermMode::empty()),
        (b"\x1b[?2004;1049h", tracked),
        (b"\x1bc", TermMode::empty()),
    ];
    for &(sequence, expected) in steps {
        let before = *term.mode().unwrap() & tracked;
        // An incomplete control sequence must not expose its future mode.
        for byte in &sequence[..sequence.len() - 1] {
            feed(&mut term, std::slice::from_ref(byte));
            assert_eq!(*term.mode().unwrap() & tracked, before);
        }
        feed(&mut term, &sequence[sequence.len() - 1..]);
        assert_eq!(*term.mode().unwrap() & tracked, expected);
        assert_eq!(*other.mode().unwrap() & tracked, TermMode::empty());
    }

    feed(&mut term, b"M\x1b[?2004;1049h\x1b[HA");
    assert!(term.mode().unwrap().contains(tracked));
    assert_eq!(term.grid().unwrap()[Line(0)][Column(0)].c, 'A');
    feed(&mut term, b"\x1b[?1049l");
    assert_eq!(*term.mode().unwrap() & tracked, TermMode::BRACKETED_PASTE);
    assert_eq!(term.grid().unwrap()[Line(0)][Column(0)].c, 'M');
    feed(&mut other, b"\x1b[?1049h");
    assert_eq!(*other.mode().unwrap() & tracked, TermMode::ALT_SCREEN);
    assert_eq!(*term.mode().unwrap() & tracked, TermMode::BRACKETED_PASTE);
}

#[test]
fn terminal_modes_preserve_state_on_rejection_and_disappear_on_stop() {
    let mut term = terminal(1);
    feed(&mut term, b"\x1b[?1049;2004h");
    let before = *term.mode().unwrap();
    assert!(term.feed(TabId(2), b"\x1b[?1049;2004l").is_err());
    assert!(
        term.feed(TabId(1), &vec![b'x'; MAX_FEED_BYTES + 1])
            .is_err()
    );
    assert_eq!(*term.mode().unwrap(), before);
    feed(&mut term, b"\x1b]2;");
    assert!(term.feed(TabId(1), &vec![b'x'; MAX_OSC_BYTES]).is_err());
    assert_eq!(term.mode(), None);
    assert!(term.feed(TabId(1), b"\x1b[?2004h").is_err());
    assert_eq!(term.mode(), None);
}

#[test]
fn terminal_osc_guard_covers_vte_entry_sequences() {
    use alacritty_terminal::vte::{Parser, Perform};
    #[derive(Default)]
    struct ObserveOsc(bool);
    impl Perform for ObserveOsc {
        fn osc_dispatch(&mut self, _: &[&[u8]], _: bool) {
            self.0 = true;
        }
    }
    // Every byte after ESC, including ignored C0/DEL/high bytes and intermediates.
    for byte in 0..=u8::MAX {
        let input = [0x1b, byte, b']', b'2', b';', b'x', 0x07];
        let mut parser = Parser::new();
        let mut observer = ObserveOsc::default();
        parser.advance(&mut observer, &input);
        let mut guard = OscLimit::Ground;
        for byte in &input[..input.len() - 1] {
            assert!(guard.accept(*byte));
        }
        if observer.0 {
            assert!(
                matches!(guard, OscLimit::String(_)),
                "missed OSC after {byte}"
            );
        }
    }
}

#[test]
fn selection_content_epoch_never_reuses_an_old_generation() {
    let mut terminal = super::Terminal::with_scrollback(crate::TabId(1), 2, 8, 0).unwrap();
    terminal.content_epoch = Some(u64::MAX - 1);
    terminal.feed(crate::TabId(1), b"x").unwrap();
    assert_eq!(terminal.content_epoch(), Some(u64::MAX));
    terminal.feed(crate::TabId(1), b"y").unwrap();
    assert_eq!(terminal.content_epoch(), None);
    terminal.resize(3, 8).unwrap();
    assert_eq!(terminal.content_epoch(), None);
}

#[test]
fn terminal_copy_preserves_tabstops_and_selected_spaces() {
    use alacritty_terminal::{
        index::{Column, Line, Point},
        selection::SelectionRange,
    };
    let id = crate::TabId(1);
    for (width, input, start, end, want) in [
        (20, "a\tB  ", (0, 0), (0, 10), "a\tB  "),
        (
            20,
            "\x1b[3g\x1b[1;6H\x1bH\ra\tB  ",
            (0, 0),
            (0, 7),
            "a\tB  ",
        ),
        (20, "a\tB", (0, 2), (0, 7), "      "),
        (20, "a\tB", (0, 1), (0, 4), "\t"),
        (6, "a\t ", (0, 0), (0, 5), "a\t "),
        (9, "a\t ", (0, 0), (0, 8), "a\t "),
        (6, "\x1b[3ga\tB", (0, 0), (0, 5), "a\tB"),
        (16, "e\u{301}\t繁  ", (0, 0), (0, 11), "e\u{301}\t繁  "),
        (4, "abcde\r\nf ", (0, 0), (2, 1), "abcde   \nf "),
        (
            16,
            "a\tB\x1b[1;4H \u{301}",
            (0, 0),
            (0, 8),
            "a\t \u{301}    B",
        ),
    ] {
        let mut terminal = super::Terminal::with_scrollback(id, 3, width, 0).unwrap();
        terminal.feed(id, input.as_bytes()).unwrap();
        let cursor = terminal.grid().unwrap().cursor.clone();
        let rows: Vec<_> = (0..3)
            .map(|row| terminal.grid().unwrap()[Line(row)].clone())
            .collect();
        let epoch = terminal.content_epoch();
        let range = SelectionRange::new(
            Point::new(Line(start.0), Column(start.1)),
            Point::new(Line(end.0), Column(end.1)),
            false,
        );
        assert_eq!(terminal.copy_range_text(range).unwrap(), want);
        assert_eq!(terminal.grid().unwrap().cursor, cursor);
        assert_eq!(terminal.content_epoch(), epoch);
        for (row, original) in rows.iter().enumerate() {
            assert_eq!(&terminal.grid().unwrap()[Line(row as i32)], original);
        }
    }
    let mut terminal = super::Terminal::with_scrollback(id, 2, 6, 0).unwrap();
    terminal.feed(id, b"a\t ").unwrap();
    let range = SelectionRange::new(
        Point::new(Line(0), Column(0)),
        Point::new(Line(0), Column(5)),
        false,
    );
    assert!(terminal.grid().unwrap().cursor.input_needs_wrap);
    assert_eq!(terminal.copy_range_text(range).unwrap(), "a\t ");
    terminal.feed(id, b"Z").unwrap();
    assert_eq!(terminal.grid().unwrap()[Line(1)][Column(0)].c, 'Z');
}
