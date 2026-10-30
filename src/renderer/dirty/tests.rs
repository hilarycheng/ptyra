use super::*;
use crate::{
    TabId, WindowId, clock::FakeClock, error::Component, geometry::Scale, terminal::Terminal,
};
use alacritty_terminal::{
    index::{Column, Line, Point},
    selection::SelectionRange,
    term::cell::Flags,
};
use std::io::ErrorKind;

#[test]
fn partial_damage_keeps_wide_glyph_cursor_and_overlays() {
    let mut terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    terminal.feed(TabId(1), "繁A\x1b[1;1H".as_bytes()).unwrap();
    let layout = CellLayout::new(
        WindowSize::new(20, 10).unwrap(),
        Scale::new(2).unwrap(),
        5.0,
        5.0,
    )
    .unwrap();
    let damage = Rect {
        x: 15,
        y: 0,
        width: 1,
        height: 10,
    };
    let cells: Vec<_> =
        dirty_draw_commands(terminal.snapshot().unwrap(), &layout, damage).collect();
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].character, '繁');
    assert!(cells[0].flags.contains(Flags::WIDE_CHAR));
    assert_eq!(cells[0].rect.x, 0);
    assert_eq!(cells[0].glyph_rect.width, 20);
    assert!(cells[1].flags.contains(Flags::WIDE_CHAR_SPACER));
    assert_eq!(cells[1].rect.x, 10);

    let cursor = dirty_cursor_draw_command(terminal.snapshot().unwrap(), &layout, damage).unwrap();
    assert_eq!(cursor.rect.x, 0);
    assert_eq!(cursor.rect.width, 20);

    let mut snapshot = terminal.snapshot().unwrap();
    snapshot.content.selection = Some(SelectionRange::new(
        Point::new(Line(0), Column(1)),
        Point::new(Line(0), Column(1)),
        false,
    ));
    let overlays: Vec<_> = dirty_overlay_commands(
        snapshot,
        &layout,
        Some(Preedit {
            tab: TabId(1),
            text: "中",
        }),
        damage,
    )
    .unwrap()
    .collect();
    assert_eq!(overlays.len(), 2);
    let OverlayCommand::Selection { rect } = overlays[0] else {
        panic!("wide selection missing")
    };
    assert_eq!((rect.x, rect.width), (0, 20));
    let OverlayCommand::Preedit { text, clip } = overlays[1] else {
        panic!("preedit missing")
    };
    assert_eq!(text, "中");
    assert_eq!((clip.x, clip.width), (0, 40));

    let outside = Rect {
        x: 40,
        y: 0,
        width: 1,
        height: 10,
    };
    assert_eq!(
        dirty_draw_commands(terminal.snapshot().unwrap(), &layout, outside).count(),
        0
    );
    assert!(dirty_cursor_draw_command(terminal.snapshot().unwrap(), &layout, outside).is_none());
    assert_eq!(
        dirty_overlay_commands(
            terminal.snapshot().unwrap(),
            &layout,
            Some(Preedit {
                tab: TabId(1),
                text: "中"
            }),
            outside,
        )
        .unwrap()
        .count(),
        0
    );
}

#[test]
fn failed_or_pending_frame_keeps_later_hidden_damage() {
    struct FailOnce(usize);
    impl Renderer for FailOnce {
        type Frame = Rect;

        fn render(&mut self, input: RenderInput<'_>) -> Result<RenderOutput<Rect>, AppError> {
            self.0 += 1;
            if self.0 == 1 {
                return Err(AppError::new(Component::Renderer, ErrorKind::Other));
            }
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
    let mut frames = FrameScheduler::new(WindowId(1), size);
    let terminal = Terminal::new(TabId(1), 2, 4).unwrap();
    let mut renderer = FailOnce(0);
    let first = Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    };
    frames.damage(first);
    let render = |frames: &mut FrameScheduler, renderer: &mut FailOnce, visible| {
        render_visible_dirty(
            frames,
            &clock,
            visible,
            renderer,
            size,
            terminal.snapshot().unwrap(),
        )
    };
    assert_eq!(
        render(&mut frames, &mut renderer, true).err(),
        Some(AppError::new(Component::Renderer, ErrorKind::Other))
    );
    assert_eq!(renderer.0, 1);
    let (pending, output) = render(&mut frames, &mut renderer, true).unwrap().unwrap();
    assert_eq!(output.frame, first);
    assert_eq!(renderer.0, 2);

    let second = Rect {
        x: 20,
        y: 0,
        width: 10,
        height: 10,
    };
    frames.damage(second);
    assert!(render(&mut frames, &mut renderer, false).unwrap().is_none());
    assert_eq!(renderer.0, 2);
    assert!(!frames.frame_done(pending.callback));
    let (next, output) = render(&mut frames, &mut renderer, true).unwrap().unwrap();
    let merged = Rect {
        x: 0,
        y: 0,
        width: 30,
        height: 10,
    };
    assert_eq!(output.frame, merged);
    assert!(frames.cancel(next.callback));
    let (_, retry) = render(&mut frames, &mut renderer, true).unwrap().unwrap();
    assert_eq!(retry.frame, merged);
    assert_eq!(renderer.0, 4);
}
