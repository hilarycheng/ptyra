use super::*;
use crate::{
    TabId, WindowId,
    clock::FakeClock,
    keymap::{Keymap, Modifiers},
};
use std::time::Duration;

fn runtime() -> Runtime {
    let config = crate::config::Config {
        scrollback_lines: 0,
        ..Default::default()
    };
    let font = ab_glyph::FontVec::try_from_vec(
        include_bytes!("../../tests/fixtures/fallback-primary.ttf").to_vec(),
    )
    .unwrap();
    let raster = raster::Raster::new(vec![font], &config).unwrap();
    let mut app = new_runtime(config, raster).unwrap();
    for tab in [TabId(1), TabId(2)] {
        app.tabs.insert(tab, 2, 40).unwrap();
    }
    app.input.map =
        Some(Keymap::from_bytes(include_bytes!("../../tests/fixtures/keyboard.xkb")).unwrap());
    app.input.focus.enter(WindowId(1), TabId(1));
    app.input.repeat.configure(20, 100).unwrap();
    app
}

fn negotiate(app: &mut Runtime, tab: TabId, bytes: &[u8], enhanced: bool) {
    let terminal = app.tabs.get_mut(tab).unwrap().terminal_mut();
    // Every byte is a separate read, including screen changes and partial CSI sequences.
    for byte in bytes.iter().chain(b"\x1b[?u") {
        terminal
            .feed_with_replies(tab, std::slice::from_ref(byte))
            .unwrap();
    }
    let reply = if enhanced { b"\x1b[?1u" } else { b"\x1b[?0u" };
    assert_eq!(terminal.take_core_replies(), (reply.to_vec(), false));
    assert!(terminal.take_keyboard_reply().is_none());
}

fn tap(app: &mut Runtime, clock: &FakeClock, code: u32, mask: u32, expected: &[u8]) {
    app.input.map.as_mut().unwrap().update_modifiers(Modifiers {
        depressed: mask,
        ..Default::default()
    });
    let bytes = app
        .input
        .key(
            code,
            true,
            app.application_cursor(),
            app.application_keypad(),
            app.keyboard_flags(),
            clock,
        )
        .unwrap();
    assert_eq!(bytes.unwrap_or_default(), expected);
    assert!(
        app.input
            .key(
                code,
                false,
                app.application_cursor(),
                app.application_keypad(),
                app.keyboard_flags(),
                clock
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn runtime_kitty_negotiation_controls_active_encoder() {
    let mut app = runtime();
    let clock = FakeClock::default();
    let one = TabId(1);
    let two = TabId(2);
    // No opt-in keeps the existing control bytes and application cursor mode.
    negotiate(&mut app, one, b"\x1b[?1h", false);
    tap(&mut app, &clock, 30, 4, b"\x01");
    tap(&mut app, &clock, 28, 1, b"\r");
    tap(&mut app, &clock, 103, 0, b"\x1bOA");
    for (sequence, enhanced) in [
        (b"\x1b[=31u".as_slice(), true),
        (b"\x1b[>0u", false),
        (b"\x1b[<u", true),
        (b"\x1b[=1;3u", false),
        (b"\x1b[=31;2u", true),
        (b"\x1b[=0;99u", true),
        (b"\x1b[=0:1u", true),
        (b"\x1b[>30u", false),
        (b"\x1b[<0u", true),
        (b"\x1b[>u", false),
        (b"\x1b[<65535u", false),
        (b"\x1b[=1u", true),
        (b"\x1b[=u", false),
    ] {
        negotiate(&mut app, one, sequence, enhanced);
        tap(
            &mut app,
            &clock,
            30,
            4,
            if enhanced { b"\x1b[97;5u" } else { b"\x01" },
        );
        tap(
            &mut app,
            &clock,
            28,
            1,
            if enhanced { b"\x1b[13;2u" } else { b"\r" },
        );
        tap(
            &mut app,
            &clock,
            103,
            0,
            if enhanced { b"\x1b[A" } else { b"\x1bOA" },
        );
        tap(&mut app, &clock, 30, 0, b"a");
    }
    // Main and alternate stacks survive independently, including across repeated switches.
    for (sequence, enhanced) in [
        (b"\x1b[=1u\x1b[?1049h".as_slice(), false),
        (b"\x1b[>1u\x1b[>0u", false),
        (b"\x1b[?1049l", true),
        (b"\x1b[?1049h", false),
        (b"\x1b[<u", true),
        (b"\x1b[?1049l\x1b[=0u\x1b[?1049h", true),
        (b"\x1bc", false),
        (b"\x1b[?1049h", false),
        (b"\x1b[?1049l", false),
    ] {
        negotiate(&mut app, one, sequence, enhanced);
        tap(
            &mut app,
            &clock,
            30,
            4,
            if enhanced { b"\x1b[97;5u" } else { b"\x01" },
        );
    }
    // Repeat samples the current mode; a switch cancels the old Tab's pending repeat.
    negotiate(&mut app, one, b"\x1b[=1u", true);
    app.input
        .key(30, true, false, false, app.keyboard_flags(), &clock)
        .unwrap();
    clock.advance(Duration::from_millis(100)).unwrap();
    assert_eq!(
        app.input
            .tick(false, false, app.keyboard_flags(), &clock)
            .unwrap()
            .unwrap(),
        b"\x1b[97;5u"
    );
    negotiate(&mut app, one, b"\x1b[=0u", false);
    clock.advance(Duration::from_millis(50)).unwrap();
    assert_eq!(
        app.input
            .tick(false, false, app.keyboard_flags(), &clock)
            .unwrap()
            .unwrap(),
        b"\x01"
    );
    app.activate_tab(two);
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(
        app.input
            .tick(false, false, app.keyboard_flags(), &clock)
            .unwrap()
            .is_none()
    );
    negotiate(&mut app, one, b"\x1b[=31u", true);
    negotiate(&mut app, two, b"", false);
    tap(&mut app, &clock, 30, 4, b"\x01");
    app.activate_tab(one);
    tap(&mut app, &clock, 30, 4, b"\x1b[97;5u");
    app.close_tab(one);
    assert_eq!(app.tabs.active(), Some(two));
    tap(&mut app, &clock, 30, 4, b"\x01");
    app.tabs.insert(TabId(3), 2, 40).unwrap();
    app.activate_tab(TabId(3));
    negotiate(&mut app, TabId(3), b"", false);
    tap(&mut app, &clock, 28, 1, b"\r");
}
