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
pub(super) struct SelectionInput {
    scale: u32,
    drag: Option<(TabId, Screen, u64)>,
    click: Option<Click>,
    publish: bool,
    pub(super) limit: usize,
    pub(super) error: Option<&'static str>,
    frame: Vec<u8>,
    frame_size: Option<(u32, u32)>,
    pub(super) repaint: bool,
}
impl Default for SelectionInput {
    fn default() -> Self {
        Self {
            scale: 1,
            drag: None,
            click: None,
            publish: false,
            limit: crate::clipboard::write::MAX_CLIPBOARD_BYTES,
            error: None,
            frame: Vec::new(),
            frame_size: None,
            repaint: false,
        }
    }
}
struct Click {
    owner: (TabId, Screen, u64),
    point: crate::input::Point,
    at: std::time::Duration,
    count: u8,
}
impl Click {
    fn near(&self, point: crate::input::Point, scale: u32) -> bool {
        let tolerance = 4. * f64::from(scale);
        (self.point.x() - point.x()).abs() <= tolerance
            && (self.point.y() - point.y()).abs() <= tolerance
    }
}
impl SelectionInput {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }
    pub fn set_scale(&mut self, scale: u32) {
        self.scale = scale;
        self.click = None;
    }
    pub fn take_publication(&mut self) -> bool {
        std::mem::take(&mut self.publish)
    }
    pub fn cancel<P: TabPty>(&mut self, tabs: &mut TabRegistry<P>) -> bool {
        self.click = None;
        self.publish = false;
        if let Some((id, screen, _)) = self.drag.take() {
            self.frame = Vec::new();
            self.frame_size = None;
            self.repaint = true;
            if let Some(tab) = tabs.get_mut(id) {
                tab.cancel_selection_drag(screen);
            }
            return true;
        }
        false
    }
    pub(super) fn validate<P: TabPty>(&mut self, tabs: &mut TabRegistry<P>, size: (u32, u32)) {
        if self.drag.is_some_and(|(id, _, _)| {
            tabs.active() != Some(id)
                || tabs
                    .get(id)
                    .and_then(|tab| tab.frozen_selection())
                    .is_none()
                || self.frame_size.is_some_and(|old| old != size)
        }) {
            self.cancel(tabs);
        }
    }
    pub(super) fn capture_frame(
        &mut self,
        terminal: &crate::terminal::Terminal,
        raster: &mut super::raster::Raster,
        size: (u32, u32),
        focused: bool,
        cursor: bool,
    ) -> super::Result<()> {
        if self.drag.is_some() && self.frame.is_empty() {
            super::resize_frame_pixels(&mut self.frame, size.0 as usize * size.1 as usize * 4)?;
            raster.draw(terminal, size, &mut self.frame, focused, cursor)?;
            self.frame_size = Some(size);
            self.repaint = true;
        }
        Ok(())
    }
    pub(super) fn draw_frozen_rows(
        &self,
        pixels: &mut [u8],
        width: u32,
        rows: std::ops::Range<u32>,
    ) -> bool {
        if self.frame.is_empty() || self.frame.len() != pixels.len() {
            return false;
        }
        let range =
            rows.start as usize * width as usize * 4..rows.end as usize * width as usize * 4;
        pixels[range.clone()].copy_from_slice(&self.frame[range]);
        true
    }
    pub fn route<P: TabPty>(
        &mut self,
        tabs: &mut TabRegistry<P>,
        event: &Routed,
        geometry: Geometry,
        clock: &impl crate::clock::Clock,
    ) -> bool {
        self.publish = false;
        self.error = None;
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
                .is_some_and(|click| !click.near(event.position, self.scale))
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
        // A held frame still shows the old cells. Never select/copy unseen core updates.
        if tab.frozen_selection().is_none() && tab.terminal().sync_blocks_selection() {
            return self.cancel(tabs);
        }
        let Some((screen, epoch, offset)) = tab.selection_owner() else {
            return false;
        };
        let owner = (event.target.tab, screen, epoch);
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
            Line(row.min(grid.screen_lines() - 1) as i32 - offset as i32),
            Column(column.min(grid.columns() - 1)),
        );
        if start {
            let at = clock.now();
            let count = self
                .click
                .as_ref()
                .filter(|click| {
                    click.owner == owner
                        && click.near(event.position, self.scale)
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
            if count == 1
                && !tabs
                    .get_mut(event.target.tab)
                    .unwrap()
                    .begin_selection_snapshot()
            {
                self.cancel(tabs);
                self.error = Some("selection unavailable");
                return false;
            }
        }
        if matches!(action, SelectionAction::Release) {
            self.drag = None;
            self.frame = Vec::new();
            self.frame_size = None;
            self.repaint = true;
        }
        self.publish = matches!(
            action,
            SelectionAction::Release | SelectionAction::Word | SelectionAction::Line
        );
        let changed = tabs
            .get_mut(event.target.tab)
            .unwrap()
            .select(screen, action, point);
        if self.publish {
            self.error = tabs.save_selection(event.target.tab, self.limit).err();
        }
        changed
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
    fn runtime_scaled_clicks_keep_logical_tolerance() {
        for scale in [1, 2, 1] {
            let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
            tabs.insert(TabId(1), 3, 8).unwrap();
            tabs.get_mut(TabId(1))
                .unwrap()
                .terminal_mut()
                .feed(TabId(1), b"abc def")
                .unwrap();
            let geometry = Geometry {
                origin: 20 * scale,
                size: (80 * scale, 30 * scale),
                cell: (10 * scale, 10 * scale),
            };
            let clock = crate::clock::FakeClock::default();
            let mut input = SelectionInput::default();
            input.set_scale(scale);
            for (x, count) in [(12., 1), (15., 2), (18., 3), (23., 1)] {
                let point = crate::input::Point::new(x, 25.).unwrap();
                let point = super::super::scale::buffer_point(point, scale).unwrap();
                input.route(
                    &mut tabs,
                    &event(1, point.x(), point.y(), Kind::Motion),
                    geometry,
                    &clock,
                );
                input.route(&mut tabs, &press(1, point.x(), point.y()), geometry, &clock);
                input.route(
                    &mut tabs,
                    &release(1, point.x(), point.y()),
                    geometry,
                    &clock,
                );
                assert_eq!(input.click.as_ref().unwrap().count, count);
                let range = tabs.get(TabId(1)).unwrap().selection_range().unwrap();
                if count == 2 {
                    assert_eq!((range.start.column.0, range.end.column.0), (0, 2));
                }
                if count == 3 {
                    assert_eq!((range.start.column.0, range.end.column.0), (0, 7));
                }
                clock.advance(std::time::Duration::from_millis(50)).unwrap();
            }
        }
    }
    #[test]
    fn runtime_selection_snapshot_survives_output() {
        use super::super::clipboard::{Binding, prepare_copy};
        use crate::clipboard::{InputSerialEvent, SeatId};
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        tabs.insert(TabId(1), 3, 8).unwrap();
        tabs.insert(TabId(2), 3, 8).unwrap();
        let clock = crate::clock::FakeClock::default();
        let geometry = Geometry {
            origin: 20,
            size: (80, 30),
            cell: (10, 10),
        };
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), "a中e\u{301} zXYmore\r\nhard".as_bytes())
            .unwrap();
        let mut input = SelectionInput::default();
        assert!(input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock));
        let frozen = tabs.get(TabId(1)).unwrap().frozen_selection().unwrap();
        assert_eq!(frozen.grid.history_size(), 0);
        assert_eq!(frozen.grid.screen_lines() * frozen.grid.columns(), 24);
        let font = ab_glyph::FontVec::try_from_vec(
            include_bytes!("../../tests/fixtures/fallback-primary.ttf").to_vec(),
        )
        .unwrap();
        let mut raster =
            super::super::raster::Raster::new(vec![font], &crate::config::Config::default())
                .unwrap();
        let size = (raster.cell_width * 8, raster.cell_height * 3);
        input
            .capture_frame(
                tabs.get(TabId(1)).unwrap().terminal(),
                &mut raster,
                size,
                true,
                true,
            )
            .unwrap();
        let original_pixels = input.frame.clone();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"\x1b[3;1Hclock\x1b[HREPLACED")
            .unwrap();
        assert!(input.route(
            &mut tabs,
            &event(1, 35., 45., Kind::Motion),
            geometry,
            &clock
        ));
        let expected = "a中e\u{301} zXYmore    \nhard";
        assert_eq!(
            tabs.get_mut(TabId(1))
                .unwrap()
                .copy_selection_text(1024)
                .unwrap(),
            expected
        );
        input
            .capture_frame(
                tabs.get(TabId(1)).unwrap().terminal(),
                &mut raster,
                size,
                true,
                true,
            )
            .unwrap();
        let mut pixels = vec![0; original_pixels.len()];
        assert!(input.draw_frozen_rows(&mut pixels, size.0, 0..size.1));
        assert_eq!(
            pixels, original_pixels,
            "refresh must not alter the frozen base frame"
        );
        let mut overlay = tabs
            .get(TabId(1))
            .unwrap()
            .selection_overlay_snapshot()
            .unwrap();
        assert_eq!(overlay.content.display_iter.next().unwrap().c, 'a');
        assert!(input.route(&mut tabs, &release(1, 35., 45.), geometry, &clock));
        assert!(input.take_publication());
        assert!(input.frame.is_empty());
        assert!(tabs.get(TabId(1)).unwrap().frozen_selection().is_none());
        assert!(tabs.get(TabId(1)).unwrap().selection_range().is_none());
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"\r\nmore output\x1b[6n")
            .unwrap();
        assert!(
            !tabs
                .get_mut(TabId(1))
                .unwrap()
                .terminal_mut()
                .take_core_replies()
                .0
                .is_empty()
        );
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(1),
        };
        let seat = SeatId(1);
        let mut binding = Binding::default();
        binding.sync(Some((1, seat)));
        let tab = tabs.get_mut(TabId(1)).unwrap();
        assert_eq!(
            prepare_copy(tab, &mut binding, seat, target, 1024).err(),
            Some("no valid input serial")
        );
        binding.record(seat, target, 3, InputSerialEvent::KeyPress);
        assert_eq!(
            prepare_copy(tab, &mut binding, seat, target, 2).err(),
            Some("selection exceeds clipboard limit")
        );
        assert_eq!(
            prepare_copy(tab, &mut binding, seat, target, 1024)
                .unwrap()
                .text,
            expected
        );
        assert!(tabs.activate(TabId(2)));
        assert_eq!(
            tabs.get_mut(TabId(2)).unwrap().copy_selection_text(1024),
            Err("no valid selection")
        );
        assert!(tabs.activate(TabId(1)));
        assert_eq!(
            tabs.get_mut(TabId(1))
                .unwrap()
                .copy_selection_text(1024)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn runtime_selection_snapshot_cancels_and_limits_without_stale_copy() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(4).unwrap();
        for id in [1, 2] {
            tabs.insert(TabId(id), 3, 8).unwrap();
        }
        let clock = crate::clock::FakeClock::default();
        let geometry = Geometry {
            origin: 20,
            size: (80, 30),
            cell: (10, 10),
        };
        let mut input = SelectionInput::with_limit(2);
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), "中\r\n1\r\n2\r\n3\r\n4".as_bytes())
            .unwrap();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .scroll_viewport(alacritty_terminal::grid::Scroll::Top)
            .unwrap();
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        assert_eq!(
            tabs.get(TabId(1))
                .unwrap()
                .frozen_selection()
                .unwrap()
                .grid
                .history_size(),
            0
        );
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        assert_eq!(input.error, Some("selection exceeds clipboard limit"));
        assert_eq!(
            tabs.get_mut(TabId(1)).unwrap().copy_selection_text(1024),
            Err("selection exceeds clipboard limit")
        );
        clock.advance(std::time::Duration::from_secs(1)).unwrap();
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        tabs.activate(TabId(2));
        input.validate(&mut tabs, geometry.size);
        assert!(tabs.get(TabId(1)).unwrap().frozen_selection().is_none());
        assert!(!input.route(&mut tabs, &release(2, 15., 25.), geometry, &clock));
        assert_eq!(
            tabs.get_mut(TabId(2)).unwrap().copy_selection_text(1024),
            Err("no valid selection")
        );
        tabs.activate(TabId(1));
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        input.cancel(&mut tabs); // Focus loss / pointer leave.
        assert_eq!(
            tabs.get_mut(TabId(1)).unwrap().copy_selection_text(1024),
            Err("no valid selection")
        );
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        tabs.resize_all(4, 8).unwrap();
        input.validate(&mut tabs, geometry.size);
        assert!(tabs.get(TabId(1)).unwrap().frozen_selection().is_none());
    }

    #[test]
    fn runtime_history_selection_copies_original_lines() {
        use alacritty_terminal::grid::Scroll;
        let id = TabId(1);
        let mut tabs = TabRegistry::<Fake>::with_scrollback(4).unwrap();
        tabs.insert(id, 3, 8).unwrap();
        tabs.insert(TabId(2), 3, 8).unwrap();
        tabs.get_mut(id)
            .unwrap()
            .terminal_mut()
            .feed(
                id,
                "A中e\u{301}  XYZW\r\nhard  \r\nlive1\r\nlive2\r\nlive3".as_bytes(),
            )
            .unwrap();
        tabs.get_mut(id)
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Top)
            .unwrap();
        let clock = crate::clock::FakeClock::default();
        let geometry = Geometry {
            origin: 20,
            size: (80, 30),
            cell: (10, 10),
        };
        let mut input = SelectionInput::default();
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 55., 45.), geometry, &clock);
        let selected = |tabs: &mut TabRegistry<Fake>| {
            let tab = tabs.get_mut(id).unwrap();
            let range = tab.selection_range()?;
            tab.terminal_mut().copy_range_text(range)
        };
        let original = "A中e\u{301}  XYZW      \nhard  ";
        assert_eq!(selected(&mut tabs).as_deref(), Some(original));
        let range = tabs.get(id).unwrap().selection_range().unwrap();
        assert_eq!((range.start.line.0, range.end.line.0), (-3, -1));
        let mut pixels = vec![0x55; 80 * 30 * 4];
        super::super::selection_overlay::draw(
            tabs.get(id).unwrap(),
            (80, 30),
            (10, 10),
            &mut pixels,
        )
        .unwrap();
        assert_eq!(pixels[0], 0xaa);
        assert_eq!(pixels[(29 * 80 + 59) * 4], 0xaa);
        assert_eq!(pixels[(29 * 80 + 60) * 4], 0x55);

        // Completed ranges stay attached to unchanged grid/history, even offscreen.
        tabs.get_mut(id)
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Bottom)
            .unwrap();
        input.cancel(&mut tabs);
        assert_eq!(selected(&mut tabs).as_deref(), Some(original));
        pixels.fill(0x55);
        super::super::selection_overlay::draw(
            tabs.get(id).unwrap(),
            (80, 30),
            (10, 10),
            &mut pixels,
        )
        .unwrap();
        assert!(pixels.iter().all(|byte| *byte == 0x55));
        tabs.activate(TabId(2));
        assert!(tabs.get(TabId(2)).unwrap().selection_range().is_none());
        tabs.activate(id);
        tabs.get_mut(id)
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Top)
            .unwrap();
        // A wide spacer hit still selects its original complete historical glyph.
        clock.advance(std::time::Duration::from_secs(1)).unwrap();
        for _ in 0..2 {
            input.route(&mut tabs, &press(1, 25., 25.), geometry, &clock);
            input.route(&mut tabs, &release(1, 25., 25.), geometry, &clock);
        }
        assert_eq!(selected(&mut tabs).as_deref(), Some("中"));
        input.route(&mut tabs, &press(1, 25., 25.), geometry, &clock);
        assert_eq!(
            selected(&mut tabs).as_deref(),
            Some("A中e\u{301}  XYZW      ")
        );
        // Runtime scroll cancels an unfinished drag before any later pointer event.
        clock.advance(std::time::Duration::from_secs(1)).unwrap();
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        tabs.get_mut(id)
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Delta(-1))
            .unwrap();
        input.cancel(&mut tabs);
        assert!(!input.route(&mut tabs, &release(1, 75., 45.), geometry, &clock));
        assert!(selected(&mut tabs).is_none());
        // A complete range is invalidated by any output/reflow, including eviction.
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        tabs.get_mut(id)
            .unwrap()
            .terminal_mut()
            .feed(id, b"\r\na\r\nb\r\nc\r\nd\r\ne")
            .unwrap();
        assert!(selected(&mut tabs).is_none());
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        tabs.resize_all(4, 6).unwrap();
        assert!(selected(&mut tabs).is_none());
    }
    #[test]
    fn primary_publication_waits_for_completed_selection() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        tabs.insert(TabId(1), 3, 8).unwrap();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"abc def")
            .unwrap();
        let geometry = Geometry {
            origin: 20,
            size: (80, 30),
            cell: (10, 10),
        };
        let clock = crate::clock::FakeClock::default();
        let mut input = SelectionInput::default();
        input.route(&mut tabs, &press(1, 5., 25.), geometry, &clock);
        assert!(!input.take_publication());
        input.route(
            &mut tabs,
            &event(1, 25., 25., Kind::Motion),
            geometry,
            &clock,
        );
        assert!(!input.take_publication());
        let range = tabs.get(TabId(1)).unwrap().selection_range();
        input.route(&mut tabs, &release(1, 25., 25.), geometry, &clock);
        assert_eq!(tabs.get(TabId(1)).unwrap().selection_range(), range);
        assert!(input.take_publication()); // Publish even if motion already set the final range.
        assert!(!input.take_publication());
        input.route(&mut tabs, &release(1, 25., 25.), geometry, &clock);
        assert!(!input.take_publication());
        clock.advance(std::time::Duration::from_secs(1)).unwrap();
        for count in 1..=3 {
            input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
            assert_eq!(input.take_publication(), count > 1);
            input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
            assert_eq!(input.take_publication(), count == 1);
        }
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.cancel(&mut tabs);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        assert!(!input.take_publication());
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
    fn runtime_selection_survives_only_valid_anchors() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(2).unwrap();
        for id in [1, 2] {
            tabs.insert(TabId(id), 2, 8).unwrap();
        }
        let clock = crate::clock::FakeClock::default();
        let geometry = Geometry {
            origin: 20,
            size: (80, 20),
            cell: (10, 10),
        };
        let mut input = SelectionInput::default();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"original")
            .unwrap();
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 25., 25.), geometry, &clock);
        assert!(tabs.get(TabId(1)).unwrap().selection_range().is_some());
        assert!(!tabs.resize_all(2, 8).unwrap());
        assert!(tabs.get(TabId(1)).unwrap().selection_range().is_some());
        tabs.get_mut(TabId(2))
            .unwrap()
            .terminal_mut()
            .feed(TabId(2), b"other")
            .unwrap();
        assert!(tabs.get(TabId(1)).unwrap().selection_range().is_some());
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"\rCHANGED")
            .unwrap();
        assert!(tabs.get(TabId(1)).unwrap().selection_range().is_none());
        // A completed click must also be invalidated, even inside the multi-click interval.
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"\x1b[?1049h\x1b[?1049l")
            .unwrap();
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        input.route(&mut tabs, &release(1, 15., 25.), geometry, &clock);
        let range = tabs.get(TabId(1)).unwrap().selection_range().unwrap();
        assert_eq!((range.start.column.0, range.end.column.0), (1, 1));
        for mutation in 0..3 {
            clock.advance(std::time::Duration::from_secs(1)).unwrap();
            input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
            match mutation {
                0 => {
                    tabs.resize_all(3, 10).unwrap();
                }
                1 => tabs
                    .get_mut(TabId(1))
                    .unwrap()
                    .terminal_mut()
                    .feed(TabId(1), b"\x1b[?1049hALT\x1b[?1049l")
                    .unwrap(),
                _ => tabs
                    .get_mut(TabId(1))
                    .unwrap()
                    .terminal_mut()
                    .feed(TabId(1), b"\r\n1\r\n2\r\n3\r\n4\r\n5")
                    .unwrap(),
            }
            if mutation == 2 {
                assert!(tabs.get(TabId(1)).unwrap().selection_range().is_some());
                assert!(input.route(&mut tabs, &release(1, 35., 25.), geometry, &clock));
            } else {
                assert!(tabs.get(TabId(1)).unwrap().selection_range().is_none());
                assert!(!input.route(&mut tabs, &release(1, 35., 25.), geometry, &clock));
            }
            assert!(tabs.get(TabId(1)).unwrap().selection_range().is_none());
        }
        clock.advance(std::time::Duration::from_secs(1)).unwrap();
        input.route(&mut tabs, &press(1, 15., 25.), geometry, &clock);
        tabs.close(TabId(1));
        assert!(!input.route(&mut tabs, &release(1, 35., 25.), geometry, &clock));
        assert!(tabs.get(TabId(2)).unwrap().selection_range().is_none());
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
