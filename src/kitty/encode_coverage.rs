use super::*;
use crate::{
    TabId, WindowId,
    input::KeyboardFocus,
    keymap::{Keymap, Modifiers},
};

const EVENT_PRIMITIVE_FLAGS: KeyboardFlags =
    READY_KEYBOARD_FLAGS.union(KeyboardFlags::REPORT_EVENT_TYPES);

fn key() -> TranslatedKey {
    let map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap();
    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(1));
    map.translate(focus.route_key(30, true).unwrap(), false)
        .unwrap()
        .unwrap()
}

// Literal protocol bytes, not expected values obtained from the encoder's mapping.
#[test]
fn kitty_supported_flags_match_complete_encoder_coverage() {
    let mut key = key();
    assert_eq!(READY_KEYBOARD_FLAGS.bits(), 1);
    complete_function_table();
    let both = EVENT_PRIMITIVE_FLAGS;
    for (symbol, press, repeat, release) in [
        (0xff95, "\x1b[57423u", "\x1b[57423;1:2u", "\x1b[57423;1:3u"),
        (0xffca, "\x1b[57376u", "\x1b[57376;1:2u", "\x1b[57376;1:3u"),
        (0xff1b, "\x1b[27u", "\x1b[27;1:2u", "\x1b[27;1:3u"),
        (0xff52, "\x1b[A", "\x1b[1;1:2A", "\x1b[1;1:3A"),
        (0xffc0, "\x1b[13~", "\x1b[13;1:2~", "\x1b[13;1:3~"),
        (0xff8d, "\x1b[57414u", "\x1b[57414;1:2u", "\x1b[57414;1:3u"),
        (0xff0d, "\r", "\r", ""),
        (0xff09, "\t", "\t", ""),
        (0xff08, "\x7f", "\x7f", ""),
        (0xffb1, "1", "1", ""),
        (0x61, "a", "a", ""),
        (0x01004e2d, "中", "中", ""),
    ] {
        key.keysyms = vec![symbol];
        key.unshifted_keysyms = vec![symbol];
        for (state, expected) in [
            (KeyState::Pressed, press),
            (KeyState::Repeated, repeat),
            (KeyState::Released, release),
        ] {
            key.state = state;
            for cursor in [false, true] {
                for keypad in [false, true] {
                    assert_eq!(
                        encode_key(&key, both, cursor, keypad).unwrap(),
                        expected.as_bytes(),
                        "keysym {symbol:x}"
                    );
                }
            }
        }
    }
    for (symbol, mask, press, repeat, release) in [
        (0xff0d, 1, "\x1b[13;2u", "\x1b[13;2:2u", "\x1b[13;2:3u"),
        (0xff09, 4, "\x1b[9;5u", "\x1b[9;5:2u", "\x1b[9;5:3u"),
        (0xff08, 8, "\x1b[127;3u", "\x1b[127;3:2u", "\x1b[127;3:3u"),
        (
            0xffb1,
            4,
            "\x1b[57400;5u",
            "\x1b[57400;5:2u",
            "\x1b[57400;5:3u",
        ),
        (0x61, 4, "\x1b[97;5u", "\x1b[97;5:2u", "\x1b[97;5:3u"),
    ] {
        key.keysyms = vec![symbol];
        key.unshifted_keysyms = vec![symbol];
        key.modifiers = Modifiers {
            depressed: mask,
            ..Default::default()
        };
        for (state, expected) in [
            (KeyState::Pressed, press),
            (KeyState::Repeated, repeat),
            (KeyState::Released, release),
        ] {
            key.state = state;
            assert_eq!(
                encode_key(&key, both, false, false).unwrap(),
                expected.as_bytes(),
                "keysym {symbol:x}"
            );
        }
    }
}

// Every non-text functional identity accepted by this XKB adapter, including all F1-F35.
fn complete_function_table() {
    let mut key = key();
    for (symbol, press, repeat, release) in [
        (0xff52, "\x1b[A", "\x1b[1;1:2A", "\x1b[1;1:3A"),
        (0xff54, "\x1b[B", "\x1b[1;1:2B", "\x1b[1;1:3B"),
        (0xff53, "\x1b[C", "\x1b[1;1:2C", "\x1b[1;1:3C"),
        (0xff51, "\x1b[D", "\x1b[1;1:2D", "\x1b[1;1:3D"),
        (0xff50, "\x1b[H", "\x1b[1;1:2H", "\x1b[1;1:3H"),
        (0xff57, "\x1b[F", "\x1b[1;1:2F", "\x1b[1;1:3F"),
        (0xff58, "\x1b[E", "\x1b[1;1:2E", "\x1b[1;1:3E"),
        (0xff63, "\x1b[2~", "\x1b[2;1:2~", "\x1b[2;1:3~"),
        (0xffff, "\x1b[3~", "\x1b[3;1:2~", "\x1b[3;1:3~"),
        (0xff55, "\x1b[5~", "\x1b[5;1:2~", "\x1b[5;1:3~"),
        (0xff56, "\x1b[6~", "\x1b[6;1:2~", "\x1b[6;1:3~"),
        (0xffbe, "\x1b[P", "\x1b[1;1:2P", "\x1b[1;1:3P"),
        (0xffbf, "\x1b[Q", "\x1b[1;1:2Q", "\x1b[1;1:3Q"),
        (0xffc0, "\x1b[13~", "\x1b[13;1:2~", "\x1b[13;1:3~"),
        (0xffc1, "\x1b[S", "\x1b[1;1:2S", "\x1b[1;1:3S"),
        (0xffc2, "\x1b[15~", "\x1b[15;1:2~", "\x1b[15;1:3~"),
        (0xffc3, "\x1b[17~", "\x1b[17;1:2~", "\x1b[17;1:3~"),
        (0xffc4, "\x1b[18~", "\x1b[18;1:2~", "\x1b[18;1:3~"),
        (0xffc5, "\x1b[19~", "\x1b[19;1:2~", "\x1b[19;1:3~"),
        (0xffc6, "\x1b[20~", "\x1b[20;1:2~", "\x1b[20;1:3~"),
        (0xffc7, "\x1b[21~", "\x1b[21;1:2~", "\x1b[21;1:3~"),
        (0xffc8, "\x1b[23~", "\x1b[23;1:2~", "\x1b[23;1:3~"),
        (0xffc9, "\x1b[24~", "\x1b[24;1:2~", "\x1b[24;1:3~"),
        (0xffca, "\x1b[57376u", "\x1b[57376;1:2u", "\x1b[57376;1:3u"),
        (0xffcb, "\x1b[57377u", "\x1b[57377;1:2u", "\x1b[57377;1:3u"),
        (0xffcc, "\x1b[57378u", "\x1b[57378;1:2u", "\x1b[57378;1:3u"),
        (0xffcd, "\x1b[57379u", "\x1b[57379;1:2u", "\x1b[57379;1:3u"),
        (0xffce, "\x1b[57380u", "\x1b[57380;1:2u", "\x1b[57380;1:3u"),
        (0xffcf, "\x1b[57381u", "\x1b[57381;1:2u", "\x1b[57381;1:3u"),
        (0xffd0, "\x1b[57382u", "\x1b[57382;1:2u", "\x1b[57382;1:3u"),
        (0xffd1, "\x1b[57383u", "\x1b[57383;1:2u", "\x1b[57383;1:3u"),
        (0xffd2, "\x1b[57384u", "\x1b[57384;1:2u", "\x1b[57384;1:3u"),
        (0xffd3, "\x1b[57385u", "\x1b[57385;1:2u", "\x1b[57385;1:3u"),
        (0xffd4, "\x1b[57386u", "\x1b[57386;1:2u", "\x1b[57386;1:3u"),
        (0xffd5, "\x1b[57387u", "\x1b[57387;1:2u", "\x1b[57387;1:3u"),
        (0xffd6, "\x1b[57388u", "\x1b[57388;1:2u", "\x1b[57388;1:3u"),
        (0xffd7, "\x1b[57389u", "\x1b[57389;1:2u", "\x1b[57389;1:3u"),
        (0xffd8, "\x1b[57390u", "\x1b[57390;1:2u", "\x1b[57390;1:3u"),
        (0xffd9, "\x1b[57391u", "\x1b[57391;1:2u", "\x1b[57391;1:3u"),
        (0xffda, "\x1b[57392u", "\x1b[57392;1:2u", "\x1b[57392;1:3u"),
        (0xffdb, "\x1b[57393u", "\x1b[57393;1:2u", "\x1b[57393;1:3u"),
        (0xffdc, "\x1b[57394u", "\x1b[57394;1:2u", "\x1b[57394;1:3u"),
        (0xffdd, "\x1b[57395u", "\x1b[57395;1:2u", "\x1b[57395;1:3u"),
        (0xffde, "\x1b[57396u", "\x1b[57396;1:2u", "\x1b[57396;1:3u"),
        (0xffdf, "\x1b[57397u", "\x1b[57397;1:2u", "\x1b[57397;1:3u"),
        (0xffe0, "\x1b[57398u", "\x1b[57398;1:2u", "\x1b[57398;1:3u"),
        (0xff61, "\x1b[57361u", "\x1b[57361;1:2u", "\x1b[57361;1:3u"),
        (0xff13, "\x1b[57362u", "\x1b[57362;1:2u", "\x1b[57362;1:3u"),
        (0xff6b, "\x1b[57362u", "\x1b[57362;1:2u", "\x1b[57362;1:3u"),
        (0xff67, "\x1b[57363u", "\x1b[57363;1:2u", "\x1b[57363;1:3u"),
        (
            0x1008ff14,
            "\x1b[57428u",
            "\x1b[57428;1:2u",
            "\x1b[57428;1:3u",
        ),
        (
            0x1008ff31,
            "\x1b[57429u",
            "\x1b[57429;1:2u",
            "\x1b[57429;1:3u",
        ),
        (
            0x1008ff15,
            "\x1b[57432u",
            "\x1b[57432;1:2u",
            "\x1b[57432;1:3u",
        ),
        (
            0x1008ff97,
            "\x1b[57433u",
            "\x1b[57433;1:2u",
            "\x1b[57433;1:3u",
        ),
        (
            0x1008ff3e,
            "\x1b[57434u",
            "\x1b[57434;1:2u",
            "\x1b[57434;1:3u",
        ),
        (
            0x1008ff17,
            "\x1b[57435u",
            "\x1b[57435;1:2u",
            "\x1b[57435;1:3u",
        ),
        (
            0x1008ff16,
            "\x1b[57436u",
            "\x1b[57436;1:2u",
            "\x1b[57436;1:3u",
        ),
        (
            0x1008ff1c,
            "\x1b[57437u",
            "\x1b[57437;1:2u",
            "\x1b[57437;1:3u",
        ),
        (
            0x1008ff11,
            "\x1b[57438u",
            "\x1b[57438;1:2u",
            "\x1b[57438;1:3u",
        ),
        (
            0x1008ff13,
            "\x1b[57439u",
            "\x1b[57439;1:2u",
            "\x1b[57439;1:3u",
        ),
        (
            0x1008ff12,
            "\x1b[57440u",
            "\x1b[57440;1:2u",
            "\x1b[57440;1:3u",
        ),
        (0xff91, "\x1b[P", "\x1b[1;1:2P", "\x1b[1;1:3P"),
        (0xff92, "\x1b[Q", "\x1b[1;1:2Q", "\x1b[1;1:3Q"),
        (0xff93, "\x1b[13~", "\x1b[13;1:2~", "\x1b[13;1:3~"),
        (0xff94, "\x1b[S", "\x1b[1;1:2S", "\x1b[1;1:3S"),
    ] {
        key.keysyms = vec![symbol];
        key.unshifted_keysyms = vec![symbol];
        for bits in [1, 2, 3] {
            let flags = KeyboardFlags::from_bits(bits).unwrap();
            for (state, expected) in [
                (KeyState::Pressed, press),
                (
                    KeyState::Repeated,
                    if bits & 2 == 0 { press } else { repeat },
                ),
                (KeyState::Released, if bits & 2 == 0 { "" } else { release }),
            ] {
                key.state = state;
                for cursor in [false, true] {
                    for keypad in [false, true] {
                        assert_eq!(
                            encode_key(&key, flags, cursor, keypad).unwrap(),
                            expected.as_bytes(),
                            "symbol={symbol:x} flags={bits} state={state:?}"
                        );
                    }
                }
            }
            key.modifiers.depressed = ModifierMask::CONTROL.0 | ModifierMask::SHIFT.0;
            key.state = KeyState::Pressed;
            // The literal event fixture supplies the identity; never consult functional()/csi().
            let expected = repeat.replace(";1:2", ";6");
            assert_eq!(
                encode_key(&key, flags, true, true).unwrap(),
                expected.as_bytes()
            );
            key.modifiers = Modifiers::default();
        }
    }
}

#[test]
fn kitty_keypad_has_text_and_distinct_non_text_fixtures() {
    let mut key = key();
    for (symbol, text, enhanced, event_only) in [
        (0xffb0, "0", "57399u", "48u"),
        (0xffb1, "1", "57400u", "49u"),
        (0xffb2, "2", "57401u", "50u"),
        (0xffb3, "3", "57402u", "51u"),
        (0xffb4, "4", "57403u", "52u"),
        (0xffb5, "5", "57404u", "53u"),
        (0xffb6, "6", "57405u", "54u"),
        (0xffb7, "7", "57406u", "55u"),
        (0xffb8, "8", "57407u", "56u"),
        (0xffb9, "9", "57408u", "57u"),
        (0xffae, ".", "57409u", "46u"),
        (0xffaf, "/", "57410u", "47u"),
        (0xffaa, "*", "57411u", "42u"),
        (0xffad, "-", "57412u", "45u"),
        (0xffab, "+", "57413u", "43u"),
        (0xffbd, "=", "57415u", "61u"),
        (0xffac, ",", "57416u", "44u"),
        (0xff80, " ", "32u", "32u"),
    ] {
        key.keysyms = vec![symbol];
        key.unshifted_keysyms = vec![symbol];
        for bits in [1, 2, 3] {
            for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
                key.state = state;
                let flags = KeyboardFlags::from_bits(bits).unwrap();
                key.modifiers = Modifiers::default();
                assert_eq!(
                    encode_key(&key, flags, true, true).unwrap(),
                    if state == KeyState::Released {
                        b""
                    } else {
                        text.as_bytes()
                    }
                );
                key.modifiers.depressed = ModifierMask::CONTROL.0 | ModifierMask::SHIFT.0;
                let identity = if bits & 1 == 0 { event_only } else { enhanced };
                let (number, suffix) = identity.split_at(identity.len() - 1);
                let expected = match (bits & 2 != 0, state) {
                    (false, KeyState::Released) => String::new(),
                    (true, KeyState::Repeated) => format!("\x1b[{number};6:2{suffix}"),
                    (true, KeyState::Released) => format!("\x1b[{number};6:3{suffix}"),
                    _ => format!("\x1b[{number};6{suffix}"),
                };
                assert_eq!(
                    encode_key(&key, flags, true, true).unwrap(),
                    expected.as_bytes(),
                    "keypad={symbol:x} flags={bits}"
                );
            }
        }
    }
    for (symbol, enhanced, event_only) in [
        (0xff96, "57417u", "1D"),
        (0xff98, "57418u", "1C"),
        (0xff97, "57419u", "1A"),
        (0xff99, "57420u", "1B"),
        (0xff9a, "57421u", "5~"),
        (0xff9b, "57422u", "6~"),
        (0xff95, "57423u", "1H"),
        (0xff9c, "57424u", "1F"),
        (0xff9e, "57425u", "2~"),
        (0xff9f, "57426u", "3~"),
        (0xff9d, "1E", "1E"),
        (0xff8d, "57414u", "13u"),
        (0xff89, "9u", "9u"),
    ] {
        key.keysyms = vec![symbol];
        key.unshifted_keysyms = vec![symbol];
        for bits in [1, 2, 3] {
            for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
                key.state = state;
                let identity = if bits & 1 == 0 { event_only } else { enhanced };
                let (number, suffix) = identity.split_at(identity.len() - 1);
                let expected = match (bits & 2 != 0, state) {
                    (false, KeyState::Released) => String::new(),
                    (true, KeyState::Repeated) => format!("\x1b[{number};6:2{suffix}"),
                    (true, KeyState::Released) => format!("\x1b[{number};6:3{suffix}"),
                    _ => format!("\x1b[{number};6{suffix}"),
                };
                assert_eq!(
                    encode_key(&key, KeyboardFlags::from_bits(bits).unwrap(), true, true).unwrap(),
                    expected.as_bytes()
                );
            }
        }
    }
}

#[test]
fn kitty_modifier_bits_and_control_event_fixtures_are_complete() {
    let mut key = key();
    for bits in 1..=3 {
        for mask in 0u16..=255 {
            key.modifiers = Modifiers {
                depressed: (u32::from(mask & 1 != 0) * ModifierMask::SHIFT.0)
                    | (u32::from(mask & 2 != 0) * ModifierMask::ALT.0)
                    | (u32::from(mask & 4 != 0) * ModifierMask::CONTROL.0)
                    | (u32::from(mask & 8 != 0) * ModifierMask::SUPER.0),
                locked: u32::from(mask & 64 != 0) * ModifierMask::LOCK.0,
                ..Default::default()
            };
            key.hyper = mask & 16 != 0;
            key.meta = mask & 32 != 0;
            key.num_lock = mask & 128 != 0;
            for (symbol, number, suffix) in [
                (0xff0d, 13, 'u'),
                (0xff09, 9, 'u'),
                (0xfe20, 9, 'u'),
                (0xff08, 127, 'u'),
                (0xff1b, 27, 'u'),
                (0xff52, 1, 'A'),
            ] {
                key.keysyms = vec![symbol];
                key.unshifted_keysyms = vec![symbol];
                for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
                    key.state = state;
                    let reset_control = mask & 63 == 0 && [13, 9, 127].contains(&number);
                    let expected = if state == KeyState::Released
                        && (bits & 2 == 0 || reset_control)
                    {
                        String::new()
                    } else if reset_control
                        || (bits == 2 && number == 27 && state == KeyState::Pressed && mask == 0)
                    {
                        char::from_u32(number).unwrap().to_string()
                    } else if bits & 2 != 0 && state != KeyState::Pressed {
                        format!(
                            "\x1b[{number};{}:{}{suffix}",
                            mask + 1,
                            if state == KeyState::Repeated { 2 } else { 3 }
                        )
                    } else if mask != 0 {
                        format!("\x1b[{number};{}{suffix}", mask + 1)
                    } else if suffix == 'A' {
                        "\x1b[A".into()
                    } else {
                        format!("\x1b[{number}{suffix}")
                    };
                    assert_eq!(
                        encode_key(&key, KeyboardFlags::from_bits(bits).unwrap(), true, true)
                            .unwrap(),
                        expected.as_bytes(),
                        "symbol={symbol:x} flags={bits} mask={mask} state={state:?}"
                    );
                }
            }
        }
    }
    key = self::key();
    for (symbol, mask, press, repeat, release) in [
        (0x61, 4, "\x01", "\x1b[97;5:2u", "\x1b[97;5:3u"),
        (0x7e, 4, "\x1e", "\x1b[126;5:2u", "\x1b[126;5:3u"),
        (0x61, 8, "\x1ba", "\x1b[97;3:2u", "\x1b[97;3:3u"),
        (0x61, 12, "\x1b\x01", "\x1b[97;7:2u", "\x1b[97;7:3u"),
        (0x61, 6, "\x1b[97;69u", "\x1b[97;69:2u", "\x1b[97;69:3u"),
        (0x7e1, 4, "\x1b[945;5u", "\x1b[945;5:2u", "\x1b[945;5:3u"),
        (
            0x01004e2d,
            8,
            "\x1b[20013;3u",
            "\x1b[20013;3:2u",
            "\x1b[20013;3:3u",
        ),
    ] {
        key.keysyms = vec![symbol];
        key.unshifted_keysyms = vec![symbol];
        key.modifiers.depressed = mask;
        for (state, expected) in [
            (KeyState::Pressed, press),
            (KeyState::Repeated, repeat),
            (KeyState::Released, release),
        ] {
            key.state = state;
            assert_eq!(
                encode_key(&key, KeyboardFlags::REPORT_EVENT_TYPES, false, false).unwrap(),
                expected.as_bytes()
            );
        }
    }
}

#[test]
fn kitty_printable_identity_preserves_utf8_and_shift() {
    let mut key = key();
    for text in (32u8..=126).map(char::from).chain([
        'é',
        'α',
        '中',
        '\u{301}',
        '🦀',
        '\u{e000}',
        '\u{10ffff}',
    ]) {
        key.keysyms = vec![Keysym::from_char(text).0];
        key.unshifted_keysyms = key.keysyms.clone();
        for bits in 0..=3 {
            for lock in [false, true] {
                for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
                    key.modifiers.locked = if lock { ModifierMask::LOCK.0 } else { 0 };
                    key.num_lock = lock;
                    key.state = state;
                    assert_eq!(
                        encode_key(&key, KeyboardFlags::from_bits(bits).unwrap(), false, false)
                            .unwrap(),
                        if state == KeyState::Released {
                            Vec::new()
                        } else {
                            text.to_string().into_bytes()
                        }
                    );
                }
            }
        }
    }

    // Exhaust every printable ASCII identity through disambiguated Ctrl/Alt/Super combinations.
    key = self::key();
    for text in (32u8..=126).map(char::from) {
        key.keysyms = vec![Keysym::from_char(text).0];
        key.unshifted_keysyms = key.keysyms.clone();
        for (mask, mods) in [
            (4, 5),
            (8, 3),
            (12, 7),
            (5, 6),
            (9, 4),
            (13, 8),
            (64, 9),
            (77, 16),
        ] {
            key.modifiers.depressed = mask;
            for bits in [1, 3] {
                for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
                    key.state = state;
                    let expected = match (bits, state) {
                        (1, KeyState::Released) => String::new(),
                        (3, KeyState::Repeated) => format!("\x1b[{};{mods}:2u", text as u32),
                        (3, KeyState::Released) => format!("\x1b[{};{mods}:3u", text as u32),
                        _ => format!("\x1b[{};{mods}u", text as u32),
                    };
                    assert_eq!(
                        encode_key(&key, KeyboardFlags::from_bits(bits).unwrap(), false, false)
                            .unwrap(),
                        expected.as_bytes()
                    );
                }
            }
        }
    }
    let mut map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/greek.xkb")).unwrap();
    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(1));
    for (mask, expected) in [
        (0, "α"),
        (1, "Α"),
        (4, "\x1b[945;5u"),
        (5, "\x1b[945;6u"),
        (8, "\x1b[945;3u"),
    ] {
        map.update_modifiers(Modifiers {
            depressed: mask,
            ..Default::default()
        });
        let key = map
            .translate(focus.route_key(30, true).unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            encode_key(&key, EVENT_PRIMITIVE_FLAGS, false, false).unwrap(),
            expected.as_bytes()
        );
    }
    let mut map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap();
    for (mask, expected) in [(0, "4"), (1, "$"), (5, "\x1b[52;6u"), (9, "\x1b[52;4u")] {
        map.update_modifiers(Modifiers {
            depressed: mask,
            ..Default::default()
        });
        let key = map
            .translate(focus.route_key(5, true).unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            encode_key(&key, EVENT_PRIMITIVE_FLAGS, false, false).unwrap(),
            expected.as_bytes()
        );
    }
}

#[test]
fn kitty_modifier_masks_do_not_duplicate_xkb_aliases() {
    let fixture = include_str!("../../tests/fixtures/keyboard.xkb");
    for (virtuals, mask, expected, hyper, meta, num_lock) in [
        (
            "NumLock=Mod2, Hyper=Mod3, Meta=Mod5",
            0xb0,
            "\x1b[97;177u",
            true,
            true,
            true,
        ),
        (
            "NumLock=Mod2, Hyper=Mod4, Meta=Mod1",
            0x48,
            "\x1b[97;11u",
            false,
            false,
            false,
        ),
        (
            "NumLock=Mod3, Hyper=Mod3, Meta=Mod3",
            0x20,
            "a",
            false,
            false,
            true,
        ),
        (
            "NumLock=Mod2, Hyper=Mod3, Meta=Mod3",
            0x20,
            "\x1b[97;17u",
            true,
            false,
            false,
        ),
        (
            "NumLock=Mod3, Hyper=Shift, Meta=Control",
            4,
            "\x1b[97;5u",
            false,
            false,
            false,
        ),
    ] {
        let fixture = fixture.replace(
            "xkb_types \"test\" {",
            &format!("xkb_types \"test\" {{ virtual_modifiers {virtuals};"),
        );
        let mut map = Keymap::from_bytes(fixture.as_bytes()).unwrap();
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        for source in 0..3 {
            let mut modifiers = Modifiers::default();
            match source {
                0 => modifiers.depressed = mask,
                1 => modifiers.latched = mask,
                _ => modifiers.locked = mask,
            };
            map.update_modifiers(modifiers);
            let key = map
                .translate(focus.route_key(30, true).unwrap(), false)
                .unwrap()
                .unwrap();
            assert_eq!((key.hyper, key.meta, key.num_lock), (hyper, meta, num_lock));
            assert_eq!(
                encode_key(&key, EVENT_PRIMITIVE_FLAGS, false, false).unwrap(),
                expected.as_bytes()
            );
        }
    }
    // Real fixture intentionally puts NumLock on Mod3, not the conventional Mod2.
    let mut map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/keypad.xkb")).unwrap();
    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(1));
    for (mask, expected) in [
        (0, "\x1b[57423u"),
        (0x10, "\x1b[57423u"),
        (0x20, "7"),
        (0x25, "\x1b[57423;134u"),
    ] {
        map.update_modifiers(Modifiers {
            locked: mask,
            ..Default::default()
        });
        let key = map
            .translate(focus.route_key(71, true).unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            encode_key(&key, EVENT_PRIMITIVE_FLAGS, false, false).unwrap(),
            expected.as_bytes()
        );
    }
}

#[test]
fn kitty_unsupported_flags_and_keys_never_partially_encode_or_advertise() {
    let mut key = key();
    for bits in 4..=u8::MAX {
        for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
            key.state = state;
            assert_eq!(
                encode_key(&key, KeyboardFlags::from_bits_retain(bits), false, false)
                    .unwrap_err()
                    .kind(),
                std::io::ErrorKind::Unsupported
            );
        }
    }
    for bits in 0..=3 {
        let flags = KeyboardFlags::from_bits(bits).unwrap();
        for symbol in (0xffe1..=0xffee).chain([0xff7e, 0xff7f, 0xff14, 0xfe03, 0xfe11]) {
            key.keysyms = vec![symbol];
            key.unshifted_keysyms = vec![symbol];
            for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
                key.state = state;
                assert!(encode_key(&key, flags, false, false).unwrap().is_empty());
            }
        }
        key.state = KeyState::Pressed;
        for (symbols, base) in [
            (vec![97, 98], vec![97, 98]),
            (vec![97], vec![97, 98]),
            (vec![0x1008ff32], vec![0x1008ff32]),
            (vec![0x0100d800], vec![0x0100d800]),
        ] {
            key.keysyms = symbols;
            key.unshifted_keysyms = base;
            assert_eq!(
                encode_key(&key, flags, false, false).unwrap_err().kind(),
                std::io::ErrorKind::Unsupported
            );
        }
    }
    let mut terminal = crate::terminal::Terminal::new(TabId(1), 2, 8).unwrap();
    for bits in 0..=255 {
        terminal
            .feed(TabId(1), format!("\x1b[={bits}u\x1b[?u").as_bytes())
            .unwrap();
        assert_eq!(
            terminal.take_keyboard_reply().unwrap().bytes,
            format!("\x1b[?{}u", bits & 1).as_bytes()
        );
    }
}

#[test]
fn kitty_event_primitive_is_not_ready_without_press_ownership() {
    assert!(!READY_KEYBOARD_FLAGS.contains(KeyboardFlags::REPORT_EVENT_TYPES));
    let mut key = key();
    for (symbol, mask, press) in [(0x61, 4, "\x1b[97;5u"), (0xff0d, 1, "\x1b[13;2u")] {
        key.keysyms = vec![symbol];
        key.unshifted_keysyms = vec![symbol];
        key.modifiers.depressed = mask;
        key.state = KeyState::Pressed;
        assert_eq!(
            encode_key(&key, READY_KEYBOARD_FLAGS, false, false).unwrap(),
            press.as_bytes()
        );
        key.state = KeyState::Repeated;
        assert_eq!(
            encode_key(&key, READY_KEYBOARD_FLAGS, false, false).unwrap(),
            press.as_bytes()
        );
        // A modifier can be released first. The stateless T111 primitive then cannot
        // distinguish this key's encoded press from a plain-text/reset-control press.
        key.modifiers.depressed = 0;
        key.state = KeyState::Released;
        for flags in [READY_KEYBOARD_FLAGS, EVENT_PRIMITIVE_FLAGS] {
            assert!(encode_key(&key, flags, false, false).unwrap().is_empty());
        }
    }
}

#[test]
fn kitty_functional_identity_uses_unshifted_key_without_changing_text_or_keypad() {
    let fixture = include_str!("../../tests/fixtures/keyboard.xkb");
    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(1));
    for (levels, mask, expected) in [
        ("XF86AudioPlay, XF86AudioPause", 1, "\x1b[57428;2u"),
        ("F1, F13", 1, "\x1b[1;2P"),
        ("XF86AudioStop, XF86Eject", 1, "\x1b[57432;2u"),
        ("XF86Eject, a", 1, "a"),
        ("Return, Tab", 1, "\x1b[13;2u"),
        ("a, F1", 1, "\x1b[97;2u"),
        ("F1, a", 1, "a"),
        ("F1, a", 5, "\x1b[1;6P"),
    ] {
        let fixture = fixture.replace("[ a, A ]", &format!("[ {levels} ]"));
        let mut map = Keymap::from_bytes(fixture.as_bytes()).unwrap();
        map.update_modifiers(Modifiers {
            depressed: mask,
            ..Default::default()
        });
        for (pressed, repeat) in [(true, false), (true, true), (false, false)] {
            let key = map
                .translate(focus.route_key(30, pressed).unwrap(), repeat)
                .unwrap()
                .unwrap();
            assert_eq!(
                encode_key(&key, READY_KEYBOARD_FLAGS, true, true).unwrap(),
                if pressed { expected.as_bytes() } else { b"" },
                "levels={levels} mask={mask}"
            );
        }
    }
    let mut map = Keymap::from_bytes(include_bytes!("../../tests/fixtures/keypad.xkb")).unwrap();
    for (mask, expected) in [
        (0, "\x1b[57423u"),
        (0x20, "7"),
        (0x21, "\x1b[57423;130u"),
        (0x25, "\x1b[57423;134u"),
    ] {
        map.update_modifiers(Modifiers {
            depressed: mask,
            ..Default::default()
        });
        let key = map
            .translate(focus.route_key(71, true).unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            encode_key(&key, READY_KEYBOARD_FLAGS, true, true).unwrap(),
            expected.as_bytes()
        );
    }
}

#[test]
fn kitty_legacy_named_modifiers_keep_existing_bytes() {
    let mut key = key();
    for (hyper, meta) in [(true, false), (false, true), (true, true)] {
        key.hyper = hyper;
        key.meta = meta;
        for (symbol, expected) in [
            (0x61, "a"),
            (0xff52, "\x1bOA"),
            (0xffbe, "\x1bOP"),
            (0xff0d, "\r"),
        ] {
            key.keysyms = vec![symbol];
            key.unshifted_keysyms = vec![symbol];
            for state in [KeyState::Pressed, KeyState::Repeated, KeyState::Released] {
                key.state = state;
                assert_eq!(
                    encode_key(&key, KeyboardFlags::empty(), true, true).unwrap(),
                    if state == KeyState::Released {
                        b""
                    } else {
                        expected.as_bytes()
                    }
                );
            }
        }
    }
}
