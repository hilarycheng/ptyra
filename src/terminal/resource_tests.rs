use super::*;
use alacritty_terminal::index::Line;

mod spare;

#[test]
fn runtime_resource_caps_apply_to_actual_owners() {
    // UI overlays never resize: no speculative 1000-row cache charged to construction.
    let overlay = Terminal::with_scrollback(TabId(0), 2, 1024, 0).unwrap();
    assert_eq!(overlay.allocated_cells(), 4096);
    for history in [0, 10] {
        let mut terminal = Terminal::with_scrollback(TabId(1), 2, 80, history).unwrap();
        terminal.feed(TabId(1), b"hello").unwrap();
        terminal.resize(3, 81).unwrap();
        let high = terminal.allocated_cells();
        assert!(high >= 4 * 81 * (6 + history + 2000));
        terminal.resize(2, 40).unwrap();
        assert_eq!(terminal.allocated_cells(), high);
        let core = terminal.core.as_mut().unwrap();
        let mut active_capacity = 0;
        for _ in 0..2 {
            let grid = core.term.grid_mut();
            for row in grid.topmost_line().0..=grid.bottommost_line().0 {
                // Public core operation returns the old empty Vec without unsafe introspection.
                active_capacity += grid[Line(row)].front_split_off(0).capacity();
            }
            core.term.swap_alt();
        }
        assert!(active_capacity <= high);
    }
    let mut wide = Terminal::with_scrollback(TabId(2), 1, 32768, 0).unwrap();
    for _ in 0..2 {
        wide.feed(TabId(2), &vec![b'x'; MAX_FEED_BYTES]).unwrap();
    }
    let before = wide.allocated_cells();
    assert!(
        wide.resize(1, 2).is_err(),
        "reject quadratic split capacity before reflow"
    );
    assert!(
        wide.resize(2, 16384).is_err(),
        "height grows at old width before columns shrink"
    );
    assert_eq!(wide.allocated_cells(), before);
    assert_eq!(wide.grid().unwrap().columns(), 32768);
    assert_eq!(
        wide.grid().unwrap()[Line(0)][alacritty_terminal::index::Column(0)].c,
        'x'
    );
}
