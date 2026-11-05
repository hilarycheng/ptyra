//! Persistent UI Preedit storage, separate from Commit bytes and terminal grid data.

use super::ImePreedit;
use crate::{
    error::{AppError, Component},
    input::FocusTarget,
    renderer::overlay::Preedit,
};
use std::io::ErrorKind;

/// One terminal Tab's UI text. It owns no PTY handle and never interprets escape sequences.
pub struct PreeditState {
    target: FocusTarget,
    preedit: ImePreedit,
}

impl PreeditState {
    pub fn new(target: FocusTarget) -> Self {
        Self {
            target,
            preedit: ImePreedit::default(),
        }
    }

    /// Apply only a completed, focus-validated ImeUpdate's Preedit field; keep Commit separate.
    pub fn apply(&mut self, target: FocusTarget, preedit: ImePreedit) -> Result<(), AppError> {
        if target != self.target {
            return Err(AppError::new(Component::Ime, ErrorKind::InvalidInput));
        }
        preedit.validate()?;
        self.preedit = preedit;
        Ok(())
    }

    pub fn preedit(&self) -> &ImePreedit {
        &self.preedit
    }

    pub fn overlay(&self) -> Option<Preedit<'_>> {
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
        assert!(ui.overlay().is_none());
        for text in [
            Some("繁體字"),
            Some("更新e\u{301}"),
            Some("\x1b[31m"),
            Some(""),
            None,
        ] {
            let previous = ui.preedit().text.clone();
            ime.stage(
                seat,
                target.window,
                &keyboard,
                ImeEvent::Preedit {
                    text,
                    cursor_begin: 0,
                    cursor_end: text.unwrap_or_default().len() as i32,
                },
            )
            .unwrap();
            assert_eq!(ui.preedit().text, previous);
            let update = ime
                .done(seat, target.window, &keyboard, ime.commit_serial())
                .unwrap();
            ui.apply(update.target, update.preedit).unwrap();
            // Fake downstream PTY consumer receives only the batch's Commit field.
            fake_pty.write_all(update.commit.as_bytes()).unwrap();
            assert!(fake_pty.is_empty());
            assert_eq!(ui.preedit().text, text.unwrap_or_default());
            let commands: Vec<_> =
                overlay_commands(terminal.snapshot().unwrap(), &layout, ui.overlay())
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
            seat,
            target.window,
            &keyboard,
            ImeEvent::Preedit {
                text: Some("候選"),
                cursor_begin: -1,
                cursor_end: -1,
            },
        )
        .unwrap();
        ime.stage(
            seat,
            target.window,
            &keyboard,
            ImeEvent::Commit {
                text: Some("確定")
            },
        )
        .unwrap();
        let update = ime
            .done(seat, target.window, &keyboard, ime.commit_serial())
            .unwrap();
        ui.apply(update.target, update.preedit).unwrap();
        fake_pty.write_all(update.commit.as_bytes()).unwrap();
        assert_eq!(fake_pty, "確定".as_bytes());
        assert_eq!(ui.preedit().text, "候選");
        assert_eq!(ui.preedit().cursor_begin, -1);
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
            assert!(ui.apply(wrong, ImePreedit::default()).is_err());
            assert_eq!(ui.preedit().text, "候選");
        }
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
            assert!(ui.apply(target, invalid).is_err());
            assert_eq!(ui.preedit().text, "候選");
        }
        let empty = ime
            .done(seat, target.window, &keyboard, ime.commit_serial())
            .unwrap();
        ui.apply(empty.target, empty.preedit).unwrap();
        assert!(ui.overlay().is_none());
        assert!(empty.commit.is_empty());
    }
}
