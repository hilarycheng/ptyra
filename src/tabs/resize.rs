use super::{MAX_TOTAL_TAB_CELLS, TabPty, TabRegistry};
use crate::error::{AppError, Component};
use std::io::ErrorKind;

impl<P: TabPty> TabRegistry<P> {
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
