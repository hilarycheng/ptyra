use super::{
    DragSelection, Screen, TabSelections, copy_logical_lines, copy_text, join_logical_lines,
    whole_glyph_range,
};
use crate::TabId;
use crate::terminal::Terminal;
use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    selection::SelectionRange,
    term::cell::Flags,
};

fn point(line: i32, column: usize) -> Point {
    Point::new(Line(line), Column(column))
}

#[test]
fn drag_selection_normalizes_direction() {
    let tab = TabId(7);
    for (anchor, end) in [(point(0, 1), point(0, 4)), (point(-2, 4), point(1, 1))] {
        let mut forward = DragSelection::new(tab);
        let mut reverse = DragSelection::new(tab);
        assert_eq!(
            forward.press(tab, anchor),
            Some(SelectionRange::new(anchor, anchor, false))
        );
        assert_eq!(
            reverse.press(tab, end),
            Some(SelectionRange::new(end, end, false))
        );
        let expected = SelectionRange::new(anchor, end, false);
        assert_eq!(forward.motion(tab, end), Some(expected));
        assert_eq!(reverse.motion(tab, anchor), Some(expected));
        assert_eq!(forward.release(tab, end), Some(expected));
        assert_eq!(reverse.release(tab, anchor), Some(expected));
        assert_eq!(forward.range(), reverse.range());
        assert_eq!(forward.motion(tab, point(3, 3)), None);
        assert_eq!(forward.range(), Some(expected));
    }
}

#[test]
fn drag_selection_rejects_other_tab_and_cancels_stale_drag() {
    let tab = TabId(7);
    let other = TabId(8);
    let start = point(0, 1);
    let end = point(1, 3);
    let mut selection = DragSelection::new(tab);
    assert_eq!(selection.motion(tab, end), None);
    assert_eq!(selection.release(tab, end), None);
    assert_eq!(selection.press(other, start), None);
    assert_eq!(selection.range(), None);
    selection.press(tab, start);
    assert_eq!(selection.motion(other, end), None);
    assert_eq!(selection.release(other, end), None);
    assert_eq!(
        selection.range(),
        Some(SelectionRange::new(start, start, false))
    );
    selection.motion(tab, end);
    selection.cancel(); // Focus or active Tab changed before release.
    assert_eq!(selection.range(), None);
    assert_eq!(selection.release(tab, end), None);

    selection.press(tab, start);
    selection.release(tab, end);
    let finalized = SelectionRange::new(start, end, false);
    selection.cancel(); // A completed selection survives a later focus change.
    assert_eq!(selection.range(), Some(finalized));
    selection.press(tab, end);
    assert_eq!(
        selection.range(),
        Some(SelectionRange::new(end, end, false))
    );
}

#[test]
fn double_click_selects_unicode_word() {
    let tab = TabId(7);
    let mut terminal = Terminal::new(tab, 2, 24).unwrap();
    terminal
        .feed(tab, "ab_3 繁體 αβZ e\u{301}!".as_bytes())
        .unwrap();
    let grid = terminal.grid().unwrap();
    assert!(grid[point(0, 5)].flags.contains(Flags::WIDE_CHAR));
    assert!(grid[point(0, 6)].flags.contains(Flags::WIDE_CHAR_SPACER));
    assert_eq!(grid[point(0, 14)].zerowidth(), Some(&['\u{301}'][..]));
    let mut selection = DragSelection::new(tab);
    for (column, start, end) in [
        (0, 0, 3),
        (3, 0, 3),
        (4, 4, 4),
        (5, 5, 5),
        (6, 5, 5),
        (7, 7, 7),
        (10, 10, 11),
        (11, 10, 11),
        (12, 12, 12),
        (14, 14, 14),
        (15, 15, 15),
    ] {
        let expected = SelectionRange::new(point(0, start), point(0, end), false);
        assert_eq!(
            selection.double_click(tab, grid, point(0, column)),
            Some(expected)
        );
        assert_eq!(selection.range(), Some(expected));
    }
    let previous = selection.range();
    assert_eq!(selection.double_click(TabId(8), grid, point(0, 0)), None);
    for invalid in [
        point(-1, 0),
        point(2, 0),
        point(0, grid.columns()),
        point(0, usize::MAX),
    ] {
        assert_eq!(selection.double_click(tab, grid, invalid), None);
        assert_eq!(selection.range(), previous);
    }
}

#[test]
fn double_click_stops_at_line_and_ignores_wrap_placeholder() {
    let tab = TabId(1);
    let mut terminal = Terminal::new(tab, 2, 4).unwrap();
    terminal.feed(tab, "abc繁".as_bytes()).unwrap();
    let grid = terminal.grid().unwrap();
    assert!(
        grid[point(0, 3)]
            .flags
            .contains(Flags::LEADING_WIDE_CHAR_SPACER)
    );
    let mut selection = DragSelection::new(tab);
    assert_eq!(selection.double_click(tab, grid, point(0, 3)), None);
    assert_eq!(
        selection.double_click(tab, grid, point(0, 2)),
        Some(SelectionRange::new(point(0, 0), point(0, 2), false))
    );
    assert_eq!(
        selection.double_click(tab, grid, point(1, 0)),
        Some(SelectionRange::new(point(1, 0), point(1, 0), false))
    );
    assert!(grid[point(1, 0)].flags.contains(Flags::WIDE_CHAR));
}

#[test]
fn triple_click_selects_logical_line() {
    let tab = TabId(1);
    let mut terminal = Terminal::new(tab, 3, 4).unwrap();
    terminal.feed(tab, b"abcdEF\r\nGH").unwrap();
    let grid = terminal.grid().unwrap();
    assert!(grid[point(0, 3)].flags.contains(Flags::WRAPLINE));
    assert!(!grid[point(1, 3)].flags.contains(Flags::WRAPLINE));
    let mut selection = DragSelection::new(tab);
    let wrapped = SelectionRange::new(point(0, 0), point(1, 3), false);
    for clicked in [point(0, 2), point(1, 1)] {
        assert_eq!(selection.triple_click(tab, grid, clicked), Some(wrapped));
        assert_eq!(selection.range(), Some(wrapped));
    }
    let hard_line = SelectionRange::new(point(2, 0), point(2, 3), false);
    assert_eq!(
        selection.triple_click(tab, grid, point(2, 1)),
        Some(hard_line)
    );
    assert_eq!(selection.triple_click(TabId(2), grid, point(0, 0)), None);
    for invalid in [point(-1, 0), point(3, 0), point(0, 4), point(0, usize::MAX)] {
        assert_eq!(selection.triple_click(tab, grid, invalid), None);
        assert_eq!(selection.range(), Some(hard_line));
    }
}

#[test]
fn triple_click_respects_scrollback_edges() {
    let tab = TabId(1);
    let mut terminal = Terminal::with_scrollback(tab, 2, 4, 4).unwrap();
    terminal.feed(tab, b"AAAA\r\nBBBB\r\nCCCC").unwrap();
    let grid = terminal.grid().unwrap();
    assert_eq!(grid.topmost_line(), Line(-1));
    let mut selection = DragSelection::new(tab);
    assert_eq!(
        selection.triple_click(tab, grid, point(-1, 2)),
        Some(SelectionRange::new(point(-1, 0), point(-1, 3), false))
    );
    assert_eq!(
        selection.triple_click(tab, grid, point(0, 2)),
        Some(SelectionRange::new(point(0, 0), point(0, 3), false))
    );
    assert_eq!(
        selection.triple_click(tab, grid, point(1, 2)),
        Some(SelectionRange::new(point(1, 0), point(1, 3), false))
    );
    assert_eq!(selection.triple_click(tab, grid, point(-2, 0)), None);

    let mut wrapped = Terminal::with_scrollback(tab, 2, 4, 4).unwrap();
    wrapped.feed(tab, b"abcdEF\r\nGH").unwrap();
    let grid = wrapped.grid().unwrap();
    assert_eq!(grid.topmost_line(), Line(-1));
    assert!(grid[point(-1, 3)].flags.contains(Flags::WRAPLINE));
    assert_eq!(
        selection.triple_click(tab, grid, point(0, 1)),
        Some(SelectionRange::new(point(-1, 0), point(0, 3), false))
    );
}

#[test]
fn copy_soft_wrap_does_not_add_newline() {
    let tab = TabId(1);
    let mut terminal = Terminal::new(tab, 3, 4).unwrap();
    terminal.feed(tab, b"abcdEFGH\r\nIJ").unwrap();
    let grid = terminal.grid().unwrap();
    let full = SelectionRange::new(point(0, 0), point(1, 3), false);
    assert_eq!(
        copy_logical_lines(grid, full),
        Some(vec!["abcdEFGH".into()])
    );
    let partial = SelectionRange::new(point(0, 2), point(1, 1), false);
    assert_eq!(copy_logical_lines(grid, partial), Some(vec!["cdEF".into()]));
    let mut reverse = DragSelection::new(tab);
    reverse.press(tab, point(1, 1));
    let normalized = reverse.release(tab, point(0, 2)).unwrap();
    assert_eq!(
        copy_logical_lines(grid, normalized),
        Some(vec!["cdEF".into()])
    );
    let hard_boundary = SelectionRange::new(point(0, 0), point(2, 1), false);
    assert_eq!(
        copy_logical_lines(grid, hard_boundary),
        Some(vec!["abcdEFGH".into(), "IJ".into()])
    );
    let mut spaced = Terminal::new(tab, 1, 4).unwrap();
    spaced.feed(tab, b"a b ").unwrap();
    assert_eq!(
        copy_logical_lines(
            spaced.grid().unwrap(),
            SelectionRange::new(point(0, 0), point(0, 3), false)
        ),
        Some(vec!["a b ".into()])
    );
}

#[test]
fn copy_soft_wrap_preserves_unicode_and_validates_range() {
    let tab = TabId(1);
    let mut terminal = Terminal::with_scrollback(tab, 2, 4, 2).unwrap();
    terminal.feed(tab, "繁e\u{301}ZQ\r\nR".as_bytes()).unwrap();
    let grid = terminal.grid().unwrap();
    assert_eq!(grid.topmost_line(), Line(-1));
    assert_eq!(
        copy_logical_lines(grid, SelectionRange::new(point(-1, 0), point(0, 0), false)),
        Some(vec!["繁e\u{301}ZQ".into()])
    );
    assert_eq!(
        copy_logical_lines(grid, SelectionRange::new(point(-1, 1), point(-1, 2), false)),
        Some(vec!["繁e\u{301}".into()])
    );
    for range in [
        SelectionRange::new(point(-2, 0), point(-1, 0), false),
        SelectionRange::new(point(0, 0), point(2, 0), false),
        SelectionRange::new(point(0, 4), point(0, 4), false),
        SelectionRange::new(point(0, usize::MAX), point(0, usize::MAX), false),
        SelectionRange {
            start: point(0, 3),
            end: point(0, 1),
            is_block: false,
        },
        SelectionRange::new(point(0, 0), point(0, 0), true),
    ] {
        assert_eq!(copy_logical_lines(grid, range), None);
    }
}

#[test]
fn copy_hard_break_preserves_newline() {
    let tab = TabId(1);
    let mut terminal = Terminal::new(tab, 3, 4).unwrap();
    terminal.feed(tab, b"abcdEFGH\r\nIJ").unwrap();
    let grid = terminal.grid().unwrap();
    assert_eq!(
        copy_text(grid, SelectionRange::new(point(0, 0), point(2, 1), false)),
        Some("abcdEFGH\nIJ".into())
    );
    assert_eq!(
        copy_text(grid, SelectionRange::new(point(0, 2), point(2, 1), false)),
        Some("cdEFGH\nIJ".into())
    );
    assert_eq!(
        copy_text(grid, SelectionRange::new(point(0, 1), point(1, 2), false)),
        Some("bcdEFG".into())
    );
    let mut blank = Terminal::new(tab, 3, 4).unwrap();
    blank.feed(tab, b"A\r\n\r\nB").unwrap();
    assert_eq!(
        copy_text(
            blank.grid().unwrap(),
            SelectionRange::new(point(0, 0), point(2, 0), false)
        ),
        Some("A   \n    \nB".into())
    );
}

#[test]
fn copy_hard_break_keeps_unicode_scrollback_and_caps_separators() {
    let tab = TabId(1);
    let mut terminal = Terminal::with_scrollback(tab, 2, 4, 2).unwrap();
    terminal
        .feed(tab, "繁e\u{301}ZQWER\r\nR".as_bytes())
        .unwrap();
    let grid = terminal.grid().unwrap();
    assert_eq!(
        copy_text(grid, SelectionRange::new(point(-1, 0), point(1, 0), false)),
        Some("繁e\u{301}ZQWER\nR".into())
    );
    assert_eq!(
        copy_text(grid, SelectionRange::new(point(-2, 0), point(0, 0), false)),
        None
    );
    let lines = vec!["ab".into(), "".into(), "c".into()];
    assert_eq!(join_logical_lines(&lines, 5), Some("ab\n\nc".into()));
    assert_eq!(join_logical_lines(&lines, 4), None);
}

#[test]
fn selection_preserves_fullwidth_glyph() {
    let tab = TabId(1);
    let mut terminal = Terminal::new(tab, 2, 8).unwrap();
    terminal.feed(tab, "A繁\u{301}Z".as_bytes()).unwrap();
    let grid = terminal.grid().unwrap();
    let full = SelectionRange::new(point(0, 1), point(0, 2), false);
    let mut forward = DragSelection::new(tab);
    forward.press(tab, point(0, 1));
    forward.release(tab, point(0, 2));
    assert_eq!(forward.range_preserving_wide(grid), Some(full));
    let mut reverse = DragSelection::new(tab);
    reverse.press(tab, point(0, 2));
    reverse.release(tab, point(0, 1));
    assert_eq!(reverse.range_preserving_wide(grid), Some(full));
    assert_eq!(copy_text(grid, full), Some("繁\u{301}".into()));
    assert_eq!(
        whole_glyph_range(grid, SelectionRange::new(point(0, 0), point(0, 1), false)),
        Some(SelectionRange::new(point(0, 0), point(0, 2), false))
    );

    let mut wrapped = Terminal::new(tab, 2, 4).unwrap();
    wrapped.feed(tab, "abc繁\u{301}".as_bytes()).unwrap();
    let grid = wrapped.grid().unwrap();
    assert_eq!(
        whole_glyph_range(grid, SelectionRange::new(point(0, 3), point(0, 3), false)),
        Some(SelectionRange::new(point(1, 0), point(1, 1), false))
    );
    assert_eq!(
        copy_text(grid, SelectionRange::new(point(0, 3), point(0, 3), false)),
        Some("繁\u{301}".into())
    );
    assert_eq!(
        whole_glyph_range(grid, SelectionRange::new(point(-1, 0), point(0, 0), false)),
        None
    );
}

#[test]
fn selection_is_scoped_to_tab() {
    let first_tab = TabId(1);
    let second_tab = TabId(2);
    let mut terminal = Terminal::new(first_tab, 2, 4).unwrap();
    assert_eq!(Screen::from_mode(terminal.mode().unwrap()), Screen::Main);
    terminal.feed(first_tab, b"\x1b[?1049h").unwrap();
    assert_eq!(
        Screen::from_mode(terminal.mode().unwrap()),
        Screen::Alternate
    );
    terminal.feed(first_tab, b"\x1b[?1049l").unwrap();
    assert_eq!(Screen::from_mode(terminal.mode().unwrap()), Screen::Main);
    let mut first = TabSelections::new(first_tab);
    let mut second = TabSelections::new(second_tab);
    let main = SelectionRange::new(point(0, 1), point(0, 2), false);
    let alternate = SelectionRange::new(point(1, 0), point(1, 1), false);
    first
        .selection_mut(first_tab, Screen::Main)
        .unwrap()
        .press(first_tab, main.start);
    first
        .selection_mut(first_tab, Screen::Main)
        .unwrap()
        .release(first_tab, main.end);
    assert_eq!(first.range(first_tab, Screen::Main), Some(main));
    assert_eq!(first.range(first_tab, Screen::Alternate), None);
    assert_eq!(second.range(second_tab, Screen::Main), None);
    assert!(first.selection_mut(second_tab, Screen::Main).is_none());
    assert_eq!(first.range(second_tab, Screen::Main), None);
    first
        .selection_mut(first_tab, Screen::Alternate)
        .unwrap()
        .press(first_tab, alternate.start);
    first
        .selection_mut(first_tab, Screen::Alternate)
        .unwrap()
        .release(first_tab, alternate.end);
    assert_eq!(first.range(first_tab, Screen::Alternate), Some(alternate));
    assert_eq!(first.range(first_tab, Screen::Main), Some(main));
    second
        .selection_mut(second_tab, Screen::Main)
        .unwrap()
        .press(second_tab, point(0, 0));
    assert_eq!(
        second.range(second_tab, Screen::Main),
        Some(SelectionRange::new(point(0, 0), point(0, 0), false))
    );
    assert_eq!(first.range(first_tab, Screen::Main), Some(main));
}
