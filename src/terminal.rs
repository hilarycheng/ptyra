//! Stateful byte-to-grid adapter. No PTY, Wayland, clipboard or renderer I/O.
mod queries;
pub(crate) mod sync;
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
/// Bounds OSC 52 encoded data before Base64 decoding; its header shares this budget.
pub const MAX_OSC_BYTES: usize = 4096;
pub const MAX_COMBINING_MARKS: usize = 64;
/// Core responses awaiting one runtime dispatch, separate from the bounded PTY FIFO.
pub const MAX_CORE_REPLY_BYTES: usize = 64 * 1024;

pub(crate) fn cell_reservation(rows: u16, columns: u16, history: usize) -> Option<usize> {
    let screen = usize::from(rows) * usize::from(columns);
    if rows == 0 || columns < 2 || screen > MAX_GRID_CELLS || history > MAX_SCROLLBACK_LINES {
        return None;
    }
    let spare = if history == 0 {
        0
    } else {
        CORE_HISTORY_CACHE_LINES
    };
    let total = 2 * screen + (history + spare) * usize::from(columns);
    (total <= MAX_TERMINAL_CELLS).then_some(total)
}

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

/// An inert protocol response bound to its source Tab; the PTY adapter owns enqueueing.
pub struct KeyboardReply {
    pub tab: TabId,
    pub bytes: Vec<u8>,
}

/// One Tab's parser and grid. Payload-bearing state deliberately has no Debug.
pub struct Terminal {
    tab: TabId,
    content_epoch: Option<u64>,
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
        cell_reservation(rows, columns, scrollback_lines).ok_or_else(invalid)?;
        let events = ClipboardEvents {
            tab,
            pending: Rc::new(RefCell::new([None, None])),
            title: Rc::new(RefCell::new(None)),
            replies: Rc::new(RefCell::new(CoreReplies::default())),
            queries: Rc::new(RefCell::new(Vec::new())),
        };
        Ok(Self {
            tab,
            content_epoch: Some(0),
            core: Some(Core {
                term: Term::new(terminal_config(scrollback_lines), &size, events.clone()),
                parser: Processor::new(),
                osc: OscLimit::Ground,
                events,
                scrollback_lines,
                cell_pixels: None,
                keyboard: crate::kitty::protocol::KeyboardProtocol::default(),
            }),
        })
    }

    /// Conservative anchor policy: any nonempty feed or changed grid size invalidates selections.
    /// Exhaustion fails closed rather than allowing an old anchor generation to be reused.
    pub(crate) fn content_epoch(&self) -> Option<u64> {
        self.content_epoch
    }

    pub(crate) fn sync_update(
        &mut self,
        clock: &impl crate::clock::Clock,
    ) -> Option<crate::clock::Deadline> {
        self.core.as_mut()?.keyboard.sync_mut().update(clock)
    }
    pub(crate) fn sync_presented(&mut self) {
        if let Some(core) = &mut self.core {
            core.keyboard.sync_mut().presented();
        }
    }
    pub(crate) fn sync_blocks_selection(&self) -> bool {
        self.core
            .as_ref()
            .is_some_and(|core| core.keyboard.sync().blocks_selection())
    }
    pub(crate) fn sync_pending(&self) -> bool {
        self.core
            .as_ref()
            .is_some_and(|core| core.keyboard.sync().pending())
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }

    pub fn take_keyboard_reply(&mut self) -> Option<KeyboardReply> {
        self.core
            .as_mut()?
            .keyboard
            .take_reply()
            .map(|bytes| KeyboardReply {
                tab: self.tab,
                bytes,
            })
    }

    /// Drain once; the adapter owns accepted bytes and must never replay a rejected batch.
    pub(crate) fn take_core_replies(&mut self) -> (Vec<u8>, bool) {
        let Some(core) = &mut self.core else {
            return (Vec::new(), false);
        };
        let replies = std::mem::take(&mut *core.events.replies.borrow_mut());
        (replies.bytes, replies.overflow)
    }

    /// Runtime supplies actual raster cell pixels; absent/invalid metrics cannot fabricate a reply.
    pub(crate) fn set_cell_pixels(&mut self, cell: (u32, u32)) {
        if let Some(core) = &mut self.core {
            core.cell_pixels = u16::try_from(cell.0)
                .ok()
                .zip(u16::try_from(cell.1).ok())
                .filter(|&(width, height)| width != 0 && height != 0);
        }
    }

    /// Untrusted OSC title, bounded by MAX_OSC_BYTES. Tab::refresh_title sanitizes it before display.
    pub fn take_title(&mut self) -> Option<String> {
        self.core.as_mut()?.events.title.borrow_mut().take()
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
        if !bytes.is_empty() {
            self.content_epoch = self.content_epoch.and_then(|epoch| epoch.checked_add(1));
        }
        if !core.feed(bytes) {
            self.core = None;
            return Err(error(ErrorKind::InvalidData));
        }
        Ok(())
    }

    /// Copy with the core's current custom tab stops, preserving selected trailing spaces.
    pub fn copy_range_text(
        &mut self,
        range: alacritty_terminal::selection::SelectionRange,
    ) -> Option<String> {
        use alacritty_terminal::{index::Column, vte::ansi::Handler};
        let core = self.core.as_mut()?;
        let range = crate::selection::whole_glyph_range(core.term.grid(), range)?;
        let columns = core.term.grid().columns();
        let mut stops = Vec::new();
        stops.try_reserve_exact(columns).ok()?;
        stops.resize(columns, false);
        let saved = core.term.grid().cursor.point.column;
        core.term.grid_mut().cursor.point.column = Column(0);
        // The public forward-tab handler only changes column and damage metadata. It
        // scans disjoint intervals; include the final clamped destination as a stop.
        while core.term.grid().cursor.point.column.0 < columns - 1 {
            core.term.move_forward_tabs(1);
            stops[core.term.grid().cursor.point.column.0] = true;
        }
        core.term.grid_mut().cursor.point.column = saved;
        crate::selection::copy_text_with_tabstops(core.term.grid(), range, &stops)
    }

    /// Borrow the core's grid without copying cells or exposing mutable state.
    /// None means a stream resource violation stopped this adapter.
    pub fn grid(&self) -> Option<&Grid<Cell>> {
        self.core.as_ref().map(|core| core.term.grid())
    }

    /// Move only the viewport using the core's page/top/bottom policy.
    /// Positive deltas move toward older history; every operation clamps to history.
    /// Returns whether the offset changed, without touching Shell cursor or input bytes.
    pub fn scroll_viewport(
        &mut self,
        scroll: alacritty_terminal::grid::Scroll,
    ) -> Result<bool, TerminalError> {
        use alacritty_terminal::grid::Scroll;
        let core = self.core.as_mut().ok_or_else(|| TerminalError {
            tab: self.tab,
            error: AppError::new(Component::Terminal, ErrorKind::NotConnected),
        })?;
        // The core adds signed deltas; clamp before addition to avoid i32 overflow.
        let scroll = match scroll {
            Scroll::Delta(lines) => Scroll::Delta(
                lines.clamp(-(MAX_SCROLLBACK_LINES as i32), MAX_SCROLLBACK_LINES as i32),
            ),
            other => other,
        };
        let before = core.term.grid().display_offset();
        core.term.scroll_display(scroll);
        Ok(core.term.grid().display_offset() != before)
    }

    /// Resize both screens while preserving content, subject to the original allocation budget.
    pub fn resize(&mut self, rows: u16, columns: u16) -> Result<(), TerminalError> {
        let error = |kind| TerminalError {
            tab: self.tab,
            error: AppError::new(Component::Terminal, kind),
        };
        let core = self
            .core
            .as_mut()
            .ok_or_else(|| error(ErrorKind::NotConnected))?;
        cell_reservation(rows, columns, core.scrollback_lines)
            .ok_or_else(|| error(ErrorKind::InvalidInput))?;
        if core.term.grid().screen_lines() != usize::from(rows)
            || core.term.grid().columns() != usize::from(columns)
        {
            self.content_epoch = self.content_epoch.and_then(|epoch| epoch.checked_add(1));
        }
        core.term.resize(Size {
            rows: rows as usize,
            columns: columns as usize,
        });
        Ok(())
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

/// Payload-bearing bounded FIFO, deliberately without Debug. Overflow rejects a complete
/// response and all later responses in that batch; accepted prefixes remain ordered.
#[derive(Default)]
struct CoreReplies {
    bytes: Vec<u8>,
    overflow: bool,
}
impl CoreReplies {
    fn push(&mut self, bytes: &[u8]) {
        if self.overflow {
            return;
        }
        if bytes.len() > MAX_CORE_REPLY_BYTES - self.bytes.len() {
            self.overflow = true;
            return;
        }
        let needed = self.bytes.len() + bytes.len();
        if needed > self.bytes.capacity() {
            let capacity = (self.bytes.capacity().max(256) * 2)
                .min(MAX_CORE_REPLY_BYTES)
                .max(needed);
            if self
                .bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .is_err()
            {
                self.overflow = true;
                return;
            }
        }
        self.bytes.extend_from_slice(bytes);
    }
}

#[derive(Clone)]
struct ClipboardEvents {
    tab: TabId,
    pending: Rc<RefCell<[Option<Osc52Write>; 2]>>,
    title: Rc<RefCell<Option<String>>>,
    replies: Rc<RefCell<CoreReplies>>,
    queries: Rc<RefCell<Vec<Event>>>,
}

impl EventListener for ClipboardEvents {
    fn send_event(&self, event: Event) {
        match event {
            Event::ColorRequest(..) | Event::TextAreaSizeRequest(..) => {
                let mut queries = self.queries.borrow_mut();
                if queries.len() == queries::MAX_QUERY_EVENTS || queries.try_reserve(1).is_err() {
                    self.replies.borrow_mut().overflow = true;
                } else {
                    queries.push(event);
                }
                return;
            }
            Event::PtyWrite(text) => {
                self.replies.borrow_mut().push(text.as_bytes());
                return;
            }
            Event::Title(title) => {
                *self.title.borrow_mut() = Some(title);
                return;
            }
            Event::ResetTitle => {
                *self.title.borrow_mut() = Some("Terminal".into());
                return;
            }
            _ => {}
        }
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
    parser: Processor<ImmediateParsing>,
    osc: OscLimit,
    events: ClipboardEvents,
    scrollback_lines: usize,
    keyboard: crate::kitty::protocol::KeyboardProtocol,
    cell_pixels: Option<(u16, u16)>,
}

impl Core {
    fn feed(&mut self, bytes: &[u8]) -> bool {
        for byte in bytes {
            let palette_dispatch = self.osc.palette_dispatch(*byte);
            if !self.osc.accept(*byte) {
                return false;
            }
            if !self.keyboard.advance(std::slice::from_ref(byte)) {
                return false;
            }
            // Check after each parser step: REP can append up to u16::MAX marks.
            if self.keyboard.sync().query {
                self.parser.advance(
                    &mut sync::QueryDispatch {
                        term: &mut self.term,
                        events: &self.events,
                        pending: self.keyboard.sync().pending(),
                    },
                    std::slice::from_ref(byte),
                );
            } else if palette_dispatch {
                self.parser.advance(
                    &mut queries::PaletteDispatch {
                        term: &mut self.term,
                        events: &self.events,
                        cell_pixels: self.cell_pixels,
                    },
                    std::slice::from_ref(byte),
                );
            } else {
                self.parser
                    .advance(&mut self.term, std::slice::from_ref(byte));
            }
            self.answer_queries();
            self.keyboard
                .set_screen(self.term.mode().contains(TermMode::ALT_SCREEN));
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
    String(OscString),
}

struct OscString {
    bytes: usize,
    // 0: empty, 1: "4", 2: "4;", 3: another OSC command.
    palette_prefix: u8,
}

impl OscLimit {
    fn palette_dispatch(&self, byte: u8) -> bool {
        matches!(
            self,
            Self::String(OscString {
                palette_prefix: 2,
                ..
            })
        ) && matches!(byte, 0x07 | 0x18 | 0x1a | 0x1b)
    }
    fn accept(&mut self, byte: u8) -> bool {
        *self = match (&self, byte) {
            (_, 0x1b) => Self::Escape,
            (Self::String(_), 0x07 | 0x18 | 0x1a) => Self::Ground,
            (Self::String(state), _) if state.bytes >= MAX_OSC_BYTES => return false,
            (Self::String(state), _) => Self::String(OscString {
                bytes: state.bytes + 1,
                palette_prefix: match (state.palette_prefix, byte) {
                    // These C0 bytes are ignored by VTE inside OSC, but still count toward the cap.
                    (prefix, 0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f) => prefix,
                    (0, b'4') => 1,
                    (1, b';') | (2, _) => 2,
                    _ => 3,
                },
            }),
            (Self::Escape, b']') => Self::String(OscString {
                bytes: 0,
                palette_prefix: 0,
            }),
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

// Intentionally keep parsing/replies immediate. Runtime gates presentation with a bounded deadline.
// Enabling VTE byte buffering here would invalidate narrow per-dispatch query handlers.
#[derive(Default)]
struct ImmediateParsing;
impl Timeout for ImmediateParsing {
    fn set_timeout(&mut self, _: Duration) {}
    fn clear_timeout(&mut self) {}
    fn pending_timeout(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
