use super::{MAX_TOTAL_TAB_CELLS, TabPty, TabRegistry};
use crate::error::{AppError, Component};
use std::io::ErrorKind;

impl<P: TabPty> TabRegistry<P> {
    /// A first configure may fit the surface budget but exceed the grid/history
    /// budget. Start a clipped 24x80 (or smaller) grid instead of losing startup.
    pub(crate) fn initial_size(&self, rows: u32, columns: u32) -> (u16, u16) {
        let requested = u16::try_from(rows).ok().zip(u16::try_from(columns).ok());
        requested
            .filter(|&(rows, columns)| {
                crate::terminal::cell_reservation(rows, columns, self.scrollback_lines).is_some()
            })
            .unwrap_or((rows.clamp(1, 24) as u16, columns.clamp(2, 80) as u16))
    }

    /// Raster metrics remain valid even when a requested grid resize is rejected.
    pub(crate) fn set_cell_pixels(&mut self, cell: (u32, u32)) {
        for tab in &mut self.entries {
            tab.terminal.set_cell_pixels(cell);
        }
    }

    /// Validate every grid and the combined reservation before mutating any Tab.
    pub fn resize_all(&mut self, rows: u16, columns: u16) -> Result<bool, AppError> {
        let invalid = || AppError::new(Component::Terminal, ErrorKind::InvalidInput);
        crate::terminal::cell_reservation(rows, columns, self.scrollback_lines)
            .ok_or_else(invalid)?;
        // The core retains Vec row capacities on shrink. Keep conservative high-water
        // dimensions until close instead of lending retained capacity to new Tabs.
        let reservations: Vec<_> = self
            .entries
            .iter()
            .map(|tab| {
                crate::terminal::cell_reservation(
                    rows.max(tab.reserved_rows),
                    columns.max(tab.reserved_columns),
                    self.scrollback_lines,
                )
                .ok_or_else(invalid)
            })
            .collect::<Result<_, _>>()?;
        if reservations
            .iter()
            .try_fold(0usize, |sum, value| sum.checked_add(*value))
            .is_none_or(|total| total > MAX_TOTAL_TAB_CELLS)
            || self
                .entries
                .iter()
                .any(|tab| tab.terminal.snapshot().is_none())
        {
            return Err(invalid());
        }
        let mut changed = false;
        for (tab, reservation) in self.entries.iter_mut().zip(reservations) {
            let snapshot = tab.terminal.snapshot().ok_or_else(invalid)?;
            if snapshot.rows != usize::from(rows) || snapshot.columns != usize::from(columns) {
                tab.terminal
                    .resize(rows, columns)
                    .map_err(|error| error.error)?;
                tab.reserved_cells = reservation;
                tab.reserved_rows = tab.reserved_rows.max(rows);
                tab.reserved_columns = tab.reserved_columns.max(columns);
                tab.dirty = true;
                changed = true;
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, tabs::tests::FakePty};
    #[test]
    fn runtime_resize_updates_tab_budgets() {
        let mut tabs = TabRegistry::<FakePty>::with_scrollback(10_000).unwrap();
        for id in 1..=5 {
            tabs.insert(TabId(id), 2, 8).unwrap();
        }
        assert!(tabs.resize_all(20, 80).unwrap());
        assert_eq!(
            tabs.reserved_cells(),
            5 * crate::terminal::cell_reservation(20, 80, 10_000).unwrap()
        );
        assert!(!tabs.resize_all(20, 80).unwrap());
        let reservation = tabs.reserved_cells();
        // Even under a rejected configure, a newly inserted Tab receives actual cell pixels.
        tabs.insert(TabId(6), 2, 8).unwrap();
        tabs.set_cell_pixels((9, 21));
        assert!(tabs.resize_all(20, 150).is_err());
        let newest = tabs.get_mut(TabId(6)).unwrap().terminal_mut();
        newest.feed(TabId(6), b"\x1b[14t").unwrap();
        assert_eq!(newest.take_core_replies().0, b"\x1b[4;42;72t");
        tabs.close(TabId(6));
        assert!(tabs.resize_all(20, 150).is_err()); // Each fits, combined budget does not.
        assert!(tabs.resize_all(0, 80).is_err());
        assert_eq!(tabs.reserved_cells(), reservation);
        for id in tabs.ids() {
            let snapshot = tabs.get(id).unwrap().terminal().snapshot().unwrap();
            assert_eq!((snapshot.rows, snapshot.columns), (20, 80));
        }
        assert!(tabs.resize_all(10, 40).unwrap());
        assert_eq!(
            tabs.reserved_cells(),
            reservation,
            "shrinking must not release retained core row capacity"
        );
    }
}
