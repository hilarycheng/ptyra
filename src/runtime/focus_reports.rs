//! Application focus transitions use each original Tab's existing ordered PTY FIFO.
use crate::{
    input::FocusTarget,
    tabs::{TabRegistry, output::TabPtyWrite},
};
use alacritty_terminal::term::TermMode;

#[derive(Default)]
pub(super) struct FocusReports {
    target: Option<FocusTarget>,
}
impl FocusReports {
    /// Returns whether any whole transition was rejected. Never retries or reroutes it.
    pub fn update<P: TabPtyWrite>(
        &mut self,
        tabs: &mut TabRegistry<P>,
        target: Option<FocusTarget>,
    ) -> bool {
        let target = target
            .filter(|target| tabs.active() == Some(target.tab) && tabs.get(target.tab).is_some());
        if target == self.target {
            return false;
        }
        let previous = std::mem::replace(&mut self.target, target);
        let mut rejected = false;
        for (source, bytes) in [(previous, b"\x1b[O"), (target, b"\x1b[I")] {
            let Some(tab) = source.and_then(|source| tabs.get_mut(source.tab)) else {
                continue;
            };
            if !tab
                .terminal()
                .mode()
                .is_some_and(|mode| mode.contains(TermMode::FOCUS_IN_OUT))
            {
                continue;
            }
            let id = tab.id();
            rejected |= tab
                .pty_mut()
                .filter(|pty| pty.tab() == id)
                .is_none_or(|pty| pty.enqueue_write(bytes).is_err());
        }
        rejected
    }
}
impl super::Runtime {
    pub(super) fn sync_focus_reports(&mut self) {
        if self
            .input
            .reports
            .update(&mut self.tabs, self.input.focus.target())
            && self.clipboard_notice != Some(super::notice::Notice::FocusRejected)
        {
            self.show_clipboard_notice(super::notice::Notice::FocusRejected);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        pty::{PtyWriteError, PtyWriteFailure},
        tabs::TabPty,
    };
    struct Fake {
        id: TabId,
        bytes: Vec<u8>,
        reject: bool,
    }
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            self.id
        }
    }
    impl TabPtyWrite for Fake {
        fn enqueue_write(&mut self, bytes: &[u8]) -> Result<(), PtyWriteError> {
            if self.reject {
                return Err(PtyWriteError {
                    tab: self.id,
                    pending: self.bytes.len(),
                    reason: PtyWriteFailure::QueueFull,
                });
            }
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }
    }
    fn target(id: u64) -> Option<FocusTarget> {
        Some(FocusTarget {
            window: WindowId(1),
            tab: TabId(id),
        })
    }
    fn mode(tabs: &mut TabRegistry<Fake>, id: u64, on: bool) {
        tabs.get_mut(TabId(id))
            .unwrap()
            .terminal_mut()
            .feed(TabId(id), if on { b"\x1b[?1004h" } else { b"\x1b[?1004l" })
            .unwrap();
    }
    fn take(tabs: &mut TabRegistry<Fake>, id: u64) -> Vec<u8> {
        std::mem::take(&mut tabs.get_mut(TabId(id)).unwrap().pty_mut().unwrap().bytes)
    }
    #[test]
    fn runtime_focus_reports_follow_tab_focus() {
        let mut tabs = TabRegistry::with_scrollback(0).unwrap();
        for id in [1, 2] {
            tabs.insert(TabId(id), 4, 20).unwrap();
            tabs.attach_pty(Fake {
                id: TabId(id),
                bytes: vec![],
                reject: false,
            })
            .ok()
            .unwrap();
        }
        let mut focus = FocusReports::default();
        assert!(!focus.update(&mut tabs, target(1)));
        assert!(take(&mut tabs, 1).is_empty());
        mode(&mut tabs, 1, true);
        assert!(!focus.update(&mut tabs, target(1))); // Enabling the mode is not a focus transition.
        assert!(take(&mut tabs, 1).is_empty());
        tabs.activate(TabId(2));
        assert!(!focus.update(&mut tabs, target(2)));
        assert_eq!(take(&mut tabs, 1), b"\x1b[O");
        assert!(take(&mut tabs, 2).is_empty());
        mode(&mut tabs, 2, true);
        assert!(!focus.update(&mut tabs, target(2)));
        assert!(!focus.update(&mut tabs, None));
        assert!(!focus.update(&mut tabs, None)); // Duplicate leave/seat removal is inert.
        assert_eq!(take(&mut tabs, 2), b"\x1b[O");
        assert!(!focus.update(&mut tabs, target(2)));
        assert!(!focus.update(&mut tabs, target(2)));
        assert_eq!(take(&mut tabs, 2), b"\x1b[I");
        tabs.activate(TabId(1));
        assert!(!focus.update(&mut tabs, target(1)));
        assert_eq!(take(&mut tabs, 1), b"\x1b[I");
        assert_eq!(take(&mut tabs, 2), b"\x1b[O");
        mode(&mut tabs, 1, false);
        assert!(!focus.update(&mut tabs, None));
        assert!(take(&mut tabs, 1).is_empty());
        assert!(!focus.update(&mut tabs, target(999)));
        assert!(!focus.update(&mut tabs, target(2))); // A stale inactive target never gets input.
        assert!(take(&mut tabs, 2).is_empty());
        mode(&mut tabs, 1, true);
        assert!(!focus.update(&mut tabs, target(1)));
        take(&mut tabs, 1);
        tabs.get_mut(TabId(1)).unwrap().pty_mut().unwrap().reject = true;
        tabs.activate(TabId(2));
        assert!(focus.update(&mut tabs, target(2)));
        assert_eq!(
            take(&mut tabs, 2),
            b"\x1b[I",
            "old target rejection cannot suppress new target's transition"
        );
        assert!(take(&mut tabs, 1).is_empty());
        tabs.get_mut(TabId(1)).unwrap().pty_mut().unwrap().reject = false;
        assert!(!focus.update(&mut tabs, target(2)));
        assert!(
            take(&mut tabs, 1).is_empty(),
            "rejected transitions are never silently retried"
        );
        tabs.close(TabId(2));
        assert!(!focus.update(&mut tabs, target(1)));
        assert_eq!(
            take(&mut tabs, 1),
            b"\x1b[I",
            "closed source never reroutes its out report to the successor"
        );
        assert!(!focus.update(&mut tabs, None));
        assert!(!focus.update(&mut tabs, None));
        assert_eq!(take(&mut tabs, 1), b"\x1b[O");
    }
}
