use super::{
    ClipboardOwner, ClipboardState, InputSerialEvent, SeatId, SeatSerialState, SelectionSerial,
    select_text_mime,
};
use crate::{TabId, WindowId, input::FocusTarget};

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
