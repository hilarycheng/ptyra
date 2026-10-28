pub const APP_NAME: &str = "Ptyra";

/// A window ID cannot be used where a tab ID is required.
///
/// ```compile_fail
/// use ptyra::{TabId, WindowId};
/// let _: TabId = WindowId(1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    Wayland,
    Pty,
    Ime,
    Clipboard,
}

pub enum AppEvent {
    Wayland,
    Pty,
    Ime,
    Clipboard,
}

impl AppEvent {
    pub fn source(&self) -> EventSource {
        match self {
            Self::Wayland => EventSource::Wayland,
            Self::Pty => EventSource::Pty,
            Self::Ime => EventSource::Ime,
            Self::Clipboard => EventSource::Clipboard,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AppEffect {
    Route(EventSource),
}

pub fn reduce(event: &AppEvent) -> AppEffect {
    AppEffect::Route(event.source())
}

#[cfg(test)]
mod tests {
    use super::{AppEffect, AppEvent, EventSource, TabId, WindowId, reduce};
    use std::any::TypeId;

    #[test]
    fn workspace_smoke() {
        assert_eq!(crate::APP_NAME, "Ptyra");
    }

    #[test]
    fn typed_ids_are_distinct() {
        assert_ne!(TypeId::of::<TabId>(), TypeId::of::<WindowId>());
        assert_eq!(TabId(1), TabId(1));
        assert_ne!(TabId(1), TabId(2));
        assert_eq!(WindowId(1), WindowId(1));
        assert_ne!(WindowId(1), WindowId(2));
    }

    #[test]
    fn app_event_routes_by_source() {
        assert_eq!(AppEvent::Wayland.source(), EventSource::Wayland);
        assert_eq!(AppEvent::Pty.source(), EventSource::Pty);
        assert_eq!(AppEvent::Ime.source(), EventSource::Ime);
        assert_eq!(AppEvent::Clipboard.source(), EventSource::Clipboard);
    }

    #[test]
    fn reducer_emits_expected_effect() {
        let cases = [
            (AppEvent::Wayland, EventSource::Wayland),
            (AppEvent::Pty, EventSource::Pty),
            (AppEvent::Ime, EventSource::Ime),
            (AppEvent::Clipboard, EventSource::Clipboard),
        ];

        for (event, source) in cases {
            assert_eq!(reduce(&event), AppEffect::Route(source));
            assert_eq!(reduce(&event), AppEffect::Route(source));
        }
    }
}
