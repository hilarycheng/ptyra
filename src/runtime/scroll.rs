//! Active-only history input. Keyboard bytes follow bottom; output preserves a scrolled view.
use crate::{
    TabId,
    tabs::{TabPty, TabRegistry},
};
use alacritty_terminal::{grid::Scroll, term::TermMode};

#[derive(Default)]
pub(super) struct Wheel {
    target: Option<TabId>,
    remainder: f64,
}
impl Wheel {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn input(
        &mut self,
        target: TabId,
        mode: TermMode,
        axis: u32,
        value: f64,
    ) -> Option<Scroll> {
        if self.target != Some(target) {
            self.reset();
            self.target = Some(target);
        }
        if mode.intersects(TermMode::MOUSE_MODE | TermMode::ALT_SCREEN) || !value.is_finite() {
            self.reset();
            return None;
        }
        if axis != 0 {
            return None;
        }
        // Ten Wayland logical units per line; accumulate sub-line touchpad motion.
        let total = (self.remainder + value).clamp(-100_000., 100_000.);
        let lines = (total / 10.).trunc() as i32;
        self.remainder = total - f64::from(lines) * 10.;
        (lines != 0).then_some(Scroll::Delta(-lines))
    }
}

fn apply<P: TabPty>(
    tabs: &mut TabRegistry<P>,
    target: TabId,
    scroll: Scroll,
) -> super::Result<bool> {
    if tabs.active() != Some(target) {
        return Ok(false);
    }
    tabs.get_mut(target)
        .ok_or("viewport unavailable")?
        .terminal_mut()
        .scroll_viewport(scroll)
        .map_err(|_| "viewport unavailable")
}
impl super::Runtime {
    pub(super) fn scroll_active(&mut self, scroll: Scroll) {
        let Some(id) = self.tabs.active() else {
            return;
        };
        match apply(&mut self.tabs, id, scroll) {
            Ok(true) => {
                self.pointer_input.cancel_drag();
                self.selection_input.cancel(&mut self.tabs);
                self.paint.dirty = true;
                if self.trace_pointer {
                    let offset = self
                        .tabs
                        .get(id)
                        .and_then(|tab| tab.terminal().grid())
                        .map(|grid| grid.display_offset())
                        .unwrap_or_default();
                    eprintln!("Ptyra viewport: tab={} offset={offset}", id.0);
                }
            }
            Err(error) => eprintln!("Ptyra scroll: {error}"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::grid::Dimensions;
    struct Fake;
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            TabId(1)
        }
    }
    #[test]
    fn runtime_scroll_input_targets_active_viewport() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(20).unwrap();
        for id in [TabId(1), TabId(2)] {
            tabs.insert(id, 4, 8).unwrap();
            for _ in 0..12 {
                tabs.get_mut(id)
                    .unwrap()
                    .terminal_mut()
                    .feed(id, b"line\r\n")
                    .unwrap();
            }
        }
        let offset = |tabs: &TabRegistry<Fake>, id| {
            tabs.get(id)
                .unwrap()
                .terminal()
                .grid()
                .unwrap()
                .display_offset()
        };
        let mut wheel = Wheel::default();
        let scroll = wheel.input(TabId(1), TermMode::empty(), 0, -25.).unwrap();
        assert!(apply(&mut tabs, TabId(1), scroll).unwrap());
        assert_eq!(offset(&tabs, TabId(1)), 2);
        assert_eq!(offset(&tabs, TabId(2)), 0);
        assert!(!apply(&mut tabs, TabId(2), Scroll::Top).unwrap());
        let scroll = wheel.input(TabId(1), TermMode::empty(), 0, -5.).unwrap();
        apply(&mut tabs, TabId(1), scroll).unwrap();
        assert_eq!(offset(&tabs, TabId(1)), 3);
        assert!(wheel.input(TabId(1), TermMode::empty(), 1, -100.).is_none());
        for mode in [
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
            TermMode::ALT_SCREEN,
        ] {
            assert!(wheel.input(TabId(1), mode, 0, -100.).is_none());
        }
        assert!(
            wheel
                .input(TabId(1), TermMode::empty(), 0, f64::NAN)
                .is_none()
        );
        assert!(wheel.input(TabId(1), TermMode::empty(), 0, -5.).is_none());
        assert!(wheel.input(TabId(2), TermMode::empty(), 0, -5.).is_none());
        let before = tabs
            .get(TabId(1))
            .unwrap()
            .terminal()
            .grid()
            .unwrap()
            .cursor
            .clone();
        apply(&mut tabs, TabId(1), Scroll::PageUp).unwrap();
        let page = offset(&tabs, TabId(1));
        assert!(page > 3);
        apply(&mut tabs, TabId(1), Scroll::PageDown).unwrap();
        assert!(offset(&tabs, TabId(1)) < page);
        apply(&mut tabs, TabId(1), Scroll::Top).unwrap();
        assert_eq!(
            offset(&tabs, TabId(1)),
            tabs.get(TabId(1))
                .unwrap()
                .terminal()
                .grid()
                .unwrap()
                .history_size()
        );
        assert_eq!(
            tabs.get(TabId(1))
                .unwrap()
                .terminal()
                .grid()
                .unwrap()
                .cursor,
            before
        );
        // New output keeps the viewed history anchored, without following the live bottom.
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"new\r\n")
            .unwrap();
        assert!(offset(&tabs, TabId(1)) > 0);
        tabs.activate(TabId(2));
        let scroll = wheel
            .input(TabId(2), TermMode::empty(), 0, -f64::MAX)
            .unwrap();
        apply(&mut tabs, TabId(2), scroll).unwrap();
        assert!(offset(&tabs, TabId(1)) > 0);
        assert!(offset(&tabs, TabId(2)) > 0);
        apply(&mut tabs, TabId(2), Scroll::Bottom).unwrap();
        assert_eq!(offset(&tabs, TabId(2)), 0);
        assert!(offset(&tabs, TabId(1)) > 0);
        wheel.reset();
        assert!(wheel.input(TabId(2), TermMode::empty(), 0, -5.).is_none());
    }
}
