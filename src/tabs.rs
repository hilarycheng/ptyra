//! Ordered, bounded ownership of independent Tab state. No platform I/O in registry operations.
use crate::{
    TabId,
    error::{AppError, Component},
    selection::TabSelections,
    terminal::Terminal,
};
use std::io::ErrorKind;

pub const MAX_TABS: usize = 64;

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
}

impl<P> Tab<P> {
    pub fn id(&self) -> TabId {
        self.id
    }
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }
    pub fn terminal_mut(&mut self) -> &mut Terminal {
        &mut self.terminal
    }
    pub fn pty(&self) -> Option<&P> {
        self.pty.as_ref()
    }
    pub fn selections(&self) -> &TabSelections {
        &self.selections
    }
    pub fn selections_mut(&mut self) -> &mut TabSelections {
        &mut self.selections
    }
}

pub struct TabRegistry<P> {
    entries: Vec<Tab<P>>,
    next_id: Option<u64>,
}

impl<P> Default for TabRegistry<P> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next_id: Some(1),
        }
    }
}

impl<P: TabPty> TabRegistry<P> {
    /// IDs are monotonic and never reused, including after a Tab closes.
    pub fn insert(&mut self, id: TabId, rows: u16, columns: u16) -> Result<(), AppError> {
        if self.next_id.is_none_or(|next| id.0 < next) || self.entries.len() >= MAX_TABS {
            return Err(AppError::new(Component::Terminal, ErrorKind::InvalidInput));
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
        let terminal = Terminal::new(id, rows, columns).map_err(|error| error.error)?;
        self.entries.push(Tab {
            id,
            terminal,
            pty: None,
            selections: TabSelections::new(id),
        });
        self.next_id = id.0.checked_add(1);
        Ok(())
    }

    /// Return rejected sessions to the caller, rather than silently dropping a live child.
    pub fn attach_pty(&mut self, pty: P) -> Result<(), P> {
        let Some(tab) = self.get_mut(pty.tab()).filter(|tab| tab.pty.is_none()) else {
            return Err(pty);
        };
        tab.pty = Some(pty);
        Ok(())
    }

    pub fn ids(&self) -> impl Iterator<Item = TabId> + '_ {
        self.entries.iter().map(|tab| tab.id)
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
