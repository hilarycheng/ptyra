use super::{MAX_PREEDIT_BYTES, OverlayCommand, Preedit, overlay_commands};
use crate::{
    TabId,
    damage::Rect,
    error::{AppError, Component},
    geometry::Scale,
    renderer::CellLayout,
    terminal::Terminal,
    window::WindowSize,
};
use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point},
    selection::SelectionRange,
    term::color,
};
use std::io::ErrorKind;

fn range(start: (i32, usize), end: (i32, usize), block: bool) -> SelectionRange {
    SelectionRange::new(
        Point::new(Line(start.0), Column(start.1)),
        Point::new(Line(end.0), Column(end.1)),
        block,
    )
}

fn rects(commands: &[OverlayCommand<'_>]) -> Vec<Rect> {
    commands
        .iter()
        .filter_map(|command| match command {
            OverlayCommand::Selection { rect } => Some(*rect),
            OverlayCommand::Preedit { .. } => None,
        })
        .collect()
}

#[test]
fn overlay_does_not_change_grid() {
    let mut terminal = Terminal::new(TabId(1), 2, 8).unwrap();
    terminal
        .feed(
            TabId(1),
            "\x1b]4;1;rgb:12/34/56\x07\x1b[31;44mabcdefghX\r\n繁e\u{301}Q\x1b[2;3H\x1b[?25l"
                .as_bytes(),
        )
        .unwrap();
    let grid_before = terminal.grid().unwrap().clone();
    assert!(grid_before.history_size() > 0);
    let cursor_before = terminal.snapshot().unwrap().content.cursor;
    let colors_before = *terminal.snapshot().unwrap().content.colors;
    let mode_before = *terminal.mode().unwrap();
    let layout = CellLayout::new(
        WindowSize::new(100, 40).unwrap(),
        Scale::new(1).unwrap(),
        10.0,
        10.0,
    )
    .unwrap();

    // Mock UI updates: compose, replace, clear with an empty string, then remove state.
    for text in [Some("繁體e\u{301}"), Some("更新\x1b[31m"), Some(""), None] {
        let mut snapshot = terminal.snapshot().unwrap();
        snapshot.content.selection = Some(range((1, 0), (1, 3), false));
        let preedit = text.map(|text| Preedit {
            tab: TabId(1),
            text,
        });
        let commands: Vec<_> = overlay_commands(snapshot, &layout, preedit)
            .unwrap()
            .collect();
        assert_eq!(
            rects(&commands),
            [
                Rect {
                    x: 0,
                    y: 10,
                    width: 20,
                    height: 10
                },
                Rect {
                    x: 20,
                    y: 10,
                    width: 10,
                    height: 10
                },
                Rect {
                    x: 30,
                    y: 10,
                    width: 10,
                    height: 10
                },
            ]
        );
        if let Some(expected) = text.filter(|text| !text.is_empty()) {
            assert_eq!(commands.len(), 4);
            let OverlayCommand::Preedit { text, clip } = commands.last().unwrap() else {
                panic!("preedit must be painted last");
            };
            assert_eq!(*text, expected);
            assert!(std::ptr::eq(*text, expected));
            assert_eq!(
                *clip,
                Rect {
                    x: 20,
                    y: 10,
                    width: 60,
                    height: 10
                }
            );
        } else {
            assert_eq!(commands.len(), 3);
        }
        // Upstream Grid equality requires a zeroed ring buffer; compare logical cells instead.
        let grid = terminal.grid().unwrap();
        assert_eq!(grid.history_size(), grid_before.history_size());
        assert_eq!(grid.display_offset(), grid_before.display_offset());
        assert_eq!(grid.cursor, grid_before.cursor);
        for line in grid_before.topmost_line().0..=grid_before.bottommost_line().0 {
            for column in 0..grid_before.columns() {
                let point = Point::new(Line(line), Column(column));
                assert_eq!(grid[point], grid_before[point]);
            }
        }
        let current = terminal.snapshot().unwrap();
        assert!(current.content.cursor == cursor_before);
        assert_eq!(current.content.mode, mode_before);
        for index in 0..color::COUNT {
            assert_eq!(current.content.colors[index], colors_before[index]);
        }
    }
    assert_eq!(
        overlay_commands(terminal.snapshot().unwrap(), &layout, None)
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn overlay_selection_preserves_ranges_and_wide_coverage() {
    let mut terminal = Terminal::new(TabId(1), 2, 6).unwrap();
    terminal
        .feed(TabId(1), "繁AB\r\nCDe\u{301}F".as_bytes())
        .unwrap();
    let layout = CellLayout::new(
        WindowSize::new(60, 20).unwrap(),
        Scale::new(2).unwrap(),
        5.0,
        5.0,
    )
    .unwrap();
    for (selection, expected) in [
        (range((0, 1), (0, 1), false), vec![(0, 0, 20)]),
        (
            range((0, 2), (1, 1), false),
            vec![
                (20, 0, 10),
                (30, 0, 10),
                (40, 0, 10),
                (50, 0, 10),
                (0, 10, 10),
                (10, 10, 10),
            ],
        ),
        (
            range((0, 2), (1, 3), true),
            vec![(20, 0, 10), (30, 0, 10), (20, 10, 10), (30, 10, 10)],
        ),
    ] {
        let mut snapshot = terminal.snapshot().unwrap();
        snapshot.content.selection = Some(selection);
        let commands: Vec<_> = overlay_commands(snapshot, &layout, None).unwrap().collect();
        let actual: Vec<_> = rects(&commands)
            .iter()
            .map(|rect| {
                assert_eq!(rect.height, 10);
                (rect.x, rect.y, rect.width)
            })
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn overlay_clips_scrollback_and_offscreen_preedit() {
    let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    terminal
        .feed(TabId(1), "繁a\r\nold\r\nnew".as_bytes())
        .unwrap();
    let layout = CellLayout::new(
        WindowSize::new(17, 15).unwrap(),
        Scale::new(1).unwrap(),
        10.0,
        10.0,
    )
    .unwrap();
    for row in [1, 2] {
        terminal
            .feed(TabId(1), format!("\x1b[{row};2H").as_bytes())
            .unwrap();
        let mut grid = terminal.grid().unwrap().clone();
        grid.scroll_display(Scroll::Top);
        let mut snapshot = terminal.snapshot().unwrap();
        snapshot.content.display_iter = grid.display_iter();
        snapshot.content.display_offset = grid.display_offset();
        snapshot.content.selection = Some(range((-1, 1), (0, 1), false));
        let commands: Vec<_> = overlay_commands(
            snapshot,
            &layout,
            Some(Preedit {
                tab: TabId(1),
                text: "中",
            }),
        )
        .unwrap()
        .collect();
        assert_eq!(
            rects(&commands),
            [
                Rect {
                    x: 0,
                    y: 0,
                    width: 17,
                    height: 10
                },
                Rect {
                    x: 0,
                    y: 10,
                    width: 10,
                    height: 5
                },
                Rect {
                    x: 10,
                    y: 10,
                    width: 7,
                    height: 5
                },
            ]
        );
        if row == 1 {
            assert_eq!(commands.len(), 4);
            let OverlayCommand::Preedit { text, clip } = commands.last().unwrap() else {
                panic!("missing visible preedit")
            };
            assert_eq!(*text, "中");
            assert_eq!(
                *clip,
                Rect {
                    x: 10,
                    y: 10,
                    width: 7,
                    height: 5
                }
            );
        } else {
            assert_eq!(commands.len(), 3);
        }
    }
}

#[test]
fn overlay_rejects_wrong_tab_and_oversized_preedit_without_payload() {
    let terminal = Terminal::new(TabId(1), 1, 2).unwrap();
    let layout = CellLayout::new(
        WindowSize::new(20, 10).unwrap(),
        Scale::new(1).unwrap(),
        10.0,
        10.0,
    )
    .unwrap();
    let exact = "繁".repeat(MAX_PREEDIT_BYTES / 3) + &"x".repeat(MAX_PREEDIT_BYTES % 3);
    let oversized = format!("{exact}密");
    for (tab, text) in [(TabId(2), "秘密"), (TabId(1), oversized.as_str())] {
        let error = overlay_commands(
            terminal.snapshot().unwrap(),
            &layout,
            Some(Preedit { tab, text }),
        )
        .err()
        .unwrap();
        assert_eq!(
            error,
            AppError::new(Component::Renderer, ErrorKind::InvalidInput)
        );
        assert_eq!(error.to_string(), "renderer: InvalidInput");
    }
    let commands: Vec<_> = overlay_commands(
        terminal.snapshot().unwrap(),
        &layout,
        Some(Preedit {
            tab: TabId(1),
            text: &exact,
        }),
    )
    .unwrap()
    .collect();
    assert_eq!(commands.len(), 1);
    let OverlayCommand::Preedit { text, .. } = &commands[0] else {
        panic!("missing preedit at limit")
    };
    assert_eq!(text.len(), MAX_PREEDIT_BYTES);
    assert!(std::ptr::eq(*text, exact.as_str()));
}
