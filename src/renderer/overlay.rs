//! Selection and preedit commands kept separate from terminal cell data.
use super::CellLayout;
use crate::{
    TabId,
    damage::Rect,
    error::{AppError, Component},
    terminal::TerminalSnapshot,
};
use alacritty_terminal::{term::cell::Flags, vte::ansi::CursorShape};
use std::io::ErrorKind;

pub const MAX_PREEDIT_BYTES: usize = 4 * 1024;

/// Borrowed UI state, not bytes to feed into the terminal. No payload Debug.
/// The IME caller must supply current, focus-validated state; this layer only checks TabId.
pub struct Preedit<'a> {
    pub tab: TabId,
    pub text: &'a str,
}

/// Paint after the base grid: selection highlights first, preedit last.
/// The backend owns colors, Unicode shaping and glyph rendering; no payload Debug.
pub enum OverlayCommand<'a> {
    Selection {
        rect: Rect,
    },
    /// Draw the complete literal text from clip's top-left, using clip as a scissor.
    /// Never interpret the text as terminal escape sequences or send it to a PTY.
    Preedit {
        text: &'a str,
        clip: Rect,
    },
}

/// Builds a separate lazy layer without allocating or modifying the terminal grid.
/// Rejects an invalid preedit before returning any commands; never truncates text.
pub fn overlay_commands<'a>(
    snapshot: TerminalSnapshot<'a>,
    layout: &'a CellLayout,
    preedit: Option<Preedit<'a>>,
) -> Result<impl Iterator<Item = OverlayCommand<'a>> + 'a, AppError> {
    if preedit.as_ref().is_some_and(|preedit| {
        preedit.tab != snapshot.tab || preedit.text.len() > MAX_PREEDIT_BYTES
    }) {
        return Err(AppError::new(Component::Renderer, ErrorKind::InvalidInput));
    }
    // shortcut: preedit uses one clipped row, add wrapping with backend text layout if needed.
    let preedit = preedit
        .filter(|preedit| !preedit.text.is_empty())
        .and_then(|preedit| {
            let point = snapshot.content.cursor.point;
            let row = snapshot
                .content
                .display_offset
                .checked_add_signed(point.line.0 as isize)?;
            if row >= snapshot.rows || point.column.0 >= snapshot.columns {
                return None;
            }
            let mut clip = layout.cell_rect(row, point.column.0)?;
            let right = layout
                .cell_rect(row, snapshot.columns - 1)
                .map_or(layout.bounds.width(), |last| last.x as u32 + last.width);
            clip.width = right - clip.x as u32;
            Some(OverlayCommand::Preedit {
                text: preedit.text,
                clip,
            })
        });
    let content = snapshot.content;
    let selection = content.display_iter.filter_map(move |indexed| {
        let range = content.selection?;
        // Select each wide glyph once; cursor styling must not alter the selection layer.
        if indexed.flags.contains(Flags::WIDE_CHAR_SPACER)
            || !range.contains_cell(&indexed, content.cursor.point, CursorShape::Hidden)
        {
            return None;
        }
        let row = content
            .display_offset
            .checked_add_signed(indexed.point.line.0 as isize)?;
        let column = indexed.point.column.0;
        if row >= snapshot.rows || column >= snapshot.columns {
            return None;
        }
        let mut rect = layout.cell_rect(row, column)?;
        if indexed.flags.contains(Flags::WIDE_CHAR)
            && let Some(next) = column.checked_add(1).and_then(|c| layout.cell_rect(row, c))
        {
            rect.width += next.width;
        }
        Some(OverlayCommand::Selection { rect })
    });
    Ok(selection.chain(preedit))
}

#[cfg(test)]
mod tests;
