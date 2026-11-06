//! Bounded row-band damage. The canonical staging image survives alternating SHM slots.
use crate::TabId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Band {
    pub start: u32,
    pub end: u32,
}
impl Band {
    pub fn new(start: u32, end: u32) -> Option<Self> {
        (start < end).then_some(Self { start, end })
    }
    pub fn terminal(
        rows: (usize, usize),
        grid_rows: usize,
        height: u32,
        cell_height: u32,
    ) -> Option<Self> {
        let (start, end) = rows;
        let end = if end >= grid_rows {
            height
        } else {
            end as u32 * cell_height
        };
        // A rejected resize can leave a grid larger than the current viewport.
        Self::new(start as u32 * cell_height, end.min(height))
    }
    pub fn add(target: &mut Option<Self>, next: Option<Self>) {
        if let Some(next) = next {
            *target = Some(match *target {
                Some(old) => Self {
                    start: old.start.min(next.start),
                    end: old.end.max(next.end),
                },
                None => next,
            });
        }
    }
    pub fn shifted(self, offset: u32) -> Self {
        Self {
            start: self.start + offset,
            end: self.end + offset,
        }
    }
    pub fn range(self) -> std::ops::Range<u32> {
        self.start..self.end
    }
}

pub(super) struct View {
    pub tab: TabId,
    pub size: (u32, u32),
    pub cell: (u32, u32),
    pub scale: u32,
    pub origin: u32,
    pub titles: Vec<(TabId, String)>,
}
pub(super) struct Plan {
    pub bar: bool,
    pub content: Option<Band>, // Content-local physical pixels, aligned to complete cell rows.
    pub surface: [Option<Band>; 2],
}
#[derive(Default)]
pub(super) struct State {
    view: Option<View>,
    overlays: Option<Band>, // Surface-local; old and new locations restore the base first.
    stale: [Option<Band>; 2],
}
impl State {
    // Call after all frame/slot/sync gates, never on an idle or hidden-view iteration.
    pub fn prepare(
        &mut self,
        view: View,
        mut content: Option<Band>,
        overlays: Option<Band>,
    ) -> Plan {
        let full = self.view.as_ref().is_none_or(|old| {
            old.tab != view.tab
                || old.size != view.size
                || old.cell != view.cell
                || old.scale != view.scale
                || old.origin != view.origin
        });
        let mut bar = full
            || self
                .view
                .as_ref()
                .is_none_or(|old| old.titles != view.titles);
        if full {
            // Old coordinates may exceed a shrunken surface, even if a slot later matches again.
            self.stale = [Band::new(0, view.size.1); 2];
            content = Band::new(0, view.size.1 - view.origin);
        } else {
            let mut restore = self.overlays;
            Band::add(&mut restore, overlays);
            if let Some(restore) = restore {
                bar |= restore.start < view.origin;
                let start = restore.start.saturating_sub(view.origin) / view.cell.1 * view.cell.1;
                let end = restore
                    .end
                    .saturating_sub(view.origin)
                    .div_ceil(view.cell.1)
                    * view.cell.1;
                Band::add(
                    &mut content,
                    Band::new(start, end.min(view.size.1 - view.origin)),
                );
            }
        }
        let surface = [
            bar.then(|| Band::new(0, view.origin)).flatten(),
            content.map(|b| b.shifted(view.origin)),
        ];
        for stale in &mut self.stale {
            for band in surface {
                Band::add(stale, band);
            }
        }
        self.view = Some(view);
        self.overlays = overlays;
        Plan {
            bar,
            content,
            surface,
        }
    }
    pub fn invalidate_slot(&mut self, index: usize, height: u32) {
        self.stale[index] = Band::new(0, height);
    }
    pub fn stale(&self, index: usize) -> Option<Band> {
        self.stale[index]
    }
    pub fn submitted(&mut self, index: usize) {
        self.stale[index] = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view(height: u32) -> View {
        View {
            tab: TabId(1),
            size: (80, height),
            cell: (8, 10),
            scale: 1,
            origin: 18,
            titles: vec![(TabId(1), "Shell".into())],
        }
    }
    #[test]
    fn runtime_damage_clips_retained_grid_after_rejected_resize() {
        assert_eq!(Band::terminal((40, 41), 58, 772, 20), None);
        assert_eq!(Band::terminal((38, 39), 58, 772, 20), Band::new(760, 772));
        assert_eq!(Band::terminal((2, 41), 58, 772, 20), Band::new(40, 772));
        assert_eq!(Band::terminal((0, 58), 58, 772, 20), Band::new(0, 772));
        assert_eq!(Band::terminal((57, 58), 58, 772, 20), None);
        struct Fake;
        impl crate::tabs::TabPty for Fake {
            fn tab(&self) -> TabId {
                TabId(1)
            }
        }
        let mut tabs = crate::tabs::TabRegistry::<Fake>::default();
        tabs.insert(TabId(1), 58, 66).unwrap();
        let terminal = tabs.get_mut(TabId(1)).unwrap().terminal_mut();
        terminal.feed(TabId(1), b"\x1b[41;1H").unwrap();
        terminal.damage_rows();
        terminal.damage_presented();
        assert!(tabs.resize_all(38, 350).is_err());
        let terminal = tabs.get_mut(TabId(1)).unwrap().terminal_mut();
        assert_eq!(terminal.snapshot().unwrap().rows, 58);
        let rows = terminal.damage_rows().unwrap();
        assert_eq!(rows, (40, 41));
        assert_eq!(Band::terminal(rows, 58, 772, 20), None);
    }
    #[test]
    fn runtime_damage_repairs_alternating_slots_and_geometry() {
        let mut state = State::default();
        let full = state.prepare(view(121), None, None);
        assert!(full.bar);
        assert_eq!(full.content, Band::new(0, 103));
        assert_eq!(state.stale(0), Band::new(0, 121));
        state.submitted(0);
        let next = state.prepare(view(121), Band::new(10, 20), None);
        assert!(!next.bar);
        assert_eq!(next.surface, [None, Band::new(28, 38)]);
        assert_eq!(state.stale(1), Band::new(0, 121));
        state.submitted(1);
        state.prepare(view(121), Band::new(80, 90), None);
        assert_eq!(state.stale(0), Band::new(28, 108));
        assert_eq!(state.stale(1), Band::new(98, 108));
        state.submitted(0);
        // Buffer coordinates are reset on shrink, including slots not used during it.
        state.prepare(view(61), None, None);
        assert_eq!(state.stale(0), Band::new(0, 61));
        assert_eq!(state.stale(1), Band::new(0, 61));
        state.submitted(0);
        state.prepare(view(121), None, None);
        assert_eq!(state.stale(0), Band::new(0, 121));
        assert_eq!(state.stale(1), Band::new(0, 121));
        state.submitted(0);
        state.submitted(1);
        let idle = state.prepare(view(121), None, None);
        assert_eq!(idle.surface, [None, None]);
        assert_eq!(state.stale(0), None);
        state.invalidate_slot(0, 121);
        assert_eq!(state.stale(0), Band::new(0, 121));
        assert_eq!(state.stale(1), None);
        let mut title = view(121);
        title.titles[0].1 = "New".into();
        let bar = state.prepare(title, None, None);
        assert_eq!(bar.surface, [Band::new(0, 18), None]);
        let mut switched = view(121);
        switched.tab = TabId(2);
        assert_eq!(
            state.prepare(switched, None, None).content,
            Band::new(0, 103)
        );
        let mut scaled = view(121);
        scaled.scale = 2;
        assert_eq!(state.prepare(scaled, None, None).content, Band::new(0, 103));
    }
    #[test]
    fn runtime_damage_restores_old_and_new_overlay_rows() {
        let mut state = State::default();
        state.prepare(view(121), None, None);
        let selection = state.prepare(view(121), None, Band::new(38, 48));
        assert_eq!(selection.content, Band::new(20, 30));
        let moved = state.prepare(view(121), None, Band::new(68, 78));
        assert_eq!(moved.content, Band::new(20, 60));
        let cleared = state.prepare(view(121), None, None);
        assert_eq!(cleared.content, Band::new(50, 60));
        assert_eq!(state.prepare(view(121), None, None).surface, [None, None]);
        // Bottom notices are surface-aligned, unlike content cell rows.
        let notice = state.prepare(view(121), None, Band::new(101, 121));
        assert_eq!(notice.content, Band::new(80, 103));
        assert_eq!(
            state.prepare(view(121), None, None).content,
            Band::new(80, 103)
        );
        let overlap = state.prepare(view(121), None, Band::new(11, 121));
        assert!(overlap.bar);
        assert_eq!(overlap.content, Band::new(0, 103));
    }
}

#[cfg(test)]
mod gates {
    use super::super::{PaintState, sync};
    use crate::{
        TabId,
        clock::FakeClock,
        tabs::{TabPty, TabRegistry},
    };
    struct Fake;
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            TabId(1)
        }
    }
    #[test]
    fn runtime_damage_waits_for_frame_slot_and_sync_release() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        tabs.insert(TabId(1), 6, 8).unwrap();
        tabs.insert(TabId(2), 6, 8).unwrap();
        let feed = |tabs: &mut TabRegistry<Fake>, id, data: &[u8]| {
            tabs.get_mut(id)
                .unwrap()
                .terminal_mut()
                .feed(id, data)
                .unwrap();
        };
        feed(&mut tabs, TabId(1), b"\x1b[3;1H");
        let term = tabs.get_mut(TabId(1)).unwrap().terminal_mut();
        term.damage_rows();
        term.damage_presented();
        let mut paint = PaintState {
            configured: true,
            dirty: true,
            ..PaintState::default()
        };
        paint.submitted();
        feed(&mut tabs, TabId(1), b"\x1b[?2026hA\x1b[5;1HB");
        paint.dirty = true;
        let clock = FakeClock::default();
        sync::refresh(&mut tabs, &clock);
        assert!(!paint.ready());
        assert!(sync::held(&tabs));
        paint.frame_pending = false;
        assert!(paint.ready());
        let ready = |paint: &PaintState, tabs: &TabRegistry<Fake>, busy: [bool; 2]| {
            paint.ready() && !sync::held(tabs) && busy.iter().any(|busy| !busy)
        };
        assert!(!ready(&paint, &tabs, [false, false]));
        feed(&mut tabs, TabId(1), b"\x1b[?2026l");
        sync::refresh(&mut tabs, &clock);
        assert!(!ready(&paint, &tabs, [true, true]));
        assert!(ready(&paint, &tabs, [true, false]));
        assert_eq!(
            tabs.get_mut(TabId(1)).unwrap().terminal_mut().damage_rows(),
            Some((2, 5))
        );
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .damage_presented();
        paint.submitted();
        paint.frame_pending = false;
        assert!(!ready(&paint, &tabs, [false, false]));
        // Hidden output retains its own damage and never marks the active view dirty.
        feed(&mut tabs, TabId(2), b"hidden\x1b[5;1Hstill hidden");
        assert!(!paint.dirty);
        assert_eq!(
            tabs.get_mut(TabId(2)).unwrap().terminal_mut().damage_rows(),
            Some((0, 6))
        );
    }
}
