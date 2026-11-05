//! Seat-scoped text-input-v3 capability state. No Wayland or IME I/O.

use crate::{
    clipboard::SeatId,
    wayland::{Binding, Capabilities, Protocol},
};

/// One capability snapshot per bound wl_seat; the adapter owns its lifetime.
/// Manager availability does not guarantee a running input method.
pub struct ImeState {
    seat: SeatId,
    manager: Option<Binding>,
}

impl ImeState {
    pub fn new(seat: SeatId, capabilities: &Capabilities) -> Self {
        Self {
            seat,
            manager: capabilities.binding(Protocol::TextInput, 1, 1),
        }
    }

    pub fn seat(&self) -> SeatId {
        self.seat
    }

    /// None means IME is unavailable; ordinary keyboard handling remains independent.
    pub fn manager_binding(&self) -> Option<Binding> {
        self.manager
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
