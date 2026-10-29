//! Stateful byte-to-grid adapter. No PTY, Wayland, clipboard or renderer I/O.
use crate::{
    TabId,
    error::{AppError, Component},
};
use alacritty_terminal::{
    Term,
    event::VoidListener,
    grid::{Dimensions, Grid},
    term::{
        Config, Osc52,
        cell::{Cell, Flags},
    },
    vte::ansi::{Processor, Timeout},
};
use std::{io::ErrorKind, time::Duration};

pub const MAX_FEED_BYTES: usize = 16 * 1024;
pub const MAX_GRID_CELLS: usize = 32 * 1024;
/// Counts all bytes between ESC ] and BEL/CAN/SUB/ESC, including separators.
pub const MAX_OSC_BYTES: usize = 4096;
pub const MAX_COMBINING_MARKS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalError {
    pub tab: TabId,
    pub error: AppError,
}

/// One Tab's parser and grid. Payload-bearing state deliberately has no Debug.
pub struct Terminal {
    tab: TabId,
    core: Option<Core>,
}

impl Terminal {
    /// Requires at least one row, two columns, and at most MAX_GRID_CELLS cells.
    pub fn new(tab: TabId, rows: u16, columns: u16) -> Result<Self, TerminalError> {
        let size = Size {
            rows: usize::from(rows),
            columns: usize::from(columns),
        };
        if rows == 0 || columns < 2 || size.rows * size.columns > MAX_GRID_CELLS {
            return Err(TerminalError {
                tab,
                error: AppError::new(Component::Terminal, ErrorKind::InvalidInput),
            });
        }
        let config = Config {
            // shortcut: history is disabled until TASK-029 supplies its bounded policy.
            scrolling_history: 0,
            kitty_keyboard: false,
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        Ok(Self {
            tab,
            core: Some(Core {
                term: Term::new(config, &size, VoidListener),
                parser: Processor::new(),
                osc: OscLimit::Ground,
            }),
        })
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }

    /// Feed only the valid prefix from a PTY Data(n) result, using its TabId.
    /// Wrong targets and oversized calls are rejected before any state change.
    /// A stream resource violation drops this core and permanently stops it;
    /// prior bytes in that call may have been consumed. Do not retry or reroute.
    /// WouldBlock/Interrupted/EOF must not reset the parser or synthesize bytes.
    pub fn feed(&mut self, tab: TabId, bytes: &[u8]) -> Result<(), TerminalError> {
        let error = |kind| TerminalError {
            tab: self.tab,
            error: AppError::new(Component::Terminal, kind),
        };
        let Some(core) = self.core.as_mut() else {
            return Err(error(ErrorKind::NotConnected));
        };
        if tab != self.tab || bytes.len() > MAX_FEED_BYTES {
            return Err(error(ErrorKind::InvalidInput));
        }
        if !core.feed(bytes) {
            self.core = None;
            return Err(error(ErrorKind::InvalidData));
        }
        Ok(())
    }

    /// Borrow the core's grid without copying cells or exposing mutable state.
    /// None means a stream resource violation stopped this adapter.
    pub fn grid(&self) -> Option<&Grid<Cell>> {
        self.core.as_ref().map(|core| core.term.grid())
    }
}

struct Core {
    term: Term<VoidListener>,
    parser: Processor<NoSync>,
    osc: OscLimit,
}

impl Core {
    fn feed(&mut self, bytes: &[u8]) -> bool {
        for byte in bytes {
            if !self.osc.accept(*byte) {
                return false;
            }
            // Check after each parser step: REP can append up to u16::MAX marks.
            self.parser
                .advance(&mut self.term, std::slice::from_ref(byte));
            let grid = self.term.grid();
            let cursor = &grid.cursor;
            let mut column = cursor.point.column;
            if !cursor.input_needs_wrap {
                column.0 = column.0.saturating_sub(1);
            }
            let row = &grid[cursor.point.line];
            if row[column].flags.contains(Flags::WIDE_CHAR_SPACER) {
                column.0 = column.0.saturating_sub(1);
            }
            if row[column]
                .zerowidth()
                .is_some_and(|marks| marks.len() > MAX_COMBINING_MARKS)
            {
                return false;
            }
        }
        true
    }
}

/// Tracks only OSC allocation boundaries, matching the pinned VTE escape rules.
/// ANSI interpretation stays entirely in VTE; over-limit streams fail closed.
enum OscLimit {
    Ground,
    Escape,
    String(usize),
}

impl OscLimit {
    fn accept(&mut self, byte: u8) -> bool {
        *self = match (&self, byte) {
            (_, 0x1b) => Self::Escape,
            (Self::String(_), 0x07 | 0x18 | 0x1a) => Self::Ground,
            (Self::String(count), _) if *count >= MAX_OSC_BYTES => return false,
            (Self::String(count), _) => Self::String(count + 1),
            (Self::Escape, b']') => Self::String(0),
            (Self::Escape, 0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f..=0xff) => Self::Escape,
            _ => Self::Ground,
        };
        true
    }
}

struct Size {
    rows: usize,
    columns: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

// shortcut: process sync updates immediately until the runtime owns their timeout.
#[derive(Default)]
struct NoSync;
impl Timeout for NoSync {
    fn set_timeout(&mut self, _: Duration) {}
    fn clear_timeout(&mut self) {}
    fn pending_timeout(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
