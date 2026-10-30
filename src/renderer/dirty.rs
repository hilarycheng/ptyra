//! Pure visibility and damage gate for a view's frame and draw commands.
use super::{
    CellDrawCommand, CellLayout, CursorDrawCommand, RenderInput, RenderOutput, Renderer,
    cursor_draw_command, draw_commands,
    overlay::{OverlayCommand, Preedit, overlay_commands},
};
use crate::{
    clock::Clock,
    damage::Rect,
    error::AppError,
    frame::{FramePlan, FrameScheduler},
    terminal::TerminalSnapshot,
    window::WindowSize,
};

pub type PreparedFrame<Frame> = (FramePlan, RenderOutput<Frame>);

/// Keep a separate scheduler per view, using physical-pixel bounds and damage.
/// Hidden views retain damage without consuming a frame; a failed render
/// restores its damage for the next attempt. `size` is the logical surface size.
pub fn render_visible_dirty<R: Renderer>(
    frames: &mut FrameScheduler,
    clock: &impl Clock,
    visible: bool,
    renderer: &mut R,
    size: WindowSize,
    terminal: TerminalSnapshot<'_>,
) -> Result<Option<PreparedFrame<R::Frame>>, AppError> {
    if !visible {
        frames.suspend();
        return Ok(None);
    }
    let Some(plan) = frames.schedule(clock)? else {
        return Ok(None);
    };
    let input = RenderInput {
        window: plan.window,
        size,
        terminal,
        damage: plan.damage,
    };
    match renderer.render(input) {
        Ok(output) => Ok(Some((plan, output))),
        Err(error) => {
            frames.cancel(plan.callback);
            Err(error)
        }
    }
}

/// Retain original glyph geometry, including a wide glyph whose lead cell is
/// outside damage. The backend must scissor painting to the damage rectangle.
pub fn dirty_draw_commands<'a>(
    snapshot: TerminalSnapshot<'a>,
    layout: &'a CellLayout,
    damage: Rect,
) -> impl Iterator<Item = CellDrawCommand<'a>> + 'a {
    draw_commands(snapshot, layout).filter(move |command| {
        command.rect.intersects(damage) || command.glyph_rect.intersects(damage)
    })
}

pub fn dirty_cursor_draw_command(
    snapshot: TerminalSnapshot<'_>,
    layout: &CellLayout,
    damage: Rect,
) -> Option<CursorDrawCommand> {
    cursor_draw_command(snapshot, layout).filter(|command| command.rect.intersects(damage))
}

pub fn dirty_overlay_commands<'a>(
    snapshot: TerminalSnapshot<'a>,
    layout: &'a CellLayout,
    preedit: Option<Preedit<'a>>,
    damage: Rect,
) -> Result<impl Iterator<Item = OverlayCommand<'a>> + 'a, AppError> {
    Ok(
        overlay_commands(snapshot, layout, preedit)?.filter(move |command| match command {
            OverlayCommand::Selection { rect } => rect.intersects(damage),
            OverlayCommand::Preedit { clip, .. } => clip.intersects(damage),
        }),
    )
}

#[cfg(test)]
mod tests;
