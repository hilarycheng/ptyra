//! Ordered, bounded ownership of independent Tab state. No platform I/O in registry operations.
pub mod bar;
mod failure;
#[cfg(target_os = "linux")]
pub mod output;
pub use failure::SessionFailure;
mod render;
mod resize;
mod routing;
mod selection;
pub use routing::ImeRouteError;
pub(crate) use selection::SelectionAction;
mod title;
use crate::{
    TabId,
    error::{AppError, Component},
    selection::TabSelections,
    terminal::Terminal,
};
use std::io::ErrorKind;

pub const MAX_TABS: usize = 64;
/// Reserved grid/history slots, not measured bytes; glyph caches stay outside Tab ownership.
pub const MAX_TOTAL_TAB_CELLS: usize = 8_000_000;
/// Actual Cell capacity, including one in-flight resize, separate from logical slots.
pub const MAX_TOTAL_TAB_ALLOCATION_CELLS: usize = 40_000_000;

pub enum TabDirection {
    Next,
    Previous,
}

/// A single spawn effect; the adapter consumes it and attaches its result by TabId.
pub struct SpawnTab {
    pub tab: TabId,
    pub rows: u16,
    pub columns: u16,
}

/// Adapter cleanup owns only the removed session. PtySession Drop closes and reaps its own child.
pub struct ClosedTab<P> {
    pub tab: TabId,
    pub pty: Option<P>,
}

pub trait TabPty {
    fn tab(&self) -> TabId;
}

#[cfg(target_os = "linux")]
impl TabPty for crate::pty::PtySession {
    fn tab(&self) -> TabId {
        self.tab()
    }
}

pub struct Tab<P> {
    id: TabId,
    terminal: Terminal,
    pty: Option<P>,
    selections: TabSelections,
    selection_epoch: Option<u64>,
    title: String,
    dirty: bool,
    preedit: Option<crate::ime::preedit::PreeditState>,
    exit_status: Option<std::process::ExitStatus>,
    failure: Option<SessionFailure>,
    reserved_cells: usize,
    reserved_rows: u16,
    reserved_columns: u16,
}

impl<P> Tab<P> {
    pub fn exit_status(&self) -> Option<std::process::ExitStatus> {
        self.exit_status
    }
    pub fn id(&self) -> TabId {
        self.id
    }
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }
    pub fn terminal_mut(&mut self) -> &mut Terminal {
        self.dirty = true;
        &mut self.terminal
    }
    pub fn pty(&self) -> Option<&P> {
        self.pty.as_ref()
    }
    pub fn pty_mut(&mut self) -> Option<&mut P> {
        self.pty.as_mut()
    }
    pub fn selections(&self) -> &TabSelections {
        &self.selections
    }
    pub fn selections_mut(&mut self) -> &mut TabSelections {
        self.sync_selection_epoch();
        self.dirty = true;
        &mut self.selections
    }
}

pub struct TabRegistry<P> {
    entries: Vec<Tab<P>>,
    next_id: Option<u64>,
    active: Option<TabId>,
    render_target: Option<TabId>,
    scrollback_lines: usize,
    pub(crate) palette: crate::config::palette::Palette,
    graphics_budget: std::rc::Rc<crate::terminal::graphics::Budget>,
    pub(crate) image_limits: crate::kitty::graphics::decode::ImageLimits,
    osc52_write: bool,
    output_cursor: usize,
}

impl<P> Default for TabRegistry<P> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next_id: Some(1),
            active: None,
            render_target: None,
            scrollback_lines: crate::terminal::DEFAULT_SCROLLBACK_LINES,
            palette: crate::config::palette::Palette::default(),
            graphics_budget: std::rc::Rc::new(crate::terminal::graphics::Budget::new(
                crate::terminal::graphics::GLOBAL_BYTES,
            )),
            image_limits: Default::default(),
            osc52_write: false,
            output_cursor: 0,
        }
    }
}

impl<P: TabPty> TabRegistry<P> {
    /// Explicit application policy only; revocation clears pending writes on every live core.
    pub fn set_osc52_write_enabled(&mut self, enabled: bool) {
        self.osc52_write = enabled;
        for tab in &mut self.entries {
            // A stopped core has already discarded all of its pending requests.
            let _ = tab.terminal.set_osc52_write_enabled(enabled);
        }
    }

    pub fn osc52_write_enabled(&self) -> bool {
        self.osc52_write
    }

    pub fn reserved_cells(&self) -> usize {
        self.entries.iter().map(|tab| tab.reserved_cells).sum()
    }

    pub(crate) fn allocated_cells(&self) -> usize {
        self.entries
            .iter()
            .map(|tab| tab.terminal.allocated_cells())
            .sum()
    }

    /// Keep the PTY for final output drainage; child exit never closes other Tabs.
    pub fn apply_effect(&mut self, effect: &crate::AppEffect) -> bool {
        let crate::AppEffect::PtyExited { tab, status } = effect else {
            return false;
        };
        let Some(tab) = self
            .get_mut(*tab)
            .filter(|tab| tab.pty.is_some() && tab.exit_status.is_none())
        else {
            return false;
        };
        tab.exit_status = Some(*status);
        tab.dirty = true;
        true
    }

    pub fn with_scrollback(lines: usize) -> Result<Self, AppError> {
        if lines > crate::terminal::MAX_SCROLLBACK_LINES {
            return Err(AppError::new(Component::Config, ErrorKind::InvalidInput));
        }
        Ok(Self {
            scrollback_lines: lines,
            ..Self::default()
        })
    }

    /// One New Tab event reserves one state and emits one effect, without spawning in the reducer.
    pub fn new_tab(&mut self, rows: u16, columns: u16) -> Result<SpawnTab, AppError> {
        let id = TabId(
            self.next_id
                .ok_or_else(|| AppError::new(Component::Terminal, ErrorKind::InvalidInput))?,
        );
        self.insert(id, rows, columns)?;
        Ok(SpawnTab {
            tab: id,
            rows,
            columns,
        })
    }

    /// IDs are monotonic and never reused, including after a Tab closes.
    pub fn insert(&mut self, id: TabId, rows: u16, columns: u16) -> Result<(), AppError> {
        if self.next_id.is_none_or(|next| id.0 < next) || self.entries.len() >= MAX_TABS {
            return Err(AppError::new(Component::Terminal, ErrorKind::InvalidInput));
        }
        let reserved_cells =
            crate::terminal::cell_reservation(rows, columns, self.scrollback_lines)
                .ok_or_else(|| AppError::new(Component::Terminal, ErrorKind::InvalidInput))?;
        if reserved_cells > MAX_TOTAL_TAB_CELLS - self.reserved_cells()
            || reserved_cells > MAX_TOTAL_TAB_ALLOCATION_CELLS - self.allocated_cells()
        {
            return Err(AppError::new(Component::Terminal, ErrorKind::OutOfMemory));
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
        let mut terminal = Terminal::with_scrollback(id, rows, columns, self.scrollback_lines)
            .map_err(|error| error.error)?;
        terminal.set_palette(self.palette);
        terminal.set_image_limits(self.image_limits)?;
        terminal.bind_graphics_budget(self.graphics_budget.clone())?;
        terminal
            .set_osc52_write_enabled(self.osc52_write)
            .map_err(|error| error.error)?;
        self.entries.push(Tab {
            id,
            terminal,
            pty: None,
            selections: TabSelections::new(id),
            selection_epoch: Some(0),
            title: "Terminal".into(),
            dirty: true,
            preedit: None,
            exit_status: None,
            failure: None,
            reserved_cells,
            reserved_rows: rows,
            reserved_columns: columns,
        });
        self.next_id = id.0.checked_add(1);
        if self.active.is_none() {
            self.active = Some(id);
        }
        Ok(())
    }

    /// Return rejected sessions to the caller, rather than silently dropping a live child.
    pub fn attach_pty(&mut self, pty: P) -> Result<(), P> {
        let Some(tab) = self
            .get_mut(pty.tab())
            .filter(|tab| tab.pty.is_none() && tab.failure.is_none())
        else {
            return Err(pty);
        };
        tab.pty = Some(pty);
        Ok(())
    }

    pub(crate) fn sync_presented(&mut self) {
        if let Some(id) = self.active
            && let Some(tab) = self.get_mut(id)
        {
            tab.terminal.sync_presented();
        }
    }
    pub(crate) fn sync_deadline(
        &mut self,
        clock: &impl crate::clock::Clock,
    ) -> Option<crate::clock::Deadline> {
        self.entries
            .iter_mut()
            .filter_map(|tab| tab.terminal.sync_update(clock))
            .min_by_key(|deadline| deadline.remaining(clock))
    }

    pub fn ids(&self) -> impl Iterator<Item = TabId> + '_ {
        self.entries.iter().map(|tab| tab.id)
    }

    pub fn active(&self) -> Option<TabId> {
        self.active
    }

    /// Choose the next neighbor, or the previous neighbor when closing the last item.
    pub fn close(&mut self, id: TabId) -> Option<ClosedTab<P>> {
        let index = self.entries.iter().position(|tab| tab.id == id)?;
        let removed = self.entries.remove(index);
        if self.active == Some(id) {
            self.active = self
                .entries
                .get(index.min(self.entries.len().saturating_sub(1)))
                .map(|tab| tab.id);
        }
        Some(ClosedTab {
            tab: id,
            pty: removed.pty,
        })
    }

    pub fn cycle(&mut self, direction: TabDirection) -> Option<TabId> {
        let index = self
            .entries
            .iter()
            .position(|tab| Some(tab.id) == self.active)?;
        let count = self.entries.len();
        let next = match direction {
            TabDirection::Next => (index + 1) % count,
            TabDirection::Previous => (index + count - 1) % count,
        };
        let id = self.entries[next].id;
        self.activate(id);
        Some(id)
    }

    /// False means no change, including unknown IDs; other Tab state is untouched.
    pub fn activate(&mut self, id: TabId) -> bool {
        if self.active == Some(id) || self.get(id).is_none() {
            return false;
        }
        self.active = Some(id);
        true
    }
    pub fn get(&self, id: TabId) -> Option<&Tab<P>> {
        self.entries.iter().find(|tab| tab.id == id)
    }
    pub fn get_mut(&mut self, id: TabId) -> Option<&mut Tab<P>> {
        self.entries.iter_mut().find(|tab| tab.id == id)
    }
}

#[cfg(test)]
mod tests;
