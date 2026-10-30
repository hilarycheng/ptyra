use super::{CellLayout, RenderInput, RenderOutput, Renderer, cursor_draw_command, draw_commands};
use crate::{
    TabId, WindowId,
    damage::Rect,
    error::{AppError, Component},
    font::{CellMetrics, FontMetrics, select_font},
    geometry::Scale,
    terminal::{MAX_OSC_BYTES, Terminal},
    window::{ConfigureState, WindowSize},
};
use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point},
    term::{
        TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, CursorShape, NamedColor, Rgb},
};

struct FakeRenderer;

#[test]
fn cursor_shape_matches_mode() {
    let mut terminal = Terminal::new(TabId(1), 3, 6).unwrap();
    terminal.feed(TabId(1), b"\x1b[2;3H").unwrap();
    let layout = CellLayout::new(
        WindowSize::new(100, 100).unwrap(),
        Scale::new(2).unwrap(),
        7.25,
        13.5,
    )
    .unwrap();
    for (mode, shape, expected) in [
        (0, CursorShape::Block, (29, 27, 15, 27)),
        (1, CursorShape::Block, (29, 27, 15, 27)),
        (2, CursorShape::Block, (29, 27, 15, 27)),
        (3, CursorShape::Underline, (29, 53, 15, 1)),
        (4, CursorShape::Underline, (29, 53, 15, 1)),
        (5, CursorShape::Beam, (29, 27, 1, 27)),
        (6, CursorShape::Beam, (29, 27, 1, 27)),
    ] {
        terminal
            .feed(TabId(1), format!("\x1b[{mode} q").as_bytes())
            .unwrap();
        let command = cursor_draw_command(terminal.snapshot().unwrap(), &layout).unwrap();
        assert_eq!(command.shape, shape);
        let rect = command.rect;
        assert_eq!((rect.x, rect.y, rect.width, rect.height), expected);
        terminal.feed(TabId(1), b"\x1b[?25l").unwrap();
        assert!(cursor_draw_command(terminal.snapshot().unwrap(), &layout).is_none());
        terminal.feed(TabId(1), b"\x1b[?25h").unwrap();
        assert_eq!(
            cursor_draw_command(terminal.snapshot().unwrap(), &layout),
            Some(command)
        );
    }
}

#[test]
fn cursor_commands_follow_wide_cells_and_scrollback() {
    let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    terminal
        .feed(TabId(1), "繁e\u{301}\x1b[1;2H".as_bytes())
        .unwrap();
    let layout = CellLayout::new(
        WindowSize::new(100, 100).unwrap(),
        Scale::new(1).unwrap(),
        7.25,
        10.0,
    )
    .unwrap();
    for (mode, expected) in [(2, (0, 0, 15, 10)), (4, (0, 9, 15, 1)), (6, (0, 0, 1, 10))] {
        terminal
            .feed(TabId(1), format!("\x1b[{mode} q").as_bytes())
            .unwrap();
        let snapshot = terminal.snapshot().unwrap();
        assert_eq!(
            snapshot.content.cursor.point,
            Point::new(Line(0), Column(0))
        );
        let rect = cursor_draw_command(snapshot, &layout).unwrap().rect;
        assert_eq!((rect.x, rect.y, rect.width, rect.height), expected);
    }
    // A combining sequence still occupies just its base cell.
    terminal.feed(TabId(1), b"\x1b[1;3H\x1b[2 q").unwrap();
    let rect = cursor_draw_command(terminal.snapshot().unwrap(), &layout)
        .unwrap()
        .rect;
    assert_eq!((rect.x, rect.y, rect.width, rect.height), (15, 0, 7, 10));

    let mut terminal = Terminal::new(TabId(2), 2, 4).unwrap();
    terminal.feed(TabId(2), b"old\r\nnew\r\nend").unwrap();
    for row in [1, 2] {
        terminal
            .feed(TabId(2), format!("\x1b[{row};2H").as_bytes())
            .unwrap();
        // Supply a fake scrolled grid; production scrolling is a separate task.
        let mut grid = terminal.grid().unwrap().clone();
        grid.scroll_display(Scroll::Top);
        let mut snapshot = terminal.snapshot().unwrap();
        snapshot.content.display_iter = grid.display_iter();
        snapshot.content.display_offset = grid.display_offset();
        assert_eq!(snapshot.content.display_offset, 1);
        let command = cursor_draw_command(snapshot, &layout);
        if row == 1 {
            let rect = command.unwrap().rect;
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (7, 10, 8, 10));
        } else {
            // The window has spare room, but the cursor is outside the grid viewport.
            assert!(command.is_none());
        }
    }
}

#[test]
fn cursor_commands_clip_without_moving_underline() {
    let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    terminal.feed(TabId(1), b"\x1b[2;2H").unwrap();
    for (width, height, mode, expected) in [
        (17, 15, 2, Some((10, 10, 7, 5))),
        (17, 15, 6, Some((10, 10, 1, 5))),
        (17, 15, 4, None),
        (17, 20, 4, Some((10, 19, 7, 1))),
        (9, 20, 2, None),
        (9, 20, 4, None),
        (9, 20, 6, None),
    ] {
        terminal
            .feed(TabId(1), format!("\x1b[{mode} q").as_bytes())
            .unwrap();
        let layout = CellLayout::new(
            WindowSize::new(width, height).unwrap(),
            Scale::new(1).unwrap(),
            10.0,
            10.0,
        )
        .unwrap();
        let actual = cursor_draw_command(terminal.snapshot().unwrap(), &layout).map(|command| {
            let rect = command.rect;
            (rect.x, rect.y, rect.width, rect.height)
        });
        assert_eq!(actual, expected);
    }
}

#[test]
fn cursor_commands_bound_one_pixel_and_extreme_layouts() {
    let terminal = Terminal::new(TabId(1), 1, 2).unwrap();
    for size in [1, i32::MAX as u32] {
        let bounds = WindowSize::new(size, size).unwrap();
        let layout = CellLayout::new(
            bounds,
            Scale::new(1).unwrap(),
            f64::from(size),
            f64::from(size),
        )
        .unwrap();
        for shape in [
            CursorShape::Block,
            CursorShape::HollowBlock,
            CursorShape::Beam,
            CursorShape::Underline,
        ] {
            let mut snapshot = terminal.snapshot().unwrap();
            snapshot.content.cursor.shape = shape;
            let command = cursor_draw_command(snapshot, &layout).unwrap();
            assert_eq!(command.shape, shape);
            assert_eq!(command.rect.clipped(bounds), Some(command.rect));
            let expected = match shape {
                CursorShape::Beam => (0, 0, 1, size),
                CursorShape::Underline => (0, size as i32 - 1, size, 1),
                _ => (0, 0, size, size),
            };
            let rect = command.rect;
            assert_eq!((rect.x, rect.y, rect.width, rect.height), expected);
        }
    }
}

#[test]
fn draw_commands_preserve_colors() {
    let mut terminal = Terminal::new(TabId(1), 1, 5).unwrap();
    terminal
        .feed(
            TabId(1),
            b"\x1b]4;1;rgb:12/34/56\x07\x1b[31;44mA\x1b[38;5;123;48;5;234mB\x1b[38;2;1;2;3;48;2;4;5;6mC\x1b[0mD\x1b[41m ",
        )
        .unwrap();
    let layout = CellLayout::new(
        WindowSize::new(50, 10).unwrap(),
        Scale::new(1).unwrap(),
        10.0,
        10.0,
    )
    .unwrap();
    let commands: Vec<_> = draw_commands(terminal.snapshot().unwrap(), &layout).collect();
    let expected = [
        (
            'A',
            Color::Named(NamedColor::Red),
            Color::Named(NamedColor::Blue),
        ),
        ('B', Color::Indexed(123), Color::Indexed(234)),
        (
            'C',
            Color::Spec(Rgb { r: 1, g: 2, b: 3 }),
            Color::Spec(Rgb { r: 4, g: 5, b: 6 }),
        ),
        (
            'D',
            Color::Named(NamedColor::Foreground),
            Color::Named(NamedColor::Background),
        ),
        (
            ' ',
            Color::Named(NamedColor::Foreground),
            Color::Named(NamedColor::Red),
        ),
    ];
    assert_eq!(commands.len(), expected.len());
    for (index, (command, expected)) in commands.iter().zip(expected).enumerate() {
        assert_eq!(
            (command.character, command.foreground, command.background),
            expected
        );
        assert_eq!(
            command.rect,
            Rect {
                x: index as i32 * 10,
                y: 0,
                width: 10,
                height: 10
            }
        );
        assert_eq!(command.glyph_rect, command.rect);
        assert!(command.zerowidth.is_empty());
        assert_eq!(command.flags, Flags::empty());
        assert_eq!(command.underline_color, None);
        assert_eq!(
            command.colors[NamedColor::Red],
            Some(Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56
            })
        );
    }
    assert!(std::ptr::eq(
        commands[0].colors,
        terminal.snapshot().unwrap().content.colors
    ));
}

#[test]
fn draw_commands_keep_unicode_and_style_data() {
    let mut terminal = Terminal::new(TabId(1), 2, 6).unwrap();
    terminal
        .feed(
            TabId(1),
            "繁e\u{301}\x1b[7;8;4;58;5;42mX\x1b[0mQ繁".as_bytes(),
        )
        .unwrap();
    let layout = CellLayout::new(
        WindowSize::new(90, 40).unwrap(),
        Scale::new(1).unwrap(),
        7.25,
        10.0,
    )
    .unwrap();
    let commands: Vec<_> = draw_commands(terminal.snapshot().unwrap(), &layout).collect();
    assert_eq!(commands.len(), 12);
    assert_eq!(commands[0].character, '繁');
    assert_eq!(commands[0].rect.width, 7);
    assert_eq!(commands[0].glyph_rect.width, 15);
    assert!(commands[1].flags.contains(Flags::WIDE_CHAR_SPACER));
    assert_eq!(commands[2].character, 'e');
    assert_eq!(commands[2].zerowidth, &['\u{301}']);
    assert_eq!(commands[3].character, 'X');
    assert!(
        commands[3]
            .flags
            .contains(Flags::INVERSE | Flags::HIDDEN | Flags::UNDERLINE)
    );
    assert_eq!(commands[3].underline_color, Some(Color::Indexed(42)));
    assert!(commands[5].flags.contains(Flags::LEADING_WIDE_CHAR_SPACER));
    assert_eq!(commands[6].character, '繁');
    assert_eq!(
        commands[6].glyph_rect,
        Rect {
            x: 0,
            y: 10,
            width: 15,
            height: 10
        }
    );

    // Conversion preserves styles and borrows combining marks without changing the grid.
    for (command, indexed) in commands
        .iter()
        .zip(terminal.snapshot().unwrap().content.display_iter)
    {
        assert_eq!(command.character, indexed.c);
        assert_eq!(command.flags, indexed.flags);
        assert_eq!(command.foreground, indexed.fg);
        assert_eq!(command.background, indexed.bg);
        assert_eq!(command.underline_color, indexed.underline_color());
        assert_eq!(command.zerowidth, indexed.zerowidth().unwrap_or_default());
    }
    assert!(std::ptr::eq(
        commands[2].zerowidth,
        terminal.grid().unwrap()[Line(0)][Column(2)]
            .zerowidth()
            .unwrap()
    ));
}

#[test]
fn draw_commands_translate_scrollback_and_clip_to_window() {
    let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    terminal
        .feed(TabId(1), "繁a\r\nold\r\nnew".as_bytes())
        .unwrap();
    // Fake a scrolled viewport without adding a terminal scrolling API to this task.
    let mut grid = terminal.grid().unwrap().clone();
    grid.scroll_display(Scroll::Top);
    let mut snapshot = terminal.snapshot().unwrap();
    snapshot.content.display_iter = grid.display_iter();
    snapshot.content.display_offset = grid.display_offset();
    assert_eq!(snapshot.content.display_offset, 1);
    let layout = CellLayout::new(
        WindowSize::new(17, 15).unwrap(),
        Scale::new(1).unwrap(),
        10.0,
        10.0,
    )
    .unwrap();
    let commands: Vec<_> = draw_commands(snapshot, &layout).collect();
    assert_eq!(commands.len(), 4);
    assert_eq!(commands[0].character, '繁');
    assert_eq!(
        commands[0].rect,
        Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }
    );
    assert_eq!(
        commands[0].glyph_rect,
        Rect {
            x: 0,
            y: 0,
            width: 17,
            height: 10
        }
    );
    assert_eq!(
        commands[1].rect,
        Rect {
            x: 10,
            y: 0,
            width: 7,
            height: 10
        }
    );
    assert_eq!(commands[2].character, 'o');
    assert_eq!(
        commands[2].rect,
        Rect {
            x: 0,
            y: 10,
            width: 10,
            height: 5
        }
    );
    assert_eq!(commands[3].character, 'l');
    assert_eq!(
        commands[3].rect,
        Rect {
            x: 10,
            y: 10,
            width: 7,
            height: 5
        }
    );
}

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
            damage: Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
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
        damage: Rect {
            x: 0,
            y: 0,
            width: 62,
            height: 46,
        },
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

#[test]
fn font_metrics_feed_layout_without_double_scaling() {
    let face = FontMetrics {
        advance: 0.5,
        ascent: 0.75,
        descent: -0.25,
        line_gap: 0.125,
    };
    let metrics = CellMetrics::new(13.0, face).unwrap();
    let size = WindowSize::new(80, 40).unwrap();
    let layout = CellLayout::new(
        size,
        Scale::new(2).unwrap(),
        metrics.width(),
        metrics.height(),
    )
    .unwrap();
    assert_eq!(
        layout.cell_rect(1, 1),
        Some(Rect {
            x: 13,
            y: 29,
            width: 13,
            height: 30
        })
    );
    assert_eq!(metrics.baseline(), 9.75);

    let tiny = CellMetrics::new(0.5, face).unwrap();
    assert!(CellLayout::new(size, Scale::new(1).unwrap(), tiny.width(), tiny.height()).is_err());
    let scaled =
        CellLayout::new(size, Scale::new(4).unwrap(), tiny.width(), tiny.height()).unwrap();
    assert_eq!(
        scaled.cell_rect(0, 0),
        Some(Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 2
        })
    );

    let maximum = CellMetrics::new(
        f64::from(i32::MAX),
        FontMetrics {
            advance: 1.0,
            ascent: 1.0,
            descent: 0.0,
            line_gap: 0.0,
        },
    )
    .unwrap();
    assert!(
        CellLayout::new(
            size,
            Scale::new(2).unwrap(),
            maximum.width(),
            maximum.height()
        )
        .is_err()
    );
}

#[test]
fn font_fallback_keeps_terminal_cell_geometry() {
    let metrics = CellMetrics::new(
        13.0,
        FontMetrics {
            advance: 0.5,
            ascent: 0.75,
            descent: -0.25,
            line_gap: 0.125,
        },
    )
    .unwrap();
    let layout = CellLayout::new(
        WindowSize::new(80, 40).unwrap(),
        Scale::new(2).unwrap(),
        metrics.width(),
        metrics.height(),
    )
    .unwrap();
    let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    terminal.feed(TabId(1), "繁A".as_bytes()).unwrap();
    let snapshot = terminal.snapshot().unwrap();
    let fonts = [" A", "繁體中文"];
    let choices: Vec<_> = snapshot
        .content
        .display_iter
        .filter(|indexed| !indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER))
        .map(|indexed| {
            (
                indexed.point,
                select_font(indexed.cell.c, &fonts, |font, scalar| font.contains(scalar)),
            )
        })
        .collect();
    assert_eq!(choices[0], (Point::new(Line(0), Column(0)), Some(1)));
    assert_eq!(choices[1], (Point::new(Line(0), Column(2)), Some(0)));
    assert_eq!(
        layout.cell_rect(0, 2),
        Some(Rect {
            x: 26,
            y: 0,
            width: 13,
            height: 29
        })
    );
    let grid = terminal.grid().unwrap();
    assert_eq!(grid[Line(0)][Column(0)].c, '繁');
    assert!(grid[Line(0)][Column(0)].flags.contains(Flags::WIDE_CHAR));
    assert!(
        grid[Line(0)][Column(1)]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
    );
    assert_eq!(grid[Line(0)][Column(2)].c, 'A');
    assert_eq!(metrics.width(), 6.5);
    assert_eq!(metrics.baseline(), 9.75);
}

#[test]
fn hidden_or_clean_view_skips_draw() {
    use super::dirty::render_visible_dirty;
    use crate::{clock::FakeClock, frame::FrameScheduler};

    struct CountingRenderer(usize);
    impl Renderer for CountingRenderer {
        type Frame = Rect;

        fn render(&mut self, input: RenderInput<'_>) -> Result<RenderOutput<Rect>, AppError> {
            self.0 += 1;
            Ok(RenderOutput {
                window: input.window,
                tab: input.terminal.tab,
                size: input.size,
                frame: input.damage,
            })
        }
    }

    let clock = FakeClock::default();
    let size = WindowSize::new(40, 20).unwrap();
    let mut frames = FrameScheduler::new(WindowId(3), size);
    let terminal = Terminal::new(TabId(7), 2, 4).unwrap();
    let mut renderer = CountingRenderer(0);
    let render = |frames: &mut FrameScheduler, renderer: &mut CountingRenderer, visible| {
        render_visible_dirty(
            frames,
            &clock,
            visible,
            renderer,
            size,
            terminal.snapshot().unwrap(),
        )
        .unwrap()
    };
    assert!(render(&mut frames, &mut renderer, true).is_none());
    let first_damage = Rect {
        x: 10,
        y: 0,
        width: 10,
        height: 10,
    };
    frames.damage(first_damage);
    assert!(render(&mut frames, &mut renderer, false).is_none());
    assert_eq!(renderer.0, 0);
    let (first, output) = render(&mut frames, &mut renderer, true).unwrap();
    assert_eq!(output.frame, first_damage);
    assert_eq!(first.damage, first_damage);
    assert_eq!(renderer.0, 1);
    assert!(render(&mut frames, &mut renderer, true).is_none());
    assert!(frames.frame_done(first.callback));
    assert!(render(&mut frames, &mut renderer, true).is_none());
    assert_eq!(renderer.0, 1);
}

#[test]
fn hit_test_maps_cell_boundaries() {
    use crate::input::{Point as PixelPoint, PointerFocus};

    let scale = Scale::new(2).unwrap();
    let layout = CellLayout::new(WindowSize::new(21, 30).unwrap(), scale, 7.25, 13.5).unwrap();
    let hit = |x, y| layout.hit_test(PixelPoint::new(x, y).unwrap(), 3, 3);
    assert_eq!(hit(0.0, 0.0), Some((0, 0)));
    assert_eq!(hit(14.999, 26.999), Some((0, 0)));
    assert_eq!(hit(15.0, 27.0), Some((1, 1)));
    assert_eq!(hit(28.999, 53.999), Some((1, 1)));
    assert_eq!(hit(29.0, 54.0), Some((2, 2)));
    assert_eq!(hit(41.999, 59.999), Some((2, 2)));
    for (x, y) in [(-0.001, 0.0), (0.0, -0.001), (42.0, 0.0), (0.0, 60.0)] {
        assert_eq!(hit(x, y), None);
    }

    let mut pointer = PointerFocus::default();
    pointer.enter(WindowId(3), PixelPoint::new(7.5, 13.5).unwrap());
    let click = pointer.button(272, true, scale).unwrap().unwrap();
    assert_eq!(layout.hit_test(click.buffer, 3, 3), Some((1, 1)));

    let larger = CellLayout::new(WindowSize::new(101, 61).unwrap(), scale, 7.25, 13.5).unwrap();
    assert_eq!(
        larger.hit_test(PixelPoint::new(43.999, 80.999).unwrap(), 3, 3),
        Some((2, 2))
    );
    assert_eq!(
        larger.hit_test(PixelPoint::new(44.0, 0.0).unwrap(), 3, 3),
        None
    );
    assert_eq!(
        larger.hit_test(PixelPoint::new(0.0, 81.0).unwrap(), 3, 3),
        None
    );

    let smaller = CellLayout::new(WindowSize::new(10, 10).unwrap(), scale, 7.25, 13.5).unwrap();
    assert_eq!(
        smaller.hit_test(PixelPoint::new(19.999, 19.999).unwrap(), 3, 3),
        Some((0, 1))
    );
    assert_eq!(
        smaller.hit_test(PixelPoint::new(20.0, 0.0).unwrap(), 3, 3),
        None
    );
    assert_eq!(
        smaller.hit_test(PixelPoint::new(0.0, 0.0).unwrap(), 0, 3),
        None
    );
    assert_eq!(
        smaller.hit_test(PixelPoint::new(0.0, 0.0).unwrap(), 3, 0),
        None
    );
    assert_eq!(
        layout.hit_test(PixelPoint::new(0.0, 0.0).unwrap(), usize::MAX, usize::MAX),
        Some((0, 0))
    );
    assert!(PixelPoint::new(f64::NAN, 0.0).is_err());
    assert!(PixelPoint::new(0.0, f64::INFINITY).is_err());
    assert!(CellLayout::new(WindowSize::new(1, 1).unwrap(), scale, 0.0, 10.0).is_err());
}
