//! Presentation-only synchronization. Parsing and replies continue without a byte buffer.
use crate::clock::{Clock, Deadline};
use alacritty_terminal::{
    event::{Event, EventListener},
    vte::{
        Params,
        ansi::{Handler, PrivateMode},
    },
};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct State {
    enabled: bool,
    expired: bool,
    frame_presented: bool,
    deadline: Option<Deadline>,
    pub query: bool,
}
impl State {
    pub fn csi(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }
        match (action, intermediates) {
            ('h' | 'l', b"?") if params.iter().any(|p| p[0] == 2026) => {
                self.set_enabled(action == 'h')
            }
            ('p', b"?$") if params.iter().next().is_some_and(|p| p[0] == 2026) => self.query = true,
            _ => {}
        }
    }
    fn set_enabled(&mut self, enabled: bool) {
        if enabled && !self.enabled && self.frame_presented {
            self.expired = false;
            self.deadline = None;
            self.frame_presented = false;
        }
        self.enabled = enabled;
    }
    pub fn reset(&mut self) {
        // RIS followed by another begin before any presentation must retain the same bound.
        self.enabled = false;
        self.query = false;
    }
    pub fn presented(&mut self) {
        self.frame_presented = true;
        self.deadline = None;
        if !self.enabled {
            self.expired = false;
        }
    }
    pub fn blocks_selection(&self) -> bool {
        !self.frame_presented && (self.enabled || self.expired || self.deadline.is_some())
    }
    pub fn pending(&self) -> bool {
        self.enabled && !self.expired
    }
    pub fn update(&mut self, clock: &impl Clock) -> Option<Deadline> {
        if !self.enabled {
            return None;
        }
        if self.expired {
            return None;
        }
        // Repeated begins, including end+begin in one drain, cannot move the first deadline.
        if self.deadline.is_none() {
            self.deadline = Deadline::after(clock, Duration::from_millis(150));
        }
        if self.deadline.is_none_or(|deadline| deadline.is_due(clock)) {
            self.expired = true;
            self.deadline = None;
        }
        self.deadline
    }
}
/// Only the final byte of a parsed DECRQM 2026 dispatch reaches this narrow handler.
/// Other commands and all actual ANSI mutation still use the normal core handler.
pub(super) struct QueryDispatch<'a> {
    pub term: &'a mut alacritty_terminal::Term<super::ClipboardEvents>,
    pub events: &'a super::ClipboardEvents,
    pub pending: bool,
}
impl Handler for QueryDispatch<'_> {
    fn report_private_mode(&mut self, mode: PrivateMode) {
        if mode.raw() == 2026 {
            self.events.send_event(Event::PtyWrite(format!(
                "\x1b[?2026;{}$y",
                if self.pending { 1 } else { 2 }
            )));
        } else {
            self.term.report_private_mode(mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, clock::FakeClock, terminal::Terminal};
    #[test]
    fn sync_sequences_preserve_queries_and_split_boundaries() {
        let bytes =
            b"\x1b[?2026h\x1b[?2026$p\x1b[6n\x1b]4;1;?;1;#123456;1;?\x07\x1b[?2026l\x1b[?2026$p";
        let expected=b"\x1b[?2026;1$y\x1b[1;1R\x1b]4;1;rgb:cccc/0000/0000\x07\x1b]4;1;rgb:1212/3434/5656\x07\x1b[?2026;2$y";
        for split in 0..=bytes.len() {
            let mut terminal = Terminal::new(TabId(1), 2, 8).unwrap();
            terminal.feed(TabId(1), &bytes[..split]).unwrap();
            terminal.feed(TabId(1), &bytes[split..]).unwrap();
            assert_eq!(
                terminal.take_core_replies(),
                (expected.to_vec(), false),
                "split {split}"
            );
            assert!(!terminal.sync_pending());
        }
        let mut terminal = Terminal::new(TabId(1), 2, 8).unwrap();
        let clock = FakeClock::default();
        for bytes in [
            b"\x1b[?2026\x18h".as_slice(),
            b"\x1b]0;[?2026h\x07",
            b"\x1b[2026h",
            b"\x1b[>2026h",
        ] {
            terminal.feed(TabId(1), bytes).unwrap();
            assert!(!terminal.sync_pending());
        }
        terminal.feed(TabId(1), b"\x1b[?1;2026h").unwrap();
        assert!(terminal.sync_update(&clock).is_some());
        assert!(
            terminal
                .mode()
                .unwrap()
                .contains(alacritty_terminal::term::TermMode::APP_CURSOR)
        );
        clock.advance(Duration::from_millis(150)).unwrap();
        assert!(terminal.sync_update(&clock).is_none());
        terminal.feed(TabId(1), b"\x1b[?2026$p").unwrap();
        assert_eq!(terminal.take_core_replies().0, b"\x1b[?2026;2$y");
        // A submitted timeout frame plus an end permits a new batch, even in one read.
        terminal.sync_presented();
        terminal.feed(TabId(1), b"\x1b[?2026l\x1b[?2026h").unwrap();
        assert!(terminal.sync_update(&clock).is_some());
        clock.advance(Duration::from_millis(149)).unwrap();
        terminal.feed(TabId(1), b"\x1b[?2026l\x1b[?2026h").unwrap();
        assert_eq!(
            terminal.sync_update(&clock).unwrap().remaining(&clock),
            Duration::from_millis(1)
        );
        clock.advance(Duration::from_millis(1)).unwrap();
        assert!(terminal.sync_update(&clock).is_none());
        let overflow = FakeClock::default();
        overflow.advance(Duration::MAX).unwrap();
        let mut overflow_terminal = Terminal::new(TabId(1), 2, 8).unwrap();
        overflow_terminal.feed(TabId(1), b"\x1b[?2026h").unwrap();
        assert!(overflow_terminal.sync_update(&overflow).is_none());
        assert!(!overflow_terminal.sync_pending());
    }
}
