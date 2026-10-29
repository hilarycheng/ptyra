use super::{CellLayout, RenderInput, RenderOutput, Renderer};
use crate::{
    TabId, WindowId,
    damage::Rect,
    error::{AppError, Component},
    geometry::Scale,
    terminal::{MAX_OSC_BYTES, Terminal},
    window::{ConfigureState, WindowSize},
};
use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{
        TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, CursorShape, NamedColor, Rgb},
};

struct FakeRenderer;

impl Renderer for FakeRenderer {
    type Frame = Vec<(Point, Cell)>;

    fn render(&mut self, input: RenderInput<'_>) -> Result<RenderOutput<Self::Frame>, AppError> {
        Ok(RenderOutput {
            window: input.window,
            tab: input.terminal.tab,
            size: input.size,
            frame: input
                .terminal
                .content
                .display_iter
                .map(|indexed| (indexed.point, indexed.cell.clone()))
                .collect(),
        })
    }
}

#[test]
fn renderer_accepts_terminal_snapshot() {
    let mut configure = ConfigureState::new(WindowId(3), WindowSize::new(640, 480).unwrap());
    configure.stage_toplevel(800, 600).unwrap();
    configure.surface_configure(17);
    let ack = configure.acknowledge().unwrap();

    let mut terminal = Terminal::new(TabId(7), 2, 6).unwrap();
    terminal
        .feed(
            TabId(7),
            "old\r\n\x1b]4;1;rgb:12/34/56\x07\x1b[31;44m繁e\u{301}XYZQ\x1b[?2004h\x1b[5 q"
                .as_bytes(),
        )
        .unwrap();
    let snapshot = terminal.snapshot().unwrap();
    assert_eq!(snapshot.tab, TabId(7));
    assert_eq!((snapshot.rows, snapshot.columns), (2, 6));
    assert_eq!(snapshot.content.display_offset, 0);
    assert_eq!(
        snapshot.content.cursor.point,
        Point::new(Line(1), Column(1))
    );
    assert_eq!(snapshot.content.cursor.shape, CursorShape::Beam);
    assert!(snapshot.content.mode.contains(TermMode::BRACKETED_PASTE));
    assert_eq!(
        snapshot.content.colors[NamedColor::Red],
        Some(Rgb {
            r: 0x12,
            g: 0x34,
            b: 0x56
        })
    );

    let output = FakeRenderer
        .render(RenderInput {
            window: ack.window,
            size: ack.size,
            terminal: snapshot,
        })
        .unwrap();
    assert_eq!(output.window, WindowId(3));
    assert_eq!(output.tab, TabId(7));
    assert_eq!(output.size, WindowSize::new(800, 600).unwrap());
    assert_eq!(output.frame.len(), 12);
    for (index, (point, _)) in output.frame.iter().enumerate() {
        assert_eq!(
            *point,
            Point::new(Line((index / 6) as i32), Column(index % 6))
        );
    }
    assert_eq!(output.frame[0].1.c, '繁');
    assert!(output.frame[0].1.flags.contains(Flags::WIDE_CHAR));
    assert!(output.frame[1].1.flags.contains(Flags::WIDE_CHAR_SPACER));
    assert_eq!(output.frame[2].1.c, 'e');
    assert_eq!(output.frame[2].1.zerowidth(), Some(&['\u{301}'][..]));
    assert!(output.frame[5].1.flags.contains(Flags::WRAPLINE));
    assert_eq!(output.frame[6].1.c, 'Q');
    assert_eq!(output.frame[0].1.fg, Color::Named(NamedColor::Red));
    assert_eq!(output.frame[0].1.bg, Color::Named(NamedColor::Blue));
    assert_eq!(terminal.grid().unwrap().history_size(), 1);
    for (point, cell) in output.frame {
        assert_eq!(cell, terminal.grid().unwrap()[point]);
    }
}

#[test]
fn terminal_snapshot_borrows_current_screen_and_keeps_tabs_separate() {
    let mut first = Terminal::new(TabId(1), 2, 4).unwrap();
    let mut second = Terminal::new(TabId(2), 3, 6).unwrap();
    first.feed(TabId(1), b"main").unwrap();
    second.feed(TabId(2), b"second").unwrap();
    let mut snapshot = first.snapshot().unwrap();
    let cell = snapshot.content.display_iter.next().unwrap();
    assert!(std::ptr::eq(
        cell.cell,
        &first.grid().unwrap()[Line(0)][Column(0)]
    ));
    assert_eq!(cell.c, 'm');
    assert_eq!(snapshot.content.display_iter.count(), 7);

    first
        .feed(TabId(1), b"\x1b[?1049h\x1b[Halt\x1b[?25l")
        .unwrap();
    let mut alternate = first.snapshot().unwrap();
    assert_eq!(alternate.tab, TabId(1));
    assert_eq!((alternate.rows, alternate.columns), (2, 4));
    assert!(alternate.content.mode.contains(TermMode::ALT_SCREEN));
    assert_eq!(alternate.content.cursor.shape, CursorShape::Hidden);
    assert_eq!(alternate.content.display_iter.next().unwrap().c, 'a');

    first.feed(TabId(1), b"\x1b[?1049l\x1b[?25h").unwrap();
    let mut restored = first.snapshot().unwrap();
    assert!(!restored.content.mode.contains(TermMode::ALT_SCREEN));
    assert_eq!(restored.content.display_iter.next().unwrap().c, 'm');
    let mut other = second.snapshot().unwrap();
    assert_eq!(other.tab, TabId(2));
    assert_eq!((other.rows, other.columns), (3, 6));
    assert!(!other.content.mode.contains(TermMode::ALT_SCREEN));
    assert_eq!(other.content.display_iter.next().unwrap().c, 's');
}

#[test]
fn stopped_terminal_has_no_render_snapshot() {
    let mut terminal = Terminal::new(TabId(9), 2, 4).unwrap();
    terminal.feed(TabId(9), b"\x1b]0;").unwrap();
    assert!(terminal.feed(TabId(9), &vec![b'x'; MAX_OSC_BYTES]).is_err());
    assert!(terminal.snapshot().is_none());
}

#[test]
fn cell_rect_rounding_is_stable() {
    let scale = Scale::new(2).unwrap();
    let original = CellLayout::new(WindowSize::new(21, 30).unwrap(), scale, 7.25, 13.5).unwrap();
    assert_eq!(
        original.cell_rect(0, 0),
        Some(Rect {
            x: 0,
            y: 0,
            width: 15,
            height: 27
        })
    );
    assert_eq!(
        original.cell_rect(0, 1),
        Some(Rect {
            x: 15,
            y: 0,
            width: 14,
            height: 27
        })
    );
    assert_eq!(
        original.cell_rect(2, 2),
        Some(Rect {
            x: 29,
            y: 54,
            width: 13,
            height: 6
        })
    );
    assert_eq!(original.cell_rect(3, 0), None);
    assert_eq!(original.cell_rect(0, 3), None);

    // Resize only clips the last cells; interior positions must not drift.
    let enlarged = CellLayout::new(WindowSize::new(101, 61).unwrap(), scale, 7.25, 13.5).unwrap();
    assert_eq!(enlarged.cell_rect(0, 1), original.cell_rect(0, 1));
    assert_eq!(
        enlarged.cell_rect(2, 2),
        Some(Rect {
            x: 29,
            y: 54,
            width: 15,
            height: 27
        })
    );

    for factor in [1, 2, 3, 4] {
        let scale = Scale::new(factor).unwrap();
        for (width, height) in [(21, 30), (101, 61), (103, 63), (21, 30)] {
            let size = WindowSize::new(width, height).unwrap();
            let bounds = scale.to_physical(size).unwrap();
            for (cell_width, cell_height) in [(7.25, 13.5), (7.1, 13.3)] {
                let layout = CellLayout::new(size, scale, cell_width, cell_height).unwrap();
                let mut bottom = 0;
                for row in 0..bounds.height() as usize {
                    let Some(first) = layout.cell_rect(row, 0) else {
                        break;
                    };
                    assert_eq!(first.y, bottom as i32);
                    let mut right = 0;
                    for column in 0..bounds.width() as usize {
                        let Some(rect) = layout.cell_rect(row, column) else {
                            break;
                        };
                        assert_eq!(rect.x, right as i32);
                        assert_eq!((rect.y, rect.height), (first.y, first.height));
                        assert!(rect.width > 0 && rect.height > 0);
                        assert_eq!(rect.clipped(bounds), Some(rect));
                        right += rect.width;
                    }
                    assert_eq!(right, bounds.width());
                    bottom += first.height;
                }
                assert_eq!(bottom, bounds.height());
            }
        }
    }
}

#[test]
fn cell_rect_rejects_invalid_metrics_and_handles_extreme_coordinates() {
    let size = WindowSize::new(80, 24).unwrap();
    let scale = Scale::new(1).unwrap();
    for value in [
        0.0,
        -0.0,
        -1.0,
        0.5,
        f64::MIN_POSITIVE,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        f64::from(i32::MAX) + 1.0,
    ] {
        for (width, height) in [(value, 1.0), (1.0, value)] {
            let error = CellLayout::new(size, scale, width, height).unwrap_err();
            assert_eq!(
                error,
                AppError::new(Component::Renderer, std::io::ErrorKind::InvalidInput)
            );
        }
    }
    let limit = i32::MAX as u32;
    let bounds = WindowSize::new(limit, limit).unwrap();
    let layout = CellLayout::new(bounds, scale, 1.0, 1.0).unwrap();
    assert_eq!(
        layout.cell_rect(limit as usize - 1, limit as usize - 1),
        Some(Rect {
            x: i32::MAX - 1,
            y: i32::MAX - 1,
            width: 1,
            height: 1
        })
    );
    for (row, column) in [
        (limit as usize, 0),
        (0, limit as usize),
        (usize::MAX, 0),
        (0, usize::MAX),
        (usize::MAX, usize::MAX),
    ] {
        assert_eq!(layout.cell_rect(row, column), None);
    }
    let oversized = CellLayout::new(bounds, Scale::new(2).unwrap(), 1.0, 1.0).unwrap_err();
    assert_eq!(oversized.component(), Component::Renderer);
    assert_eq!(oversized.kind(), std::io::ErrorKind::InvalidInput);
    assert!(CellLayout::new(size, Scale::new(2).unwrap(), f64::MAX, 1.0).is_err());
    let one_pixel = CellLayout::new(
        WindowSize::new(1, 1).unwrap(),
        Scale::new(2).unwrap(),
        0.5,
        0.5,
    )
    .unwrap();
    assert_eq!(
        one_pixel.cell_rect(1, 1),
        Some(Rect {
            x: 1,
            y: 1,
            width: 1,
            height: 1
        })
    );
    let largest = CellLayout::new(
        WindowSize::new(1, 1).unwrap(),
        Scale::new(i32::MAX).unwrap(),
        1.0,
        1.0,
    )
    .unwrap();
    assert_eq!(
        largest.cell_rect(0, 0),
        Some(Rect {
            x: 0,
            y: 0,
            width: limit,
            height: limit
        })
    );
    assert_eq!(largest.cell_rect(0, 1), None);
}

#[test]
fn cell_rect_maps_terminal_snapshot_without_changing_cells() {
    let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    terminal.feed(TabId(1), "繁e\u{301}".as_bytes()).unwrap();
    let input = RenderInput {
        window: WindowId(1),
        size: WindowSize::new(31, 23).unwrap(),
        terminal: terminal.snapshot().unwrap(),
    };
    let layout = CellLayout::new(input.size, Scale::new(2).unwrap(), 7.25, 13.5).unwrap();
    let rects: Vec<_> = input
        .terminal
        .content
        .display_iter
        .map(|cell| {
            layout
                .cell_rect(cell.point.line.0 as usize, cell.point.column.0)
                .unwrap()
        })
        .collect();
    assert_eq!(rects.len(), 8);
    assert_eq!(
        rects[0],
        Rect {
            x: 0,
            y: 0,
            width: 15,
            height: 27
        }
    );
    assert_eq!(
        rects[1],
        Rect {
            x: 15,
            y: 0,
            width: 14,
            height: 27
        }
    );
    assert_eq!(
        rects[2],
        Rect {
            x: 29,
            y: 0,
            width: 15,
            height: 27
        }
    );
    assert_eq!(
        rects[4],
        Rect {
            x: 0,
            y: 27,
            width: 15,
            height: 19
        }
    );
    let grid = terminal.grid().unwrap();
    assert_eq!(grid[Line(0)][Column(0)].c, '繁');
    assert!(
        grid[Line(0)][Column(1)]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
    );
    assert_eq!(grid[Line(0)][Column(2)].zerowidth(), Some(&['\u{301}'][..]));
}
