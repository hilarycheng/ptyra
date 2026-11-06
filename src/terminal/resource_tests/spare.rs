use super::super::*;
use alacritty_terminal::{
    grid::GridCell,
    index::{Column, Line},
    term::cell::ResetDiscriminant,
};
use std::cell::Cell as Counter;

thread_local! {
    static LIVE: Counter<usize> = const { Counter::new(0) };
}

#[derive(PartialEq)]
struct CountedCell {
    flags: Flags,
}

impl Default for CountedCell {
    fn default() -> Self {
        LIVE.with(|live| live.set(live.get() + 1));
        Self {
            flags: Flags::empty(),
        }
    }
}

impl Clone for CountedCell {
    fn clone(&self) -> Self {
        LIVE.with(|live| live.set(live.get() + 1));
        Self { flags: self.flags }
    }
}

impl Drop for CountedCell {
    fn drop(&mut self) {
        LIVE.with(|live| live.set(live.get() - 1));
    }
}

impl ResetDiscriminant<()> for CountedCell {
    fn discriminant(&self) {}
}

impl GridCell for CountedCell {
    fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }
    fn reset(&mut self, template: &Self) {
        self.flags = template.flags;
    }
    fn flags(&self) -> &Flags {
        &self.flags
    }
    fn flags_mut(&mut self) -> &mut Flags {
        &mut self.flags
    }
}

#[test]
fn runtime_resource_caps_apply_to_actual_owners() {
    assert_eq!(LIVE.with(Counter::get), 0);
    let mut terminal = Terminal::with_scrollback(TabId(1), 2, 80, 0).unwrap();
    // The same pinned Grid owners as Term, with instrumented cell lifetimes.
    let mut main = Grid::<CountedCell>::new(2, 80, 0);
    let mut alternate = Grid::<CountedCell>::new(2, 80, 0);
    // Each Grid also owns current/saved cursor template cells outside row storage.
    let row_cells = || LIVE.with(Counter::get) - 4;
    assert_eq!(row_cells(), terminal.allocated_cells());

    for rows in [3, 2, 4, 1, 3] {
        let (retained, _) = terminal.resize_allocation(rows, 80).unwrap();
        main.resize(true, usize::from(rows), 80);
        alternate.resize(false, usize::from(rows), 80);
        terminal.resize(rows, 80).unwrap();
        assert_eq!(terminal.allocated_cells(), retained);
        assert!(row_cells() <= retained);
        if rows == 2 {
            assert_eq!(row_cells(), 2 * 1002 * 80);
            assert!(row_cells() > terminal.grid().unwrap().total_lines() * 80 * 2);
        }
    }

    terminal.feed(TabId(1), b"\x1b[2J\x1b[Hx\xcc\x81").unwrap();
    let extra = std::sync::Arc::downgrade(
        terminal.grid().unwrap()[Line(0)][Column(0)]
            .extra
            .as_ref()
            .unwrap(),
    );
    drop(terminal);
    assert!(
        extra.upgrade().is_none(),
        "closing the real owner releases cells"
    );
    let before_close = LIVE.with(Counter::get);
    drop(main);
    assert_eq!(LIVE.with(Counter::get), before_close / 2);
    drop(alternate);
    assert_eq!(
        LIVE.with(Counter::get),
        0,
        "both spare-row owners release storage"
    );
}

#[test]
fn runtime_resource_caps_apply_to_actual_owners_after_wide_reflow() {
    let mut terminal = Terminal::with_scrollback(TabId(2), 52, 80, 1).unwrap();
    let mut text = "x".repeat(49 * 80 + 29);
    text.push('中');
    text.push_str(&"x".repeat(49 + 2 * 80));
    terminal.feed(TabId(2), text.as_bytes()).unwrap();
    terminal.resize(52, 79).unwrap();
    let budget = terminal.allocated_cells();
    let grid = terminal.core.as_mut().unwrap().term.grid_mut();
    let mut capacity = 0;
    let mut largest = 0;
    for line in grid.topmost_line().0..=grid.bottommost_line().0 {
        let row = &mut grid[Line(line)];
        let allocation = row.front_split_off(0);
        largest = largest.max(allocation.capacity());
        capacity += allocation.capacity();
        // Restore the original allocation without unsafe access or changing cells.
        row.append_front(allocation);
    }
    assert!(
        largest > 2 * 80,
        "wide-spacer tail growth followed by append_front can exceed twice the width"
    );
    assert!(largest <= 4 * 80);
    assert!(capacity <= budget);
}
