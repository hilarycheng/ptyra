//! Explicit-click adapter. The only child argv is a gated HTTP(S) URI, never a shell command.
use super::{
    pointer::{Kind, Routed},
    selection::Geometry,
};
use crate::{
    error::{AppError, Component},
    geometry::Scale,
    hyperlink::OpenHyperlink,
    input::{FocusTarget, Point, PointerClick},
    keymap::Modifiers,
    renderer::CellLayout,
    tabs::{TabPty, TabRegistry},
    window::WindowSize,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Presented {
    tab: crate::TabId,
    epoch: u64,
    offset: usize,
    geometry: (u32, (u32, u32), (u32, u32)),
}
impl Presented {
    pub fn capture<P: TabPty>(
        tabs: &TabRegistry<P>,
        geometry: Geometry,
        blocked: bool,
    ) -> Option<Self> {
        if blocked {
            return None;
        }
        let tab = tabs.active()?;
        let terminal = tabs.get(tab)?.terminal();
        Some(Self {
            tab,
            epoch: terminal.content_epoch()?,
            offset: terminal.grid()?.display_offset(),
            geometry: (geometry.origin, geometry.size, geometry.cell),
        })
    }
    fn matches<P: TabPty>(&self, tabs: &TabRegistry<P>, geometry: Geometry) -> bool {
        Self::capture(tabs, geometry, false).as_ref() == Some(self)
    }
}

pub(super) fn route<P: TabPty>(
    tabs: &TabRegistry<P>,
    event: &Routed,
    geometry: Geometry,
    presented: Option<&Presented>,
    modifiers: Modifiers,
    focus: Option<FocusTarget>,
    open: impl FnOnce(OpenHyperlink) -> Result<(), AppError>,
) -> Result<bool, AppError> {
    if tabs.active() != Some(event.target.tab)
        || focus != Some(event.target)
        || !presented.is_some_and(|frame| frame.matches(tabs, geometry))
    {
        return Ok(false);
    }
    let Kind::Button {
        button, pressed, ..
    } = event.kind
    else {
        return Ok(false);
    };
    if !pressed || button != 272 || !modifiers.ctrl() || modifiers.alt() {
        return Ok(false);
    }
    let Some(tab) = tabs.get(event.target.tab) else {
        return Ok(false);
    };
    let point = Point::new(
        event.position.x(),
        event.position.y() - f64::from(geometry.origin),
    )?;
    let layout = CellLayout::new(
        WindowSize::new(geometry.size.0, geometry.size.1)
            .map_err(|_| AppError::new(Component::Renderer, std::io::ErrorKind::InvalidInput))?,
        Scale::new(1)
            .map_err(|_| AppError::new(Component::Renderer, std::io::ErrorKind::InvalidInput))?,
        f64::from(geometry.cell.0),
        f64::from(geometry.cell.1),
    )?;
    let click = PointerClick {
        window: event.target.window,
        surface: point,
        buffer: point,
        button,
        pressed,
    };
    let Some(request) =
        crate::hyperlink::request_open(tab.terminal(), &layout, event.target, &click, modifiers)?
    else {
        return Ok(false);
    };
    open(request)?;
    Ok(true)
}

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
};
const MAX_OPENERS: usize = 4;
/// Bounded blocking waiters report one status byte to the existing poll loop.
/// This avoids blocking keyboard/PTY work, detached zombies, or periodic child polling.
#[derive(Default)]
pub(super) struct Opener {
    pending: Vec<UnixStream>,
}
impl Opener {
    fn channel(&mut self) -> std::io::Result<UnixStream> {
        if self.pending.len() >= MAX_OPENERS {
            return Err(std::io::ErrorKind::WouldBlock.into());
        }
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        self.pending.push(reader);
        Ok(writer)
    }
    pub fn open(&mut self, request: OpenHyperlink) -> Result<(), AppError> {
        let mut status = self
            .channel()
            .map_err(|e| AppError::from_io(Component::Terminal, e))?;
        let uri = request.into_uri();
        if let Err(error) = std::thread::Builder::new()
            .name("ptyra-opener".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let ok = command(&uri).status().is_ok_and(|status| status.success());
                let _ = status.write_all(&[u8::from(ok)]);
            })
        {
            self.pending.pop();
            return Err(AppError::from_io(Component::Terminal, error));
        }
        Ok(())
    }
    pub fn fds(&self) -> impl Iterator<Item = &UnixStream> {
        self.pending.iter()
    }
    /// Drain completion metadata only. A failed opener never terminates the terminal.
    pub fn pump(&mut self) -> bool {
        let mut failed = false;
        self.pending.retain_mut(|stream| {
            let mut status = [0];
            loop {
                match stream.read(&mut status) {
                    Ok(1) => {
                        failed |= status[0] != 1;
                        return false;
                    }
                    Ok(_) => {
                        failed = true;
                        return false;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return true,
                    Err(_) => {
                        failed = true;
                        return false;
                    }
                }
            }
        });
        failed
    }
}
fn command(uri: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command
        .arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, WindowId};
    use alacritty_terminal::grid::Scroll;
    struct Fake;
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            TabId(1)
        }
    }
    #[test]
    fn runtime_hyperlink_open_requires_explicit_click() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(20).unwrap();
        tabs.insert(TabId(1), 2, 8).unwrap();
        tabs.insert(TabId(2), 2, 8).unwrap();
        tabs.activate(TabId(1));
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(1),
        };
        let mut event = Routed {
            target,
            position: Point::new(5., 25.).unwrap(),
            kind: Kind::Motion,
        };
        let geometry = Geometry {
            origin: 20,
            size: (80, 20),
            cell: (10, 10),
        };
        let ctrl = Modifiers {
            depressed: kbvm::ModifierMask::CONTROL.0,
            ..Modifiers::default()
        };
        let uri = "https://example.test/a;$(id)?x='quoted'&y=1";
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(
                TabId(1),
                format!("\x1b]8;;{uri}\x1b\\link\x1b]8;;\x1b\\\r\nnext\r\nlast").as_bytes(),
            )
            .unwrap();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Top)
            .unwrap();
        let presented = Presented::capture(&tabs, geometry, false).unwrap();
        let mut opened = Vec::new();
        assert!(
            !route(
                &tabs,
                &event,
                geometry,
                Some(&presented),
                ctrl,
                Some(target),
                |r| {
                    opened.push(r.into_uri());
                    Ok(())
                }
            )
            .unwrap()
        );
        event.kind = Kind::Button {
            button: 272,
            pressed: true,
            serial: 1,
        };
        assert!(
            route(
                &tabs,
                &event,
                geometry,
                Some(&presented),
                ctrl,
                Some(target),
                |r| {
                    opened.push(r.into_uri());
                    Ok(())
                }
            )
            .unwrap()
        );
        assert_eq!(opened, [uri]);
        for (position, kind, modifiers, focus) in [
            (
                (5., 25.),
                Kind::Button {
                    button: 272,
                    pressed: false,
                    serial: 2,
                },
                ctrl,
                Some(target),
            ),
            (
                (5., 25.),
                Kind::Button {
                    button: 273,
                    pressed: true,
                    serial: 3,
                },
                ctrl,
                Some(target),
            ),
            (
                (5., 25.),
                Kind::Button {
                    button: 272,
                    pressed: true,
                    serial: 4,
                },
                Modifiers::default(),
                Some(target),
            ),
            (
                (5., 25.),
                Kind::Button {
                    button: 272,
                    pressed: true,
                    serial: 5,
                },
                ctrl,
                None,
            ),
            (
                (5., 5.),
                Kind::Button {
                    button: 272,
                    pressed: true,
                    serial: 6,
                },
                ctrl,
                Some(target),
            ),
            (
                (85., 25.),
                Kind::Button {
                    button: 272,
                    pressed: true,
                    serial: 7,
                },
                ctrl,
                Some(target),
            ),
        ] {
            event.position = Point::new(position.0, position.1).unwrap();
            event.kind = kind;
            assert!(
                !route(
                    &tabs,
                    &event,
                    geometry,
                    Some(&presented),
                    modifiers,
                    focus,
                    |_| panic!("not an explicit linked click")
                )
                .unwrap()
            );
        }
        event.position = Point::new(5., 25.).unwrap();
        let error = route(
            &tabs,
            &event,
            geometry,
            Some(&presented),
            ctrl,
            Some(target),
            |_| {
                Err(AppError::from_io(
                    Component::Terminal,
                    std::io::Error::new(std::io::ErrorKind::NotFound, uri),
                ))
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(!error.to_string().contains(uri));
        assert!(tabs.activate(TabId(2)));
        assert!(
            !route(
                &tabs,
                &event,
                geometry,
                Some(&presented),
                ctrl,
                Some(target),
                |_| panic!("stale tab")
            )
            .unwrap()
        );
    }
    #[test]
    fn hyperlink_adapter_is_literal_bounded_and_recoverable() {
        let uri = "https://example.test/$(touch${IFS}/tmp/never);'&x=1";
        let cmd = command(uri);
        assert_eq!(cmd.get_program(), "xdg-open");
        assert_eq!(cmd.get_args().collect::<Vec<_>>(), [uri]);
        let mut opener = Opener::default();
        let mut writers = Vec::new();
        for _ in 0..MAX_OPENERS {
            writers.push(opener.channel().unwrap());
        }
        assert_eq!(
            opener.channel().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!opener.pump());
        writers[0].write_all(&[1]).unwrap();
        writers[1].write_all(&[0]).unwrap();
        assert!(opener.pump());
        assert_eq!(opener.fds().count(), 2);
        drop(writers);
        assert!(opener.pump());
        assert_eq!(opener.fds().count(), 0);
        let mut writer = opener.channel().unwrap();
        writer.write_all(&[1]).unwrap();
        assert!(!opener.pump());
        assert_eq!(opener.fds().count(), 0);
    }
    #[test]
    fn hyperlink_rejects_unsubmitted_content_and_obscuring_notices() {
        let mut tabs = TabRegistry::<Fake>::with_scrollback(20).unwrap();
        tabs.insert(TabId(1), 2, 8).unwrap();
        let geometry = Geometry {
            origin: 20,
            size: (80, 20),
            cell: (10, 10),
        };
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"old\r\nnext\r\nlast")
            .unwrap();
        let presented = Presented::capture(&tabs, geometry, false).unwrap();
        assert!(presented.matches(&tabs, geometry));
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .scroll_viewport(Scroll::Top)
            .unwrap();
        assert!(!presented.matches(&tabs, geometry));
        let presented = Presented::capture(&tabs, geometry, false).unwrap();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), b"\x1b[Hnew")
            .unwrap();
        assert!(!presented.matches(&tabs, geometry));
        let presented = Presented::capture(&tabs, geometry, false).unwrap();
        assert!(!presented.matches(
            &tabs,
            Geometry {
                origin: 21,
                ..geometry
            }
        ));
        assert!(Presented::capture(&tabs, geometry, true).is_none());
        tabs.insert(TabId(2), 2, 8).unwrap();
        tabs.activate(TabId(2));
        assert!(!presented.matches(&tabs, geometry));
    }
}
