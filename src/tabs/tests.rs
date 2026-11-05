use super::*;

#[derive(Debug)]
pub(super) struct FakePty(TabId);
impl TabPty for FakePty {
    fn tab(&self) -> TabId {
        self.0
    }
}

#[test]
fn close_tab_does_not_kill_other_pty() {
    use std::{cell::RefCell, rc::Rc};
    #[derive(Debug)]
    struct OwnedPty(TabId, Rc<RefCell<Vec<TabId>>>);
    impl TabPty for OwnedPty {
        fn tab(&self) -> TabId {
            self.0
        }
    }
    impl Drop for OwnedPty {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    let stopped = Rc::new(RefCell::new(Vec::new()));
    let mut tabs = TabRegistry::default();
    for id in 1..=3 {
        tabs.insert(TabId(id), 2, 8).unwrap();
        tabs.attach_pty(OwnedPty(TabId(id), stopped.clone()))
            .unwrap();
    }
    let closed = tabs.close(TabId(2)).unwrap();
    assert_eq!(closed.tab, TabId(2));
    assert!(
        stopped.borrow().is_empty(),
        "the reducer returns ownership for adapter cleanup"
    );
    drop(closed);
    assert_eq!(*stopped.borrow(), [TabId(2)]);
    assert_eq!(tabs.active(), Some(TabId(1)));
    assert!(tabs.close(TabId(2)).is_none());
    assert!(tabs.insert(TabId(2), 2, 8).is_err());
    drop(tabs.close(TabId(1)).unwrap());
    assert_eq!(tabs.active(), Some(TabId(3)));
    assert_eq!(tabs.get(TabId(3)).unwrap().pty().unwrap().tab(), TabId(3));
    drop(tabs.close(TabId(3)).unwrap());
    assert_eq!(tabs.active(), None);
    assert_eq!(*stopped.borrow(), [TabId(2), TabId(1), TabId(3)]);
    let pending = tabs.new_tab(2, 8).unwrap();
    assert_eq!(pending.tab, TabId(4));
    assert!(tabs.close(pending.tab).unwrap().pty.is_none());
    let late = tabs
        .attach_pty(OwnedPty(pending.tab, stopped.clone()))
        .unwrap_err();
    assert_eq!(late.tab(), pending.tab);
}

#[test]
fn tab_cycle_wraps_at_ends() {
    let mut tabs = TabRegistry::<FakePty>::default();
    assert_eq!(tabs.cycle(TabDirection::Next), None);
    assert_eq!(tabs.cycle(TabDirection::Previous), None);
    tabs.insert(TabId(2), 2, 8).unwrap();
    assert_eq!(tabs.cycle(TabDirection::Next), Some(TabId(2)));
    assert_eq!(tabs.cycle(TabDirection::Previous), Some(TabId(2)));
    tabs.insert(TabId(4), 2, 8).unwrap();
    tabs.insert(TabId(9), 2, 8).unwrap();
    for id in [4, 9, 2, 4] {
        assert_eq!(tabs.cycle(TabDirection::Next), Some(TabId(id)));
    }
    for id in [2, 9, 4, 2] {
        assert_eq!(tabs.cycle(TabDirection::Previous), Some(TabId(id)));
    }
    assert_eq!(
        tabs.ids().collect::<Vec<_>>(),
        [TabId(2), TabId(4), TabId(9)]
    );
}

#[test]
fn tab_switch_updates_active_id() {
    let mut tabs = TabRegistry::<FakePty>::default();
    assert_eq!(tabs.active(), None);
    assert!(!tabs.activate(TabId(1)));
    tabs.insert(TabId(1), 2, 8).unwrap();
    tabs.insert(TabId(2), 2, 8).unwrap();
    assert_eq!(tabs.active(), Some(TabId(1)));
    tabs.get_mut(TabId(1))
        .unwrap()
        .terminal_mut()
        .feed(TabId(1), b"first")
        .unwrap();
    assert!(tabs.activate(TabId(2)));
    assert!(!tabs.activate(TabId(2)));
    assert!(!tabs.activate(TabId(99)));
    assert_eq!(tabs.active(), Some(TabId(2)));
    assert!(tabs.activate(TabId(1)));
    assert_eq!(
        tabs.get(TabId(1))
            .unwrap()
            .terminal()
            .grid()
            .unwrap()
            .cursor
            .point
            .column
            .0,
        5
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
        0
    );
}

#[test]
fn new_tab_emits_single_spawn() {
    let mut tabs = TabRegistry::<FakePty>::default();
    assert!(tabs.new_tab(0, 8).is_err());
    assert_eq!(tabs.ids().count(), 0);
    let effect = tabs.new_tab(2, 8).unwrap();
    assert_eq!(effect.tab, TabId(1));
    assert_eq!((effect.rows, effect.columns), (2, 8));
    let mut spawns = Vec::new();
    let mut spawn = |effect: SpawnTab| {
        spawns.push(effect.tab);
        FakePty(effect.tab)
    };
    tabs.attach_pty(spawn(effect)).unwrap();
    assert_eq!(tabs.ids().count(), 1);
    assert_eq!(spawns, [TabId(1)]);
    assert_eq!(tabs.new_tab(2, 8).unwrap().tab, TabId(2));
    let mut exhausted = TabRegistry::<FakePty>::default();
    exhausted.insert(TabId(u64::MAX), 2, 8).unwrap();
    assert!(exhausted.new_tab(2, 8).is_err());
    assert_eq!(exhausted.ids().count(), 1);
}

#[test]
fn tab_registry_keeps_unique_entries() {
    use crate::selection::Screen;
    use alacritty_terminal::index::{Column, Line, Point};
    let mut tabs = TabRegistry::<FakePty>::default();
    tabs.insert(TabId(1), 2, 8).unwrap();
    tabs.insert(TabId(2), 2, 8).unwrap();
    tabs.attach_pty(FakePty(TabId(1))).unwrap();
    tabs.attach_pty(FakePty(TabId(2))).unwrap();
    assert!(tabs.insert(TabId(1), 2, 8).is_err());
    assert!(tabs.attach_pty(FakePty(TabId(1))).is_err());
    assert!(tabs.attach_pty(FakePty(TabId(99))).is_err());
    assert!(tabs.insert(TabId(3), 0, 8).is_err());
    assert_eq!(tabs.ids().collect::<Vec<_>>(), [TabId(1), TabId(2)]);
    let first = tabs.get_mut(TabId(1)).unwrap();
    first.terminal_mut().feed(TabId(1), b"A").unwrap();
    first
        .selections_mut()
        .selection_mut(TabId(1), Screen::Main)
        .unwrap()
        .press(TabId(1), Point::new(Line(0), Column(0)));
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
        0
    );
    assert!(
        tabs.get(TabId(2))
            .unwrap()
            .selections()
            .range(TabId(2), Screen::Main)
            .is_none()
    );
    assert_eq!(tabs.get(TabId(1)).unwrap().pty().unwrap().tab(), TabId(1));
    assert_eq!(tabs.get(TabId(2)).unwrap().pty().unwrap().tab(), TabId(2));
    for id in 3..=MAX_TABS as u64 {
        tabs.insert(TabId(id), 2, 8).unwrap();
    }
    assert!(tabs.insert(TabId(MAX_TABS as u64 + 1), 2, 8).is_err());
    assert_eq!(tabs.ids().count(), MAX_TABS);
}
