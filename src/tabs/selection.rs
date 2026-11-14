use super::Tab;
use crate::selection::Screen;
use alacritty_terminal::{grid::Dimensions, index::Point, selection::SelectionRange};

pub(crate) enum SelectionAction {
    Press,
    Motion,
    Release,
    Word,
    Line,
}
impl<P> Tab<P> {
    pub(crate) fn begin_selection_snapshot(&mut self) -> bool {
        self.frozen_selection = None;
        self.saved_selection = None;
        self.sync_selection_epoch();
        self.frozen_selection = self.terminal.selection_snapshot();
        self.frozen_selection.is_some()
    }

    pub(crate) fn frozen_selection(&self) -> Option<&crate::terminal::SelectionSnapshot> {
        let frozen = self.frozen_selection.as_ref()?;
        let grid = self.terminal.grid()?;
        (self.failure.is_none()
            && grid.screen_lines() == frozen.grid.screen_lines()
            && self.terminal.selection_layout_epoch() == Some(frozen.layout_epoch)
            && grid.columns() == frozen.grid.columns()
            && Screen::from_mode(self.terminal.mode()?) == frozen.screen)
            .then_some(frozen)
    }

    pub(crate) fn selection_owner(&self) -> Option<(Screen, u64, usize)> {
        if let Some(frozen) = self.frozen_selection() {
            Some((frozen.screen, self.selection_epoch?, frozen.offset))
        } else {
            Some((
                Screen::from_mode(self.terminal.mode()?),
                self.terminal.content_epoch()?,
                self.terminal.grid()?.display_offset(),
            ))
        }
    }

    pub(crate) fn copy_selection_text(&mut self, limit: usize) -> Result<String, &'static str> {
        if let Some(saved) = &self.saved_selection {
            let text = saved.as_ref().map_err(|error| *error)?;
            if text.len() > limit {
                return Err("selection exceeds clipboard limit");
            }
            let mut copy = String::new();
            copy.try_reserve_exact(text.len())
                .map_err(|_| "selection unavailable")?;
            copy.push_str(text);
            return Ok(copy);
        }
        let range = self.selection_range().ok_or("no valid selection")?;
        match self.frozen_selection() {
            Some(frozen) => frozen.copy(range, limit),
            None => self.terminal.copy_range_text_with_limit(range, limit),
        }
        .ok_or("selection exceeds clipboard limit")
    }

    pub(crate) fn selection_overlay_snapshot(
        &self,
    ) -> Option<crate::terminal::TerminalSnapshot<'_>> {
        let mut snapshot = self.terminal.snapshot()?;
        snapshot.content.selection = self.selection_range();
        if let Some(frozen) = self.frozen_selection() {
            snapshot.content.display_iter = frozen.grid.display_iter();
            snapshot.content.display_offset = 0;
            snapshot.content.selection = snapshot
                .content
                .selection
                .and_then(|range| frozen.range(range));
        }
        Some(snapshot)
    }

    pub(super) fn sync_selection_epoch(&mut self) {
        if self.frozen_selection().is_some() {
            return;
        }
        let epoch = self.terminal.content_epoch();
        if self.selection_epoch != epoch {
            self.selections = crate::selection::TabSelections::new(self.id);
            self.selection_epoch = epoch;
        }
    }
    pub fn selection_range(&self) -> Option<SelectionRange> {
        if let Some(frozen) = self.frozen_selection() {
            let range = self.selections.range(self.id, frozen.screen)?;
            let mut range = frozen.range(range)?;
            range.start.line.0 -= frozen.offset as i32;
            range.end.line.0 -= frozen.offset as i32;
            return Some(range);
        }
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
        if matches!(
            action,
            SelectionAction::Press | SelectionAction::Word | SelectionAction::Line
        ) {
            self.saved_selection = None;
        }
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
        self.frozen_selection = None;
        self.selections
            .selection_mut(self.id, screen)
            .expect("owned selection")
            .cancel();
        self.dirty = true;
    }
}

impl<P: super::TabPty> super::TabRegistry<P> {
    pub(crate) fn save_selection(
        &mut self,
        id: crate::TabId,
        limit: usize,
    ) -> Result<(), &'static str> {
        let used: usize = self
            .entries
            .iter()
            .filter(|tab| tab.id != id)
            .filter_map(|tab| tab.saved_selection.as_ref()?.as_ref().ok())
            .map(String::len)
            .sum();
        let tab = self.get_mut(id).ok_or("selection unavailable")?;
        let text = tab.copy_selection_text(
            limit.min(crate::clipboard::write::MAX_CLIPBOARD_BYTES.saturating_sub(used)),
        );
        let result = text.as_ref().map(|_| ()).map_err(|error| *error);
        tab.saved_selection = Some(text);
        tab.frozen_selection = None;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId,
        clipboard::write::MAX_CLIPBOARD_BYTES,
        tabs::{TabRegistry, tests::FakePty},
    };
    use alacritty_terminal::index::{Column, Line};

    #[test]
    fn selection_snapshots_share_a_bounded_text_budget() {
        let mut tabs = TabRegistry::<FakePty>::with_scrollback(0).unwrap();
        for id in [1, 2] {
            tabs.insert(TabId(id), 1, 8).unwrap();
        }
        tabs.get_mut(TabId(1)).unwrap().saved_selection =
            Some(Ok("x".repeat(MAX_CLIPBOARD_BYTES - 1)));
        let point = Point::new(Line(0), Column(0));
        let tab = tabs.get_mut(TabId(2)).unwrap();
        tab.terminal_mut().feed(TabId(2), "中".as_bytes()).unwrap();
        tab.select(Screen::Main, SelectionAction::Word, point);
        assert_eq!(
            tabs.save_selection(TabId(2), MAX_CLIPBOARD_BYTES),
            Err("selection exceeds clipboard limit")
        );
        tabs.close(TabId(1));
        tabs.get_mut(TabId(2))
            .unwrap()
            .select(Screen::Main, SelectionAction::Word, point);
        assert!(tabs.save_selection(TabId(2), MAX_CLIPBOARD_BYTES).is_ok());
        assert_eq!(
            tabs.get_mut(TabId(2))
                .unwrap()
                .copy_selection_text(3)
                .unwrap(),
            "中"
        );
        *tabs.get_mut(TabId(2)).unwrap().selections_mut() =
            crate::selection::TabSelections::new(TabId(2));
        assert_eq!(
            tabs.get_mut(TabId(2)).unwrap().copy_selection_text(3),
            Err("no valid selection")
        );
    }
}
