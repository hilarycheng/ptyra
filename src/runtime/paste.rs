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
    identity: Arc<()>,
    pub pending: Option<PendingPaste>,
}
impl Paste {
    pub fn capture(focus: &KeyboardFocus) -> super::Result<Self> {
        Ok(Self {
            target: focus.target().ok_or("no keyboard focus")?,
            epoch: focus.epoch(),
            identity: Arc::new(()),
            pending: None,
        })
    }
    pub fn prompt_token(&self, focus: &KeyboardFocus) -> Option<Arc<()>> {
        (self.valid(focus)
            && self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.confirmation_prompt().is_some()))
        .then(|| self.identity.clone())
    }
    pub fn decide(&mut self, focus: &KeyboardFocus, token: &Arc<()>, accepted: bool) -> bool {
        if !self.valid(focus) || !Arc::ptr_eq(&self.identity, token) {
            return false;
        }
        self.pending
            .as_mut()
            .is_some_and(|pending| pending.resolve_confirmation(self.target, accepted))
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
/// A short fixed warning, never a preview of untrusted Clipboard text.
const PROMPT: [&str; 2] = [
    "Multiline paste may run commands.",
    "Enter: paste    Esc: cancel",
];
fn prompt_geometry(size: (u32, u32), cell: (u32, u32)) -> Option<(u16, u16, u32)> {
    let columns = (size.0 / cell.0.max(1)).min(1024);
    if columns < 12 {
        return None;
    }
    let rows: u32 = PROMPT
        .iter()
        .map(|line| (line.len() as u32).div_ceil(columns))
        .sum();
    let height = rows.checked_mul(cell.1.max(1))?;
    (height <= size.1).then_some((rows as u16, columns as u16, height))
}
pub(super) fn draw_prompt(
    raster: &mut super::raster::Raster,
    size: (u32, u32),
    pixels: &mut [u8],
) -> super::Result<bool> {
    let Some((rows, columns, height)) =
        prompt_geometry(size, (raster.cell_width, raster.cell_height))
    else {
        return Ok(false);
    };
    let mut terminal = Terminal::with_scrollback(crate::TabId(0), rows, columns, 0)
        .map_err(|_| "paste prompt allocation failed")?;
    let mut text = String::from("\x1b[37;41m\x1b[2J\x1b[H\x1b[?25l");
    for (index, line) in PROMPT.iter().enumerate() {
        if index > 0 {
            text.push_str("\r\n");
        }
        text.push_str(line);
    }
    terminal
        .feed(crate::TabId(0), text.as_bytes())
        .map_err(|_| "paste prompt text rejected")?;
    let offset = (size.1 - height) as usize * size.0 as usize * 4;
    raster.draw(
        &terminal,
        (size.0, height),
        pixels
            .get_mut(offset..)
            .ok_or("invalid paste prompt buffer")?,
        false,
    )?;
    Ok(true)
}

impl super::Runtime {
    /// Only a fresh key press against the currently presented prompt can accept it.
    pub(super) fn paste_prompt_key(&mut self, code: u32, pressed: bool) -> super::Result<bool> {
        let Some(token) = self
            .paste
            .as_ref()
            .and_then(|paste| paste.prompt_token(&self.input.focus))
        else {
            return Ok(false);
        };
        self.input.repeat.cancel();
        if !pressed {
            self.input.consumed.remove(&code);
            return Ok(true);
        }
        if !self.input.consumed.insert(code) {
            return Ok(true);
        }
        let Some(route) = self.input.focus.route_key(code, true) else {
            return Ok(true);
        };
        let Some(map) = &self.input.map else {
            return Ok(true);
        };
        let Some(key) = map
            .translate(route, false)
            .map_err(|_| "key translation failed")?
        else {
            return Ok(true);
        };
        let cancel = key.keysyms.contains(&0xff1b);
        let accept = prompt_geometry(
            self.paint.size,
            (self.raster.cell_width, self.raster.cell_height),
        )
        .is_some()
            && !key.modifiers.ctrl()
            && !key.modifiers.alt()
            && !key.modifiers.shift()
            && key
                .keysyms
                .iter()
                .any(|symbol| matches!(*symbol, 0xff0d | 0xff8d))
            && self
                .presented_paste
                .as_ref()
                .is_some_and(|shown| Arc::ptr_eq(shown, &token));
        if (cancel || accept)
            && self
                .paste
                .as_mut()
                .is_some_and(|paste| paste.decide(&self.input.focus, &token, accept))
        {
            self.presented_paste = None;
            self.paint.dirty = true;
            if cancel {
                self.cancel_waiting_paste();
                eprintln!("Ptyra paste: confirmation cancelled");
            } else {
                self.pump_paste();
            }
        }
        Ok(true)
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
    #[test]
    fn runtime_multiline_paste_waits_for_user_decision() {
        let mut focus = KeyboardFocus::default();
        focus.enter(WindowId(1), TabId(1));
        let terminal = Terminal::new(TabId(1), 2, 8).unwrap();
        let mut first = Paste::capture(&focus).unwrap();
        first
            .complete(&focus, &terminal, "secret\ntext".into())
            .unwrap();
        let token = first.prompt_token(&focus).unwrap();
        assert!(first.pending.as_mut().unwrap().ready_bytes().is_none());
        assert!(!first.decide(&focus, &Arc::new(()), true));
        assert!(first.decide(&focus, &token, true));
        assert_eq!(
            first.pending.as_mut().unwrap().ready_bytes().unwrap(),
            b"secret\ntext"
        );
        assert!(!first.decide(&focus, &token, true));
        let mut next = Paste::capture(&focus).unwrap();
        next.complete(&focus, &terminal, "other\n".into()).unwrap();
        assert!(!next.decide(&focus, &token, true));
        let next_token = next.prompt_token(&focus).unwrap();
        assert!(next.decide(&focus, &next_token, false));
        assert!(next.pending.as_ref().unwrap().is_complete());
        assert!(next.prompt_token(&focus).is_none());
        let mut stale = Paste::capture(&focus).unwrap();
        stale.complete(&focus, &terminal, "stale\n".into()).unwrap();
        let token = stale.prompt_token(&focus).unwrap();
        focus.enter(WindowId(1), TabId(2));
        assert!(stale.prompt_token(&focus).is_none());
        assert!(!stale.decide(&focus, &token, true));
    }

    #[test]
    fn runtime_multiline_prompt_fits_before_acceptance() {
        assert_eq!(prompt_geometry((800, 600), (8, 16)), Some((2, 100, 32)));
        assert_eq!(prompt_geometry((96, 96), (8, 16)), Some((6, 12, 96)));
        assert!(prompt_geometry((95, 600), (8, 16)).is_none());
        assert!(prompt_geometry((96, 95), (8, 16)).is_none());
    }
}
