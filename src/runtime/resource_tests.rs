use super::*;
use alacritty_terminal::grid::Dimensions;

#[test]
fn runtime_resource_caps_apply_to_actual_owners() {
    let mut pixels = Vec::new();
    for bytes in [9 * 1024 * 1024, 9 * 1024 * 1024 + 4, MAX_PIXELS * 4] {
        resize_frame_pixels(&mut pixels, bytes).unwrap();
        assert_eq!(pixels.len(), bytes);
        assert!(pixels.capacity() <= MAX_PIXELS * 4);
    }
    resize_frame_pixels(&mut pixels, 4).unwrap();
    assert_eq!(
        pixels.capacity(),
        MAX_PIXELS * 4,
        "shrink retains bounded high water"
    );
    assert!(resize_frame_pixels(&mut pixels, MAX_PIXELS * 4 + 1).is_err());
    assert_eq!(pixels.len(), 4);
    // Two fixed SHM slots plus this one CPU image, each at most 16 MiB.
    let slots: [Option<Slot>; 2] = [None, None];
    assert_eq!((slots.len() + 1) * MAX_PIXELS * 4, 48 * 1024 * 1024);
}

#[test]
fn runtime_resource_caps_apply_to_actual_owners_across_tabs() {
    use crate::{TabId, tabs::TabPty};
    struct Fake(TabId);
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            self.0
        }
    }
    let config = crate::config::Config::from_toml(Some("scrollback_lines=0")).unwrap();
    let mut tabs = config.tab_registry::<Fake>().unwrap();
    for _ in 0..crate::tabs::MAX_TABS {
        sessions::spawn_tab(&mut tabs, 2, 2, |request| Ok(Fake(request.tab))).unwrap();
    }
    let initial = tabs.allocated_cells();
    assert!(
        tabs.resize_all(20, 80).is_err(),
        "logical slots fit but actual spare/row capacity does not"
    );
    assert_eq!(tabs.allocated_cells(), initial);
    for id in tabs.ids() {
        assert_eq!(
            tabs.get(id).unwrap().terminal().grid().unwrap().columns(),
            2
        );
    }
    for id in 1..=16 {
        tabs.close(TabId(id)).unwrap();
    }
    assert!(tabs.resize_all(20, 80).unwrap());
    let high = tabs.allocated_cells();
    assert!(high <= crate::tabs::MAX_TOTAL_TAB_ALLOCATION_CELLS);
    tabs.resize_all(10, 40).unwrap();
    assert_eq!(tabs.allocated_cells(), high);
    assert!(sessions::spawn_tab(&mut tabs, 2, 2, |_| Err("fake spawn failure")).is_err());
    assert_eq!(tabs.allocated_cells(), high);
    for id in tabs.ids().collect::<Vec<_>>() {
        tabs.close(id).unwrap();
    }
    assert_eq!(tabs.allocated_cells(), 0);
}

#[test]
fn runtime_resource_caps_apply_to_actual_owners_at_default_wide_sizes() {
    use crate::{TabId, tabs::TabPty};
    struct Fake;
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            TabId(1)
        }
    }
    let mut tabs = crate::config::Config::default()
        .tab_registry::<Fake>()
        .unwrap();
    for id in 1..=5 {
        tabs.insert(TabId(id), 24, 80).unwrap();
    }
    // Ordinary wide windows and multiple Tabs must not be blocked by conservative headroom.
    for columns in [160, 240, 80] {
        tabs.resize_all(24, columns).unwrap();
        for id in tabs.ids() {
            assert_eq!(
                tabs.get(id).unwrap().terminal().grid().unwrap().columns(),
                usize::from(columns)
            );
        }
    }
    for id in 2..=5 {
        tabs.close(TabId(id)).unwrap();
    }
    tabs.resize_all(25, 300).unwrap();
    assert!(tabs.allocated_cells() <= crate::tabs::MAX_TOTAL_TAB_ALLOCATION_CELLS);
}
