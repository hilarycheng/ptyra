use super::{KeyState, Keymap, MAX_KEYMAP_BYTES, Modifiers};
use crate::{TabId, WindowId, input::KeyboardFocus};

const MAP: &[u8] = include_bytes!("../../tests/fixtures/greek.xkb");

#[test]
fn keymap_translates_modifiers() {
    let mut keymap = Keymap::from_bytes(MAP).unwrap();
    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(7));
    let key = keymap
        .translate(focus.route_key(30, true).unwrap(), false)
        .unwrap()
        .unwrap();
    assert_eq!(key.target.tab, TabId(7));
    assert_eq!(key.physical_code, 30);
    assert_eq!(key.keysyms, [0x07e1]); // Greek alpha, not a fixed US-layout lookup.
    assert_eq!(key.state, KeyState::Pressed);
    assert!(key.repeatable);
    assert!(!key.modifiers.ctrl());

    let modifiers = Modifiers {
        depressed: 1,
        latched: 4,
        locked: 8,
        group: 0,
    };
    keymap.update_modifiers(modifiers);
    let key = keymap
        .translate(focus.route_key(30, true).unwrap(), true)
        .unwrap()
        .unwrap();
    assert_eq!(key.keysyms, [0x07c1]);
    assert_eq!(key.state, KeyState::Repeated);
    assert_eq!(key.modifiers, modifiers);
    assert!(key.modifiers.ctrl() && key.modifiers.shift() && key.modifiers.alt());
    let release = keymap
        .translate(focus.route_key(30, false).unwrap(), false)
        .unwrap()
        .unwrap();
    assert_eq!(release.state, KeyState::Released);
    assert_eq!(release.keysyms, [0x07c1]);
    assert!(
        keymap
            .translate(focus.route_key(30, false).unwrap(), true)
            .is_err()
    );

    keymap.update_modifiers(Modifiers {
        group: 1,
        ..modifiers
    });
    assert_eq!(
        keymap
            .translate(focus.route_key(30, true).unwrap(), false)
            .unwrap()
            .unwrap()
            .keysyms,
        [b'A' as u32]
    );
    keymap.update_modifiers(Modifiers::default());
    let shift = keymap
        .translate(focus.route_key(42, true).unwrap(), false)
        .unwrap()
        .unwrap();
    assert_eq!(shift.keysyms, [0xffe1]);
    assert!(!shift.repeatable);
    assert_eq!(
        keymap
            .translate(focus.route_key(16, true).unwrap(), false)
            .unwrap()
            .unwrap()
            .keysyms,
        [0x01004e2d]
    );
    assert!(
        keymap
            .translate(focus.route_key(999, true).unwrap(), false)
            .unwrap()
            .is_none()
    );
    assert!(
        keymap
            .translate(focus.route_key(u32::MAX, true).unwrap(), false)
            .is_err()
    );
    focus.leave(WindowId(1));
    assert!(focus.route_key(30, true).is_none());
}

#[test]
fn keymap_rejects_invalid_or_excessive_input() {
    assert!(Keymap::from_bytes(&[]).is_err());
    assert!(Keymap::from_bytes(b"private-keymap-payload").is_err());
    let error = Keymap::from_bytes(b"private-keymap-payload").err().unwrap();
    assert!(!format!("{error:?} {error}").contains("private-keymap-payload"));
    assert!(Keymap::from_bytes(&vec![b' '; MAX_KEYMAP_BYTES + 1]).is_err());
    let mut terminated = MAP.to_vec();
    terminated.push(0);
    assert!(Keymap::from_bytes(&terminated).is_ok());
    terminated.push(b'x');
    assert!(Keymap::from_bytes(&terminated).is_err());
    assert!(
        Keymap::from_bytes(
            b"xkb_keymap { xkb_symbols { include \"/no-keymap-file-should-be-opened\" }; };"
        )
        .is_err()
    );
    let text = std::str::from_utf8(MAP).unwrap();
    let excessive_level = text.replace("map[Shift] = Level2", "map[Shift] = 1000000000");
    assert!(Keymap::from_bytes(excessive_level.as_bytes()).is_err());
    let nested = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert!(
        Keymap::from_bytes(
            text.replace("map[Shift] = Level2", &format!("map[Shift] = {nested}"))
                .as_bytes()
        )
        .is_err()
    );
    let chain = "1 + // ;\n".repeat(200) + "1";
    assert!(
        Keymap::from_bytes(
            text.replace("map[Shift] = Level2", &format!("map[Shift] = {chain}"))
                .as_bytes()
        )
        .is_err()
    );
}

#[test]
fn keymap_expression_budget_ignores_quoted_delimiters() {
    use super::check_expression_budget;
    let long_chain = "1 + // ;\n".repeat(129) + "1;";
    assert!(check_expression_budget(long_chain.as_bytes()).is_err());
    let hash_comments = "1 + # ;\n".repeat(129) + "1;";
    assert!(check_expression_budget(hash_comments.as_bytes()).is_err());
    let key_names = "<+;> + ".repeat(129) + "<+;>;";
    assert!(check_expression_budget(key_names.as_bytes()).is_err());
    let strings = "\"escaped\\\";+\" + ".repeat(129) + "\"last\";";
    assert!(check_expression_budget(strings.as_bytes()).is_err());
    let literal = format!("name = \"{}\";", "+;".repeat(1000));
    assert!(check_expression_budget(literal.as_bytes()).is_ok());
    assert!(check_expression_budget(b"name = \"unterminated\\").is_err());
    assert!(check_expression_budget(b"<unterminated").is_err());
    // Separate statements do not create one deeply recursive expression tree.
    assert!(check_expression_budget("map[Shift+Control] = 1;".repeat(200).as_bytes()).is_ok());
}
