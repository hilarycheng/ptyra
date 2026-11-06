//! Transactional native-session creation; the registry owns every terminal and PTY.
use super::Result;
use crate::{
    TabId,
    tabs::{SpawnTab, TabPty, TabRegistry},
};

pub(super) fn spawn_tab<P: TabPty>(
    tabs: &mut TabRegistry<P>,
    rows: u16,
    columns: u16,
    spawn: impl FnOnce(&SpawnTab) -> Result<P>,
) -> Result<TabId> {
    let request = tabs
        .new_tab(rows, columns)
        .map_err(|_| "tab allocation limit reached")?;
    let result =
        spawn(&request).and_then(|pty| tabs.attach_pty(pty).map_err(|_| "invalid spawned tab"));
    if let Err(error) = result {
        tabs.close(request.tab);
        return Err(error);
    }
    Ok(request.tab)
}

/// A stale close does nothing; only the returned session is dropped/reaped.
pub(super) fn close_tab<P: TabPty>(tabs: &mut TabRegistry<P>, id: TabId) -> bool {
    tabs.close(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake(TabId);
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            self.0
        }
    }
    #[test]
    fn runtime_close_tab_preserves_other_sessions() {
        use std::{cell::RefCell, rc::Rc};
        struct Owned(TabId, Rc<RefCell<Vec<TabId>>>);
        impl TabPty for Owned {
            fn tab(&self) -> TabId {
                self.0
            }
        }
        impl Drop for Owned {
            fn drop(&mut self) {
                self.1.borrow_mut().push(self.0);
            }
        }
        let dropped = Rc::new(RefCell::new(Vec::new()));
        let mut tabs = TabRegistry::with_scrollback(0).unwrap();
        for _ in 0..3 {
            spawn_tab(&mut tabs, 2, 8, |request| {
                Ok(Owned(request.tab, dropped.clone()))
            })
            .unwrap();
        }
        tabs.activate(TabId(2));
        assert!(close_tab(&mut tabs, TabId(2)));
        assert_eq!(*dropped.borrow(), [TabId(2)]);
        assert_eq!(tabs.active(), Some(TabId(3)));
        assert!(!close_tab(&mut tabs, TabId(2)));
        assert!(!close_tab(&mut tabs, TabId(99)));
        assert_eq!(tabs.ids().collect::<Vec<_>>(), [TabId(1), TabId(3)]);
        close_tab(&mut tabs, TabId(3));
        close_tab(&mut tabs, TabId(1));
        assert!(tabs.active().is_none());
        assert_eq!(tabs.reserved_cells(), 0);
        assert_eq!(
            spawn_tab(&mut tabs, 2, 8, |r| Ok(Owned(r.tab, dropped.clone()))).unwrap(),
            TabId(4)
        );
    }
    #[test]
    fn runtime_new_tab_spawns_one_session() {
        let mut tabs = TabRegistry::with_scrollback(12).unwrap();
        let mut calls = 0;
        let first = spawn_tab(&mut tabs, 4, 20, |request| {
            calls += 1;
            Ok(Fake(request.tab))
        })
        .unwrap();
        let second = spawn_tab(&mut tabs, 4, 20, |request| {
            calls += 1;
            Ok(Fake(request.tab))
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_ne!(first, second);
        assert_eq!(tabs.active(), Some(first));
        assert_eq!(
            tabs.reserved_cells(),
            2 * crate::terminal::cell_reservation(4, 20, 12).unwrap()
        );
        tabs.get_mut(second)
            .unwrap()
            .terminal_mut()
            .feed(second, b"second")
            .unwrap();
        assert_eq!(
            tabs.get(first)
                .unwrap()
                .terminal()
                .grid()
                .unwrap()
                .cursor
                .point
                .column
                .0,
            0
        );
        let reserved = tabs.reserved_cells();
        assert!(spawn_tab(&mut tabs, 4, 20, |_| Err("spawn failed")).is_err());
        assert_eq!(tabs.ids().collect::<Vec<_>>(), [first, second]);
        assert_eq!(tabs.reserved_cells(), reserved);
        assert_eq!(tabs.active(), Some(first));
    }
}
