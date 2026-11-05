//! A user-action gate that emits an opener request, never launches a process from PTY output.
use crate::{
    error::{AppError, Component},
    input::{FocusTarget, PointerClick},
    keymap::Modifiers,
    renderer::CellLayout,
    terminal::{MAX_OSC_BYTES, Terminal},
};
use std::io::ErrorKind;

/// Sensitive URI has no Debug. The adapter must pass it as a literal argument, never a shell command.
pub struct OpenHyperlink {
    uri: String,
}
impl OpenHyperlink {
    pub fn into_uri(self) -> String {
        self.uri
    }
}

/// Only an explicit Ctrl+left press over a linked visible cell can request opening HTTP(S).
pub fn request_open(
    terminal: &Terminal,
    layout: &CellLayout,
    target: FocusTarget,
    click: &PointerClick,
    modifiers: Modifiers,
) -> Result<Option<OpenHyperlink>, AppError> {
    if target.tab != terminal.tab()
        || click.window != target.window
        || !click.pressed
        || click.button != 0x110
        || !modifiers.ctrl()
        || modifiers.alt()
    {
        return Ok(None);
    }
    let Some(snapshot) = terminal.snapshot() else {
        return Ok(None);
    };
    let Some((row, column)) = layout.hit_test(click.buffer, snapshot.rows, snapshot.columns) else {
        return Ok(None);
    };
    let offset = snapshot.content.display_offset;
    let link = snapshot
        .content
        .display_iter
        .into_iter()
        .find(|cell| {
            cell.point.column.0 == column
                && offset.checked_add_signed(cell.point.line.0 as isize) == Some(row)
        })
        .and_then(|cell| cell.cell.hyperlink());
    let Some(link) = link else {
        return Ok(None);
    };
    let uri = link.uri();
    let Some((scheme, rest)) = uri.split_once("://") else {
        return Ok(None);
    };
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || uri.len() > MAX_OSC_BYTES
        || uri
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\\')
        || rest.split(['/', '?', '#']).next().is_none_or(str::is_empty)
    {
        return Ok(None);
    }
    let mut owned = String::new();
    owned
        .try_reserve_exact(uri.len())
        .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
    owned.push_str(uri);
    Ok(Some(OpenHyperlink { uri: owned }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, WindowId, geometry::Scale, input::Point, window::WindowSize};
    #[test]
    fn hyperlink_does_not_auto_execute() {
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(7),
        };
        let layout = CellLayout::new(
            WindowSize::new(80, 20).unwrap(),
            Scale::new(1).unwrap(),
            10.0,
            10.0,
        )
        .unwrap();
        let mut terminal = Terminal::new(target.tab, 2, 8).unwrap();
        terminal
            .feed(
                target.tab,
                b"\x1b]8;;https://example.test/path\x1b\\link\x1b]8;;\x1b\\",
            )
            .unwrap();
        let mut opened = Vec::new();
        let mut click = PointerClick {
            window: target.window,
            surface: Point::new(5.0, 5.0).unwrap(),
            buffer: Point::new(5.0, 5.0).unwrap(),
            button: 0x110,
            pressed: true,
        };
        assert!(
            request_open(&terminal, &layout, target, &click, Modifiers::default())
                .unwrap()
                .is_none()
        );
        assert!(opened.is_empty());
        let ctrl = Modifiers {
            depressed: kbvm::ModifierMask::CONTROL.0,
            ..Modifiers::default()
        };
        click.pressed = false;
        assert!(
            request_open(&terminal, &layout, target, &click, ctrl)
                .unwrap()
                .is_none()
        );
        click.pressed = true;
        let request = request_open(&terminal, &layout, target, &click, ctrl)
            .unwrap()
            .unwrap();
        opened.push(request.into_uri());
        assert_eq!(opened, ["https://example.test/path"]);
        click.window = WindowId(2);
        assert!(
            request_open(&terminal, &layout, target, &click, ctrl)
                .unwrap()
                .is_none()
        );
        click.window = target.window;
        for uri in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/plain,hi",
            "https://",
            "https://example.test/a b",
        ] {
            terminal
                .feed(
                    target.tab,
                    format!("\x1b[1;1H\x1b]8;;{uri}\x1b\\X\x1b]8;;\x1b\\").as_bytes(),
                )
                .unwrap();
            assert!(
                request_open(&terminal, &layout, target, &click, ctrl)
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(opened.len(), 1);
    }
}
