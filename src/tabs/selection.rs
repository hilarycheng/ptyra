use super::Tab;
use crate::selection::Screen;
use alacritty_terminal::{index::Point, selection::SelectionRange};

pub(crate) enum SelectionAction {
    Press,
    Motion,
    Release,
    Word,
    Line,
}
impl<P> Tab<P> {
    pub(super) fn sync_selection_epoch(&mut self) {
        let epoch = self.terminal.content_epoch();
        if self.selection_epoch != epoch {
            self.selections = crate::selection::TabSelections::new(self.id);
            self.selection_epoch = epoch;
        }
    }
    pub fn selection_range(&self) -> Option<SelectionRange> {
        if self.selection_epoch.is_none() || self.selection_epoch != self.terminal.content_epoch() {
            return None;
        }
        let screen = Screen::from_mode(self.terminal.mode()?);
        crate::selection::whole_glyph_range(
            self.terminal.grid()?,
            self.selections.range(self.id, screen)?,
        )
    }
    pub(crate) fn select(&mut self, screen: Screen, action: SelectionAction, point: Point) -> bool {
        self.sync_selection_epoch();
        if self.selection_epoch.is_none() {
            return false;
        }
        let selection = self
            .selections
            .selection_mut(self.id, screen)
            .expect("owned selection");
        let changed = match action {
            SelectionAction::Press => selection.press(self.id, point),
            SelectionAction::Motion => selection.motion(self.id, point),
            SelectionAction::Release => selection.release(self.id, point),
            SelectionAction::Word => self
                .terminal
                .grid()
                .and_then(|grid| selection.double_click(self.id, grid, point)),
            SelectionAction::Line => self
                .terminal
                .grid()
                .and_then(|grid| selection.triple_click(self.id, grid, point)),
        }
        .is_some();
        self.dirty |= changed;
        changed
    }
    pub(crate) fn cancel_selection_drag(&mut self, screen: Screen) {
        self.selections
            .selection_mut(self.id, screen)
            .expect("owned selection")
            .cancel();
        self.dirty = true;
    }
}
