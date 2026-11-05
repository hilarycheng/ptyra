use super::{
    ClipboardOwner, ClipboardState, DataOffer, DataSourceRequest, InputSerialEvent, PasteKeyAction,
    PastePayload, PasteSink, SeatId, SeatSerialState, SelectionSerial, copy_shortcut,
    paste_shortcut, plan_data_source, select_text_mime,
};
use crate::{
    TabId, WindowId,
    input::{FocusTarget, KeyboardFocus},
    keymap::{KeyState, Modifiers},
    terminal::Terminal,
};
use alacritty_terminal::{
    index::{Column, Line, Point},
    selection::SelectionRange,
};
use kbvm::ModifierMask;

#[test]
fn clipboard_state_rejects_stale_offer() {
    let mut clipboard = ClipboardState::new();
    assert_eq!(clipboard.owner(), None);
    let old = clipboard.offer_created().unwrap();
    let newer = clipboard.offer_created().unwrap();
    assert!(clipboard.select_offer(newer));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    assert!(!clipboard.select_offer(old));
    assert!(!clipboard.select_offer(newer));
    clipboard.cancel_offer(old);
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    let pending = clipboard.offer_created().unwrap();
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    clipboard.cancel_offer(pending);
    assert!(!clipboard.select_offer(pending));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    assert!(clipboard.set_local_owner(TabId(7)));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::Local(TabId(7))));
    assert!(!clipboard.select_offer(newer));
    clipboard.cancel_offer(newer);
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::Local(TabId(7))));
    let newest = clipboard.offer_created().unwrap();
    assert!(clipboard.select_offer(newest));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newest)));
    clipboard.cancel_offer(newest);
    assert_eq!(clipboard.owner(), None);
    assert!(!clipboard.select_offer(newest));
}

#[test]
fn mime_prefers_utf8_text() {
    let offered = [
        "text/html",
        "text/plain",
        "UTF8_STRING",
        "text/plain; charset=UTF-8",
    ];
    assert_eq!(select_text_mime(&offered), Some(offered[3]));
    assert_eq!(select_text_mime(&offered[..3]), Some("UTF8_STRING"));
    assert_eq!(select_text_mime(&offered[..2]), Some("text/plain"));
    assert_eq!(
        select_text_mime(&["TEXT/PLAIN;CHARSET=\"utf-8\""]),
        Some("TEXT/PLAIN;CHARSET=\"utf-8\"")
    );
    assert_eq!(
        select_text_mime(&["text/html", "text/plain;charset=iso-8859-1", "STRING"]),
        None
    );
    assert_eq!(select_text_mime(&[]), None);
}

#[test]
fn clipboard_serial_requires_input_event() {
    let seat = SeatId(1);
    let other_seat = SeatId(2);
    let target = FocusTarget {
        window: WindowId(1),
        tab: TabId(7),
    };
    let other_target = FocusTarget {
        window: WindowId(1),
        tab: TabId(8),
    };
    let mut serials = SeatSerialState::default();
    assert_eq!(serials.take_for_selection(seat, target), None);
    serials.focus(seat, target);
    assert!(!serials.record_event(seat, target, 12, InputSerialEvent::Other));
    assert!(!serials.record_event(other_seat, target, 13, InputSerialEvent::KeyPress));
    assert!(!serials.record_event(seat, other_target, 14, InputSerialEvent::PointerButtonPress));
    assert_eq!(serials.take_for_selection(seat, target), None);
    assert!(serials.record_event(seat, target, 0, InputSerialEvent::KeyPress));
    assert_eq!(
        serials.take_for_selection(seat, target),
        Some(SelectionSerial(0))
    );
    assert_eq!(serials.take_for_selection(seat, target), None);
    assert!(serials.record_event(seat, target, 15, InputSerialEvent::PointerButtonPress));
    serials.focus(seat, other_target);
    assert_eq!(serials.take_for_selection(seat, target), None);
    assert_eq!(serials.take_for_selection(seat, other_target), None);
    assert!(serials.record_event(seat, other_target, 16, InputSerialEvent::KeyPress));
    serials.clear();
    assert_eq!(serials.take_for_selection(seat, other_target), None);
}

#[test]
fn data_source_advertises_text_mime() {
    let seat = SeatId(1);
    let target = FocusTarget {
        window: WindowId(1),
        tab: TabId(7),
    };
    let mut serials = SeatSerialState::default();
    assert_eq!(plan_data_source(&mut serials, seat, target), None);
    serials.focus(seat, target);
    assert_eq!(plan_data_source(&mut serials, seat, target), None);
    assert!(serials.record_event(seat, target, 42, InputSerialEvent::KeyPress));
    assert_eq!(
        plan_data_source(&mut serials, seat, target),
        Some([
            DataSourceRequest::Create,
            DataSourceRequest::Offer("text/plain;charset=utf-8"),
            DataSourceRequest::Offer("text/plain"),
            DataSourceRequest::SetSelection {
                seat,
                serial: SelectionSerial(42)
            },
        ])
    );
    assert_eq!(plan_data_source(&mut serials, seat, target), None);
    assert!(serials.record_event(seat, target, 43, InputSerialEvent::PointerButtonPress));
    assert_eq!(plan_data_source(&mut serials, SeatId(2), target), None);
    assert_eq!(
        plan_data_source(&mut serials, seat, target).unwrap()[3],
        DataSourceRequest::SetSelection {
            seat,
            serial: SelectionSerial(43)
        }
    );
}

#[test]
fn data_offer_tracks_formats() {
    let mut clipboard = ClipboardState::new();
    let id = clipboard.offer_created().unwrap();
    let mut offer = DataOffer::new(id);
    assert_eq!(offer.id(), id);
    assert_eq!(offer.preferred_text_mime(), None);
    assert!(offer.advertise("text/plain").unwrap());
    assert!(offer.advertise("text/plain;charset=utf-8").unwrap());
    assert!(!offer.advertise("text/plain").unwrap());
    assert_eq!(
        offer.formats(),
        &[
            "text/plain".to_owned(),
            "text/plain;charset=utf-8".to_owned()
        ]
    );
    assert_eq!(
        offer.preferred_text_mime(),
        Some("text/plain;charset=utf-8")
    );
    assert!(offer.advertise("bad\0mime").is_err());
    assert!(offer.advertise(&"x".repeat(257)).is_err());
    assert!(clipboard.select_offer(id));
    offer.cancel();
    clipboard.cancel_offer(id);
    assert!(offer.is_cancelled());
    assert!(offer.formats().is_empty());
    assert_eq!(offer.preferred_text_mime(), None);
    assert!(offer.advertise("text/plain").is_err());
    assert_eq!(clipboard.owner(), None);
}

#[test]
fn copy_shortcut_requires_selection() {
    let tab = TabId(7);
    let target = FocusTarget {
        window: WindowId(1),
        tab,
    };
    let mut terminal = Terminal::new(tab, 1, 4).unwrap();
    terminal.feed(tab, b"ab").unwrap();
    let grid = terminal.grid().unwrap();
    let modifiers = Modifiers {
        depressed: ModifierMask::CONTROL.0 | ModifierMask::SHIFT.0,
        ..Modifiers::default()
    };
    let range = SelectionRange::new(
        Point::new(Line(0), Column(0)),
        Point::new(Line(0), Column(1)),
        false,
    );
    assert!(
        copy_shortcut(
            target,
            modifiers,
            b'c' as u32,
            KeyState::Pressed,
            None,
            grid
        )
        .is_none()
    );
    assert!(
        copy_shortcut(
            target,
            Modifiers::default(),
            b'C' as u32,
            KeyState::Pressed,
            Some(range),
            grid
        )
        .is_none()
    );
    assert!(
        copy_shortcut(
            target,
            modifiers,
            b'c' as u32,
            KeyState::Released,
            Some(range),
            grid
        )
        .is_none()
    );
    assert!(
        copy_shortcut(
            target,
            modifiers,
            b'c' as u32,
            KeyState::Repeated,
            Some(range),
            grid
        )
        .is_none()
    );
    assert!(
        copy_shortcut(
            target,
            modifiers,
            b'v' as u32,
            KeyState::Pressed,
            Some(range),
            grid
        )
        .is_none()
    );
    let intent = copy_shortcut(
        target,
        modifiers,
        b'C' as u32,
        KeyState::Pressed,
        Some(range),
        grid,
    )
    .unwrap();
    assert_eq!(intent.target(), target);
    assert_eq!(intent.text(), "ab");
    assert!(
        copy_shortcut(
            target,
            modifiers,
            b'c' as u32,
            KeyState::Pressed,
            Some(SelectionRange::new(
                Point::new(Line(2), Column(0)),
                Point::new(Line(2), Column(0)),
                false
            )),
            grid
        )
        .is_none()
    );
    assert_eq!(ClipboardState::new().owner(), None);
}

#[test]
fn paste_shortcut_routes_to_focused_tab() {
    let mut focus = KeyboardFocus::default();
    let modifiers = Modifiers {
        depressed: ModifierMask::CONTROL.0 | ModifierMask::SHIFT.0,
        ..Modifiers::default()
    };
    focus.enter(WindowId(1), TabId(7));
    let old = focus.route_key(47, true).unwrap();
    match paste_shortcut(&focus, &old, modifiers, b'V' as u32, KeyState::Pressed) {
        PasteKeyAction::Paste(intent) => assert_eq!(intent.target().tab, TabId(7)),
        _ => panic!("paste shortcut must be consumed"),
    }
    assert!(matches!(
        paste_shortcut(&focus, &old, modifiers, b'V' as u32, KeyState::Released),
        PasteKeyAction::Consumed
    ));
    assert!(matches!(
        paste_shortcut(&focus, &old, modifiers, b'V' as u32, KeyState::Repeated),
        PasteKeyAction::Consumed
    ));
    assert!(matches!(
        paste_shortcut(
            &focus,
            &old,
            Modifiers::default(),
            b'V' as u32,
            KeyState::Pressed
        ),
        PasteKeyAction::Shell
    ));
    focus.enter(WindowId(1), TabId(8));
    assert!(matches!(
        paste_shortcut(&focus, &old, modifiers, b'V' as u32, KeyState::Pressed),
        PasteKeyAction::Consumed
    ));
    let current = focus.route_key(47, true).unwrap();
    match paste_shortcut(&focus, &current, modifiers, b'v' as u32, KeyState::Pressed) {
        PasteKeyAction::Paste(intent) => assert_eq!(intent.target().tab, TabId(8)),
        _ => panic!("current tab must receive paste intent"),
    }
    focus.clear();
    assert!(matches!(
        paste_shortcut(&focus, &current, modifiers, b'v' as u32, KeyState::Pressed),
        PasteKeyAction::Consumed
    ));
}

#[test]
fn paste_never_switches_target_mid_transfer() {
    struct Sink {
        tab: TabId,
        received: Vec<u8>,
    }

    impl PasteSink for Sink {
        type Error = ();

        fn tab(&self) -> TabId {
            self.tab
        }

        fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.received.extend_from_slice(bytes);
            Ok(())
        }
    }

    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(7));
    let route = focus.route_key(47, true).unwrap();
    let modifiers = Modifiers {
        depressed: ModifierMask::CONTROL.0 | ModifierMask::SHIFT.0,
        ..Modifiers::default()
    };
    let PasteKeyAction::Paste(intent) =
        paste_shortcut(&focus, &route, modifiers, b'V' as u32, KeyState::Pressed)
    else {
        panic!("expected paste intent");
    };
    let text = "繁".repeat(6000);
    let mut transfer = intent.with_payload(PastePayload::new(text.clone()).unwrap());
    let mut original = Sink {
        tab: TabId(7),
        received: Vec::new(),
    };
    let mut switched = Sink {
        tab: TabId(8),
        received: Vec::new(),
    };

    assert!(transfer.enqueue_next(&mut original).unwrap());
    assert!(!transfer.is_complete());
    focus.enter(WindowId(1), TabId(8));
    assert!(!transfer.enqueue_next(&mut switched).unwrap());
    assert!(switched.received.is_empty());
    assert!(!transfer.is_complete());
    while transfer.enqueue_next(&mut original).unwrap() {}
    assert!(transfer.is_complete());
    assert_eq!(original.received, text.as_bytes());
    assert!(switched.received.is_empty());
}

#[test]
fn paste_to_closed_tab_is_cancelled() {
    struct Sink {
        tab: TabId,
        received: Vec<u8>,
        blocked: bool,
        calls: usize,
    }

    impl PasteSink for Sink {
        type Error = ();

        fn tab(&self) -> TabId {
            self.tab
        }

        fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.calls += 1;
            if self.blocked {
                return Err(());
            }
            self.received.extend_from_slice(bytes);
            Ok(())
        }
    }

    // Close or focus loss can arrive before delivery, between chunks, or on retry.
    for delivered_chunk in [false, true] {
        for focus_lost in [false, true] {
            let mut focus = KeyboardFocus::default();
            focus.enter(WindowId(1), TabId(7));
            let route = focus.route_key(47, true).unwrap();
            let modifiers = Modifiers {
                depressed: ModifierMask::CONTROL.0 | ModifierMask::SHIFT.0,
                ..Modifiers::default()
            };
            let PasteKeyAction::Paste(intent) =
                paste_shortcut(&focus, &route, modifiers, b'V' as u32, KeyState::Pressed)
            else {
                panic!("expected paste intent");
            };
            let mut transfer = intent.with_payload(PastePayload::new("繁".repeat(6000)).unwrap());
            let mut sink = Sink {
                tab: TabId(7),
                received: Vec::new(),
                blocked: false,
                calls: 0,
            };
            transfer.cancel_for_tab(TabId(8));
            assert!(!transfer.is_complete());
            if delivered_chunk {
                assert!(transfer.enqueue_next(&mut sink).unwrap());
            }
            sink.blocked = true;
            assert_eq!(transfer.enqueue_next(&mut sink), Err(()));
            let sent = sink.received.clone();
            let calls = sink.calls;

            if focus_lost {
                focus.leave(WindowId(1));
            }
            transfer.cancel_for_tab(TabId(7));
            transfer.cancel_for_tab(TabId(7));
            assert!(transfer.is_complete());
            assert!(transfer.payload.bytes.is_empty());
            assert_eq!(transfer.payload.bytes.capacity(), 0);
            sink.blocked = false;
            for tab in [TabId(8), TabId(7)] {
                focus.enter(WindowId(1), tab);
                sink.tab = tab;
                assert!(!transfer.enqueue_next(&mut sink).unwrap());
            }
            assert_eq!(sink.received, sent);
            assert_eq!(sink.calls, calls);
        }
    }
}
