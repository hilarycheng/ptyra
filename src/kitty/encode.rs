use super::KeyboardFlags;
use crate::{
    error::{AppError, Component},
    keymap::{KeyState, TranslatedKey},
};
use kbvm::{Keysym, ModifierMask};

pub struct KeyEncoder {
    pub flags: KeyboardFlags,
    pub application_cursor: bool,
    pub application_keypad: bool,
}

impl KeyEncoder {
    pub fn route(
        &self,
        key: TranslatedKey,
        owner: crate::ime::key::KeyOwner,
        focus: &crate::input::KeyboardFocus,
        map: &crate::keymap::Keymap,
        repeat: &mut crate::repeat::RepeatScheduler,
        clock: &impl crate::clock::Clock,
    ) -> Result<Option<Vec<u8>>, AppError> {
        let Some(key) = crate::ime::key::route_key(key, owner, focus, map, repeat, clock)? else {
            return Ok(None);
        };
        let bytes = encode_key(
            &key,
            self.flags,
            self.application_cursor,
            self.application_keypad,
        )?;
        Ok((!bytes.is_empty()).then_some(bytes))
    }
}

/// Encode only keys returned by the ownership gate; IME commits use their separate UTF-8 path.
/// Unsupported enhancements or XKB multi-symbol keys fail explicitly, without partial output.
pub fn encode_key(
    key: &TranslatedKey,
    flags: KeyboardFlags,
    application_cursor: bool,
    application_keypad: bool,
) -> Result<Vec<u8>, AppError> {
    let unsupported = || AppError::new(Component::Terminal, std::io::ErrorKind::Unsupported);
    if !(flags - (KeyboardFlags::DISAMBIGUATE_ESC_CODES | KeyboardFlags::REPORT_EVENT_TYPES))
        .is_empty()
    {
        return Err(unsupported());
    }
    let report_events = flags.contains(KeyboardFlags::REPORT_EVENT_TYPES);
    if key.state == KeyState::Released && !report_events || key.keysyms.is_empty() {
        return Ok(Vec::new());
    }
    let event = match (report_events, key.state) {
        (true, KeyState::Repeated) => Some(2),
        (true, KeyState::Released) => Some(3),
        _ => None,
    };
    let ([symbol], [base]) = (key.keysyms.as_slice(), key.unshifted_keysyms.as_slice()) else {
        return Err(unsupported());
    };
    let modifiers = key.modifiers;
    let super_key = modifiers.effective() & ModifierMask::SUPER.0 != 0;
    if flags.is_empty() && super_key {
        return Err(unsupported());
    }
    let (mut symbol, mut base) = (*symbol, *base);
    if flags.is_empty()
        && let Some((normal, suffix)) = keypad(symbol)
    {
        // NumLock comes from the actual XKB mapping, never an assumed Mod2 bit.
        // Shift/Ctrl keypad combinations use the normal modified-navigation path.
        if application_keypad
            && !key.num_lock
            && !modifiers.shift()
            && !modifiers.ctrl()
            && !(0xff91..=0xff94).contains(&symbol)
        {
            let mut bytes = vec![0x1b, b'O', suffix];
            if modifiers.alt() {
                bytes.insert(0, 0x1b);
            }
            return Ok(bytes);
        }
        symbol = normal;
        base = if symbol < 128 {
            symbol
        } else {
            keypad(base).map_or(base, |mapping| mapping.0)
        };
    }
    let mods = 1
        + u8::from(modifiers.shift())
        + 2 * u8::from(modifiers.alt())
        + 4 * u8::from(modifiers.ctrl())
        + 8 * u8::from(super_key);
    let enhanced = flags.contains(KeyboardFlags::DISAMBIGUATE_ESC_CODES);
    let control = match symbol {
        0xff0d => Some(13),
        0xff09 | 0xfe20 => Some(9),
        0xff08 => Some(127),
        0xff1b => Some(27),
        _ => None,
    };
    if let Some(code) = control {
        if key.state == KeyState::Released && code != 27 {
            return Ok(Vec::new());
        }
        if enhanced && (code == 27 || mods != 1) || super_key || code == 27 && event.is_some() {
            return Ok(csi(code, mods, event, 'u'));
        }
        let mut bytes = if code == 9 && modifiers.shift() {
            b"\x1b[Z".to_vec()
        } else {
            vec![if code == 127 && modifiers.ctrl() {
                8
            } else {
                code as u8
            }]
        };
        if modifiers.alt() {
            bytes.insert(0, 0x1b);
        }
        return Ok(bytes);
    }
    if let Some((number, suffix, ss3)) = functional(symbol, enhanced || report_events) {
        return Ok(
            if !enhanced
                && event.is_none()
                && mods == 1
                && ss3
                && (application_cursor || (0xffbe..=0xffc1).contains(&symbol))
            {
                format!("\x1bO{suffix}").into_bytes()
            } else {
                csi(number, mods, event, suffix)
            },
        );
    }
    // Modifier-only events have no legacy byte representation.
    if (0xffe1..=0xffee).contains(&symbol) || matches!(symbol, 0xff7f | 0xff14) {
        return Ok(Vec::new());
    }
    let text = Keysym(symbol)
        .char()
        .filter(|ch| !ch.is_control())
        .ok_or_else(unsupported)?;
    let base = Keysym(base)
        .char()
        .filter(|ch| !ch.is_control())
        .ok_or_else(unsupported)?;
    if super_key
        || !flags.is_empty() && modifiers.ctrl() && modifiers.shift()
        || enhanced && (modifiers.ctrl() || modifiers.alt())
    {
        return Ok(csi(base as u32, mods, event, 'u'));
    }
    if key.state == KeyState::Released {
        return Ok(Vec::new());
    }
    let text = if modifiers.ctrl() {
        match base {
            'a'..='z' => char::from(base as u8 - b'a' + 1),
            ' ' | '@' | '2' => '\0',
            '[' | '3' => '\x1b',
            '\\' | '4' => '\x1c',
            ']' | '5' => '\x1d',
            '^' | '6' => '\x1e',
            '_' | '7' | '/' => '\x1f',
            '8' | '?' => '\x7f',
            _ => text,
        }
    } else {
        text
    };
    let mut bytes = text.to_string().into_bytes();
    if modifiers.alt() {
        bytes.insert(0, 0x1b);
    }
    Ok(bytes)
}

// VT100/VT220 keypad SS3 suffixes; navigation aliases retain the existing cursor mode.
fn keypad(symbol: u32) -> Option<(u32, u8)> {
    Some(match symbol {
        0xff80 => (b' ' as u32, b' '),
        0xff89 => (0xff09, b'I'),
        0xff8d => (0xff0d, b'M'),
        0xff91..=0xff94 => (0xffbe + symbol - 0xff91, b'P' + (symbol - 0xff91) as u8),
        0xff95 => (0xff50, b'w'),
        0xff96 => (0xff51, b't'),
        0xff97 => (0xff52, b'x'),
        0xff98 => (0xff53, b'v'),
        0xff99 => (0xff54, b'r'),
        0xff9a => (0xff55, b'y'),
        0xff9b => (0xff56, b's'),
        0xff9c => (0xff57, b'q'),
        0xff9d => (0xff58, b'u'),
        0xff9e => (0xff63, b'p'),
        0xff9f => (0xffff, b'n'),
        0xffaa..=0xffaf => (
            b"*+,-./"[(symbol - 0xffaa) as usize] as u32,
            b'j' + (symbol - 0xffaa) as u8,
        ),
        0xffb0..=0xffb9 => (
            b'0' as u32 + symbol - 0xffb0,
            b'p' + (symbol - 0xffb0) as u8,
        ),
        0xffbd => (b'=' as u32, b'X'),
        _ => return None,
    })
}

fn csi(number: u32, mods: u8, event: Option<u8>, suffix: char) -> Vec<u8> {
    if let Some(event) = event {
        return format!("\x1b[{number};{mods}:{event}{suffix}").into_bytes();
    }
    if mods != 1 {
        return format!("\x1b[{number};{mods}{suffix}").into_bytes();
    }
    if matches!(suffix, 'u' | '~') {
        format!("\x1b[{number}{suffix}").into_bytes()
    } else {
        format!("\x1b[{suffix}").into_bytes()
    }
}

fn functional(symbol: u32, enhanced: bool) -> Option<(u32, char, bool)> {
    Some(match symbol {
        0xff52 => (1, 'A', true),
        0xff54 => (1, 'B', true),
        0xff53 => (1, 'C', true),
        0xff51 => (1, 'D', true),
        0xff50 => (1, 'H', true),
        0xff57 => (1, 'F', true),
        0xff58 => (1, 'E', false),
        0xff63 => (2, '~', false),
        0xffff => (3, '~', false),
        0xff55 => (5, '~', false),
        0xff56 => (6, '~', false),
        0xffc0 if enhanced => (13, '~', false),
        0xffbe..=0xffc1 => (1, char::from(b'P' + (symbol - 0xffbe) as u8), true),
        0xffc2..=0xffc9 => (
            [15, 17, 18, 19, 20, 21, 23, 24][(symbol - 0xffc2) as usize],
            '~',
            false,
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        input::KeyboardFocus,
        keymap::{Keymap, Modifiers},
    };
    use kbvm::ModifierMask;

    #[test]
    fn legacy_keypad_aliases_and_modifier_bytes_are_explicit() {
        let map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap();
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        let mut key = map
            .translate(focus.route_key(30, true).unwrap(), false)
            .unwrap()
            .unwrap();
        for (symbol, normal, application) in [
            (0xff80, " ", "\x1bO "),
            (0xff89, "\t", "\x1bOI"),
            (0xff91, "\x1bOP", "\x1bOP"),
            (0xff92, "\x1bOQ", "\x1bOQ"),
            (0xff93, "\x1bOR", "\x1bOR"),
            (0xff94, "\x1bOS", "\x1bOS"),
        ] {
            key.keysyms = vec![symbol];
            key.unshifted_keysyms = vec![symbol];
            assert_eq!(
                encode_key(&key, KeyboardFlags::empty(), false, false).unwrap(),
                normal.as_bytes()
            );
            assert_eq!(
                encode_key(&key, KeyboardFlags::empty(), false, true).unwrap(),
                application.as_bytes()
            );
        }
        for (symbol, number, suffix) in [
            (0xff50, 1, 'H'),
            (0xff57, 1, 'F'),
            (0xff52, 1, 'A'),
            (0xff54, 1, 'B'),
            (0xff53, 1, 'C'),
            (0xff51, 1, 'D'),
            (0xff63, 2, '~'),
            (0xffff, 3, '~'),
            (0xff55, 5, '~'),
            (0xff56, 6, '~'),
        ] {
            key.keysyms = vec![symbol];
            key.unshifted_keysyms = vec![symbol];
            for mask in [1, 4, 5, 8, 9, 12, 13] {
                key.modifiers = Modifiers {
                    depressed: mask,
                    ..Default::default()
                };
                let modifier = 1
                    + u32::from(mask & 1 != 0)
                    + 2 * u32::from(mask & 8 != 0)
                    + 4 * u32::from(mask & 4 != 0);
                assert_eq!(
                    encode_key(&key, KeyboardFlags::empty(), true, true).unwrap(),
                    format!("\x1b[{number};{modifier}{suffix}").as_bytes()
                );
            }
        }
        key.modifiers = Modifiers::default();
        key.keysyms = vec![0x1008ff12];
        key.unshifted_keysyms = key.keysyms.clone();
        assert_eq!(
            encode_key(&key, KeyboardFlags::empty(), false, false)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        key.keysyms = vec![b'a' as u32, b'b' as u32];
        key.unshifted_keysyms = key.keysyms.clone();
        assert!(encode_key(&key, KeyboardFlags::empty(), false, false).is_err());
    }
    #[test]
    fn kitty_encoder_skips_ime_consumed_text() {
        use crate::{clock::FakeClock, ime::key::KeyOwner, repeat::RepeatScheduler};
        let map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/greek.xkb")).unwrap();
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        let clock = FakeClock::default();
        let mut repeat = RepeatScheduler::default();
        let encoder = KeyEncoder {
            flags: KeyboardFlags::DISAMBIGUATE_ESC_CODES | KeyboardFlags::REPORT_EVENT_TYPES,
            application_cursor: false,
            application_keypad: false,
        };
        for (pressed, repeated) in [(true, false), (true, true), (false, false)] {
            let key = map
                .translate(focus.route_key(16, pressed).unwrap(), repeated)
                .unwrap()
                .unwrap();
            assert!(
                encoder
                    .route(key, KeyOwner::Ime, &focus, &map, &mut repeat, &clock)
                    .unwrap()
                    .is_none()
            );
        }
        use crate::{
            clipboard::{PasteSink, SeatId},
            ime::{ImeEvent, ImeState},
            wayland::{Capabilities, Global},
        };
        struct Sink(Vec<u8>);
        impl PasteSink for Sink {
            type Error = std::convert::Infallible;
            fn tab(&self) -> TabId {
                TabId(1)
            }
            fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
        }
        let caps = Capabilities::from_globals(&[Global {
            name: 1,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut ime = ImeState::new(SeatId(1), &caps);
        ime.enter(SeatId(1), WindowId(1));
        ime.sync_enabled(SeatId(1), &focus, true);
        let epoch = ime.capture_focus(SeatId(1), WindowId(1), &focus);
        ime.stage(&epoch, &focus, ImeEvent::Commit { text: Some("中") })
            .unwrap();
        let mut update = ime.done(&epoch, &focus, ime.commit_serial()).unwrap();
        let mut sink = Sink(Vec::new());
        assert!(update.enqueue_commit(&ime, &focus, &mut sink).unwrap());
        assert!(!update.enqueue_commit(&ime, &focus, &mut sink).unwrap());
        assert_eq!(sink.0, "中".as_bytes());
        let key = map
            .translate(focus.route_key(16, true).unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            encoder
                .route(key, KeyOwner::Terminal, &focus, &map, &mut repeat, &clock)
                .unwrap()
                .unwrap(),
            "中".as_bytes()
        );
        let key = map
            .translate(focus.route_key(16, true).unwrap(), false)
            .unwrap()
            .unwrap();
        focus.enter(WindowId(1), TabId(2));
        assert!(
            encoder
                .route(key, KeyOwner::Terminal, &focus, &map, &mut repeat, &clock)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn kitty_key_event_types_are_distinct() {
        let map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap();
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        let mut key = map
            .translate(focus.route_key(103, true).unwrap(), false)
            .unwrap()
            .unwrap();
        let flags = KeyboardFlags::DISAMBIGUATE_ESC_CODES | KeyboardFlags::REPORT_EVENT_TYPES;
        for (state, expected) in [
            (KeyState::Pressed, "\x1b[A"),
            (KeyState::Repeated, "\x1b[1;1:2A"),
            (KeyState::Released, "\x1b[1;1:3A"),
        ] {
            key.state = state;
            assert_eq!(
                encode_key(&key, flags, true, false).unwrap(),
                expected.as_bytes()
            );
        }
        for symbol in [0xff0d, 0xff09, 0xff08, u32::from(b'a')] {
            key.keysyms = vec![symbol];
            key.unshifted_keysyms = vec![symbol];
            assert!(encode_key(&key, flags, false, false).unwrap().is_empty());
        }
        key.keysyms = vec![0xff1b];
        key.unshifted_keysyms = vec![0xff1b];
        assert_eq!(
            encode_key(&key, flags, false, false).unwrap(),
            b"\x1b[27;1:3u"
        );
        assert!(
            encode_key(&key, KeyboardFlags::DISAMBIGUATE_ESC_CODES, false, false)
                .unwrap()
                .is_empty()
        );
        key.state = KeyState::Repeated;
        assert_eq!(
            encode_key(&key, flags, false, false).unwrap(),
            b"\x1b[27;1:2u"
        );
        assert_eq!(
            encode_key(&key, KeyboardFlags::empty(), false, false).unwrap(),
            b"\x1b"
        );
        key.keysyms = vec![u32::from(b'a')];
        key.unshifted_keysyms = key.keysyms.clone();
        assert_eq!(encode_key(&key, flags, false, false).unwrap(), b"a");
    }

    #[test]
    fn kitty_encoder_handles_modified_enter() {
        let mut map =
            Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap();
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        for (code, mask, flags, expected) in [
            (28, 0, 0, "\r"),
            (28, ModifierMask::SHIFT.0, 0, "\r"),
            (28, ModifierMask::SHIFT.0, 1, "\x1b[13;2u"),
            (28, ModifierMask::CONTROL.0, 1, "\x1b[13;5u"),
            (30, 0, 0, "a"),
            (30, ModifierMask::SHIFT.0, 1, "A"),
            (30, ModifierMask::CONTROL.0, 0, "\x01"),
            (
                30,
                ModifierMask::CONTROL.0 | ModifierMask::SHIFT.0,
                1,
                "\x1b[97;6u",
            ),
            (
                5,
                ModifierMask::SHIFT.0 | ModifierMask::ALT.0,
                1,
                "\x1b[52;4u",
            ),
            (103, 0, 0, "\x1b[A"),
            (103, ModifierMask::CONTROL.0, 0, "\x1b[1;5A"),
        ] {
            map.update_modifiers(Modifiers {
                depressed: mask,
                ..Modifiers::default()
            });
            let key = map
                .translate(focus.route_key(code, true).unwrap(), false)
                .unwrap()
                .unwrap();
            assert_eq!(
                encode_key(&key, KeyboardFlags::from_bits(flags).unwrap(), false, false).unwrap(),
                expected.as_bytes()
            );
        }
        map.update_modifiers(Modifiers::default());
        let key = map
            .translate(focus.route_key(103, true).unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            encode_key(&key, KeyboardFlags::empty(), true, false).unwrap(),
            b"\x1bOA"
        );
        assert!(encode_key(&key, KeyboardFlags::REPORT_ASSOCIATED_TEXT, false, false).is_err());
        let mut key = key;
        for (symbol, flags, expected) in [
            (0xff1b, 0, "\x1b"),
            (0xff1b, 1, "\x1b[27u"),
            (0xffbe, 0, "\x1bOP"),
            (0xffbe, 1, "\x1b[P"),
            (0xffc0, 1, "\x1b[13~"),
            (0xffff, 1, "\x1b[3~"),
            (kbvm::Keysym::from_char('中').0, 1, "中"),
        ] {
            key.keysyms = vec![symbol];
            key.unshifted_keysyms = vec![symbol];
            assert_eq!(
                encode_key(&key, KeyboardFlags::from_bits(flags).unwrap(), false, false).unwrap(),
                expected.as_bytes()
            );
        }
    }
}
