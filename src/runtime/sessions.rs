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

/// Bounded exit metadata, never terminal payloads. EOF policy is close-after-drain.
#[derive(Default)]
pub(super) struct ExitHistory(
    pub std::collections::VecDeque<(TabId, Option<std::process::ExitStatus>)>,
);
impl ExitHistory {
    pub fn record<P: TabPty>(
        &mut self,
        tabs: &mut TabRegistry<P>,
        id: TabId,
        status: Option<std::process::ExitStatus>,
    ) {
        if tabs.get(id).is_none() || self.0.iter().any(|(old, _)| *old == id) {
            return;
        }
        if let Some(status) = status {
            tabs.apply_effect(&crate::AppEffect::PtyExited { tab: id, status });
        }
        if self.0.len() == crate::tabs::MAX_TABS {
            self.0.pop_front();
        }
        self.0.push_back((id, status));
    }
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
    fn runtime_child_exit_is_per_tab() {
        use std::os::unix::process::ExitStatusExt;
        let mut tabs = TabRegistry::with_scrollback(0).unwrap();
        for _ in 0..2 {
            spawn_tab(&mut tabs, 2, 20, |r| Ok(Fake(r.tab))).unwrap();
        }
        tabs.get_mut(TabId(2))
            .unwrap()
            .terminal_mut()
            .feed(TabId(2), b"final output")
            .unwrap();
        let mut history = ExitHistory::default();
        history.record(
            &mut tabs,
            TabId(2),
            Some(std::process::ExitStatus::from_raw(7 << 8)),
        );
        assert_eq!(
            tabs.get(TabId(2)).unwrap().exit_status().unwrap().code(),
            Some(7)
        );
        assert_eq!(
            tabs.get(TabId(2))
                .unwrap()
                .terminal()
                .grid()
                .unwrap()
                .cursor
                .point
                .column
                .0,
            12
        );
        assert!(tabs.get(TabId(1)).unwrap().exit_status().is_none());
        close_tab(&mut tabs, TabId(2));
        history.record(&mut tabs, TabId(2), None);
        assert_eq!(history.0.len(), 1);
        assert_eq!(history.0[0].1.unwrap().code(), Some(7));
        assert_eq!(tabs.active(), Some(TabId(1)));
        assert!(tabs.get(TabId(1)).unwrap().pty().is_some());
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
