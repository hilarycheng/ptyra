//! Renderer data contract; no PTY handles, Wayland objects or platform I/O.
pub mod dirty;
pub mod overlay;

use crate::{
    TabId, WindowId,
    damage::Rect,
    error::{AppError, Component},
    geometry::Scale,
    input::Point,
    terminal::TerminalSnapshot,
    window::WindowSize,
};
use alacritty_terminal::{
    term::{cell::Flags, color::Colors},
    vte::ansi::{Color, CursorShape},
};
use std::io::ErrorKind;

/// Fixed cell advances anchored at the viewport's top-left, in physical pixels.
#[derive(Debug)]
pub struct CellLayout {
    bounds: WindowSize,
    scale: Scale,
    width: f64,
    height: f64,
}

impl CellLayout {
    /// Takes a logical window size and logical cell advances supplied by the caller.
    /// Each scaled advance must be finite and within 1..=i32::MAX pixels.
    /// Recreate after resize or scale changes; this does not calculate font metrics.
    pub fn new(
        size: WindowSize,
        scale: Scale,
        cell_width: f64,
        cell_height: f64,
    ) -> Result<Self, AppError> {
        let invalid = || AppError::new(Component::Renderer, ErrorKind::InvalidInput);
        let width = cell_width * f64::from(scale.factor());
        let height = cell_height * f64::from(scale.factor());
        let valid = 1.0..=f64::from(i32::MAX);
        if !valid.contains(&width) || !valid.contains(&height) {
            return Err(invalid());
        }
        let bounds = scale.to_physical(size).map_err(|_| invalid())?;
        Ok(Self {
            bounds,
            scale,
            width,
            height,
        })
    }

    /// Maps a zero-based viewport row/column to a clipped physical-pixel rectangle.
    /// Edges round to nearest (ties upward); adjacent cells share the same edge.
    /// Fully outside cells return None. Translate any scrollback offset first.
    pub fn cell_rect(&self, row: usize, column: usize) -> Option<Rect> {
        // Clamp before integer conversion, including for extreme cell indices.
        let edge = |index: usize, step: f64, limit: u32| {
            (index as f64 * step).round().min(f64::from(limit)) as u32
        };
        let x = edge(column, self.width, self.bounds.width());
        let y = edge(row, self.height, self.bounds.height());
        if x == self.bounds.width() || y == self.bounds.height() {
            return None;
        }
        let right = edge(column.checked_add(1)?, self.width, self.bounds.width());
        let bottom = edge(row.checked_add(1)?, self.height, self.bounds.height());
        Some(Rect {
            x: x as i32,
            y: y as i32,
            width: right - x,
            height: bottom - y,
        })
    }

    /// The same clipped cell in surface-local units, rounded outward to cover its pixels.
    pub fn surface_cell_rect(&self, row: usize, column: usize) -> Option<Rect> {
        let rect = self.cell_rect(row, column)?;
        let scale = self.scale.factor();
        let x = rect.x as u32 / scale;
        let y = rect.y as u32 / scale;
        Some(Rect {
            x: x as i32,
            y: y as i32,
            width: (rect.x as u32 + rect.width).div_ceil(scale) - x,
            height: (rect.y as u32 + rect.height).div_ceil(scale) - y,
        })
    }

    /// Maps a grid-local physical pixel to a visible cell. Callers subtract any
    /// tab bar/content origin from a surface-local pointer position first.
    /// Exact shared edges belong to the cell on the right/below.
    pub fn hit_test(&self, pixel: Point, rows: usize, columns: usize) -> Option<(usize, usize)> {
        let row = hit_axis(pixel.y(), rows, |row| {
            self.cell_rect(row, 0).map(|rect| {
                (
                    f64::from(rect.y),
                    f64::from(rect.y) + f64::from(rect.height),
                )
            })
        })?;
        let column = hit_axis(pixel.x(), columns, |column| {
            self.cell_rect(0, column)
                .map(|rect| (f64::from(rect.x), f64::from(rect.x) + f64::from(rect.width)))
        })?;
        Some((row, column))
    }
}

fn hit_axis(
    position: f64,
    cells: usize,
    bounds: impl Fn(usize) -> Option<(f64, f64)>,
) -> Option<usize> {
    let (mut low, mut high) = (0, cells);
    while low < high {
        let middle = low + (high - low) / 2;
        match bounds(middle) {
            Some((start, _)) if start <= position => low = middle + 1,
            _ => high = middle,
        }
    }
    let index = low.checked_sub(1)?;
    let (start, end) = bounds(index)?;
    (start <= position && position < end).then_some(index)
}

/// Backend-neutral cell instruction with raw terminal colors and style flags.
/// Paint all cell backgrounds before glyphs so wide glyphs are not covered by spacers.
/// The backend applies INVERSE, skips HIDDEN/spacer glyphs, and interprets other styles.
/// Payload-bearing commands deliberately have no Debug.
pub struct CellDrawCommand<'a> {
    /// Background coverage in clipped physical pixels, including blank/spacer cells.
    pub rect: Rect,
    /// Glyph coverage spans two cells for WIDE_CHAR, clipped to the window.
    pub glyph_rect: Rect,
    pub character: char,
    pub zerowidth: &'a [char],
    pub foreground: Color,
    pub background: Color,
    pub flags: Flags,
    pub underline_color: Option<Color>,
    /// OSC overrides; unresolved named/indexed colors use the backend's configured palette.
    pub colors: &'a Colors,
}

/// Converts a snapshot lazily, without allocating or copying grid/combining/palette data.
/// The caller retains window/tab routing and consumes commands before mutating the terminal.
/// Glyph lookup/rasterization belongs to the backend and can use the shared GlyphCache.
pub fn draw_commands<'a>(
    snapshot: TerminalSnapshot<'a>,
    layout: &'a CellLayout,
) -> impl Iterator<Item = CellDrawCommand<'a>> + 'a {
    let content = snapshot.content;
    content.display_iter.filter_map(move |indexed| {
        let row = content
            .display_offset
            .checked_add_signed(indexed.point.line.0 as isize)?;
        let column = indexed.point.column.0;
        let rect = layout.cell_rect(row, column)?;
        let cell = indexed.cell;
        let mut glyph_rect = rect;
        if cell.flags.contains(Flags::WIDE_CHAR)
            && let Some(next) = column.checked_add(1).and_then(|c| layout.cell_rect(row, c))
        {
            glyph_rect.width += next.width;
        }
        Some(CellDrawCommand {
            rect,
            glyph_rect,
            character: cell.c,
            zerowidth: cell.zerowidth().unwrap_or_default(),
            foreground: cell.fg,
            background: cell.bg,
            flags: cell.flags,
            underline_color: cell.underline_color(),
            colors: content.colors,
        })
    })
}

/// Cursor geometry in physical pixels; the backend owns color and glyph recoloring.
/// shortcut: HollowBlock keeps a clipped envelope, add original edges with an outline backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorDrawCommand {
    pub shape: CursorShape,
    pub rect: Rect,
}

/// Uses the core's cursor shape and wide-cell-normalized position without changing it.
/// Hidden or off-viewport cursors produce no command. No blink/focus scheduling occurs here.
/// shortcut: Beam/Underline use one physical pixel, revisit thickness with backend calibration.
pub fn cursor_draw_command(
    snapshot: TerminalSnapshot<'_>,
    layout: &CellLayout,
) -> Option<CursorDrawCommand> {
    let mut content = snapshot.content;
    let cursor = content.cursor;
    if cursor.shape == CursorShape::Hidden {
        return None;
    }
    let row = content
        .display_offset
        .checked_add_signed(cursor.point.line.0 as isize)?;
    let column = cursor.point.column.0;
    if row >= snapshot.rows || column >= snapshot.columns {
        return None;
    }
    let mut rect = layout.cell_rect(row, column)?;
    let cell = content
        .display_iter
        .find(|cell| cell.point == cursor.point)?;
    if cell.flags.contains(Flags::WIDE_CHAR)
        && let Some(next) = column.checked_add(1).and_then(|c| layout.cell_rect(row, c))
    {
        rect.width += next.width;
    }
    match cursor.shape {
        CursorShape::Beam => rect.width = 1,
        CursorShape::Underline => {
            // Position against the full cell bottom before clipping, so it cannot move upward.
            let y = (row.checked_add(1)? as f64 * layout.height).round() - 1.0;
            if y >= f64::from(layout.bounds.height()) {
                return None;
            }
            rect.y = y as i32;
            rect.height = 1;
        }
        _ => (),
    }
    Some(CursorDrawCommand {
        shape: cursor.shape,
        rect,
    })
}

/// One synchronous render input. Payload-bearing types deliberately have no Debug.
pub struct RenderInput<'a> {
    pub window: WindowId,
    /// Acknowledged surface size in logical units, not a pixel allocation size.
    pub size: WindowSize,
    pub terminal: TerminalSnapshot<'a>,
    /// Physical-pixel scissor; only intersecting content may be painted.
    pub damage: Rect,
}

/// Prepared frame data; the platform adapter owns submission and presentation.
pub struct RenderOutput<Frame> {
    pub window: WindowId,
    pub tab: TabId,
    /// The logical size this frame was prepared for.
    pub size: WindowSize,
    pub frame: Frame,
}

/// Prepares frames from data only; implementations must scissor to input.damage
/// and must not perform PTY/Wayland I/O.
pub trait Renderer {
    /// Storage stays backend-specific so this contract does not select SHM or GPU.
    type Frame;

    fn render(&mut self, input: RenderInput<'_>) -> Result<RenderOutput<Self::Frame>, AppError>;
}

#[cfg(test)]
mod tests;
