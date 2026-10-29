use super::{RenderInput, RenderOutput, Renderer};
use crate::{
    TabId, WindowId,
    error::AppError,
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
