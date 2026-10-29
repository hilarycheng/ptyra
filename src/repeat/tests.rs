use super::{MAX_REPEAT_DELAY_MS, MAX_REPEAT_RATE, RepeatScheduler};
use crate::{
    TabId, WindowId,
    clock::FakeClock,
    input::KeyboardFocus,
    keymap::{KeyState, Keymap, Modifiers, TranslatedKey},
};
use std::time::Duration;

const MAP: &[u8] = include_bytes!("../../tests/fixtures/greek.xkb");

fn setup() -> (FakeClock, KeyboardFocus, Keymap, RepeatScheduler) {
    let clock = FakeClock::default();
    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(1));
    let map = Keymap::from_bytes(MAP).unwrap();
    let mut repeat = RepeatScheduler::default();
    repeat.configure(25, 500).unwrap();
    (clock, focus, map, repeat)
}

fn key(map: &Keymap, focus: &KeyboardFocus, code: u32, pressed: bool) -> TranslatedKey {
    map.translate(focus.route_key(code, pressed).unwrap(), false)
        .unwrap()
        .unwrap()
}

#[test]
fn key_repeat_stops_on_release() {
    let (clock, focus, map, mut repeat) = setup();
    assert!(repeat.deadline(&focus, &map).is_none());
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    assert_eq!(
        repeat.deadline(&focus, &map).unwrap().remaining(&clock),
        Duration::from_millis(500)
    );
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    clock.advance(Duration::from_millis(499)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    clock.advance(Duration::from_millis(1)).unwrap();
    let event = repeat.poll(&clock, &focus, &map).unwrap().unwrap();
    assert_eq!(event.state, KeyState::Repeated);
    assert_eq!(event.physical_code, 30);
    assert_eq!(event.keysyms, [0x07e1]);
    assert_eq!(event.target.tab, TabId(1));
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    clock.advance(Duration::from_millis(39)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    clock.advance(Duration::from_millis(1)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_some());
    repeat
        .on_key(&key(&map, &focus, 30, false), &clock)
        .unwrap();
    assert!(repeat.deadline(&focus, &map).is_none());
    clock.advance(Duration::from_secs(60)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    repeat
        .on_key(&key(&map, &focus, 30, false), &clock)
        .unwrap();
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
}

#[test]
fn repeat_focus_lifecycle_invalidates_old_input() {
    let (clock, mut focus, map, mut repeat) = setup();
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    focus.leave(WindowId(2)); // An unrelated surface leave does not revoke focus.
    clock.advance(Duration::from_millis(500)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_some());
    focus.leave(WindowId(1));
    assert!(repeat.deadline(&focus, &map).is_none());
    focus.enter(WindowId(1), TabId(1));
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());

    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    let stale_release = key(&map, &focus, 30, false);
    focus.leave(WindowId(1));
    focus.enter(WindowId(1), TabId(1)); // Leave/re-enter entirely between timer polls.
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    repeat.on_key(&stale_release, &clock).unwrap();
    clock.advance(Duration::from_millis(500)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_some());

    for (window, tab) in [(1, 2), (2, 2)] {
        repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
        focus.enter(WindowId(window), TabId(tab));
        clock.advance(Duration::from_secs(1)).unwrap();
        assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
        assert!(repeat.deadline(&focus, &map).is_none());
    }
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    focus.clear(); // Keyboard/seat capability disappearance.
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
}

#[test]
fn repeat_uses_latest_key_and_current_modifiers() {
    let (clock, focus, mut map, mut repeat) = setup();
    let press = key(&map, &focus, 30, true);
    repeat.on_key(&press, &clock).unwrap();
    clock.advance(Duration::from_millis(400)).unwrap();
    repeat.on_key(&press, &clock).unwrap(); // A duplicate press does not postpone repeat.
    repeat.on_key(&key(&map, &focus, 42, true), &clock).unwrap();
    let modifiers = Modifiers {
        depressed: 1,
        latched: 4,
        locked: 8,
        group: 0,
    };
    map.update_modifiers(modifiers);
    clock.advance(Duration::from_millis(100)).unwrap();
    let event = repeat.poll(&clock, &focus, &map).unwrap().unwrap();
    assert_eq!(event.keysyms, [0x07c1]);
    assert_eq!(event.modifiers, modifiers);
    repeat.on_key(&event, &clock).unwrap(); // Repeated events must not re-arm the delay.
    repeat
        .on_key(&key(&map, &focus, 42, false), &clock)
        .unwrap();
    map.update_modifiers(Modifiers::default());
    clock.advance(Duration::from_millis(40)).unwrap();
    assert_eq!(
        repeat.poll(&clock, &focus, &map).unwrap().unwrap().keysyms,
        [0x07e1]
    );

    repeat.on_key(&key(&map, &focus, 16, true), &clock).unwrap();
    repeat
        .on_key(&key(&map, &focus, 30, false), &clock)
        .unwrap();
    clock.advance(Duration::from_millis(499)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    clock.advance(Duration::from_millis(1)).unwrap();
    assert_eq!(
        repeat.poll(&clock, &focus, &map).unwrap().unwrap().keysyms,
        [0x01004e2d]
    );
    repeat
        .on_key(&key(&map, &focus, 16, false), &clock)
        .unwrap();
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    repeat.on_key(&key(&map, &focus, 42, true), &clock).unwrap();
    assert!(repeat.deadline(&focus, &map).is_none());

    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    repeat.on_key(&key(&map, &focus, 16, true), &clock).unwrap();
    repeat
        .on_key(&key(&map, &focus, 16, false), &clock)
        .unwrap();
    clock.advance(Duration::from_secs(1)).unwrap();
    // Releasing the winning key does not revive an older, still-held key.
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
}

#[test]
fn repeat_keymap_replacement_cancels_pending() {
    let (clock, focus, mut map, mut repeat) = setup();
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    let old_release = key(&map, &focus, 30, false);
    map = Keymap::from_bytes(MAP).unwrap(); // Same contents and same variable, new keymap.
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    assert!(repeat.deadline(&focus, &map).is_none());
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    repeat.on_key(&old_release, &clock).unwrap();
    clock.advance(Duration::from_millis(500)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_some());
    repeat.cancel();
    assert!(repeat.deadline(&focus, &map).is_none());
}

#[test]
fn repeat_is_independent_per_keyboard() {
    let (clock, focus_a, map_a, mut repeat_a) = setup();
    let (_, focus_b, map_b, mut repeat_b) = setup(); // Even the same Window/Tab IDs are separate.
    repeat_a
        .on_key(&key(&map_a, &focus_a, 30, true), &clock)
        .unwrap();
    repeat_b
        .on_key(&key(&map_b, &focus_b, 16, true), &clock)
        .unwrap();
    repeat_a
        .on_key(&key(&map_b, &focus_b, 30, false), &clock)
        .unwrap();
    clock.advance(Duration::from_millis(500)).unwrap();
    assert_eq!(
        repeat_a
            .poll(&clock, &focus_a, &map_a)
            .unwrap()
            .unwrap()
            .physical_code,
        30
    );
    assert_eq!(
        repeat_b
            .poll(&clock, &focus_b, &map_b)
            .unwrap()
            .unwrap()
            .physical_code,
        16
    );
    repeat_a
        .on_key(&key(&map_a, &focus_a, 30, false), &clock)
        .unwrap();
    clock.advance(Duration::from_millis(40)).unwrap();
    assert!(repeat_a.poll(&clock, &focus_a, &map_a).unwrap().is_none());
    assert!(repeat_b.poll(&clock, &focus_b, &map_b).unwrap().is_some());
    assert!(repeat_b.poll(&clock, &focus_a, &map_b).unwrap().is_none());
    assert!(repeat_b.deadline(&focus_b, &map_b).is_none());
}

#[test]
fn repeat_configuration_and_time_are_bounded() {
    let (clock, focus, map, _) = setup();
    let mut repeat = RepeatScheduler::default();
    let press = key(&map, &focus, 30, true);
    repeat.on_key(&press, &clock).unwrap();
    assert!(repeat.deadline(&focus, &map).is_none());
    for (rate, delay) in [
        (-1, 500),
        (25, -1),
        (MAX_REPEAT_RATE + 1, 0),
        (1, MAX_REPEAT_DELAY_MS + 1),
        (i32::MAX, i32::MAX),
    ] {
        repeat.configure(25, 500).unwrap();
        repeat.on_key(&press, &clock).unwrap();
        assert!(repeat.configure(rate, delay).is_err());
        assert!(repeat.deadline(&focus, &map).is_none());
        repeat.on_key(&press, &clock).unwrap();
        assert!(repeat.deadline(&focus, &map).is_none());
    }
    repeat.configure(25, 500).unwrap();
    repeat.on_key(&press, &clock).unwrap();
    repeat.configure(0, i32::MAX).unwrap();
    assert!(repeat.deadline(&focus, &map).is_none());
    repeat.on_key(&press, &clock).unwrap();
    assert!(repeat.deadline(&focus, &map).is_none());
    repeat.configure(1, MAX_REPEAT_DELAY_MS).unwrap();
    repeat.on_key(&press, &clock).unwrap();
    assert_eq!(
        repeat.deadline(&focus, &map).unwrap().remaining(&clock),
        Duration::from_secs(60)
    );
    repeat.configure(MAX_REPEAT_RATE, 0).unwrap();
    repeat.on_key(&press, &clock).unwrap();
    let repeated = repeat.poll(&clock, &focus, &map).unwrap().unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    assert_eq!(
        repeat.deadline(&focus, &map).unwrap().remaining(&clock),
        Duration::from_millis(1)
    );
    repeat.configure(0, 500).unwrap();
    repeat.on_key(&repeated, &clock).unwrap();
    assert!(repeat.deadline(&focus, &map).is_none());
    repeat.configure(3, 0).unwrap(); // New settings cancel the previous hold.
    assert!(repeat.deadline(&focus, &map).is_none());
    repeat.on_key(&press, &clock).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_some());
    assert_eq!(
        repeat.deadline(&focus, &map).unwrap().remaining(&clock),
        Duration::from_nanos(333_333_334)
    );

    let end = FakeClock::default();
    end.advance(Duration::MAX).unwrap();
    repeat.configure(25, 1).unwrap();
    assert!(repeat.on_key(&press, &end).is_err());
    assert!(repeat.deadline(&focus, &map).is_none());
    repeat.configure(25, 0).unwrap();
    repeat.on_key(&press, &end).unwrap();
    assert!(repeat.poll(&end, &focus, &map).is_err());
    assert!(repeat.deadline(&focus, &map).is_none());
}

#[test]
fn repeat_late_wakeup_does_not_catch_up() {
    let (clock, focus, map, mut repeat) = setup();
    repeat.on_key(&key(&map, &focus, 30, true), &clock).unwrap();
    clock.advance(Duration::from_secs(3600)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_some());
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
    assert_eq!(
        repeat.deadline(&focus, &map).unwrap().remaining(&clock),
        Duration::from_millis(40)
    );
    clock.advance(Duration::from_millis(40)).unwrap();
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_some());
    repeat
        .on_key(&key(&map, &focus, 30, false), &clock)
        .unwrap();
    repeat.on_key(&key(&map, &focus, 16, true), &clock).unwrap();
    clock.advance(Duration::from_millis(40)).unwrap(); // A wakeup from the older hold.
    assert!(repeat.poll(&clock, &focus, &map).unwrap().is_none());
}
