use crate::{damage::Rect, ime::ImePreedit, renderer::CellLayout, terminal::TerminalSnapshot};
use unicode_width::UnicodeWidthChar;

pub(in crate::runtime) struct Composition<'a> {
    pub text: &'a str,
    pub clip: Rect,
    pub begin: Option<u32>,
    pub end: Option<u32>,
    pub anchor: Rect,
}

impl Composition<'_> {
    pub fn surface_anchor(&self, origin: u32, scale: u32) -> Rect {
        let x = self.anchor.x as u32;
        let y = self.anchor.y as u32 + origin;
        Rect {
            x: (x / scale) as i32,
            y: (y / scale) as i32,
            width: (x + self.anchor.width).div_ceil(scale) - x / scale,
            height: (y + self.anchor.height).div_ceil(scale) - y / scale,
        }
    }
}

// Keep one visible row, pan at character boundaries to keep the active caret visible.
pub(in crate::runtime) fn compose<'a>(
    snapshot: TerminalSnapshot<'_>,
    layout: &CellLayout,
    preedit: &'a ImePreedit,
) -> super::Result<Option<Composition<'a>>> {
    preedit.validate().map_err(|_| "invalid IME overlay")?;
    let point = snapshot.content.cursor.point;
    let Some(row) = snapshot
        .content
        .display_offset
        .checked_add_signed(point.line.0 as isize)
        .filter(|row| *row < snapshot.rows)
    else {
        return Ok(None);
    };
    let Some(mut anchor) = layout.cell_rect(row, point.column.0) else {
        return Ok(None);
    };
    let cell = layout.cell_rect(row, 0).ok_or("invalid IME layout")?;
    let last = layout
        .cell_rect(row, snapshot.columns - 1)
        .unwrap_or(anchor);
    let columns = (last.x as u32 + last.width) / cell.width;
    if columns == 0 {
        return Ok(None);
    }
    let width = |text: &str| {
        text.chars()
            .filter_map(UnicodeWidthChar::width)
            .sum::<usize>() as u32
    };
    let offset = |byte| {
        usize::try_from(byte)
            .ok()
            .and_then(|i| preedit.text.get(..i))
            .map(width)
    };
    let begin = offset(preedit.cursor_begin);
    let end = offset(preedit.cursor_end);
    let start =
        (point.column.0 as u32).min(columns.saturating_sub(width(&preedit.text).saturating_add(1)));
    let available = columns - start;
    let caret = begin.unwrap_or(0);
    let skip = caret.saturating_add(1).saturating_sub(available);
    let (mut byte, mut column, mut skipped) = (0, 0, 0);
    for (index, ch) in preedit.text.char_indices() {
        let span = ch.width().unwrap_or(0) as u32;
        if span > 0 && column <= caret && column >= skip {
            byte = index;
            skipped = column;
            break;
        }
        column += span;
        if column <= caret && column >= skip {
            byte = index + ch.len_utf8();
            skipped = column;
        }
    }
    if skip == 0 {
        byte = 0;
        skipped = 0;
    }
    let clip = Rect {
        x: (start * cell.width) as i32,
        y: cell.y,
        width: available * cell.width,
        height: cell.height,
    };
    let begin = begin.map(|n| n.saturating_sub(skipped));
    let end = end.map(|n| n.saturating_sub(skipped));
    anchor.x = clip.x + (begin.unwrap_or(0).min(available - 1) * cell.width) as i32;
    anchor.width = cell.width;
    Ok(Some(Composition {
        text: &preedit.text[byte..],
        clip,
        begin,
        end,
        anchor,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, geometry::Scale, renderer::CellLayout, terminal::Terminal, window::WindowSize,
    };

    #[test]
    fn runtime_ime_anchor_tracks_viewport_and_long_preedit() {
        let tab = TabId(1);
        let mut terminal = Terminal::new(tab, 2, 8).unwrap();
        terminal.feed(tab, b"\x1b[2;8H").unwrap();
        let preedit = crate::ime::ImePreedit {
            text: "A\u{301}中文中文中文".into(),
            cursor_begin: 21,
            cursor_end: 21,
        };
        for scale in [1, 2] {
            let layout = CellLayout::new(
                WindowSize::new(80, 40).unwrap(),
                Scale::new(scale).unwrap(),
                10.0,
                20.0,
            )
            .unwrap();
            let view = compose(terminal.snapshot().unwrap(), &layout, &preedit)
                .unwrap()
                .unwrap();
            assert_eq!(view.clip.x, 0);
            assert!(view.text.is_char_boundary(0));
            assert!(view.text.len() < preedit.text.len());
            assert!(view.anchor.x >= view.clip.x);
            assert!(view.anchor.x as u32 + view.anchor.width <= 80 * scale as u32);
            assert_eq!(view.anchor.y, 20 * scale);
            let anchor = view.surface_anchor(20 * scale as u32, scale as u32);
            assert_eq!(anchor.y, 40);
            assert_eq!(anchor.height, 20);
            assert!(anchor.x < 80);
            assert_eq!(terminal.grid().unwrap().cursor.point.column.0, 7);
        }
        let layout = CellLayout::new(
            WindowSize::new(80, 40).unwrap(),
            Scale::new(1).unwrap(),
            10.0,
            20.0,
        )
        .unwrap();
        let selected = crate::ime::ImePreedit {
            text: "A\u{301}中文".into(),
            cursor_begin: 3,
            cursor_end: 6,
        };
        let view = compose(terminal.snapshot().unwrap(), &layout, &selected)
            .unwrap()
            .unwrap();
        assert_eq!((view.begin, view.end), (Some(1), Some(3)));
        assert_eq!(view.clip.x, 20);
        let narrow = CellLayout::new(
            WindowSize::new(40, 40).unwrap(),
            Scale::new(1).unwrap(),
            10.0,
            20.0,
        )
        .unwrap();
        terminal.resize(2, 4).unwrap();
        let view = compose(terminal.snapshot().unwrap(), &narrow, &preedit)
            .unwrap()
            .unwrap();
        assert!(view.anchor.x < 40);
        assert!(view.text.len() < preedit.text.len());
        terminal.feed(tab, b"\r\n\r\n").unwrap();
        terminal
            .scroll_viewport(alacritty_terminal::grid::Scroll::Top)
            .unwrap();
        assert!(
            compose(terminal.snapshot().unwrap(), &layout, &preedit)
                .unwrap()
                .is_none()
        );
    }
}
