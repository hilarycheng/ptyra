pub mod clipboard;
pub mod clock;
pub mod config;
pub mod damage;
pub mod error;
pub mod font;
pub mod frame;
pub mod geometry;
pub mod hyperlink;
pub mod ime;
pub mod input;
pub mod install;
pub mod keymap;
pub mod kitty;
#[cfg(target_os = "linux")]
pub mod pty;
pub mod renderer;
pub mod repeat;
pub mod selection;
pub mod shell;
pub mod tabs;
pub mod terminal;
pub mod wayland;
pub mod window;

pub const APP_NAME: &str = "Ptyra";

#[cfg(test)]
mod fuzz_tests;
#[cfg(test)]
mod policy_tests;

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
    WaylandDisconnected(std::io::ErrorKind),
    Pty,
    PtyExited {
        tab: TabId,
        status: std::process::ExitStatus,
    },
    Ime,
    Clipboard,
}

impl AppEvent {
    pub fn source(&self) -> EventSource {
        match self {
            Self::Wayland | Self::WaylandDisconnected(_) => EventSource::Wayland,
            Self::Pty | Self::PtyExited { .. } => EventSource::Pty,
            Self::Ime => EventSource::Ime,
            Self::Clipboard => EventSource::Clipboard,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AppEffect {
    Route(EventSource),
    StopRendering(error::AppError),
    /// Records this Tab's child exit; does not close its PTY or stop the app.
    PtyExited {
        tab: TabId,
        status: std::process::ExitStatus,
    },
}

pub fn reduce(event: &AppEvent) -> AppEffect {
    match event {
        AppEvent::WaylandDisconnected(kind) => {
            AppEffect::StopRendering(error::AppError::new(error::Component::Wayland, *kind))
        }
        AppEvent::PtyExited { tab, status } => AppEffect::PtyExited {
            tab: *tab,
            status: *status,
        },
        _ => AppEffect::Route(event.source()),
    }
}

#[cfg(test)]
mod tests {
    use super::{AppEffect, AppEvent, EventSource, TabId, WindowId, reduce};
    use std::any::TypeId;

    #[test]
    fn disconnect_reports_typed_error() {
        use crate::{
            clock::FakeClock,
            damage::Rect,
            error::{AppError, Component},
            frame::FrameScheduler,
            window::WindowSize,
        };
        use std::io::ErrorKind;
        let clock = FakeClock::default();
        let mut frames = FrameScheduler::new(WindowId(1), WindowSize::new(100, 100).unwrap());
        let dirty = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        frames.damage(dirty);
        let pending = frames.schedule(&clock).unwrap().unwrap();
        let event = AppEvent::WaylandDisconnected(ErrorKind::BrokenPipe);
        assert_eq!(event.source(), EventSource::Wayland);
        let effect = reduce(&event);
        let error = AppError::new(Component::Wayland, ErrorKind::BrokenPipe);
        assert_eq!(effect, AppEffect::StopRendering(error));
        assert_eq!(frames.apply_effect(&effect), Some(error));
        assert!(!frames.frame_done(pending.callback));
        frames.damage(dirty);
        frames.resize(WindowSize::new(200, 200).unwrap());
        assert!(frames.schedule(&clock).unwrap().is_none());
        assert_eq!(frames.apply_effect(&effect), None);
        assert_eq!(
            frames.apply_effect(&AppEffect::Route(EventSource::Wayland)),
            None
        );
        assert!(frames.schedule(&clock).unwrap().is_none());
    }

    #[derive(Default)]
    struct FakeWaylandAdapter {
        calls: Vec<AppEffect>,
    }

    impl FakeWaylandAdapter {
        fn record(&mut self, effect: AppEffect) {
            self.calls.push(effect);
        }
    }

    #[derive(Default)]
    struct FakePtyAdapter {
        calls: Vec<AppEffect>,
    }

    impl FakePtyAdapter {
        fn record(&mut self, effect: AppEffect) {
            self.calls.push(effect);
        }
    }

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

    #[test]
    fn fake_adapter_records_calls() {
        let mut wayland = FakeWaylandAdapter::default();
        let mut pty = FakePtyAdapter::default();

        wayland.record(reduce(&AppEvent::Wayland));
        pty.record(reduce(&AppEvent::Pty));
        wayland.record(reduce(&AppEvent::Clipboard));

        assert_eq!(
            wayland.calls,
            [
                AppEffect::Route(EventSource::Wayland),
                AppEffect::Route(EventSource::Clipboard),
            ]
        );
        assert_eq!(pty.calls, [AppEffect::Route(EventSource::Pty)]);
    }
}
