use super::DragSelection;
use crate::TabId;
use alacritty_terminal::{
    index::{Column, Line, Point},
    selection::SelectionRange,
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
