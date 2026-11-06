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

pub(super) fn active_title<P: TabPty>(tabs: &TabRegistry<P>) -> &str {
    tabs.active()
        .and_then(|id| tabs.get(id))
        .map(|tab| tab.title())
        .unwrap_or(crate::APP_NAME)
}

/// A stopped Tab's tiny diagnostic is not a replacement shell's initial size.
pub(super) fn new_tab_size<P: TabPty>(
    tabs: &TabRegistry<P>,
    rows: u32,
    columns: u32,
) -> Option<crate::pty::PtySize> {
    let tab = tabs.get(tabs.active()?)?;
    let (rows, columns) = if tab.failure().is_some() {
        tabs.initial_size(rows, columns)
    } else {
        let snapshot = tab.terminal().snapshot()?;
        (snapshot.rows as u16, snapshot.columns as u16)
    };
    Some(crate::pty::PtySize { rows, columns })
}

/// EOF is normal only if the owned child can be reaped successfully.
fn finish_owned_child<P>(
    pty: &mut P,
    mut wait: impl FnMut(
        &mut P,
    ) -> std::result::Result<
        Option<std::process::ExitStatus>,
        crate::error::AppError,
    >,
    terminate: impl FnOnce(&mut P) -> std::result::Result<(), crate::error::AppError>,
) -> std::result::Result<Option<std::process::ExitStatus>, crate::error::AppError> {
    let status = wait(pty)?;
    if status.is_some() {
        return Ok(status);
    }
    terminate(pty)?;
    wait(pty)
}
pub(super) fn finish_child(
    pty: &mut crate::pty::PtySession,
) -> std::result::Result<Option<std::process::ExitStatus>, crate::error::AppError> {
    finish_owned_child(
        pty,
        crate::pty::PtySession::try_wait,
        crate::pty::PtySession::terminate,
    )
}

impl super::Runtime {
    pub(super) fn stop_session(&mut self, id: TabId, failure: crate::tabs::SessionFailure) {
        self.tabs.stop_session(id, failure);
        if self
            .paste
            .as_ref()
            .is_some_and(|paste| paste.target.tab == id)
        {
            self.cancel_paste();
        }
        if self.tabs.active() == Some(id) {
            self.cancel_pointer_drag();
            self.input.retarget(id);
        }
        self.ready_pty.retain(|ready| *ready != id);
        self.paint.dirty = true;
        eprintln!("Ptyra Tab {}: {}", id.0, failure.title());
    }
    pub(super) fn flush_tab(&mut self, id: TabId) {
        let result = self
            .tabs
            .get_mut(id)
            .and_then(|tab| tab.pty_mut())
            .filter(|pty| pty.wants_write())
            .map(|pty| pty.flush_write());
        match result {
            Some(Err(_)) => self.stop_session(id, crate::tabs::SessionFailure::Io),
            Some(Ok(result)) => {
                if let crate::pty::PtyWriteStatus::Written(count) = result.status
                    && let Some(trace) = &mut self.input_trace
                {
                    trace.add(4, count);
                }
            }
            None => {}
        }
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
    fn runtime_bad_stream_does_not_stop_other_tabs_replacement_size() {
        let mut tabs = TabRegistry::<Fake>::default();
        spawn_tab(&mut tabs, 24, 80, |r| Ok(Fake(r.tab))).unwrap();
        assert!(tabs.resize_all(38, 350).is_err());
        tabs.stop_session(TabId(1), crate::tabs::SessionFailure::Stream);
        assert!(tabs.resize_all(38, 350).is_err());
        let size = new_tab_size(&tabs, 38, 350).unwrap();
        assert_eq!(
            size,
            crate::pty::PtySize {
                rows: 24,
                columns: 80
            }
        );
        let fresh = spawn_tab(&mut tabs, size.rows, size.columns, |r| Ok(Fake(r.tab))).unwrap();
        assert!(tabs.resize_all(38, 350).is_err());
        let snapshot = tabs.get(fresh).unwrap().terminal().snapshot().unwrap();
        assert_eq!((snapshot.rows, snapshot.columns), (24, 80));
        assert_eq!(tabs.get(TabId(1)).unwrap().title(), "Stopped: stream error");
    }
    #[test]
    fn runtime_bad_stream_does_not_stop_other_tabs_cleanup_errors() {
        use crate::error::{AppError, Component};
        use std::{collections::VecDeque, io::ErrorKind, os::unix::process::ExitStatusExt};
        let error = AppError::new(Component::Pty, ErrorKind::Other);
        let status = std::process::ExitStatus::from_raw(0);
        for (steps, kill_error, expected, kills) in [
            (vec![Err(error)], false, Err(error), 0),
            (vec![Ok(None)], true, Err(error), 1),
            (vec![Ok(None), Err(error)], false, Err(error), 1),
            (vec![Ok(Some(status))], false, Ok(Some(status)), 0),
            (vec![Ok(None), Ok(Some(status))], false, Ok(Some(status)), 1),
        ] {
            let mut state = (VecDeque::from(steps), 0);
            assert_eq!(
                finish_owned_child(
                    &mut state,
                    |s| s.0.pop_front().unwrap(),
                    |s| {
                        s.1 += 1;
                        if kill_error { Err(error) } else { Ok(()) }
                    }
                ),
                expected
            );
            assert_eq!(state.1, kills);
            assert!(state.0.is_empty());
        }
    }
    #[test]
    fn runtime_titles_are_sanitized_and_tab_scoped() {
        let mut tabs = TabRegistry::with_scrollback(0).unwrap();
        for _ in 0..2 {
            spawn_tab(&mut tabs, 2, 20, |r| Ok(Fake(r.tab))).unwrap();
        }
        let second = tabs.get_mut(TabId(2)).unwrap();
        second
            .terminal_mut()
            .feed(
                TabId(2),
                "\x1b[22;0t\x1b]2;繁體\u{202e}hidden\x07".as_bytes(),
            )
            .unwrap();
        assert!(second.refresh_title().unwrap());
        assert_eq!(second.title(), "繁體hidden");
        assert_eq!(active_title(&tabs), "Terminal");
        tabs.activate(TabId(2));
        assert_eq!(active_title(&tabs), "繁體hidden");
        let second = tabs.get_mut(TabId(2)).unwrap();
        second.set_title(&"字".repeat(1000)).unwrap();
        assert_eq!(second.title().len(), 255);
        second.terminal_mut().feed(TabId(2), b"\x1b[23;0t").unwrap();
        second.refresh_title().unwrap();
        assert_eq!(active_title(&tabs), "Terminal");
        assert_eq!(tabs.get(TabId(1)).unwrap().title(), "Terminal");
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
