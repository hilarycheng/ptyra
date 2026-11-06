//! One active-view cursor deadline, independent of PTY traffic and frame callbacks.
use crate::{
    TabId, WindowId,
    clock::{Clock, Deadline},
    input::KeyboardFocus,
    tabs::{TabPty, TabRegistry},
};
use alacritty_terminal::vte::ansi::CursorShape;
use std::{sync::Arc, time::Duration};
const INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, PartialEq, Eq)]
struct Appearance {
    tab: TabId,
    position: (usize, usize),
    shape: CursorShape,
    focused: bool,
}
struct View {
    appearance: Appearance,
    blinking: bool,
    epoch: Arc<()>,
}
impl View {
    fn matches(&self, other: &Self) -> bool {
        self.appearance == other.appearance
            && self.blinking == other.blinking
            && Arc::ptr_eq(&self.epoch, &other.epoch)
    }
}
pub(super) fn focused<P: TabPty>(tabs: &TabRegistry<P>, focus: &KeyboardFocus) -> bool {
    focus
        .target()
        .is_some_and(|target| target.window == WindowId(1) && Some(target.tab) == tabs.active())
}

pub(super) struct State {
    view: Option<View>,
    visible: bool,
    deadline: Option<Deadline>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            view: None,
            visible: true,
            deadline: None,
        }
    }
}
impl State {
    pub fn visible(&self) -> bool {
        self.visible
    }
    pub fn deadline(&self) -> Option<Deadline> {
        self.deadline
    }
    fn appearance(&self) -> Option<Appearance> {
        self.view
            .as_ref()
            .filter(|_| self.visible)
            .map(|view| view.appearance)
    }
    pub fn refresh<P: TabPty>(
        &mut self,
        tabs: &TabRegistry<P>,
        focus: &KeyboardFocus,
        configured: bool,
        clock: &impl Clock,
    ) -> bool {
        let before = self.appearance();
        let next = tabs.active().filter(|_| configured).and_then(|id| {
            let terminal = tabs.get(id)?.terminal();
            let snapshot = terminal.snapshot()?;
            let cursor = snapshot.content.cursor;
            let row = snapshot
                .content
                .display_offset
                .checked_add_signed(cursor.point.line.0 as isize)?;
            if cursor.shape == CursorShape::Hidden
                || row >= snapshot.rows
                || cursor.point.column.0 >= snapshot.columns
            {
                return None;
            }
            Some(View {
                appearance: Appearance {
                    tab: id,
                    position: (row, cursor.point.column.0),
                    shape: cursor.shape,
                    focused: focused(tabs, focus),
                },
                blinking: terminal.cursor_blinking(),
                epoch: focus.epoch(),
            })
        });
        let unchanged = match (&self.view, &next) {
            (Some(old), Some(new)) => old.matches(new),
            (None, None) => true,
            _ => false,
        };
        if !unchanged {
            self.visible = true;
            self.deadline = next
                .as_ref()
                .filter(|view| view.blinking && view.appearance.focused)
                .and_then(|_| Deadline::after(clock, INTERVAL));
            self.view = next;
        } else if self.deadline.is_some_and(|deadline| deadline.is_due(clock)) {
            // A late wake toggles once, never replays missed intervals. Overflow stays visible.
            self.deadline = Deadline::after(clock, INTERVAL);
            self.visible = self.deadline.is_none() || !self.visible;
        }
        before != self.appearance()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        clock::FakeClock,
        runtime::{PaintState, sync},
    };
    use alacritty_terminal::grid::Scroll;
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
    fn runtime_cursor_blink_has_no_idle_busy_loop() {
        let a = TabId(1);
        let b = TabId(2);
        let mut tabs = TabRegistry::<Fake>::default();
        tabs.insert(a, 4, 8).unwrap();
        tabs.insert(b, 4, 8).unwrap();
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), a);
        let clock = FakeClock::default();
        let mut blink = State::default();
        assert!(!blink.refresh(&tabs, &focus, false, &clock));
        assert!(blink.visible());
        assert!(blink.deadline().is_none());
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(blink.deadline().is_none(), "default cursor is steady");
        for _ in 0..100 {
            clock.advance(INTERVAL).unwrap();
            assert!(!blink.refresh(&tabs, &focus, true, &clock));
            assert!(blink.deadline().is_none());
        }
        for (code, shape) in [
            (1, CursorShape::Block),
            (3, CursorShape::Underline),
            (5, CursorShape::Beam),
        ] {
            feed(&mut tabs, a, format!("\x1b[{code} q").as_bytes());
            blink.refresh(&tabs, &focus, true, &clock);
            assert_eq!(blink.view.as_ref().unwrap().appearance.shape, shape);
            assert!(blink.visible());
            let deadline = blink
                .deadline()
                .expect("blinking mode needs a finite deadline");
            assert_eq!(deadline.remaining(&clock), INTERVAL);
            for _ in 0..10 {
                clock.advance(Duration::from_millis(49)).unwrap();
                feed(&mut tabs, b, b"background output");
                feed(&mut tabs, a, b"\x1b[31m");
                assert!(!blink.refresh(&tabs, &focus, true, &clock));
                assert_eq!(
                    blink.deadline(),
                    Some(deadline),
                    "traffic must not delay blinking"
                );
            }
            clock.advance(Duration::from_millis(9)).unwrap();
            assert!(!blink.refresh(&tabs, &focus, true, &clock));
            clock.advance(Duration::from_millis(1)).unwrap();
            assert!(blink.refresh(&tabs, &focus, true, &clock));
            assert!(!blink.visible());
            assert_eq!(blink.deadline().unwrap().remaining(&clock), INTERVAL);
            assert!(!blink.refresh(&tabs, &focus, true, &clock));
            // A late wake emits just one appearance change and rearms from now.
            clock.advance(Duration::from_secs(50)).unwrap();
            assert!(blink.refresh(&tabs, &focus, true, &clock));
            assert!(blink.visible());
            assert_eq!(blink.deadline().unwrap().remaining(&clock), INTERVAL);
            assert!(!blink.refresh(&tabs, &focus, true, &clock));
            clock.advance(INTERVAL).unwrap();
            assert!(blink.refresh(&tabs, &focus, true, &clock));
            assert!(!blink.visible());
            // The paired steady mode restores the exact shape, not a default block.
            feed(&mut tabs, a, format!("\x1b[{} q", code + 1).as_bytes());
            assert!(blink.refresh(&tabs, &focus, true, &clock));
            assert!(blink.visible());
            assert_eq!(blink.view.as_ref().unwrap().appearance.shape, shape);
            assert!(blink.deadline().is_none());
        }
        feed(&mut tabs, a, b"\x1b[?12h");
        assert!(
            !blink.refresh(&tabs, &focus, true, &clock),
            "mode-only change leaves visible pixels alone"
        );
        clock.advance(INTERVAL).unwrap();
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        feed(&mut tabs, a, b"\x1b[?12l");
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(blink.visible());
        assert!(blink.deadline().is_none());
        feed(&mut tabs, a, b"\x1b[?12h\x1b[?25l");
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(blink.view.is_none());
        assert!(blink.deadline().is_none());
        clock.advance(Duration::from_secs(50)).unwrap();
        assert!(!blink.refresh(&tabs, &focus, true, &clock));
        feed(&mut tabs, a, b"\x1b[?25h");
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert_eq!(blink.deadline().unwrap().remaining(&clock), INTERVAL);
        clock.advance(INTERVAL).unwrap();
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(!blink.visible());
        focus.leave(WindowId(1));
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(blink.visible(), "unfocused outline stays visible");
        assert!(blink.deadline().is_none());
        clock.advance(Duration::from_secs(10)).unwrap();
        assert!(!blink.refresh(&tabs, &focus, true, &clock));
        focus.enter(WindowId(1), a);
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        clock.advance(INTERVAL).unwrap();
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(!blink.visible());
        focus.leave(WindowId(1));
        focus.enter(WindowId(1), a);
        assert!(
            blink.refresh(&tabs, &focus, true, &clock),
            "same-target new focus epoch restores cursor"
        );
        assert!(blink.visible());
        feed(&mut tabs, a, b"\x1b[2;3H");
        clock.advance(Duration::from_millis(100)).unwrap();
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert_eq!(blink.deadline().unwrap().remaining(&clock), INTERVAL);
        tabs.activate(b);
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(
            blink.deadline().is_none(),
            "stale focus target must not animate new tab"
        );
        feed(&mut tabs, a, b"\x1b[1 qmore hidden output");
        assert!(!blink.refresh(&tabs, &focus, true, &clock));
        focus.enter(WindowId(1), b);
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(
            blink.deadline().is_none(),
            "background blinking tab never arms active steady tab"
        );
        feed(&mut tabs, b, b"\x1b[1 q");
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(blink.deadline().is_some());
        tabs.close(b).unwrap();
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(blink.deadline().is_none());
        focus.enter(WindowId(99), a);
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(blink.deadline().is_none());
        focus.enter(WindowId(1), a);
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(blink.deadline().is_some());
        feed(&mut tabs, a, b"\x1bc");
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(blink.visible());
        assert!(blink.deadline().is_none(), "RIS restores steady default");
        feed(&mut tabs, a, b"\x1b[1 q\x1b[0 q");
        blink.refresh(&tabs, &focus, true, &clock);
        assert!(
            blink.deadline().is_none(),
            "DECSCUSR0 restores configured steady default"
        );
        feed(&mut tabs, a, b"\x1b[1 q\r\n\r\n\r\n\r\n\r\n\r\n");
        blink.refresh(&tabs, &focus, true, &clock);
        tabs.get_mut(a)
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Top)
            .unwrap();
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(blink.view.is_none());
        assert!(
            blink.deadline().is_none(),
            "cursor below history viewport cannot blink"
        );
        tabs.get_mut(a)
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Bottom)
            .unwrap();
        assert!(blink.refresh(&tabs, &focus, true, &clock));
        assert!(blink.visible());
        assert_eq!(blink.deadline().unwrap().remaining(&clock), INTERVAL);
        let mut paint = PaintState {
            configured: true,
            frame_pending: true,
            ..Default::default()
        };
        clock.advance(INTERVAL).unwrap();
        paint.dirty |= blink.refresh(&tabs, &focus, true, &clock);
        assert!(paint.dirty);
        assert!(!paint.ready(), "blink must not bypass pending frame gate");
        paint.frame_pending = false;
        feed(&mut tabs, a, b"\x1b[?2026h");
        sync::refresh(&mut tabs, &clock);
        assert!(paint.ready());
        assert!(
            sync::held(&tabs),
            "draw must retain synchronized presentation gate"
        );
        clock.advance(Duration::from_millis(150)).unwrap();
        sync::refresh(&mut tabs, &clock);
        assert!(!sync::held(&tabs));
        paint.submitted();
        paint.frame_pending = false;
        assert!(!paint.ready());
        assert!(!blink.refresh(&tabs, &focus, true, &clock));
        let limit = FakeClock::default();
        limit.advance(Duration::MAX - INTERVAL).unwrap();
        let mut overflow = State::default();
        overflow.refresh(&tabs, &focus, true, &limit);
        assert!(overflow.deadline().is_some());
        limit.advance(INTERVAL).unwrap();
        assert!(!overflow.refresh(&tabs, &focus, true, &limit));
        assert!(overflow.visible());
        assert!(overflow.deadline().is_none());
        assert!(!overflow.refresh(&tabs, &focus, true, &limit));
        assert!(!blink.visible());
        assert!(!blink.refresh(&tabs, &focus, false, &clock));
        assert!(blink.deadline().is_none());
    }
}
