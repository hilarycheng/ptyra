//! Pure per-Tab mouse drag selection state.
use crate::TabId;
use alacritty_terminal::{index::Point, selection::SelectionRange};

/// Own one instance per Tab. Input points are grid coordinates, so callers
/// translate the hit-tested viewport row by the current display offset.
pub struct DragSelection {
    tab: TabId,
    settled: Option<SelectionRange>,
    dragging: Option<(Point, Point)>,
}

impl DragSelection {
    pub fn new(tab: TabId) -> Self {
        Self {
            tab,
            settled: None,
            dragging: None,
        }
    }

    pub fn range(&self) -> Option<SelectionRange> {
        self.dragging
            .map(|(start, end)| normalized(start, end))
            .or(self.settled)
    }

    pub fn press(&mut self, tab: TabId, point: Point) -> Option<SelectionRange> {
        if tab != self.tab {
            return None;
        }
        self.settled = None;
        self.dragging = Some((point, point));
        self.range()
    }

    pub fn motion(&mut self, tab: TabId, point: Point) -> Option<SelectionRange> {
        if tab != self.tab {
            return None;
        }
        let (start, _) = self.dragging?;
        self.dragging = Some((start, point));
        self.range()
    }

    pub fn release(&mut self, tab: TabId, point: Point) -> Option<SelectionRange> {
        let range = self.motion(tab, point)?;
        self.dragging = None;
        self.settled = Some(range);
        Some(range)
    }

    /// Call on focus loss or Tab switch. Completed selection remains owned by this Tab.
    pub fn cancel(&mut self) {
        self.dragging = None;
    }
}

fn normalized(start: Point, end: Point) -> SelectionRange {
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    SelectionRange::new(start, end, false)
}

#[cfg(test)]
mod tests;
