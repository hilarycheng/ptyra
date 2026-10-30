//! Pure per-Tab mouse drag selection state.
use crate::TabId;
use alacritty_terminal::{
    grid::{Dimensions, Grid},
    index::{Column, Line, Point},
    selection::SelectionRange,
    term::cell::{Cell, Flags},
};

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

    /// Select a word on one grid row. ASCII alphanumeric/underscore cells group
    /// together; other narrow Unicode alphanumerics group together; wide glyphs,
    /// punctuation and whitespace each select one cell. Combining marks remain
    /// attached to their base cell. A trailing wide spacer selects its lead.
    pub fn double_click(
        &mut self,
        tab: TabId,
        grid: &Grid<Cell>,
        point: Point,
    ) -> Option<SelectionRange> {
        if tab != self.tab {
            return None;
        }
        let range = word_range(grid, point)?;
        self.dragging = None;
        self.settled = Some(range);
        Some(range)
    }

    /// Select the full logical line, including adjacent soft-wrapped rows.
    pub fn triple_click(
        &mut self,
        tab: TabId,
        grid: &Grid<Cell>,
        point: Point,
    ) -> Option<SelectionRange> {
        if tab != self.tab
            || point.line < grid.topmost_line()
            || point.line > grid.bottommost_line()
            || point.column.0 >= grid.columns()
        {
            return None;
        }
        let last = Column(grid.columns().checked_sub(1)?);
        let mut start = point.line;
        while start > grid.topmost_line()
            && grid[Point::new(start - 1, last)]
                .flags
                .contains(Flags::WRAPLINE)
        {
            start -= 1;
        }
        let mut end = point.line;
        while end < grid.bottommost_line()
            && grid[Point::new(end, last)].flags.contains(Flags::WRAPLINE)
        {
            end += 1;
        }
        let range = SelectionRange::new(Point::new(start, Column(0)), Point::new(end, last), false);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Ascii,
    Unicode,
    Single,
}

fn word_class(cell: &Cell) -> WordClass {
    if cell
        .flags
        .intersects(Flags::WIDE_CHAR | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
    {
        WordClass::Single
    } else if cell.c.is_ascii_alphanumeric() || cell.c == '_' {
        WordClass::Ascii
    } else if cell.c.is_alphanumeric() {
        WordClass::Unicode
    } else {
        WordClass::Single
    }
}

fn word_range(grid: &Grid<Cell>, mut point: Point) -> Option<SelectionRange> {
    let columns = grid.columns();
    if point.line < grid.topmost_line()
        || point.line > grid.bottommost_line()
        || point.column.0 >= columns
    {
        return None;
    }
    let flags = grid[point].flags;
    if flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
        return None;
    }
    if flags.contains(Flags::WIDE_CHAR_SPACER) {
        point.column.0 = point.column.0.checked_sub(1)?;
        if !grid[point].flags.contains(Flags::WIDE_CHAR) {
            return None;
        }
    }
    let class = word_class(&grid[point]);
    if class == WordClass::Single {
        return Some(SelectionRange::new(point, point, false));
    }
    let mut start = point.column.0;
    while start > 0 && word_class(&grid[Point::new(point.line, Column(start - 1))]) == class {
        start -= 1;
    }
    let mut end = point.column.0;
    while end + 1 < columns && word_class(&grid[Point::new(point.line, Column(end + 1))]) == class {
        end += 1;
    }
    Some(SelectionRange::new(
        Point::new(point.line, Column(start)),
        Point::new(point.line, Column(end)),
        false,
    ))
}

/// Copy selected cells into logical-line fragments. Hard-line separators are
/// deliberately left to the caller; a soft wrap joins adjacent rows here.
pub fn copy_logical_lines(grid: &Grid<Cell>, range: SelectionRange) -> Option<Vec<String>> {
    const MAX_COPY_BYTES: usize = 16 * 1024 * 1024;
    let columns = grid.columns();
    if range.is_block
        || range.start > range.end
        || range.start.line < grid.topmost_line()
        || range.end.line > grid.bottommost_line()
        || range.start.column.0 >= columns
        || range.end.column.0 >= columns
    {
        return None;
    }

    let last = Column(columns.checked_sub(1)?);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut bytes = 0usize;
    for row in range.start.line.0..=range.end.line.0 {
        let line = Line(row);
        if line > range.start.line
            && !grid[Point::new(line - 1, last)]
                .flags
                .contains(Flags::WRAPLINE)
        {
            lines.push(std::mem::take(&mut current));
        }
        let mut first = if line == range.start.line {
            range.start.column.0
        } else {
            0
        };
        let end = if line == range.end.line {
            range.end.column.0
        } else {
            last.0
        };
        if grid[Point::new(line, Column(first))]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
            && first > 0
        {
            first -= 1;
        }
        for column in first..=end {
            let cell = &grid[Point::new(line, Column(column))];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            for c in std::iter::once(cell.c).chain(cell.zerowidth().into_iter().flatten().copied())
            {
                bytes = bytes.checked_add(c.len_utf8())?;
                if bytes > MAX_COPY_BYTES {
                    return None;
                }
                current.push(c);
            }
        }
    }
    lines.push(current);
    Some(lines)
}

#[cfg(test)]
mod tests;
