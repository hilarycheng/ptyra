//! Stateful byte-to-grid adapter. No PTY, Wayland, clipboard or renderer I/O.
use crate::{
    TabId,
    clipboard::SelectionKind,
    error::{AppError, Component},
};
use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Grid},
    term::{
        ClipboardType, Config, Osc52, RenderableContent, TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Processor, Timeout},
};
use std::{cell::RefCell, io::ErrorKind, rc::Rc, time::Duration};

pub const MAX_FEED_BYTES: usize = 16 * 1024;
pub const MAX_GRID_CELLS: usize = 32 * 1024;
pub const DEFAULT_SCROLLBACK_LINES: usize = 5_000;
pub const MAX_SCROLLBACK_LINES: usize = 10_000;
/// Cell slots across both screens, scrollback, and the core's spare row cache.
pub const MAX_TERMINAL_CELLS: usize = 2_000_000;
// alacritty_terminal 0.26.0 Storage grows in batches with up to 1,000 spare rows.
const CORE_HISTORY_CACHE_LINES: usize = 1_000;
/// Counts all bytes between ESC ] and BEL/CAN/SUB/ESC, including separators.
pub const MAX_OSC_BYTES: usize = 4096;
pub const MAX_COMBINING_MARKS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalError {
    pub tab: TabId,
    pub error: AppError,
}

/// An authorized Clipboard Write request; sensitive text deliberately has no Debug.
pub struct Osc52Write {
    pub tab: TabId,
    pub selection: SelectionKind,
    pub text: String,
}

/// One Tab's parser and grid. Payload-bearing state deliberately has no Debug.
pub struct Terminal {
    tab: TabId,
    core: Option<Core>,
}

/// Borrowed view of one Tab's visible cells and render state; no payload Debug.
pub struct TerminalSnapshot<'a> {
    pub tab: TabId,
    pub rows: usize,
    pub columns: usize,
    pub content: RenderableContent<'a>,
}

impl Terminal {
    /// Uses DEFAULT_SCROLLBACK_LINES, subject to the same bounds as with_scrollback.
    pub fn new(tab: TabId, rows: u16, columns: u16) -> Result<Self, TerminalError> {
        Self::with_scrollback(tab, rows, columns, DEFAULT_SCROLLBACK_LINES)
    }

    /// Bounds main-screen history per Tab; zero disables it. Alternate history is zero.
    /// Requires nonzero rows, at least two columns, and at most MAX_GRID_CELLS per
    /// screen. Rejects excessive history lines or total cell slots before allocation.
    /// Limits remain fixed for this instance; oversized settings are not clamped.
    pub fn with_scrollback(
        tab: TabId,
        rows: u16,
        columns: u16,
        scrollback_lines: usize,
    ) -> Result<Self, TerminalError> {
        let size = Size {
            rows: usize::from(rows),
            columns: usize::from(columns),
        };
        let invalid = || TerminalError {
            tab,
            error: AppError::new(Component::Terminal, ErrorKind::InvalidInput),
        };
        let screen_cells = size.rows * size.columns;
        if rows == 0
            || columns < 2
            || screen_cells > MAX_GRID_CELLS
            || scrollback_lines > MAX_SCROLLBACK_LINES
        {
            return Err(invalid());
        }
        let spare_rows = if scrollback_lines == 0 {
            0
        } else {
            CORE_HISTORY_CACHE_LINES
        };
        let total_cells = 2 * screen_cells + (scrollback_lines + spare_rows) * size.columns;
        if total_cells > MAX_TERMINAL_CELLS {
            return Err(invalid());
        }
        let events = ClipboardEvents {
            tab,
            pending: Rc::new(RefCell::new([None, None])),
        };
        Ok(Self {
            tab,
            core: Some(Core {
                term: Term::new(terminal_config(scrollback_lines), &size, events.clone()),
                parser: Processor::new(),
                osc: OscLimit::Ground,
                events,
                scrollback_lines,
            }),
        })
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }

    /// Call only for an explicit user policy change; PTY output cannot grant permission.
    /// Read stays disabled. Revoking permission discards undelivered Write requests.
    pub fn set_osc52_write_enabled(&mut self, enabled: bool) -> Result<(), TerminalError> {
        let core = self.core.as_mut().ok_or_else(|| TerminalError {
            tab: self.tab,
            error: AppError::new(Component::Terminal, ErrorKind::NotConnected),
        })?;
        let mut config = terminal_config(core.scrollback_lines);
        if enabled {
            config.osc52 = Osc52::OnlyCopy;
        } else {
            *core.events.pending.borrow_mut() = [None, None];
        }
        core.term.set_options(config);
        Ok(())
    }

    /// Take the latest value per selection; repeated writes replace that selection's pending value.
    /// The adapter must still obtain a valid seat serial before publishing it.
    pub fn take_osc52_write(&mut self, selection: SelectionKind) -> Option<Osc52Write> {
        self.core.as_mut()?.events.pending.borrow_mut()[selection as usize].take()
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

    /// Current core flags, including ALT_SCREEN and BRACKETED_PASTE.
    /// These describe terminal state, not implemented application capabilities.
    /// None means a stream resource violation stopped this adapter.
    pub fn mode(&self) -> Option<&TermMode> {
        self.core.as_ref().map(|core| core.term.mode())
    }

    /// Borrows visible content without copying the grid or scrollback.
    /// None means the adapter stopped. The borrow prevents concurrent mutation.
    ///
    /// ```compile_fail
    /// use ptyra::{TabId, terminal::Terminal};
    /// let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    /// let snapshot = terminal.snapshot().unwrap();
    /// terminal.feed(TabId(1), b"changed").unwrap();
    /// assert_eq!(snapshot.content.display_iter.count(), 8);
    /// ```
    pub fn snapshot(&self) -> Option<TerminalSnapshot<'_>> {
        let core = self.core.as_ref()?;
        Some(TerminalSnapshot {
            tab: self.tab,
            rows: core.term.grid().screen_lines(),
            columns: core.term.grid().columns(),
            content: core.term.renderable_content(),
        })
    }
}

fn terminal_config(scrollback_lines: usize) -> Config {
    Config {
        scrolling_history: scrollback_lines,
        kitty_keyboard: false,
        osc52: Osc52::Disabled,
        ..Config::default()
    }
}

#[derive(Clone)]
struct ClipboardEvents {
    tab: TabId,
    pending: Rc<RefCell<[Option<Osc52Write>; 2]>>,
}

impl EventListener for ClipboardEvents {
    fn send_event(&self, event: Event) {
        if let Event::ClipboardStore(kind, text) = event {
            let selection = match kind {
                ClipboardType::Clipboard => SelectionKind::Clipboard,
                ClipboardType::Selection => SelectionKind::Primary,
            };
            self.pending.borrow_mut()[selection as usize] = Some(Osc52Write {
                tab: self.tab,
                selection,
                text,
            });
        }
    }
}

struct Core {
    term: Term<ClipboardEvents>,
    parser: Processor<NoSync>,
    osc: OscLimit,
    events: ClipboardEvents,
    scrollback_lines: usize,
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
