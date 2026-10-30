use super::DragSelection;
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
