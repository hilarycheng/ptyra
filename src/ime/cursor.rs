//! IME anchors use surface-local units, independently of cursor painting or blinking.
use crate::{damage::Rect, renderer::CellLayout, terminal::TerminalSnapshot};

/// Full cursor cell at the current grid origin (surface top-left), including while the cursor is hidden.
pub fn cursor_rectangle(snapshot: TerminalSnapshot<'_>, layout: &CellLayout) -> Option<Rect> {
    let point = snapshot.content.cursor.point;
    let row = snapshot
        .content
        .display_offset
        .checked_add_signed(point.line.0 as isize)?;
    if row >= snapshot.rows || point.column.0 >= snapshot.columns {
        return None;
    }
    layout.surface_cell_rect(row, point.column.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, geometry::Scale, terminal::Terminal, window::WindowSize};

    #[test]
    fn ime_cursor_rect_tracks_text_cell() {
        let tab = TabId(1);
        let mut terminal = Terminal::new(tab, 4, 10).unwrap();
        for scale in [1, 2, 3] {
            let layout = CellLayout::new(
                WindowSize::new(100, 80).unwrap(),
                Scale::new(scale).unwrap(),
                10.0,
                20.0,
            )
            .unwrap();
            terminal.feed(tab, "\x1b[2;1H中a".as_bytes()).unwrap();
            assert_eq!(
                cursor_rectangle(terminal.snapshot().unwrap(), &layout),
                Some(Rect {
                    x: 30,
                    y: 20,
                    width: 10,
                    height: 20
                })
            );
            // Cursor shape and visibility do not change the input anchor.
            terminal.feed(tab, b"\x1b[?25l\x1b[6 q\x1b[3;5H").unwrap();
            assert_eq!(
                cursor_rectangle(terminal.snapshot().unwrap(), &layout),
                Some(Rect {
                    x: 40,
                    y: 40,
                    width: 10,
                    height: 20
                })
            );
        }
        let clipped = CellLayout::new(
            WindowSize::new(45, 45).unwrap(),
            Scale::new(2).unwrap(),
            10.0,
            20.0,
        )
        .unwrap();
        assert_eq!(
            cursor_rectangle(terminal.snapshot().unwrap(), &clipped),
            Some(Rect {
                x: 40,
                y: 40,
                width: 5,
                height: 5
            })
        );
        terminal.feed(tab, b"\x1b[4;10H").unwrap();
        assert_eq!(
            cursor_rectangle(terminal.snapshot().unwrap(), &clipped),
            None
        );
    }
}
