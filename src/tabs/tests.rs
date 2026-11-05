use super::*;

#[derive(Debug)]
struct FakePty(TabId);
impl TabPty for FakePty {
    fn tab(&self) -> TabId {
        self.0
    }
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
