//! Seat-scoped text-input-v3 capability and focus state. No Wayland or IME I/O.

use crate::{
    WindowId,
    clipboard::SeatId,
    input::{FocusTarget, KeyboardFocus},
    wayland::{Binding, Capabilities, Protocol},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeRequest {
    Enable,
    Disable,
    Commit,
}

/// One capability snapshot per bound wl_seat; the adapter owns its lifetime.
/// Manager availability does not guarantee a running input method.
pub struct ImeState {
    seat: SeatId,
    manager: Option<Binding>,
    surface: Option<WindowId>,
    enabled_target: Option<FocusTarget>,
}

impl ImeState {
    pub fn new(seat: SeatId, capabilities: &Capabilities) -> Self {
        Self {
            seat,
            manager: capabilities.binding(Protocol::TextInput, 1, 1),
            surface: None,
            enabled_target: None,
        }
    }

    pub fn seat(&self) -> SeatId {
        self.seat
    }

    /// None means IME is unavailable; ordinary keyboard handling remains independent.
    pub fn manager_binding(&self) -> Option<Binding> {
        self.manager
    }

    /// The adapter maps a text-input-v3 enter surface to its owning WindowId.
    pub fn enter(&mut self, seat: SeatId, surface: WindowId) -> bool {
        if seat != self.seat || self.manager.is_none() {
            return false;
        }
        self.surface = Some(surface);
        // Every enter invalidates the compositor's text-input state.
        self.enabled_target = None;
        true
    }

    pub fn leave(&mut self, seat: SeatId, surface: WindowId) -> bool {
        if seat != self.seat || self.surface != Some(surface) {
            return false;
        }
        self.surface = None;
        self.enabled_target = None;
        true
    }

    /// Use the keyboard state belonging to this seat; both protocols must focus the same surface.
    /// This checks current focus only, not the epoch of a queued IME transaction.
    pub fn target_for(
        &self,
        seat: SeatId,
        surface: WindowId,
        keyboard: &KeyboardFocus,
    ) -> Option<FocusTarget> {
        if seat != self.seat || self.surface != Some(surface) {
            return None;
        }
        keyboard.target().filter(|target| target.window == surface)
    }

    /// Send the complete returned sequence on this seat's text-input object before processing more events.
    /// Call after focus changes; enabled=false also covers focus on non-editable UI.
    pub fn sync_enabled(
        &mut self,
        seat: SeatId,
        keyboard: &KeyboardFocus,
        enabled: bool,
    ) -> &'static [ImeRequest] {
        use ImeRequest::{Commit, Disable, Enable};

        if seat != self.seat || self.manager.is_none() {
            return &[];
        }
        let target = if enabled {
            self.surface
                .and_then(|surface| self.target_for(seat, surface, keyboard))
        } else {
            None
        };
        if target == self.enabled_target {
            return &[];
        }
        let requests: &'static [ImeRequest] = match (self.enabled_target, target) {
            (None, Some(_)) => &[Enable, Commit],
            (Some(_), None) => &[Disable, Commit],
            (Some(_), Some(_)) => &[Disable, Commit, Enable, Commit],
            (None, None) => &[],
        };
        self.enabled_target = target;
        requests
    }
}

#[cfg(test)]
mod tests {
    use super::ImeState;
    use crate::{
        TabId, WindowId,
        clipboard::SeatId,
        input::KeyboardFocus,
        wayland::{Binding, Capabilities, Global},
    };

    #[test]
    fn ime_enable_disable_requests_ordered() {
        use super::ImeRequest::{Commit, Disable, Enable};

        let capabilities = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let seat = SeatId(1);
        let window = WindowId(1);
        let mut keyboard = KeyboardFocus::default();
        let mut ime = ImeState::new(seat, &capabilities);
        let mut unavailable = ImeState::new(seat, &Capabilities::default());
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
        keyboard.enter(window, TabId(7));
        assert!(unavailable.sync_enabled(seat, &keyboard, true).is_empty());
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
        assert!(ime.enter(seat, window));
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Enable, Commit]);
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
        assert!(ime.sync_enabled(SeatId(2), &keyboard, false).is_empty());
        assert!(!ime.leave(seat, WindowId(2)));
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());

        let mut other = ImeState::new(SeatId(2), &capabilities);
        assert!(other.enter(SeatId(2), window));
        assert_eq!(
            other.sync_enabled(SeatId(2), &keyboard, true),
            [Enable, Commit]
        );
        keyboard.enter(window, TabId(8));
        assert_eq!(
            ime.sync_enabled(seat, &keyboard, true),
            [Disable, Commit, Enable, Commit]
        );
        assert_eq!(ime.sync_enabled(seat, &keyboard, false), [Disable, Commit]);
        assert!(ime.sync_enabled(seat, &keyboard, false).is_empty());
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Enable, Commit]);

        // Keyboard leave can precede IME leave; disable while the surface is still entered.
        keyboard.leave(window);
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Disable, Commit]);
        assert!(ime.leave(seat, window));
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
        assert!(ime.enter(seat, window));
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
        keyboard.enter(window, TabId(7));
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Enable, Commit]);
        assert!(other.sync_enabled(SeatId(2), &keyboard, true).is_empty());

        // After IME leave, requests are ignored by the compositor until the next enter.
        assert!(ime.leave(seat, window));
        assert!(ime.sync_enabled(seat, &keyboard, false).is_empty());
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
        assert!(ime.enter(seat, window));
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Enable, Commit]);
        // Each enter invalidates state, even when the WindowId is unchanged.
        assert!(ime.enter(seat, window));
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Enable, Commit]);
        keyboard.enter(WindowId(2), TabId(9));
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Disable, Commit]);
        assert!(ime.enter(seat, WindowId(2)));
        assert_eq!(ime.sync_enabled(seat, &keyboard, true), [Enable, Commit]);
        assert!(!ime.leave(seat, window));
        assert!(ime.sync_enabled(seat, &keyboard, true).is_empty());
    }

    #[test]
    fn ime_focus_matches_surface() {
        let capabilities = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let seat = SeatId(1);
        let other_seat = SeatId(2);
        let first = WindowId(1);
        let second = WindowId(2);
        let mut ime = ImeState::new(seat, &capabilities);
        let mut other_ime = ImeState::new(other_seat, &capabilities);
        let mut keyboard = KeyboardFocus::default();
        let mut other_keyboard = KeyboardFocus::default();
        let mut unavailable = ImeState::new(seat, &Capabilities::default());
        assert!(!unavailable.enter(seat, first));
        assert!(!unavailable.leave(seat, first));
        assert!(unavailable.target_for(seat, first, &keyboard).is_none());
        assert!(!ime.enter(other_seat, first));
        assert!(ime.target_for(seat, first, &keyboard).is_none());

        // IME enter may arrive before keyboard enter.
        assert!(ime.enter(seat, first));
        assert!(ime.target_for(seat, first, &keyboard).is_none());
        keyboard.enter(first, TabId(7));
        assert_eq!(ime.target_for(seat, first, &keyboard), keyboard.target());
        assert!(ime.target_for(other_seat, first, &keyboard).is_none());
        assert!(ime.target_for(seat, second, &keyboard).is_none());
        assert!(!ime.enter(other_seat, second));
        assert!(!ime.leave(other_seat, first));
        assert!(!ime.leave(seat, second));
        assert_eq!(ime.target_for(seat, first, &keyboard), keyboard.target());

        other_keyboard.enter(second, TabId(8));
        assert!(other_ime.enter(other_seat, second));
        assert_eq!(
            other_ime.target_for(other_seat, second, &other_keyboard),
            other_keyboard.target()
        );

        // Keyboard enter may also arrive first; old-surface events remain blocked.
        keyboard.enter(second, TabId(9));
        assert!(ime.target_for(seat, first, &keyboard).is_none());
        assert!(ime.target_for(seat, second, &keyboard).is_none());
        assert!(ime.enter(seat, second));
        assert!(!ime.leave(seat, first));
        assert_eq!(ime.target_for(seat, second, &keyboard), keyboard.target());
        keyboard.leave(second);
        assert!(ime.target_for(seat, second, &keyboard).is_none());
        assert!(ime.leave(seat, second));
        assert!(!ime.leave(seat, second));
        keyboard.enter(second, TabId(9));
        assert!(ime.target_for(seat, second, &keyboard).is_none());
        assert_eq!(
            other_ime.target_for(other_seat, second, &other_keyboard),
            other_keyboard.target()
        );
        assert!(ime.enter(seat, second));
        assert!(ime.leave(seat, second));
        assert!(ime.target_for(seat, second, &keyboard).is_none());
        assert_eq!(keyboard.route_key(30, true).unwrap().target.tab, TabId(9));
    }

    #[test]
    fn ime_capability_does_not_require_gtk() {
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(WindowId(1), TabId(7));
        let seats = [
            Global {
                name: 1,
                interface: "wl_seat",
                version: 9,
            },
            Global {
                name: 2,
                interface: "wl_seat",
                version: 9,
            },
        ];
        let absent = Capabilities::from_globals(&seats);
        let mut first = ImeState::new(SeatId(1), &absent);
        let second = ImeState::new(SeatId(2), &absent);
        assert_eq!(first.seat(), SeatId(1));
        assert_eq!(second.seat(), SeatId(2));
        assert_eq!(first.manager_binding(), None);
        assert_eq!(second.manager_binding(), None);

        // Mock registry additions: only the v3 interface with a usable version qualifies.
        for (interface, version, expected) in [
            ("zwp_text_input_manager_v1", 1, None),
            ("zwp_text_input_manager_v2", 1, None),
            ("zwp_input_method_manager_v2", 1, None),
            ("zwp_text_input_manager_v3", 0, None),
            (
                "zwp_text_input_manager_v3",
                1,
                Some(Binding {
                    name: 3,
                    version: 1,
                }),
            ),
            (
                "zwp_text_input_manager_v3",
                4,
                Some(Binding {
                    name: 3,
                    version: 1,
                }),
            ),
        ] {
            let globals = [
                seats[0],
                seats[1],
                Global {
                    name: 3,
                    interface,
                    version,
                },
            ];
            let capabilities = Capabilities::from_globals(&globals);
            first = ImeState::new(SeatId(1), &capabilities);
            assert_eq!(first.seat(), SeatId(1));
            assert_eq!(first.manager_binding(), expected);
            assert_eq!(second.manager_binding(), None);
            let available_second = ImeState::new(SeatId(2), &capabilities);
            assert_eq!(available_second.seat(), SeatId(2));
            assert_eq!(available_second.manager_binding(), expected);

            // Removing the manager only changes the reconstructed capability snapshot.
            first = ImeState::new(SeatId(1), &absent);
            assert_eq!(first.manager_binding(), None);
            assert_eq!(available_second.manager_binding(), expected);
            let key = keyboard.route_key(30, true).unwrap();
            assert_eq!(key.target.tab, TabId(7));
            assert_eq!(key.code, 30);
            assert!(key.pressed);
            assert!(!keyboard.route_key(30, false).unwrap().pressed);
        }
    }
}
