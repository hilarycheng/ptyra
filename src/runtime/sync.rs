//! Synchronized presentation deadlines share the runtime's event-driven poll timeout.
use crate::{
    clock::{Clock, Deadline},
    tabs::{TabPty, TabRegistry},
};
pub(super) fn refresh<P: TabPty>(
    tabs: &mut TabRegistry<P>,
    clock: &impl Clock,
) -> Option<Deadline> {
    tabs.sync_deadline(clock)
}
pub(super) fn held<P: TabPty>(tabs: &TabRegistry<P>) -> bool {
    tabs.active()
        .and_then(|id| tabs.get(id))
        .is_some_and(|tab| tab.terminal().sync_pending())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, clock::FakeClock};
    use std::time::Duration;
    struct Fake;
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            TabId(1)
        }
    }
    fn feed(tabs: &mut TabRegistry<Fake>, id: TabId, bytes: &[u8]) {
        tabs.get_mut(id)
            .unwrap()
            .terminal_mut()
            .feed(id, bytes)
            .unwrap();
    }
    #[test]
    fn runtime_sync_update_timeout_releases_frame() {
        let mut tabs = TabRegistry::<Fake>::default();
        tabs.insert(TabId(1), 4, 40).unwrap();
        tabs.insert(TabId(2), 4, 40).unwrap();
        let clock = FakeClock::default();
        assert!(refresh(&mut tabs, &clock).is_none());
        assert!(!held(&tabs));
        feed(&mut tabs, TabId(1), b"\x1b[?2026hvisible later\x1b[6n");
        let deadline =
            refresh(&mut tabs, &clock).expect("sync begin must schedule a bounded deadline");
        assert!(held(&tabs));
        assert_eq!(deadline.remaining(&clock), Duration::from_millis(150));
        assert_eq!(
            tabs.get_mut(TabId(1))
                .unwrap()
                .terminal_mut()
                .take_core_replies()
                .0,
            b"\x1b[1;14R"
        );
        assert_eq!(
            tabs.get(TabId(1)).unwrap().terminal().grid().unwrap()
                [alacritty_terminal::index::Line(0)][alacritty_terminal::index::Column(0)]
            .c,
            'v'
        );
        clock.advance(Duration::from_millis(149)).unwrap();
        feed(&mut tabs, TabId(1), b"\x1b[?2026h");
        assert_eq!(
            refresh(&mut tabs, &clock).unwrap().remaining(&clock),
            Duration::from_millis(1)
        );
        assert!(held(&tabs));
        tabs.activate(TabId(2));
        assert!(!held(&tabs));
        clock.advance(Duration::from_millis(1)).unwrap();
        assert!(refresh(&mut tabs, &clock).is_none());
        tabs.activate(TabId(1));
        assert!(!held(&tabs));
        feed(&mut tabs, TabId(1), b"\x1b[?2026hstill bounded");
        assert!(refresh(&mut tabs, &clock).is_none());
        assert!(!held(&tabs));
        tabs.sync_presented();
        feed(&mut tabs, TabId(1), b"\x1b[?2026l");
        refresh(&mut tabs, &clock);
        feed(&mut tabs, TabId(1), b"\x1b[?2026hnew batch");
        assert!(refresh(&mut tabs, &clock).is_some());
        assert!(held(&tabs));
        feed(&mut tabs, TabId(1), b"\x1b[?2026l");
        assert!(refresh(&mut tabs, &clock).is_none());
        assert!(!held(&tabs));
        feed(&mut tabs, TabId(2), b"\x1b[?2026hhidden");
        assert!(refresh(&mut tabs, &clock).is_some());
        assert!(!held(&tabs));
        tabs.activate(TabId(2));
        assert!(held(&tabs));
        feed(&mut tabs, TabId(2), b"\x1bc");
        assert!(refresh(&mut tabs, &clock).is_none());
        assert!(!held(&tabs));
    }
    #[test]
    fn sync_reset_cannot_extend_hold_or_select_unseen_text() {
        use super::super::{
            pointer::{Kind, Routed},
            selection::{Geometry, SelectionInput},
        };
        use crate::{
            WindowId,
            input::{FocusTarget, Point},
        };
        let mut tabs = TabRegistry::<Fake>::default();
        tabs.insert(TabId(1), 2, 8).unwrap();
        let clock = FakeClock::default();
        feed(&mut tabs, TabId(1), b"old\r\x1b[?2026hnew");
        refresh(&mut tabs, &clock);
        let mut selection = SelectionInput::default();
        let geometry = Geometry {
            origin: 20,
            size: (80, 20),
            cell: (10, 10),
        };
        let mut event = Routed {
            target: FocusTarget {
                window: WindowId(1),
                tab: TabId(1),
            },
            position: Point::new(5., 25.).unwrap(),
            kind: Kind::Button {
                button: 272,
                pressed: true,
                serial: 1,
            },
        };
        for _ in 0..2 {
            event.kind = Kind::Button {
                button: 272,
                pressed: true,
                serial: 1,
            };
            selection.route(&mut tabs, &event, geometry, &clock);
            event.kind = Kind::Button {
                button: 272,
                pressed: false,
                serial: 2,
            };
            selection.route(&mut tabs, &event, geometry, &clock);
        }
        assert!(
            !selection.take_publication(),
            "held pixels must not publish unseen replacement text"
        );
        assert!(tabs.get(TabId(1)).unwrap().selection_range().is_none());
        clock.advance(Duration::from_millis(149)).unwrap();
        feed(&mut tabs, TabId(1), b"\x1bc\x1b[?2026hnew");
        assert_eq!(
            refresh(&mut tabs, &clock).unwrap().remaining(&clock),
            Duration::from_millis(1)
        );
        clock.advance(Duration::from_millis(1)).unwrap();
        assert!(refresh(&mut tabs, &clock).is_none());
        assert!(!held(&tabs));
        assert!(
            tabs.get(TabId(1))
                .unwrap()
                .terminal()
                .sync_blocks_selection()
        );
        tabs.sync_presented();
        assert!(
            !tabs
                .get(TabId(1))
                .unwrap()
                .terminal()
                .sync_blocks_selection()
        );
        feed(
            &mut tabs,
            TabId(1),
            b"\x1b[?2026l\x1b[?2026hnext\x1b[?2026$p",
        );
        assert_eq!(
            refresh(&mut tabs, &clock).unwrap().remaining(&clock),
            Duration::from_millis(150)
        );
        assert_eq!(
            tabs.get_mut(TabId(1))
                .unwrap()
                .terminal_mut()
                .take_core_replies()
                .0,
            b"\x1b[?2026;1$y"
        );
        assert!(held(&tabs));
    }
}
