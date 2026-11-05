use crate::{
    TabId, WindowId,
    clipboard::{
        SeatId,
        read::{ClipboardReceive, ReadProgress},
        select_text_mime,
    },
    ime::{ImeEvent, ImeState},
    input::KeyboardFocus,
    terminal::Terminal,
    wayland::{Capabilities, Global},
};

#[test]
fn malformed_input_never_panics() {
    let mut seeds: Vec<Vec<u8>> = include_str!("../tests/fixtures/fuzz.hex")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split_whitespace()
                .map(|hex| u8::from_str_radix(hex, 16).unwrap())
                .collect()
        })
        .collect();
    let mut state = 0x5054_5952_u32;
    for _ in 0..256 {
        let mut bytes = Vec::new();
        for _ in 0..(state as usize % 128) {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            bytes.push(state as u8);
        }
        state = state.wrapping_add(1);
        seeds.push(bytes);
    }
    seeds.push([b"\x1b]2;".as_slice(), &vec![b'x'; 4097]].concat());
    seeds.push("中".repeat(1500).into_bytes());
    for bytes in seeds {
        let mut reference = None;
        for chunk_size in [1, 7, 16_384] {
            let mut terminal = Terminal::with_scrollback(TabId(1), 3, 8, 0).unwrap();
            for chunk in bytes.chunks(chunk_size) {
                if terminal.feed(TabId(1), chunk).is_err() {
                    break;
                }
            }
            let result = terminal.snapshot().map(|snapshot| {
                snapshot
                    .content
                    .display_iter
                    .map(|cell| cell.c)
                    .collect::<String>()
            });
            if let Some(expected) = &reference {
                assert_eq!(&result, expected);
            }
            reference = Some(result);
        }
        let mut receive = ClipboardReceive::with_limit(8192).unwrap();
        let mut reader = bytes.as_slice();
        if !bytes.is_empty() {
            assert!(
                matches!(receive.read_ready(&mut reader), Ok(ReadProgress::Read(n)) if n == bytes.len())
            );
        }
        match (std::str::from_utf8(&bytes), receive.read_ready(&mut reader)) {
            (Ok(expected), Ok(ReadProgress::Complete(actual))) => assert_eq!(actual, expected),
            (Err(_), Err(error)) => assert_eq!(error.kind(), std::io::ErrorKind::InvalidData),
            _ => panic!("UTF-8 validation disagrees"),
        }
        assert!(matches!(
            receive.read_ready(&mut reader),
            Ok(ReadProgress::Closed)
        ));
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(
            select_text_mime(&[&text, "text/plain;charset=utf-8"]),
            Some("text/plain;charset=utf-8")
        );
        let caps = Capabilities::from_globals(&[Global {
            name: 1,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(WindowId(1), TabId(1));
        let mut ime = ImeState::new(SeatId(1), &caps);
        ime.enter(SeatId(1), WindowId(1));
        ime.sync_enabled(SeatId(1), &keyboard, true);
        let focus = ime.capture_focus(SeatId(1), WindowId(1), &keyboard);
        for cursor in [i32::MIN, -1, 0, 1, text.len() as i32, i32::MAX] {
            let result = ime.stage(
                &focus,
                &keyboard,
                ImeEvent::Preedit {
                    text: Some(&text),
                    cursor_begin: cursor,
                    cursor_end: cursor,
                },
            );
            let valid = text.len() <= 4096
                && (cursor == -1
                    || usize::try_from(cursor).is_ok_and(|i| text.is_char_boundary(i)));
            assert_eq!(result.is_ok(), valid);
            assert_eq!(ime.done(&focus, &keyboard, 1).is_some(), valid);
        }
        let result = ime.stage(&focus, &keyboard, ImeEvent::Commit { text: Some(&text) });
        assert_eq!(result.is_ok(), text.len() <= 4096);
        keyboard.enter(WindowId(1), TabId(2));
        assert!(ime.done(&focus, &keyboard, 1).is_none());
    }
}
