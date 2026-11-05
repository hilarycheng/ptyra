use super::{Tab, TabPty, TabRegistry};
use crate::{
    error::AppError,
    ime::{ImeState, ImeUpdate, preedit::PreeditState},
    input::KeyboardFocus,
    renderer::overlay::Preedit,
};

#[derive(Debug)]
pub enum ImeRouteError<E> {
    Preedit(AppError),
    Pty(E),
}

impl<P> Tab<P> {
    pub fn preedit_overlay(
        &mut self,
        ime: &ImeState,
        keyboard: &KeyboardFocus,
    ) -> Option<Preedit<'_>> {
        self.preedit.as_mut()?.overlay(ime, keyboard)
    }
}
use crate::{
    WindowId,
    clipboard::{PasteSink, PendingPaste},
};

impl<P: TabPty + PasteSink> TabRegistry<P> {
    /// The adapter synchronizes KeyboardFocus/IME on Tab activation before dispatching input.
    pub fn route_ime(
        &mut self,
        window: WindowId,
        ime: &ImeState,
        keyboard: &KeyboardFocus,
        update: &mut ImeUpdate,
    ) -> Result<bool, ImeRouteError<P::Error>> {
        if update.target.window != window
            || self.active != Some(update.target.tab)
            || !update.is_current(ime, keyboard)
        {
            update.commit.clear();
            update.preedit = Default::default();
            return Ok(false);
        }
        let Some(tab) = self.get_mut(update.target.tab) else {
            update.commit.clear();
            return Ok(false);
        };
        let ui = tab
            .preedit
            .get_or_insert_with(|| PreeditState::new(update.target));
        tab.dirty |= ui
            .apply(update, ime, keyboard)
            .map_err(ImeRouteError::Preedit)?;
        let Some(pty) = tab.pty.as_mut() else {
            return Ok(false);
        };
        update
            .enqueue_commit(ime, keyboard, pty)
            .map_err(ImeRouteError::Pty)
    }

    /// Route by the original target, even if another Tab is active. Closed targets cancel pending bytes.
    pub fn route_paste(
        &mut self,
        window: WindowId,
        paste: &mut PendingPaste,
    ) -> Result<bool, P::Error> {
        let target = paste.target();
        if target.window != window {
            return Ok(false);
        }
        let Some(tab) = self.get_mut(target.tab) else {
            paste.cancel_for_tab(target.tab);
            return Ok(false);
        };
        let Some(pty) = tab.pty.as_mut() else {
            return Ok(false);
        };
        paste.enqueue_next(pty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId,
        clipboard::{PasteKeyAction, PastePayload, copy_shortcut, paste_shortcut},
        input::KeyboardFocus,
        keymap::{KeyState, Modifiers},
    };
    use alacritty_terminal::{
        index::{Column, Line, Point},
        selection::SelectionRange,
    };
    #[derive(Debug)]
    struct FakePty(TabId, Vec<u8>);
    impl TabPty for FakePty {
        fn tab(&self) -> TabId {
            self.0
        }
    }
    impl PasteSink for FakePty {
        type Error = std::convert::Infallible;
        fn tab(&self) -> TabId {
            self.0
        }
        fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.1.extend_from_slice(bytes);
            Ok(())
        }
    }

    #[test]
    fn ime_commit_targets_correct_tab() {
        use crate::{
            clipboard::SeatId,
            ime::{ImeEvent, ImeState},
            wayland::{Capabilities, Global},
        };
        let mut tabs = TabRegistry::default();
        let window = WindowId(1);
        let seat = SeatId(1);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(window, TabId(1));
        for id in [1, 2] {
            tabs.insert(TabId(id), 2, 8).unwrap();
            tabs.attach_pty(FakePty(TabId(id), Vec::new())).unwrap();
        }
        let caps = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut ime = ImeState::new(seat, &caps);
        ime.enter(seat, window);
        ime.sync_enabled(seat, &keyboard, true);
        let focus = ime.capture_focus(seat, window, &keyboard);
        ime.stage(&focus, &keyboard, ImeEvent::Commit { text: Some("舊") })
            .unwrap();
        let mut old = ime.done(&focus, &keyboard, ime.commit_serial()).unwrap();
        tabs.activate(TabId(2));
        keyboard.enter(window, TabId(2));
        ime.sync_enabled(seat, &keyboard, true);
        assert!(!tabs.route_ime(window, &ime, &keyboard, &mut old).unwrap());
        assert!(old.commit.is_empty());
        let focus = ime.capture_focus(seat, window, &keyboard);
        ime.stage(&focus, &keyboard, ImeEvent::Commit { text: Some("新") })
            .unwrap();
        ime.stage(
            &focus,
            &keyboard,
            ImeEvent::Preedit {
                text: Some("組字"),
                cursor_begin: 0,
                cursor_end: 6,
            },
        )
        .unwrap();
        let mut update = ime.done(&focus, &keyboard, ime.commit_serial()).unwrap();
        assert!(
            tabs.route_ime(window, &ime, &keyboard, &mut update)
                .unwrap()
        );
        assert!(
            !tabs
                .route_ime(window, &ime, &keyboard, &mut update)
                .unwrap()
        );
        assert_eq!(
            tabs.get(TabId(2)).unwrap().pty().unwrap().1,
            "新".as_bytes()
        );
        assert_eq!(
            tabs.get_mut(TabId(2))
                .unwrap()
                .preedit_overlay(&ime, &keyboard)
                .unwrap()
                .text,
            "組字"
        );
        assert!(tabs.get(TabId(1)).unwrap().pty().unwrap().1.is_empty());
        tabs.activate(TabId(1));
        keyboard.enter(window, TabId(1));
        ime.sync_enabled(seat, &keyboard, true);
        assert!(
            tabs.get_mut(TabId(2))
                .unwrap()
                .preedit_overlay(&ime, &keyboard)
                .is_none()
        );
        assert!(
            tabs.get_mut(TabId(1))
                .unwrap()
                .preedit_overlay(&ime, &keyboard)
                .is_none()
        );
    }

    #[test]
    fn copy_paste_targets_original_tab() {
        let mut tabs = TabRegistry::default();
        let window = WindowId(1);
        let mut keyboard = KeyboardFocus::default();
        keyboard.enter(window, TabId(1));
        for (id, text) in [(1, b"A"), (2, b"B")] {
            tabs.insert(TabId(id), 2, 8).unwrap();
            tabs.attach_pty(FakePty(TabId(id), Vec::new())).unwrap();
            tabs.get_mut(TabId(id))
                .unwrap()
                .terminal_mut()
                .feed(TabId(id), text)
                .unwrap();
        }
        let mods = Modifiers {
            depressed: kbvm::ModifierMask::CONTROL.0 | kbvm::ModifierMask::SHIFT.0,
            ..Modifiers::default()
        };
        let route = keyboard.route_key(47, true).unwrap();
        let PasteKeyAction::Paste(intent) =
            paste_shortcut(&keyboard, &route, mods, 0x56, KeyState::Pressed)
        else {
            panic!("paste missing")
        };
        let copied = copy_shortcut(
            route.target,
            mods,
            0x43,
            KeyState::Pressed,
            Some(SelectionRange::new(
                Point::new(Line(0), Column(0)),
                Point::new(Line(0), Column(0)),
                false,
            )),
            tabs.get(TabId(1)).unwrap().terminal().grid().unwrap(),
        )
        .unwrap();
        assert_eq!(copied.text(), "A");
        let mut pending = intent
            .with_payload(
                PastePayload::new(copied.into_text()).unwrap(),
                tabs.get(TabId(1)).unwrap().terminal(),
            )
            .unwrap();
        tabs.activate(TabId(2));
        keyboard.enter(window, TabId(2));
        assert!(!tabs.route_paste(WindowId(2), &mut pending).unwrap());
        assert!(tabs.route_paste(window, &mut pending).unwrap());
        assert!(!tabs.route_paste(window, &mut pending).unwrap());
        assert_eq!(tabs.get(TabId(1)).unwrap().pty().unwrap().1, b"A");
        assert!(tabs.get(TabId(2)).unwrap().pty().unwrap().1.is_empty());
        let mut late = intent
            .with_payload(
                PastePayload::new("late".into()).unwrap(),
                tabs.get(TabId(1)).unwrap().terminal(),
            )
            .unwrap();
        assert!(tabs.close(TabId(1)).is_some());
        assert!(!tabs.route_paste(window, &mut late).unwrap());
        assert!(late.is_complete());
        assert!(tabs.get(TabId(2)).unwrap().pty().unwrap().1.is_empty());
    }
}
