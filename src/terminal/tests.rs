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
fn terminal_external_events_are_inert_and_kitty_stays_disabled() {
    let mut term = terminal(1);
    // Clipboard requests, title, device query, and hyperlink have no I/O listener.
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
