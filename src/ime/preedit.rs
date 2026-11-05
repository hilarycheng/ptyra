//! Persistent UI Preedit storage, separate from Commit bytes and terminal grid data.

use super::{ImeFocus, ImePreedit, ImeState, ImeUpdate};
use crate::{
    error::{AppError, Component},
    input::{FocusTarget, KeyboardFocus},
    renderer::overlay::Preedit,
};
use std::io::ErrorKind;

/// One terminal Tab's UI text. It owns no PTY handle and never interprets escape sequences.
pub struct PreeditState {
    target: FocusTarget,
    preedit: ImePreedit,
    focus: Option<ImeFocus>,
}

impl PreeditState {
    pub fn new(target: FocusTarget) -> Self {
        Self {
            target,
            preedit: ImePreedit::default(),
            focus: None,
        }
    }

    /// Move the completed batch's Preedit into UI state; leave Commit available for PTY routing.
    pub fn apply(
        &mut self,
        update: &mut ImeUpdate,
        ime: &ImeState,
        keyboard: &KeyboardFocus,
    ) -> Result<bool, AppError> {
        self.sync_focus(ime, keyboard);
        if update.target != self.target {
            return Err(AppError::new(Component::Ime, ErrorKind::InvalidInput));
        }
        if !update.is_current(ime, keyboard) {
            return Ok(false);
        }
        update.preedit.validate()?;
        self.preedit = std::mem::take(&mut update.preedit);
        self.focus = Some(update.focus.clone());
        Ok(true)
    }

    /// Call on focus/Tab changes to clear old UI state before the next render.
    pub fn sync_focus(&mut self, ime: &ImeState, keyboard: &KeyboardFocus) -> bool {
        if self
            .focus
            .as_ref()
            .is_some_and(|focus| !ime.matches_focus(focus, keyboard))
        {
            self.focus = None;
            self.preedit = ImePreedit::default();
            return true;
        }
        false
    }

    pub fn preedit(&mut self, ime: &ImeState, keyboard: &KeyboardFocus) -> &ImePreedit {
        self.sync_focus(ime, keyboard);
        &self.preedit
    }

    pub fn overlay(&mut self, ime: &ImeState, keyboard: &KeyboardFocus) -> Option<Preedit<'_>> {
        self.sync_focus(ime, keyboard);
        (!self.preedit.text.is_empty()).then_some(Preedit {
            tab: self.target.tab,
            text: &self.preedit.text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PreeditState;
    use crate::{
        TabId, WindowId,
        clipboard::SeatId,
        geometry::Scale,
        ime::{ImeEvent, ImePreedit, ImeState, MAX_IME_TEXT_BYTES},
        input::{FocusTarget, KeyboardFocus},
        renderer::{
            CellLayout,
            overlay::{OverlayCommand, overlay_commands},
        },
        terminal::Terminal,
        wayland::{Capabilities, Global},
        window::WindowSize,
    };
    use std::io::Write;

    #[test]
    fn ime_preedit_does_not_cross_tab() {
        let seat = SeatId(1);
        let window = WindowId(1);
        let first = FocusTarget {
            window,
            tab: TabId(7),
        };
        let second = FocusTarget {
            window,
            tab: TabId(8),
        };
        let caps = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(window, first.tab);
        let mut ime = ImeState::new(seat, &caps);
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        let mut a = PreeditState::new(first);
        let mut b = PreeditState::new(second);
        let old = ime.capture_focus(seat, window, &keyboard);
        let event = || ImeEvent::Preedit {
            text: Some("舊組字"),
            cursor_begin: 0,
            cursor_end: 9,
        };
        ime.stage(&old, &keyboard, event()).unwrap();
        let mut update = ime.done(&old, &keyboard, ime.commit_serial()).unwrap();
        assert!(a.apply(&mut update, &ime, &keyboard).unwrap());
        assert_eq!(a.overlay(&ime, &keyboard).unwrap().text, "舊組字");
        ime.stage(&old, &keyboard, event()).unwrap();
        let mut delayed = ime.done(&old, &keyboard, ime.commit_serial()).unwrap();

        keyboard.enter(window, second.tab);
        assert!(a.sync_focus(&ime, &keyboard));
        assert!(a.overlay(&ime, &keyboard).is_none());
        ime.sync_enabled(seat, &keyboard, true);
        assert!(b.apply(&mut delayed, &ime, &keyboard).is_err());
        assert!(!a.apply(&mut delayed, &ime, &keyboard).unwrap());
        let current = ime.capture_focus(seat, window, &keyboard);
        ime.stage(
            &current,
            &keyboard,
            ImeEvent::Preedit {
                text: Some("新"),
                cursor_begin: 0,
                cursor_end: 3,
            },
        )
        .unwrap();
        let mut update = ime.done(&current, &keyboard, ime.commit_serial()).unwrap();
        assert!(b.apply(&mut update, &ime, &keyboard).unwrap());
        assert_eq!(b.overlay(&ime, &keyboard).unwrap().text, "新");

        keyboard.enter(window, first.tab);
        ime.sync_enabled(seat, &keyboard, true);
        // The render accessor also clears stale state if the focus notification was delayed.
        assert!(b.overlay(&ime, &keyboard).is_none());
        assert!(a.overlay(&ime, &keyboard).is_none());
        assert!(!a.apply(&mut delayed, &ime, &keyboard).unwrap());
        let current = ime.capture_focus(seat, window, &keyboard);
        ime.stage(&current, &keyboard, event()).unwrap();
        let mut update = ime.done(&current, &keyboard, ime.commit_serial()).unwrap();
        assert!(a.apply(&mut update, &ime, &keyboard).unwrap());
        assert!(!a.apply(&mut delayed, &ime, &keyboard).unwrap());
        assert_eq!(a.overlay(&ime, &keyboard).unwrap().text, "舊組字");
        keyboard.leave(window);
        assert!(a.overlay(&ime, &keyboard).is_none());
        assert!(a.preedit(&ime, &keyboard).text.is_empty());
    }

    #[test]
    fn preedit_never_writes_to_pty() {
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(7),
        };
        let capabilities = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let seat = SeatId(1);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(target.window, target.tab);
        let mut ime = ImeState::new(seat, &capabilities);
        ime.enter(seat, target.window);
        ime.sync_enabled(seat, &keyboard, true);
        let mut ui = PreeditState::new(target);
        let mut terminal = Terminal::new(target.tab, 2, 16).unwrap();
        terminal.feed(target.tab, b"prompt> ").unwrap();
        let before: Vec<_> = terminal
            .grid()
            .unwrap()
            .display_iter()
            .map(|cell| cell.cell.clone())
            .collect();
        let cursor = terminal.grid().unwrap().cursor.clone();
        let layout = CellLayout::new(
            WindowSize::new(160, 20).unwrap(),
            Scale::new(1).unwrap(),
            10.0,
            10.0,
        )
        .unwrap();
        let mut fake_pty = Vec::new();
        assert!(ui.overlay(&ime, &keyboard).is_none());
        for text in [
            Some("繁體字"),
            Some("更新e\u{301}"),
            Some("\x1b[31m"),
            Some(""),
            None,
        ] {
            let previous = ui.preedit(&ime, &keyboard).text.clone();
            ime.stage(
                &ime.capture_focus(seat, target.window, &keyboard),
                &keyboard,
                ImeEvent::Preedit {
                    text,
                    cursor_begin: 0,
                    cursor_end: text.unwrap_or_default().len() as i32,
                },
            )
            .unwrap();
            assert_eq!(ui.preedit(&ime, &keyboard).text, previous);
            let mut update = ime
                .done(
                    &ime.capture_focus(seat, target.window, &keyboard),
                    &keyboard,
                    ime.commit_serial(),
                )
                .unwrap();
            ui.apply(&mut update, &ime, &keyboard).unwrap();
            // Fake downstream PTY consumer receives only the batch's Commit field.
            fake_pty.write_all(update.commit.as_bytes()).unwrap();
            assert!(fake_pty.is_empty());
            assert_eq!(ui.preedit(&ime, &keyboard).text, text.unwrap_or_default());
            let commands: Vec<_> = overlay_commands(
                terminal.snapshot().unwrap(),
                &layout,
                ui.overlay(&ime, &keyboard),
            )
            .unwrap()
            .collect();
            match text.filter(|text| !text.is_empty()) {
                Some(expected) => {
                    assert_eq!(commands.len(), 1);
                    let OverlayCommand::Preedit { text, .. } = &commands[0] else {
                        panic!("expected Preedit overlay")
                    };
                    assert_eq!(*text, expected);
                }
                None => assert!(commands.is_empty()),
            }
            assert_eq!(terminal.grid().unwrap().cursor, cursor);
            assert_eq!(
                terminal
                    .grid()
                    .unwrap()
                    .display_iter()
                    .map(|cell| cell.cell.clone())
                    .collect::<Vec<_>>(),
                before
            );
        }
        ime.stage(
            &ime.capture_focus(seat, target.window, &keyboard),
            &keyboard,
            ImeEvent::Preedit {
                text: Some("候選"),
                cursor_begin: -1,
                cursor_end: -1,
            },
        )
        .unwrap();
        ime.stage(
            &ime.capture_focus(seat, target.window, &keyboard),
            &keyboard,
            ImeEvent::Commit {
                text: Some("確定")
            },
        )
        .unwrap();
        let mut update = ime
            .done(
                &ime.capture_focus(seat, target.window, &keyboard),
                &keyboard,
                ime.commit_serial(),
            )
            .unwrap();
        ui.apply(&mut update, &ime, &keyboard).unwrap();
        fake_pty.write_all(update.commit.as_bytes()).unwrap();
        assert_eq!(fake_pty, "確定".as_bytes());
        assert_eq!(ui.preedit(&ime, &keyboard).text, "候選");
        assert_eq!(ui.preedit(&ime, &keyboard).cursor_begin, -1);
        for wrong in [
            FocusTarget {
                tab: TabId(8),
                ..target
            },
            FocusTarget {
                window: WindowId(2),
                ..target
            },
        ] {
            update.target = wrong;
            assert!(ui.apply(&mut update, &ime, &keyboard).is_err());
            assert_eq!(ui.preedit(&ime, &keyboard).text, "候選");
        }
        update.target = target;
        for invalid in [
            ImePreedit {
                text: "x".repeat(MAX_IME_TEXT_BYTES + 1),
                cursor_begin: 0,
                cursor_end: 0,
            },
            ImePreedit {
                text: "中".into(),
                cursor_begin: 1,
                cursor_end: 3,
            },
        ] {
            update.preedit = invalid;
            assert!(ui.apply(&mut update, &ime, &keyboard).is_err());
            assert_eq!(ui.preedit(&ime, &keyboard).text, "候選");
        }
        let mut empty = ime
            .done(
                &ime.capture_focus(seat, target.window, &keyboard),
                &keyboard,
                ime.commit_serial(),
            )
            .unwrap();
        ui.apply(&mut empty, &ime, &keyboard).unwrap();
        assert!(ui.overlay(&ime, &keyboard).is_none());
        assert!(empty.commit.is_empty());
    }
}
