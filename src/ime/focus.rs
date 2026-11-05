use super::{ImeState, ImeUpdate};
use crate::{
    WindowId,
    clipboard::SeatId,
    input::{FocusTarget, KeyboardFocus},
};
use std::sync::Arc;

/// Capture at event ingress, before queuing work; never relabel a queued event with current focus.
#[derive(Clone)]
pub struct ImeFocus {
    seat: SeatId,
    surface: WindowId,
    pub(super) target: Option<FocusTarget>,
    keyboard_epoch: Arc<()>,
    ime_epoch: Arc<()>,
}

impl ImeState {
    pub fn capture_focus(
        &self,
        seat: SeatId,
        surface: WindowId,
        keyboard: &KeyboardFocus,
    ) -> ImeFocus {
        ImeFocus {
            seat,
            surface,
            target: keyboard.target(),
            keyboard_epoch: keyboard.epoch(),
            ime_epoch: self.focus_epoch.clone(),
        }
    }

    pub(super) fn matches_focus(&self, focus: &ImeFocus, keyboard: &KeyboardFocus) -> bool {
        focus.seat == self.seat
            && self.surface == Some(focus.surface)
            && Arc::ptr_eq(&focus.ime_epoch, &self.focus_epoch)
            && focus.target.is_some_and(|target| {
                target.window == focus.surface
                    && self.enabled_target == Some(target)
                    && keyboard.matches(target, &focus.keyboard_epoch)
                    && self
                        .enabled_focus
                        .as_ref()
                        .is_some_and(|epoch| Arc::ptr_eq(epoch, &focus.keyboard_epoch))
            })
    }
}

impl ImeUpdate {
    pub fn is_current(&self, ime: &ImeState, keyboard: &KeyboardFocus) -> bool {
        self.focus.target == Some(self.target) && ime.matches_focus(&self.focus, keyboard)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        TabId, WindowId,
        clipboard::{PasteSink, SeatId},
        ime::{ImeEvent, ImeState},
        input::KeyboardFocus,
        wayland::{Capabilities, Global},
    };

    #[test]
    fn ime_restart_keeps_normal_keyboard() {
        use crate::{
            clock::FakeClock,
            ime::key::{KeyOwner, route_key},
            keymap::Keymap,
            repeat::RepeatScheduler,
        };
        let seat = SeatId(1);
        let window = WindowId(1);
        let caps = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(window, TabId(7));
        let map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/greek.xkb")).unwrap();
        let clock = FakeClock::default();
        let mut repeat = RepeatScheduler::default();
        let mut ime = ImeState::new(seat, &caps);
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        let old = ime.capture_focus(seat, window, &keyboard);
        ime.stage(&old, &keyboard, ImeEvent::Commit { text: Some("old") })
            .unwrap();
        for capabilities in [&Capabilities::default(), &caps] {
            ime.reset(capabilities);
            assert!(ime.done(&old, &keyboard, 1).is_none());
            assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
            let key = map
                .translate(keyboard.route_key(30, true).unwrap(), false)
                .unwrap()
                .unwrap();
            assert!(
                route_key(
                    key,
                    KeyOwner::Terminal,
                    &keyboard,
                    &map,
                    &mut repeat,
                    &clock
                )
                .unwrap()
                .is_some()
            );
        }
        assert!(ime.enter(seat, window));
        assert!(!ime.sync_enabled(seat, &keyboard, true).is_empty());
        assert!(ime.done(&old, &keyboard, 1).is_none());
        let fresh = ime.capture_focus(seat, window, &keyboard);
        ime.stage(&fresh, &keyboard, ImeEvent::Commit { text: Some("新") })
            .unwrap();
        assert_eq!(ime.done(&fresh, &keyboard, 1).unwrap().commit, "新");
    }

    #[test]
    fn stale_ime_transaction_is_ignored() {
        struct Sink(TabId, Vec<u8>);
        impl PasteSink for Sink {
            type Error = std::convert::Infallible;
            fn tab(&self) -> TabId {
                self.0
            }
            fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
                self.1.extend_from_slice(bytes);
                Ok(())
            }
        }
        let seat = SeatId(1);
        let window = WindowId(1);
        let caps = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(window, TabId(7));
        let mut ime = ImeState::new(seat, &caps);
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        let old = ime.capture_focus(seat, window, &keyboard);
        ime.stage(&old, &keyboard, ImeEvent::Commit { text: Some("舊") })
            .unwrap();
        let mut pending = ime.done(&old, &keyboard, 1).unwrap();
        ime.stage(
            &old,
            &keyboard,
            ImeEvent::Commit {
                text: Some("未完成"),
            },
        )
        .unwrap();
        keyboard.enter(window, TabId(8));
        assert!(ime.done(&old, &keyboard, 1).is_none());
        ime.sync_enabled(seat, &keyboard, true);
        let fresh = ime.capture_focus(seat, window, &keyboard);
        ime.stage(&fresh, &keyboard, ImeEvent::Commit { text: Some("新") })
            .unwrap();
        assert!(
            !ime.stage(
                &old,
                &keyboard,
                ImeEvent::Commit {
                    text: Some("延遲")
                }
            )
            .unwrap()
        );
        assert!(ime.done(&old, &keyboard, 1).is_none());
        let mut update = ime.done(&fresh, &keyboard, ime.commit_serial()).unwrap();
        let mut sink = Sink(TabId(8), Vec::new());
        assert!(update.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
        assert_eq!(sink.1, "新".as_bytes());
        assert!(!pending.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
        assert!(pending.commit.is_empty());

        keyboard.enter(window, TabId(7));
        ime.sync_enabled(seat, &keyboard, true);
        assert!(
            !ime.stage(&old, &keyboard, ImeEvent::Commit { text: Some("ABA") })
                .unwrap()
        );
        let token = ime.capture_focus(seat, window, &keyboard);
        ime.stage(
            &token,
            &keyboard,
            ImeEvent::Commit {
                text: Some("pending"),
            },
        )
        .unwrap();
        let mut pending = ime.done(&token, &keyboard, ime.commit_serial()).unwrap();
        keyboard.leave(window);
        keyboard.enter(window, TabId(7));
        // Even before sync_enabled, returning to the same Tab cannot revive old input.
        assert!(!pending.is_current(&ime, &keyboard));
        assert!(ime.done(&token, &keyboard, ime.commit_serial()).is_none());
        assert!(!ime.sync_enabled(seat, &keyboard, true).is_empty());
        sink.0 = TabId(7);
        assert!(!pending.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
        let token = ime.capture_focus(seat, window, &keyboard);
        ime.sync_enabled(seat, &keyboard, false);
        ime.sync_enabled(seat, &keyboard, true);
        assert!(
            !ime.stage(
                &token,
                &keyboard,
                ImeEvent::Commit {
                    text: Some("disabled")
                }
            )
            .unwrap()
        );
        let token = ime.capture_focus(seat, window, &keyboard);
        ime.leave(seat, window);
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        assert!(ime.done(&token, &keyboard, ime.commit_serial()).is_none());
    }
}
