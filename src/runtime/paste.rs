//! Focus-bound native paste preparation. Payloads and focus tokens are never logged.
use crate::{
    clipboard::{PasteIntent, PastePayload, PendingPaste, SelectionKind},
    input::{FocusTarget, KeyboardFocus},
    terminal::Terminal,
};
use std::sync::Arc;
pub(super) struct Paste {
    pub target: FocusTarget,
    epoch: Arc<()>,
    pub pending: Option<PendingPaste>,
}
impl Paste {
    pub fn capture(focus: &KeyboardFocus) -> super::Result<Self> {
        Ok(Self {
            target: focus.target().ok_or("no keyboard focus")?,
            epoch: focus.epoch(),
            pending: None,
        })
    }
    pub fn valid(&self, focus: &KeyboardFocus) -> bool {
        focus.matches(self.target, &self.epoch)
    }
    pub fn complete(
        &mut self,
        focus: &KeyboardFocus,
        terminal: &Terminal,
        text: String,
    ) -> super::Result<()> {
        if !self.valid(focus) || self.pending.is_some() {
            return Err("paste target changed");
        }
        self.pending = Some(
            PasteIntent::for_target(self.target, SelectionKind::Clipboard)
                .with_payload(
                    PastePayload::new(text).map_err(|_| "paste payload exceeds limit")?,
                    terminal,
                )
                .map_err(|_| "paste target unavailable")?,
        );
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, WindowId};
    #[test]
    fn runtime_paste_preserves_payload_and_target() {
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        for enabled in [false, true] {
            let mut terminal = Terminal::new(TabId(1), 2, 8).unwrap();
            if enabled {
                terminal.feed(TabId(1), b"\x1b[?2004h").unwrap();
            }
            let mut paste = Paste::capture(&focus).unwrap();
            let text = if enabled {
                " 繁體\t\r\n e\u{301} "
            } else {
                " 繁體\te\u{301} "
            };
            paste.complete(&focus, &terminal, text.into()).unwrap();
            let expected = if enabled {
                format!("\x1b[200~{text}\x1b[201~")
            } else {
                text.into()
            };
            assert_eq!(
                paste.pending.as_mut().unwrap().ready_bytes().unwrap(),
                expected.as_bytes()
            );
            assert!(
                paste
                    .complete(&focus, &terminal, "duplicate".into())
                    .is_err()
            );
        }
        let terminal = Terminal::new(TabId(1), 2, 8).unwrap();
        let mut paste = Paste::capture(&focus).unwrap();
        paste
            .complete(&focus, &terminal, "unsafe\n".into())
            .unwrap();
        assert!(paste.pending.as_mut().unwrap().ready_bytes().is_none());
        let mut stale = Paste::capture(&focus).unwrap();
        focus.enter(WindowId(1), TabId(2));
        assert!(stale.complete(&focus, &terminal, "stale".into()).is_err());
        focus.enter(WindowId(1), TabId(1));
        assert!(
            stale
                .complete(&focus, &terminal, "round trip".into())
                .is_err()
        );
        let mut other = Paste::capture(&focus).unwrap();
        let wrong = Terminal::new(TabId(2), 2, 8).unwrap();
        assert!(other.complete(&focus, &wrong, "wrong tab".into()).is_err());
        focus.clear();
        assert!(!other.valid(&focus));
    }
}
