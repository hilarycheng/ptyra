//! Pointer-to-grid routing. The tab owns selection state; this controller owns only a drag.
use super::pointer::{Kind, Routed};
use crate::{
    TabId,
    selection::Screen,
    tabs::{SelectionAction, TabPty, TabRegistry},
};
use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
};

#[derive(Clone, Copy)]
pub(super) struct Geometry {
    pub origin: u32,
    pub size: (u32, u32),
    pub cell: (u32, u32),
}
#[derive(Default)]
pub(super) struct SelectionInput {
    drag: Option<(TabId, Screen)>,
    click: Option<Click>,
}
struct Click {
    owner: (TabId, Screen),
    point: crate::input::Point,
    at: std::time::Duration,
    count: u8,
}
impl Click {
    fn near(&self, point: crate::input::Point) -> bool {
        (self.point.x() - point.x()).abs() <= 4. && (self.point.y() - point.y()).abs() <= 4.
    }
}
impl SelectionInput {
    pub fn cancel<P: TabPty>(&mut self, tabs: &mut TabRegistry<P>) -> bool {
        self.click = None;
        if let Some((id, screen)) = self.drag.take() {
            if let Some(tab) = tabs.get_mut(id) {
                tab.cancel_selection_drag(screen);
            }
            return true;
        }
        false
    }
    pub fn route<P: TabPty>(
        &mut self,
        tabs: &mut TabRegistry<P>,
        event: &Routed,
        geometry: Geometry,
        clock: &impl crate::clock::Clock,
    ) -> bool {
        if tabs.active() != Some(event.target.tab) {
            self.cancel(tabs);
            return false;
        }
        if matches!(event.kind, Kind::Leave) {
            return self.cancel(tabs);
        }
        if matches!(event.kind, Kind::Motion)
            && self
                .click
                .as_ref()
                .is_some_and(|click| !click.near(event.position))
        {
            self.click = None;
        }
        let mut action = match event.kind {
            Kind::Button {
                button: 272,
                pressed: true,
                ..
            } => SelectionAction::Press,
            Kind::Button {
                button: 272,
                pressed: false,
                ..
            } => SelectionAction::Release,
            Kind::Motion => SelectionAction::Motion,
            _ => return false,
        };
        let Some(tab) = tabs.get(event.target.tab) else {
            return false;
        };
        let Some(mode) = tab.terminal().mode() else {
            return false;
        };
        let screen = Screen::from_mode(mode);
        let owner = (event.target.tab, screen);
        let start = matches!(action, SelectionAction::Press);
        if !start && self.drag != Some(owner) {
            if self.drag.is_some() {
                self.cancel(tabs);
            }
            return false;
        }
        let (x, y) = (
            event.position.x(),
            event.position.y() - f64::from(geometry.origin),
        );
        if start
            && (x < 0.
                || y < 0.
                || x >= f64::from(geometry.size.0)
                || y >= f64::from(geometry.size.1))
        {
            self.cancel(tabs);
            return false;
        }
        let Some(grid) = tab.terminal().grid() else {
            return false;
        };
        if geometry.cell.0 == 0 || geometry.cell.1 == 0 {
            return false;
        }
        // Once a drag starts, out-of-content coordinates clip to the nearest grid edge.
        let column = (x.max(0.) / f64::from(geometry.cell.0)) as usize;
        let row = (y.max(0.) / f64::from(geometry.cell.1)) as usize;
        let point = Point::new(
            Line(row.min(grid.screen_lines() - 1) as i32),
            Column(column.min(grid.columns() - 1)),
        );
        if start {
            let at = clock.now();
            let count = self
                .click
                .as_ref()
                .filter(|click| {
                    click.owner == owner
                        && click.near(event.position)
                        && at
                            .checked_sub(click.at)
                            .is_some_and(|elapsed| elapsed <= std::time::Duration::from_millis(400))
                })
                .map_or(1, |click| click.count % 3 + 1);
            self.click = Some(Click {
                owner,
                point: event.position,
                at,
                count,
            });
            action = match count {
                2 => SelectionAction::Word,
                3 => SelectionAction::Line,
                _ => SelectionAction::Press,
            };
            self.drag = (count == 1).then_some(owner);
        }
        if matches!(action, SelectionAction::Release) {
            self.drag = None;
        }
        tabs.get_mut(event.target.tab)
            .unwrap()
            .select(screen, action, point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        input::{FocusTarget, Point},
        tabs::{TabPty, TabRegistry},
    };
    struct Fake(TabId);
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            self.0
        }
    }
    fn event(
        tab: u64,
        x: f64,
        y: f64,
        kind: super::super::pointer::Kind,
    ) -> super::super::pointer::Routed {
        super::super::pointer::Routed {
            target: FocusTarget {
                window: WindowId(1),
                tab: TabId(tab),
            },
            position: Point::new(x, y).unwrap(),
            kind,
        }
    }
    fn press(tab: u64, x: f64, y: f64) -> super::super::pointer::Routed {
        event(
            tab,
            x,
            y,
            super::super::pointer::Kind::Button {
                button: 272,
                pressed: true,
                serial: 1,
            },
        )
    }
    fn release(tab: u64, x: f64, y: f64) -> super::super::pointer::Routed {
        event(
            tab,
            x,
            y,
            super::super::pointer::Kind::Button {
                button: 272,
                pressed: false,
                serial: 2,
            },
        )
    }
    #[test]
    fn runtime_click_count_selects_word_or_line() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        tabs.insert(TabId(1), 3, 8).unwrap();
        tabs.insert(TabId(2), 3, 8).unwrap();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), "abc 中e\u{301}zzXY".as_bytes())
            .unwrap();
        let geometry = Geometry {
            origin: 20,
            size: (80, 30),
            cell: (10, 10),
        };
        let clock = crate::clock::FakeClock::default();
        let mut input = SelectionInput::default();
        for _ in 0..2 {
            input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
            input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
            clock.advance(std::time::Duration::from_millis(50)).unwrap();
        }
        let selected = |tabs: &TabRegistry<Fake>| {
            let tab = tabs.get(TabId(1)).unwrap();
            crate::selection::copy_text(
                tab.terminal().grid().unwrap(),
                tab.selection_range().unwrap(),
            )
            .unwrap()
        };
        assert_eq!(selected(&tabs), "abc");
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        assert!(selected(&tabs).starts_with("abc 中e\u{301}zzXY"));
        assert_eq!(
            tabs.get(TabId(1))
                .unwrap()
                .selection_range()
                .unwrap()
                .end
                .line
                .0,
            1
        );
        clock
            .advance(std::time::Duration::from_millis(401))
            .unwrap();
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        assert_eq!(selected(&tabs), "b");
        for _ in 0..2 {
            input.route(&mut tabs, &press(1, 55., 25.), geometry, &clock);
            input.route(&mut tabs, &release(1, 55., 25.), geometry, &clock);
        }
        assert_eq!(selected(&tabs), "中");
        input.cancel(&mut tabs);
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        assert_eq!(selected(&tabs), "b");
        input.route(
            &mut tabs,
            &event(1, 70., 25., super::super::pointer::Kind::Motion),
            geometry,
            &clock,
        );
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        assert_eq!(selected(&tabs), "b");
        tabs.get_mut(TabId(2))
            .unwrap()
            .terminal_mut()
            .feed(TabId(2), b"hello")
            .unwrap();
        tabs.activate(TabId(2));
        input.route(&mut tabs, &press(2, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(2, 15., 25.), geometry, &clock);
        let range = tabs.get(TabId(2)).unwrap().selection_range().unwrap();
        assert_eq!((range.start.column.0, range.end.column.0), (1, 1));
        tabs.get_mut(TabId(2))
            .unwrap()
            .terminal_mut()
            .feed(TabId(2), b"\x1b[?1049h\x1b[Halt")
            .unwrap();
        input.route(&mut tabs, &press(2, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(2, 15., 25.), geometry, &clock);
        let range = tabs.get(TabId(2)).unwrap().selection_range().unwrap();
        assert_eq!((range.start.column.0, range.end.column.0), (1, 1));
    }
    #[test]
    fn runtime_drag_selects_whole_glyphs() {
        let clock = crate::clock::FakeClock::default();
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        for id in [1, 2] {
            tabs.insert(TabId(id), 2, 8).unwrap();
        }
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), "a中e\u{301}z".as_bytes())
            .unwrap();
        let geometry = Geometry {
            origin: 20,
            size: (80, 20),
            cell: (10, 10),
        };
        let mut input = SelectionInput::default();
        assert!(!input.route(&mut tabs, &press(1, 20., 10.), geometry, &clock)); // Tab strip.
        for (start, end) in [(25., 35.), (35., 25.)] {
            clock.advance(std::time::Duration::from_secs(1)).unwrap();
            assert!(input.route(&mut tabs, &press(1, start, 25.), geometry, &clock));
            assert!(input.route(&mut tabs, &release(1, end, 25.), geometry, &clock));
            let tab = tabs.get(TabId(1)).unwrap();
            assert_eq!(
                crate::selection::copy_text(
                    tab.terminal().grid().unwrap(),
                    tab.selection_range().unwrap()
                )
                .unwrap(),
                "中e\u{301}"
            );
        }
        input.route(&mut tabs, &press(1, 0., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 900., 25.), geometry, &clock);
        let tab = tabs.get(TabId(1)).unwrap();
        assert_eq!(tab.selection_range().unwrap().end.column.0, 7); // Drag clips at grid edge.
        input.route(&mut tabs, &press(1, 10., 25.), geometry, &clock);
        input.cancel(&mut tabs);
        assert!(!input.route(&mut tabs, &release(1, 30., 25.), geometry, &clock));
        input.route(&mut tabs, &press(1, 10., 25.), geometry, &clock);
        tabs.activate(TabId(2));
        assert!(!input.route(&mut tabs, &release(1, 30., 25.), geometry, &clock));
        assert!(!input.route(&mut tabs, &release(2, 30., 25.), geometry, &clock));
        assert!(tabs.get(TabId(2)).unwrap().selection_range().is_none());
        tabs.activate(TabId(1));
        input.route(&mut tabs, &press(1, 10., 25.), geometry, &clock);
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"\x1b[?1049h")
            .unwrap();
        assert!(!input.route(&mut tabs, &release(1, 30., 25.), geometry, &clock));
        assert!(tabs.get(TabId(1)).unwrap().selection_range().is_none());
    }
}
