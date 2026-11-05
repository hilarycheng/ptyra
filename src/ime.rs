//! Seat-scoped text-input-v3 capability and focus state. No Wayland or IME I/O.

pub mod cursor;
pub mod preedit;

use crate::{
    WindowId,
    clipboard::{PasteSink, SeatId},
    error::{AppError, Component},
    input::{FocusTarget, KeyboardFocus},
    wayland::{Binding, Capabilities, Protocol},
};
use std::io::ErrorKind;

/// Per Preedit or Commit value; repeated events replace, rather than append, pending text.
pub const MAX_IME_TEXT_BYTES: usize = 4096;

/// Sensitive event payloads deliberately have no Debug implementation.
pub enum ImeEvent<'a> {
    Preedit {
        text: Option<&'a str>,
        cursor_begin: i32,
        cursor_end: i32,
    },
    Commit {
        text: Option<&'a str>,
    },
    DeleteSurroundingText {
        before_length: u32,
        after_length: u32,
    },
}

#[derive(Default)]
pub struct ImePreedit {
    pub text: String,
    pub cursor_begin: i32,
    pub cursor_end: i32,
}

impl ImePreedit {
    fn validate(&self) -> Result<(), AppError> {
        if self.text.len() > MAX_IME_TEXT_BYTES {
            return Err(AppError::new(Component::Ime, ErrorKind::FileTooLarge));
        }
        let valid_cursor = (self.cursor_begin == -1 && self.cursor_end == -1)
            || [self.cursor_begin, self.cursor_end]
                .into_iter()
                .all(|index| {
                    usize::try_from(index).is_ok_and(|index| self.text.is_char_boundary(index))
                });
        if !valid_cursor {
            return Err(AppError::new(Component::Ime, ErrorKind::InvalidData));
        }
        Ok(())
    }
}

/// One complete done batch, consumed by later overlay/PTY adapters. No payload Debug or I/O.
pub struct ImeUpdate {
    pub target: FocusTarget,
    pub serial: u32,
    /// False still permits applying text, but not updating the compositor-facing state.
    pub state_is_current: bool,
    pub preedit: ImePreedit,
    pub commit: String,
}

impl ImeUpdate {
    /// Queue literal UTF-8 once; retain this update on error and retry when the sink can accept it.
    /// False means empty/already queued or wrong Tab. PTY flushing owns partial-write retries.
    pub fn enqueue_commit<S: PasteSink>(&mut self, sink: &mut S) -> Result<bool, S::Error> {
        if self.commit.is_empty() || sink.tab() != self.target.tab {
            return Ok(false);
        }
        sink.enqueue(self.commit.as_bytes())?;
        self.commit.clear();
        Ok(true)
    }
}

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
    commit_serial: u32,
    pending_preedit: ImePreedit,
    pending_commit: String,
    invalid_batch: bool,
}

impl ImeState {
    pub fn new(seat: SeatId, capabilities: &Capabilities) -> Self {
        Self {
            seat,
            manager: capabilities.binding(Protocol::TextInput, 1, 1),
            surface: None,
            enabled_target: None,
            commit_serial: 0,
            pending_preedit: ImePreedit::default(),
            pending_commit: String::new(),
            invalid_batch: false,
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
        self.clear_pending();
        true
    }

    pub fn leave(&mut self, seat: SeatId, surface: WindowId) -> bool {
        if seat != self.seat || self.surface != Some(surface) {
            return false;
        }
        self.surface = None;
        self.enabled_target = None;
        self.clear_pending();
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
        self.clear_pending();
        self.commit_serial = self.commit_serial.wrapping_add(
            requests
                .iter()
                .filter(|request| matches!(request, ImeRequest::Commit))
                .count() as u32,
        );
        requests
    }

    pub fn commit_serial(&self) -> u32 {
        self.commit_serial
    }

    /// Only stage text for the enabled target; no partial batch is exposed to consumers.
    /// Invalid data discards the entire batch until done, without recording its contents.
    pub fn stage(
        &mut self,
        seat: SeatId,
        surface: WindowId,
        keyboard: &KeyboardFocus,
        event: ImeEvent<'_>,
    ) -> Result<bool, AppError> {
        let Some(target) = self.target_for(seat, surface, keyboard) else {
            return Ok(false);
        };
        if self.enabled_target != Some(target) || self.invalid_batch {
            return Ok(false);
        }
        let result = match event {
            // No editable surrounding text is advertised; never infer edits from terminal output.
            ImeEvent::DeleteSurroundingText { .. } => return Ok(false),
            ImeEvent::Commit { text } => copy_ime_text(text).map(|text| self.pending_commit = text),
            ImeEvent::Preedit {
                text,
                cursor_begin,
                cursor_end,
            } => copy_ime_text(text).and_then(|text| {
                let preedit = ImePreedit {
                    text,
                    cursor_begin,
                    cursor_end,
                };
                preedit.validate()?;
                self.pending_preedit = preedit;
                Ok(())
            }),
        };
        if let Err(error) = result {
            self.clear_pending();
            self.invalid_batch = true;
            return Err(error);
        }
        Ok(true)
    }

    /// All values reset after done; absent text events mean empty values, including clearing Preedit.
    /// A done serial can repeat across batches and is not a text-event deduplication key.
    pub fn done(
        &mut self,
        seat: SeatId,
        surface: WindowId,
        keyboard: &KeyboardFocus,
        serial: u32,
    ) -> Option<ImeUpdate> {
        let target = self.target_for(seat, surface, keyboard)?;
        if self.enabled_target != Some(target) {
            return None;
        }
        if self.invalid_batch {
            self.clear_pending();
            return None;
        }
        Some(ImeUpdate {
            target,
            serial,
            state_is_current: serial == self.commit_serial,
            preedit: std::mem::take(&mut self.pending_preedit),
            commit: std::mem::take(&mut self.pending_commit),
        })
    }

    fn clear_pending(&mut self) {
        self.pending_preedit = ImePreedit::default();
        self.pending_commit = String::new();
        self.invalid_batch = false;
    }
}

fn copy_ime_text(text: Option<&str>) -> Result<String, AppError> {
    let text = text.unwrap_or_default();
    if text.len() > MAX_IME_TEXT_BYTES {
        return Err(AppError::new(Component::Ime, ErrorKind::FileTooLarge));
    }
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| AppError::new(Component::Ime, ErrorKind::OutOfMemory))?;
    owned.push_str(text);
    Ok(owned)
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
    fn ime_delete_does_not_inject_backspace() {
        use super::ImeEvent;
        use crate::{clipboard::PasteSink, terminal::Terminal};

        #[derive(Default)]
        struct Sink(Vec<u8>);
        impl PasteSink for Sink {
            type Error = std::convert::Infallible;
            fn tab(&self) -> TabId {
                TabId(7)
            }
            fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
        }

        let seat = SeatId(1);
        let window = WindowId(1);
        let capabilities = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(window, TabId(7));
        let mut ime = ImeState::new(seat, &capabilities);
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        let mut sink = Sink::default();
        let mut terminal = Terminal::new(TabId(7), 2, 16).unwrap();
        terminal.feed(TabId(7), "prompt> 中文".as_bytes()).unwrap();
        let cells = || {
            terminal
                .grid()
                .unwrap()
                .display_iter()
                .map(|cell| cell.cell.clone())
                .collect::<Vec<_>>()
        };
        let before = cells();
        let cursor = terminal.grid().unwrap().cursor.clone();
        for (before_length, after_length) in [(0, 0), (1, 0), (0, 3), (3, 6), (u32::MAX, u32::MAX)]
        {
            for mixed in [false, true] {
                if mixed {
                    ime.stage(
                        seat,
                        window,
                        &keyboard,
                        ImeEvent::Commit {
                            text: Some("確定")
                        },
                    )
                    .unwrap();
                    ime.stage(
                        seat,
                        window,
                        &keyboard,
                        ImeEvent::Preedit {
                            text: Some("候選"),
                            cursor_begin: 0,
                            cursor_end: 6,
                        },
                    )
                    .unwrap();
                }
                // Repeated deletion requests neither generate bytes nor erase staged text.
                for _ in 0..2 {
                    assert!(
                        !ime.stage(
                            seat,
                            window,
                            &keyboard,
                            ImeEvent::DeleteSurroundingText {
                                before_length,
                                after_length,
                            }
                        )
                        .unwrap()
                    );
                }
                assert!(sink.0.is_empty());
                let mut update = ime
                    .done(seat, window, &keyboard, ime.commit_serial())
                    .unwrap();
                assert_eq!(update.preedit.text, if mixed { "候選" } else { "" });
                assert_eq!(update.enqueue_commit(&mut sink).unwrap(), mixed);
                assert_eq!(sink.0, if mixed { "確定".as_bytes() } else { b"" });
                assert!(!update.enqueue_commit(&mut sink).unwrap());
                sink.0.clear();
                let mut empty = ime
                    .done(seat, window, &keyboard, ime.commit_serial())
                    .unwrap();
                assert!(!empty.enqueue_commit(&mut sink).unwrap());
                assert!(sink.0.is_empty());
                assert_eq!(cells(), before);
                assert_eq!(terminal.grid().unwrap().cursor, cursor);
            }
        }
    }

    #[test]
    fn ime_done_applies_staged_events() {
        use super::{ImeEvent, MAX_IME_TEXT_BYTES};
        use crate::error::{AppError, Component};
        use std::io::ErrorKind;

        let capabilities = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let seat = SeatId(1);
        let window = WindowId(1);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(window, TabId(7));
        let mut ime = ImeState::new(seat, &capabilities);
        let commit = |text| ImeEvent::Commit { text };
        assert!(
            !ime.stage(seat, window, &keyboard, commit(Some("ignored")))
                .unwrap()
        );
        assert!(ime.done(seat, window, &keyboard, 0).is_none());
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        assert_eq!(ime.commit_serial(), 1);
        assert!(
            ime.stage(seat, window, &keyboard, commit(Some("舊")))
                .unwrap()
        );
        assert!(
            ime.stage(
                seat,
                window,
                &keyboard,
                ImeEvent::Preedit {
                    text: Some("組字"),
                    cursor_begin: 0,
                    cursor_end: 6,
                }
            )
            .unwrap()
        );
        assert!(
            ime.stage(seat, window, &keyboard, commit(Some("中文\n")))
                .unwrap()
        );
        assert!(
            !ime.stage(SeatId(2), window, &keyboard, commit(Some("wrong seat")))
                .unwrap()
        );
        assert!(
            !ime.stage(seat, WindowId(2), &keyboard, commit(Some("wrong surface")))
                .unwrap()
        );
        assert!(ime.done(SeatId(2), window, &keyboard, 1).is_none());
        let update = ime.done(seat, window, &keyboard, 1).unwrap();
        assert_eq!(update.target, keyboard.target().unwrap());
        assert_eq!(update.serial, 1);
        assert!(update.state_is_current);
        assert_eq!(update.commit, "中文\n");
        assert_eq!(update.preedit.text, "組字");
        assert_eq!(
            (update.preedit.cursor_begin, update.preedit.cursor_end),
            (0, 6)
        );
        let empty = ime.done(seat, window, &keyboard, 1).unwrap();
        assert!(empty.commit.is_empty());
        assert!(empty.preedit.text.is_empty());
        assert!(
            ime.stage(seat, window, &keyboard, commit(Some("下一批")))
                .unwrap()
        );
        let older_serial = ime.done(seat, window, &keyboard, 0).unwrap();
        assert_eq!(older_serial.commit, "下一批");
        assert!(!older_serial.state_is_current);
        assert_eq!(ime.commit_serial(), 1);

        ime.stage(
            seat,
            window,
            &keyboard,
            ImeEvent::Preedit {
                text: None,
                cursor_begin: -1,
                cursor_end: -1,
            },
        )
        .unwrap();
        ime.stage(seat, window, &keyboard, commit(None)).unwrap();
        let cleared = ime.done(seat, window, &keyboard, 1).unwrap();
        assert!(cleared.preedit.text.is_empty());
        assert!(cleared.commit.is_empty());
        assert_eq!(cleared.preedit.cursor_begin, -1);

        let limit = "a".repeat(MAX_IME_TEXT_BYTES);
        ime.stage(seat, window, &keyboard, commit(Some(&limit)))
            .unwrap();
        ime.stage(
            seat,
            window,
            &keyboard,
            ImeEvent::Preedit {
                text: Some(&limit),
                cursor_begin: 0,
                cursor_end: MAX_IME_TEXT_BYTES as i32,
            },
        )
        .unwrap();
        let bounded = ime.done(seat, window, &keyboard, 1).unwrap();
        assert_eq!(bounded.commit, limit);
        assert_eq!(bounded.preedit.text, limit);
        for (begin, end) in [(1, 3), (0, 4), (-1, 0), (-2, -2)] {
            ime.stage(seat, window, &keyboard, commit(Some("must not escape")))
                .unwrap();
            assert_eq!(
                ime.stage(
                    seat,
                    window,
                    &keyboard,
                    ImeEvent::Preedit {
                        text: Some("中"),
                        cursor_begin: begin,
                        cursor_end: end,
                    }
                ),
                Err(AppError::new(Component::Ime, ErrorKind::InvalidData))
            );
            assert!(
                !ime.stage(seat, window, &keyboard, commit(Some("still invalid")))
                    .unwrap()
            );
            assert!(ime.done(seat, window, &keyboard, 1).is_none());
        }
        let oversized = "a".repeat(MAX_IME_TEXT_BYTES + 1);
        for event in [
            commit(Some(&oversized)),
            ImeEvent::Preedit {
                text: Some(&oversized),
                cursor_begin: 0,
                cursor_end: 0,
            },
        ] {
            assert_eq!(
                ime.stage(seat, window, &keyboard, event),
                Err(AppError::new(Component::Ime, ErrorKind::FileTooLarge))
            );
            assert!(ime.done(seat, window, &keyboard, 1).is_none());
        }
        ime.stage(seat, window, &keyboard, commit(Some("cancelled")))
            .unwrap();
        keyboard.enter(window, TabId(8));
        assert!(ime.done(seat, window, &keyboard, 1).is_none());
        ime.sync_enabled(seat, &keyboard, true);
        assert_eq!(ime.commit_serial(), 3);
        assert!(
            ime.done(seat, window, &keyboard, 3)
                .unwrap()
                .commit
                .is_empty()
        );
        ime.stage(seat, window, &keyboard, commit(Some("cancelled")))
            .unwrap();
        ime.leave(seat, window);
        assert!(ime.done(seat, window, &keyboard, 3).is_none());
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        assert!(
            ime.done(seat, window, &keyboard, 4)
                .unwrap()
                .commit
                .is_empty()
        );
        ime.stage(seat, window, &keyboard, commit(Some("disabled")))
            .unwrap();
        ime.sync_enabled(seat, &keyboard, false);
        assert!(ime.done(seat, window, &keyboard, 5).is_none());
        ime.sync_enabled(seat, &keyboard, true);
        assert!(
            ime.done(seat, window, &keyboard, 6)
                .unwrap()
                .commit
                .is_empty()
        );
        let mut other = ImeState::new(SeatId(2), &capabilities);
        other.enter(SeatId(2), window);
        other.sync_enabled(SeatId(2), &keyboard, true);
        other
            .stage(SeatId(2), window, &keyboard, commit(Some("other seat")))
            .unwrap();
        ime.stage(seat, window, &keyboard, commit(Some("first seat")))
            .unwrap();
        assert_eq!(
            ime.done(seat, window, &keyboard, 6).unwrap().commit,
            "first seat"
        );
        assert_eq!(
            other.done(SeatId(2), window, &keyboard, 1).unwrap().commit,
            "other seat"
        );
        ime.commit_serial = u32::MAX;
        ime.sync_enabled(seat, &keyboard, false);
        assert_eq!(ime.commit_serial(), 0);
    }

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
