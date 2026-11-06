//! Failed sessions retain only a bounded, drawable diagnostic, never an input sink.
use super::{Tab, TabPty, TabRegistry};
use crate::{
    TabId,
    error::{AppError, Component},
    terminal::Terminal,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionFailure {
    Stream,
    Io,
}
impl SessionFailure {
    pub(crate) fn from_error(error: AppError) -> Self {
        if error.component() == Component::Terminal {
            Self::Stream
        } else {
            Self::Io
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Stream => "Stopped: stream error",
            Self::Io => "Stopped: PTY error",
        }
    }
}
impl<P> Tab<P> {
    pub fn failure(&self) -> Option<SessionFailure> {
        self.failure
    }
}
impl<P: TabPty> TabRegistry<P> {
    /// Drop this source's parser, pending input and owned child exactly once. The last
    /// failed Tab stays visible until explicitly closed; it never respawns a shell.
    pub(crate) fn stop_session(&mut self, id: TabId, failure: SessionFailure) -> bool {
        let palette = self.palette;
        let Some(tab) = self.get_mut(id).filter(|tab| tab.failure.is_none()) else {
            return false;
        };
        tab.pty.take();
        // The minimum legal diagnostic grid cannot exceed any existing reservation.
        tab.terminal = Terminal::with_scrollback(id, 1, 2, 0).expect("fixed diagnostic dimensions");
        tab.terminal.set_palette(palette);
        tab.terminal
            .feed(id, b"\x1b[?25l")
            .expect("fixed diagnostic control");
        tab.selections = crate::selection::TabSelections::new(id);
        tab.selection_epoch = None;
        tab.preedit = None;
        tab.title = failure.title().into();
        tab.failure = Some(failure);
        tab.reserved_rows = 1;
        tab.reserved_columns = 2;
        tab.reserved_cells = 4;
        tab.dirty = true;
        true
    }
}
