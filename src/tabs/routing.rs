use super::{TabPty, TabRegistry};
use crate::{
    WindowId,
    clipboard::{PasteSink, PendingPaste},
};

impl<P: TabPty + PasteSink> TabRegistry<P> {
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
