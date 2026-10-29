//! Renderer data contract; no PTY handles, Wayland objects or platform I/O.
use crate::{TabId, WindowId, error::AppError, terminal::TerminalSnapshot, window::WindowSize};

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
