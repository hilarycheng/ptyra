//! Renderer data contract; no PTY handles, Wayland objects or platform I/O.
use crate::{
    TabId, WindowId,
    damage::Rect,
    error::{AppError, Component},
    geometry::Scale,
    terminal::TerminalSnapshot,
    window::WindowSize,
};
use std::io::ErrorKind;

/// Fixed cell advances anchored at the viewport's top-left, in physical pixels.
#[derive(Debug)]
pub struct CellLayout {
    bounds: WindowSize,
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
}

/// One synchronous render input. Payload-bearing types deliberately have no Debug.
pub struct RenderInput<'a> {
    pub window: WindowId,
    /// Acknowledged surface size in logical units, not a pixel allocation size.
    pub size: WindowSize,
    pub terminal: TerminalSnapshot<'a>,
}

/// Prepared frame data; the platform adapter owns submission and presentation.
pub struct RenderOutput<Frame> {
    pub window: WindowId,
    pub tab: TabId,
    /// The logical size this frame was prepared for.
    pub size: WindowSize,
    pub frame: Frame,
}

/// Prepares frames from data only; implementations must not perform PTY/Wayland I/O.
pub trait Renderer {
    /// Storage stays backend-specific so this contract does not select SHM or GPU.
    type Frame;

    fn render(&mut self, input: RenderInput<'_>) -> Result<RenderOutput<Self::Frame>, AppError>;
}

#[cfg(test)]
mod tests;
