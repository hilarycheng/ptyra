//! IME anchors use surface-local units, independently of cursor painting or blinking.
use super::{ImeRequest, ImeState};
use crate::{clipboard::SeatId, input::KeyboardFocus};
use crate::{damage::Rect, renderer::CellLayout, terminal::TerminalSnapshot};

impl ImeState {
    /// Call with the rebuilt layout after resize/scale changes and after cursor/focus changes.
    /// Send both returned requests in order before processing more events.
    pub fn update_cursor_rectangle(
        &mut self,
        seat: SeatId,
        keyboard: &KeyboardFocus,
        snapshot: TerminalSnapshot<'_>,
        layout: &CellLayout,
    ) -> Option<[ImeRequest; 2]> {
        self.update_cursor_rect(
            seat,
            keyboard,
            snapshot.tab,
            cursor_rectangle(snapshot, layout),
        )
    }

    pub(crate) fn update_cursor_rect(
        &mut self,
        seat: SeatId,
        keyboard: &KeyboardFocus,
        tab: crate::TabId,
        rect: Option<Rect>,
    ) -> Option<[ImeRequest; 2]> {
        let target = self.target_for(seat, self.surface?, keyboard)?;
        let focus = self.capture_focus(seat, target.window, keyboard);
        if !self.matches_focus(&focus, keyboard) || tab != target.tab {
            return None;
        }
        if rect == self.cursor_rectangle {
            return None;
        }
        self.cursor_rectangle = rect;
        let rect = rect?;
        self.commit_serial = self.commit_serial.wrapping_add(1);
        Some([ImeRequest::SetCursorRectangle(rect), ImeRequest::Commit])
    }
}

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
    fn ime_cursor_rect_recomputes_after_resize() {
        use crate::{
            WindowId,
            clipboard::SeatId,
            ime::{ImeRequest, ImeState},
            input::KeyboardFocus,
            wayland::{Capabilities, Global},
        };
        let seat = SeatId(1);
        let window = WindowId(1);
        let tab = TabId(7);
        let caps = Capabilities::from_globals(&[Global {
            name: 3,
            interface: "zwp_text_input_manager_v3",
            version: 1,
        }]);
        let mut focus = KeyboardFocus::default();
        focus.enter(window, tab);
        let mut ime = ImeState::new(seat, &caps);
        let mut terminal = Terminal::new(tab, 4, 10).unwrap();
        terminal.feed(tab, b"\x1b[2;4H").unwrap();
        let layout = |width, height, scale| {
            CellLayout::new(
                WindowSize::new(width, height).unwrap(),
                Scale::new(scale).unwrap(),
                10.25,
                20.25,
            )
            .unwrap()
        };
        let first = layout(100, 80, 1);
        assert!(
            ime.update_cursor_rectangle(seat, &focus, terminal.snapshot().unwrap(), &first)
                .is_none()
        );
        ime.enter(seat, window);
        ime.sync_enabled(seat, &focus, true);
        let mut previous = None;
        for current in [
            first,
            layout(100, 80, 3),
            layout(35, 25, 3),
            layout(100, 80, 3),
        ] {
            let rect = cursor_rectangle(terminal.snapshot().unwrap(), &current).unwrap();
            assert_ne!(previous, Some(rect));
            let serial = ime.commit_serial();
            assert_eq!(
                ime.update_cursor_rectangle(seat, &focus, terminal.snapshot().unwrap(), &current),
                Some([ImeRequest::SetCursorRectangle(rect), ImeRequest::Commit])
            );
            assert_eq!(ime.commit_serial(), serial + 1);
            assert!(
                ime.update_cursor_rectangle(seat, &focus, terminal.snapshot().unwrap(), &current)
                    .is_none()
            );
            assert_eq!(ime.commit_serial(), serial + 1);
            previous = Some(rect);
        }
        let current = layout(100, 80, 3);
        let wrong = Terminal::new(TabId(8), 4, 10).unwrap();
        assert!(
            ime.update_cursor_rectangle(seat, &focus, wrong.snapshot().unwrap(), &current)
                .is_none()
        );
        assert!(
            ime.update_cursor_rectangle(SeatId(2), &focus, terminal.snapshot().unwrap(), &current)
                .is_none()
        );
        ime.sync_enabled(seat, &focus, false);
        assert!(
            ime.update_cursor_rectangle(seat, &focus, terminal.snapshot().unwrap(), &current)
                .is_none()
        );
        ime.sync_enabled(seat, &focus, true);
        assert!(
            ime.update_cursor_rectangle(seat, &focus, terminal.snapshot().unwrap(), &current)
                .is_some()
        );
    }

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
