use super::KeyboardFlags;
use crate::{
    error::{AppError, Component},
    keymap::{KeyState, TranslatedKey},
};
use kbvm::{Keysym, ModifierMask};

/// Encode only keys returned by the ownership gate; IME commits use their separate UTF-8 path.
/// Unsupported enhancements or XKB multi-symbol keys fail explicitly, without partial output.
pub fn encode_key(
    key: &TranslatedKey,
    flags: KeyboardFlags,
    application_cursor: bool,
) -> Result<Vec<u8>, AppError> {
    let unsupported = || AppError::new(Component::Terminal, std::io::ErrorKind::Unsupported);
    if !(flags - KeyboardFlags::DISAMBIGUATE_ESC_CODES).is_empty() {
        return Err(unsupported());
    }
    if key.state == KeyState::Released || key.keysyms.is_empty() {
        return Ok(Vec::new());
    }
    let ([symbol], [base]) = (key.keysyms.as_slice(), key.unshifted_keysyms.as_slice()) else {
        return Err(unsupported());
    };
    let modifiers = key.modifiers;
    let super_key = modifiers.effective() & ModifierMask::SUPER.0 != 0;
    let mods = 1
        + u8::from(modifiers.shift())
        + 2 * u8::from(modifiers.alt())
        + 4 * u8::from(modifiers.ctrl())
        + 8 * u8::from(super_key);
    let enhanced = flags.contains(KeyboardFlags::DISAMBIGUATE_ESC_CODES);
    let control = match *symbol {
        0xff0d => Some(13),
        0xff09 | 0xfe20 => Some(9),
        0xff08 => Some(127),
        0xff1b => Some(27),
        _ => None,
    };
    if let Some(code) = control {
        if enhanced && (code == 27 || mods != 1) || super_key {
            return Ok(csi(code, mods, 'u'));
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
    if let Some((number, suffix, ss3)) = functional(*symbol, enhanced) {
        return Ok(
            if !enhanced
                && mods == 1
                && ss3
                && (application_cursor || (0xffbe..=0xffc1).contains(symbol))
            {
                format!("\x1bO{suffix}").into_bytes()
            } else {
                csi(number, mods, suffix)
            },
        );
    }
    // Modifier-only events have no legacy byte representation.
    if (0xffe1..=0xffee).contains(symbol) {
        return Ok(Vec::new());
    }
    let text = Keysym(*symbol)
        .char()
        .filter(|ch| !ch.is_control())
        .ok_or_else(unsupported)?;
    let base = Keysym(*base)
        .char()
        .filter(|ch| !ch.is_control())
        .ok_or_else(unsupported)?;
    if super_key
        || modifiers.ctrl() && modifiers.shift()
        || enhanced && (modifiers.ctrl() || modifiers.alt())
    {
        return Ok(csi(base as u32, mods, 'u'));
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

fn csi(number: u32, mods: u8, suffix: char) -> Vec<u8> {
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
                encode_key(&key, KeyboardFlags::from_bits(flags).unwrap(), false).unwrap(),
                expected.as_bytes()
            );
        }
        map.update_modifiers(Modifiers::default());
        let key = map
            .translate(focus.route_key(103, true).unwrap(), false)
            .unwrap()
            .unwrap();
        assert_eq!(
            encode_key(&key, KeyboardFlags::empty(), true).unwrap(),
            b"\x1bOA"
        );
        assert!(encode_key(&key, KeyboardFlags::REPORT_ASSOCIATED_TEXT, false).is_err());
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
                encode_key(&key, KeyboardFlags::from_bits(flags).unwrap(), false).unwrap(),
                expected.as_bytes()
            );
        }
    }
}
