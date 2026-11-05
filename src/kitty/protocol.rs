use super::KeyboardFlags;
use alacritty_terminal::vte::{Params, Parser, Perform};
use std::collections::VecDeque;

const MAX_KEYBOARD_REPLIES: usize = 32;
const MAX_KEYBOARD_STACK: usize = 32;

#[derive(Default)]
pub(crate) struct KeyboardProtocol {
    parser: Parser,
    state: KeyboardState,
}

#[derive(Default)]
struct KeyboardState {
    // shortcut: advertise no modes until screen-scoped state and input ownership are wired.
    supported: KeyboardFlags,
    active: ModeStack,
    inactive: ModeStack,
    alternate: bool,
    replies: VecDeque<KeyboardFlags>,
    failed: bool,
}

#[derive(Default)]
struct ModeStack {
    current: KeyboardFlags,
    stack: [KeyboardFlags; MAX_KEYBOARD_STACK],
    depth: usize,
}

impl KeyboardProtocol {
    pub(crate) fn set_screen(&mut self, alternate: bool) {
        if self.state.alternate != alternate {
            std::mem::swap(&mut self.state.active, &mut self.state.inactive);
            self.state.alternate = alternate;
        }
    }

    /// Terminal's OSC and per-feed limits must run before this secondary VTE observer.
    pub(crate) fn advance(&mut self, bytes: &[u8]) -> bool {
        self.parser.advance(&mut self.state, bytes);
        !self.state.failed
    }

    pub(crate) fn take_reply(&mut self) -> Option<Vec<u8>> {
        self.state
            .replies
            .pop_front()
            .map(|flags| format!("\x1b[?{}u", flags.bits()).into_bytes())
    }
}

impl Perform for KeyboardState {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore || self.failed || action != 'u' || params.len() > 2 {
            return;
        }
        let mut values = [0u16; 2];
        for (index, param) in params.iter().enumerate() {
            if param.len() != 1 {
                return;
            }
            values[index] = param[0];
        }
        match intermediates {
            b"?" if params.len() <= 1 && values[0] == 0 => {
                if self.replies.len() == MAX_KEYBOARD_REPLIES
                    || self.replies.try_reserve(1).is_err()
                {
                    self.failed = true;
                } else {
                    self.replies.push_back(self.active.current & self.supported);
                }
            }
            b"=" => {
                let flags = KeyboardFlags::from_bits_truncate(values[0] as u8) & self.supported;
                match values[1] {
                    0 | 1 => self.active.current = flags,
                    2 => self.active.current |= flags,
                    3 => self.active.current.remove(flags),
                    _ => return,
                }
                self.active.depth = self.active.depth.max(1);
                self.active.stack[self.active.depth - 1] = self.active.current;
            }
            b">" if params.len() <= 1 => {
                if self.active.depth == MAX_KEYBOARD_STACK {
                    self.active.stack.copy_within(1.., 0);
                    self.active.depth -= 1;
                }
                self.active.current =
                    KeyboardFlags::from_bits_truncate(values[0] as u8) & self.supported;
                self.active.stack[self.active.depth] = self.active.current;
                self.active.depth += 1;
            }
            b"<" if params.len() <= 1 => {
                self.active.depth = self
                    .active
                    .depth
                    .saturating_sub(usize::from(values[0].max(1)));
                self.active.current = self
                    .active
                    .depth
                    .checked_sub(1)
                    .map(|index| self.active.stack[index])
                    .unwrap_or_default();
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if !ignore && intermediates.is_empty() && byte == b'c' {
            self.active = ModeStack::default();
            self.inactive = ModeStack::default();
            self.alternate = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, kitty::KeyboardFlags, terminal::Terminal};

    #[test]
    fn kitty_mode_stack_is_screen_scoped() {
        let mut protocol = KeyboardProtocol::default();
        protocol.state.supported = KeyboardFlags::all();
        protocol.advance(b"\x1b[>1u");
        protocol.set_screen(true);
        protocol.advance(b"\x1b[?u\x1b[>2u");
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?0u");
        protocol.set_screen(true);
        protocol.advance(b"\x1b[?u");
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?2u");
        protocol.set_screen(false);
        protocol.advance(b"\x1b[?u\x1b[<u");
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?1u");
        protocol.set_screen(true);
        protocol.advance(b"\x1b[?u\x1bc");
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?2u");
        for alternate in [false, true] {
            protocol.set_screen(alternate);
            protocol.advance(b"\x1b[?u");
            assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?0u");
        }
    }

    #[test]
    fn kitty_stack_is_bounded() {
        let mut protocol = KeyboardProtocol::default();
        protocol.state.supported = KeyboardFlags::all();
        assert!(protocol.advance(b"\x1b[=1u\x1b[>2u\x1b[=3u\x1b[<u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?1u");
        assert!(protocol.advance(b"\x1bc"));
        for i in 0..=32 {
            assert!(protocol.advance(format!("\x1b[>{}u", i % 32).as_bytes()));
        }
        assert_eq!(protocol.state.active.depth, MAX_KEYBOARD_STACK);
        assert!(protocol.advance(b"\x1b[<31u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?1u");
        assert!(protocol.advance(b"\x1b[<65535u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?0u");
        assert_eq!(protocol.state.active.depth, 0);
        assert!(protocol.advance(b"\x1b[>1u\x1b[>u\x1b[<u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?1u");
        let mut terminal = Terminal::with_scrollback(TabId(1), 2, 8, 0).unwrap();
        for _ in 0..10_000 {
            terminal.feed(TabId(1), b"\x1b[>31u").unwrap();
        }
        terminal.feed(TabId(1), b"\x1b[<65535u\x1b[?u").unwrap();
        assert_eq!(terminal.take_keyboard_reply().unwrap().bytes, b"\x1b[?0u");
        assert!(terminal.grid().is_some());
    }

    #[test]
    fn kitty_query_reports_current_flags() {
        let mut terminal = Terminal::new(TabId(7), 2, 8).unwrap();
        for bytes in [b"\x1b[=".as_slice(), b"31u\x1b[?", b"u"] {
            terminal.feed(TabId(7), bytes).unwrap();
        }
        let reply = terminal.take_keyboard_reply().unwrap();
        assert_eq!(reply.tab, TabId(7));
        assert_eq!(reply.bytes, b"\x1b[?0u");
        assert!(terminal.take_keyboard_reply().is_none());
        let mut protocol = KeyboardProtocol::default();
        protocol.state.supported = KeyboardFlags::DISAMBIGUATE_ESC_CODES;
        assert!(protocol.advance(b"\x1b[=31u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?1u");
        assert!(protocol.advance(b"\x1b[=1;3u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?0u");
        assert!(protocol.advance(b"\x1b[=1;2u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?1u");
        assert!(protocol.advance(b"\x1b[=0;99u\x1b[?u"));
        assert_eq!(protocol.take_reply().unwrap(), b"\x1b[?1u");
        assert!(protocol.advance(b"\x1b]2;[?u\x07"));
        assert!(protocol.take_reply().is_none());
        assert!(protocol.advance(b"\x1bPq[?u\x1b\\"));
        assert!(protocol.take_reply().is_none());
        for _ in 0..MAX_KEYBOARD_REPLIES {
            assert!(protocol.advance(b"\x1b[?u"));
        }
        assert!(!protocol.advance(b"\x1b[?u"));
    }
}
