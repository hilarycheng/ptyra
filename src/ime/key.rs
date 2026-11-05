//! Ownership gate before repeat scheduling and either legacy or Kitty keyboard encoding.
use crate::{
    clock::Clock,
    error::AppError,
    input::KeyboardFocus,
    keymap::{Keymap, TranslatedKey},
    repeat::RepeatScheduler,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyOwner {
    Ime,
    Terminal,
}

/// The adapter supplies ownership for each press/repeat/release, never inferred from Preedit or IME enabled state.
/// text-input-v3 has no per-key consumed event; compositor-forwarded wl_keyboard keys belong to Terminal.
/// Only returned keys reach an encoder. IME text uses the separate done/Commit path.
pub fn route_key(
    key: TranslatedKey,
    owner: KeyOwner,
    keyboard: &KeyboardFocus,
    keymap: &Keymap,
    repeat: &mut RepeatScheduler,
    clock: &impl Clock,
) -> Result<Option<TranslatedKey>, AppError> {
    if !keyboard.matches(key.target, &key.focus_epoch) || !keymap.matches(&key.keymap_epoch) {
        return Ok(None);
    }
    if owner == KeyOwner::Ime {
        repeat.cancel();
        return Ok(None);
    }
    repeat.on_key(&key, clock)?;
    Ok(Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        clipboard::{PasteSink, SeatId},
        clock::FakeClock,
        ime::{ImeEvent, ImeState},
        wayland::{Capabilities, Global},
    };
    use std::time::Duration;

    #[test]
    fn ime_toggle_key_is_not_swallowed() {
        use crate::{
            clipboard::{PasteKeyAction, copy_shortcut, paste_shortcut},
            keymap::Modifiers,
            terminal::Terminal,
        };
        let map_text = br#"xkb_keymap {
            xkb_keycodes { <SPCE> = 65; <LFSH> = 50; };
            xkb_types { type "ONE_LEVEL" { modifiers = None; map[None] = Level1; }; };
            xkb_compatibility {};
            xkb_symbols { key <SPCE> { [ space ] }; key <LFSH> { repeat = false, [ Shift_L ] }; };
        };"#;
        let mut map = Keymap::from_bytes(map_text).unwrap();
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(7));
        let terminal = Terminal::new(TabId(7), 2, 8).unwrap();
        let clock = FakeClock::default();
        let mut repeat = RepeatScheduler::default();
        for (code, modifiers) in [
            (
                57,
                Modifiers {
                    depressed: kbvm::ModifierMask::CONTROL.0,
                    ..Modifiers::default()
                },
            ),
            (42, Modifiers::default()),
        ] {
            map.update_modifiers(modifiers);
            for pressed in [true, false] {
                let raw = focus.route_key(code, pressed).unwrap();
                let key = map
                    .translate(focus.route_key(code, pressed).unwrap(), false)
                    .unwrap()
                    .unwrap();
                let keysym = key.keysyms[0];
                assert!(matches!(
                    paste_shortcut(&focus, &raw, modifiers, keysym, key.state),
                    PasteKeyAction::Shell
                ));
                assert!(
                    copy_shortcut(
                        key.target,
                        modifiers,
                        keysym,
                        key.state,
                        None,
                        terminal.grid().unwrap()
                    )
                    .is_none()
                );
                assert!(
                    route_key(key, KeyOwner::Terminal, &focus, &map, &mut repeat, &clock)
                        .unwrap()
                        .is_some()
                );
            }
        }
    }

    #[test]
    fn ime_key_is_not_sent_twice() {
        let clock = FakeClock::default();
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(WindowId(1), TabId(7));
        let map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/greek.xkb")).unwrap();
        let mut repeat = RepeatScheduler::default();
        repeat.configure(25, 500).unwrap();
        let key = |pressed, repeated| {
            map.translate(keyboard.route_key(30, pressed).unwrap(), repeated)
                .unwrap()
                .unwrap()
        };
        let normal = route_key(
            key(true, false),
            KeyOwner::Terminal,
            &keyboard,
            &map,
            &mut repeat,
            &clock,
        )
        .unwrap()
        .unwrap();
        assert_eq!(normal.keysyms, [0x07e1]);
        assert!(repeat.deadline(&keyboard, &map).is_some());
        for (pressed, repeated) in [(true, false), (true, true), (false, false)] {
            assert!(
                route_key(
                    key(pressed, repeated),
                    KeyOwner::Ime,
                    &keyboard,
                    &map,
                    &mut repeat,
                    &clock
                )
                .unwrap()
                .is_none()
            );
            assert!(repeat.deadline(&keyboard, &map).is_none());
        }
        clock.advance(Duration::from_secs(1)).unwrap();
        assert!(repeat.poll(&clock, &keyboard, &map).unwrap().is_none());

        struct Sink(Vec<u8>);
        impl PasteSink for Sink {
            type Error = std::convert::Infallible;
            fn tab(&self) -> TabId {
                TabId(7)
            }
            fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
        }
        let caps = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut ime = ImeState::new(SeatId(1), &caps);
        ime.enter(SeatId(1), WindowId(1));
        ime.sync_enabled(SeatId(1), &keyboard, true);
        let focus = ime.capture_focus(SeatId(1), WindowId(1), &keyboard);
        ime.stage(&focus, &keyboard, ImeEvent::Commit { text: Some("中") })
            .unwrap();
        let mut sink = Sink(Vec::new());
        let mut update = ime.done(&focus, &keyboard, ime.commit_serial()).unwrap();
        assert!(update.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
        assert!(!update.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
        assert_eq!(sink.0, "中".as_bytes());

        // IME enabled/Commit activity does not consume forwarded keyboard events.
        assert!(
            route_key(
                key(true, false),
                KeyOwner::Terminal,
                &keyboard,
                &map,
                &mut repeat,
                &clock
            )
            .unwrap()
            .is_some()
        );
        clock.advance(Duration::from_millis(500)).unwrap();
        let repeated = repeat.poll(&clock, &keyboard, &map).unwrap().unwrap();
        assert!(
            route_key(
                repeated,
                KeyOwner::Terminal,
                &keyboard,
                &map,
                &mut repeat,
                &clock
            )
            .unwrap()
            .is_some()
        );
        let release = route_key(
            key(false, false),
            KeyOwner::Terminal,
            &keyboard,
            &map,
            &mut repeat,
            &clock,
        )
        .unwrap()
        .unwrap();
        assert_eq!(release.state, crate::keymap::KeyState::Released);
        assert!(repeat.deadline(&keyboard, &map).is_none());
        let stale = key(true, false);
        keyboard.enter(WindowId(1), TabId(8));
        assert!(
            route_key(
                stale,
                KeyOwner::Terminal,
                &keyboard,
                &map,
                &mut repeat,
                &clock
            )
            .unwrap()
            .is_none()
        );
        let stale = map
            .translate(keyboard.route_key(30, true).unwrap(), false)
            .unwrap()
            .unwrap();
        let replacement =
            Keymap::from_bytes(include_bytes!("../../tests/fixtures/greek.xkb")).unwrap();
        assert!(
            route_key(
                stale,
                KeyOwner::Terminal,
                &keyboard,
                &replacement,
                &mut repeat,
                &clock
            )
            .unwrap()
            .is_none()
        );
    }
}
