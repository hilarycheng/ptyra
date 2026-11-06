//! Cell-based application mouse routing, with press-owned Shift selection override.
use super::{
    pointer::{Kind, Routed},
    selection::Geometry,
};
use crate::{
    input::FocusTarget,
    keymap::Modifiers,
    tabs::{TabRegistry, output::TabPtyWrite},
};
use alacritty_terminal::{grid::Dimensions, term::TermMode};

#[derive(Default)]
pub(super) struct MouseInput {
    owner: Option<(FocusTarget, TermMode)>,
    buttons: [Option<bool>; 3], // true: accepted app press; false: local selection/paste.
    last_cell: Option<(usize, usize)>,
    wheel: [f64; 2],
}
impl MouseInput {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// True consumes the event, even if this tracking mode needs no report.
    pub fn route<P: TabPtyWrite>(
        &mut self,
        tabs: &mut TabRegistry<P>,
        event: &Routed,
        geometry: Geometry,
        modifiers: Modifiers,
        focus: Option<FocusTarget>,
    ) -> Result<bool, ()> {
        if matches!(event.kind, Kind::Leave) || tabs.active() != Some(event.target.tab) {
            self.reset();
            return Ok(false);
        }
        let Some(tab) = tabs.get(event.target.tab) else {
            self.reset();
            return Ok(false);
        };
        let Some(mode) = tab.terminal().mode().copied() else {
            self.reset();
            return Ok(false);
        };
        let owner = (
            event.target,
            mode & (TermMode::MOUSE_MODE
                | TermMode::SGR_MOUSE
                | TermMode::UTF8_MOUSE
                | TermMode::ALT_SCREEN),
        );
        if self.owner != Some(owner) {
            self.reset();
            self.owner = Some(owner);
        }
        let enabled = mode.intersects(TermMode::MOUSE_MODE);
        if !enabled {
            return Ok(false);
        }
        if focus != Some(event.target) {
            self.reset();
            return Ok(true);
        }
        let Some(grid) = tab.terminal().grid() else {
            return Ok(true);
        };
        let (x, y) = (
            event.position.x(),
            event.position.y() - f64::from(geometry.origin),
        );
        let inside =
            x >= 0. && y >= 0. && x < f64::from(geometry.size.0) && y < f64::from(geometry.size.1);
        if geometry.cell.0 == 0 || geometry.cell.1 == 0 {
            self.reset();
            return Ok(true);
        }
        let cell = (
            (x.max(0.) / f64::from(geometry.cell.0)) as usize,
            (y.max(0.) / f64::from(geometry.cell.1)) as usize,
        );
        let cell = (
            cell.0.min(grid.columns() - 1),
            cell.1.min(grid.screen_lines() - 1),
        );
        let held = self.buttons.iter().position(|owner| *owner == Some(true));
        let local = self.buttons.contains(&Some(false));
        let mut bytes = Vec::new();
        let mut accepted_button = None;
        match event.kind {
            Kind::Button {
                button, pressed, ..
            } => {
                let index = match button {
                    272 => 0,
                    274 => 1,
                    273 => 2,
                    _ => return Ok(!modifiers.shift()),
                };
                if pressed {
                    if self.buttons[index].is_some() {
                        return Ok(self.buttons[index] == Some(true));
                    }
                    if !inside {
                        return Ok(true);
                    }
                    if modifiers.shift() || local {
                        self.buttons[index] = Some(false);
                        return Ok(false);
                    }
                    if let Some(report) = encode(mode, index as u8, cell, false, modifiers) {
                        bytes = report;
                        accepted_button = Some(index);
                    }
                } else {
                    match self.buttons[index].take() {
                        Some(false) => return Ok(false),
                        Some(true) => {
                            if let Some(report) = encode(mode, index as u8, cell, true, modifiers) {
                                bytes = report;
                            }
                        }
                        None => return Ok(true),
                    }
                }
                self.last_cell = Some(cell);
            }
            Kind::Motion => {
                if local || modifiers.shift() && held.is_none() {
                    return Ok(false);
                }
                if !inside && held.is_none() || self.last_cell == Some(cell) {
                    return Ok(true);
                }
                self.last_cell = Some(cell);
                if (mode.contains(TermMode::MOUSE_MOTION)
                    || mode.contains(TermMode::MOUSE_DRAG) && held.is_some())
                    && let Some(report) = encode(
                        mode,
                        32 + held.map_or(3, |index| index as u8),
                        cell,
                        false,
                        modifiers,
                    )
                {
                    bytes = report;
                }
            }
            Kind::Axis { axis, value } => {
                if modifiers.shift() || local {
                    self.wheel = [0.; 2];
                    return Ok(false);
                }
                if !inside || axis > 1 || !value.is_finite() {
                    return Ok(true);
                }
                let axis = axis as usize;
                // Ten logical units per tick, at most 64 reports per event.
                let total = (self.wheel[axis] + value).clamp(-640., 640.);
                let ticks = (total / 10.).trunc() as i32;
                self.wheel[axis] = total - f64::from(ticks) * 10.;
                let code = 64 + 2 * axis as u8 + u8::from(ticks > 0);
                if let Some(report) = encode(mode, code, cell, false, modifiers) {
                    for _ in 0..ticks.unsigned_abs() {
                        bytes.extend_from_slice(&report);
                    }
                }
            }
            Kind::Enter | Kind::Leave => return Ok(!modifiers.shift()),
        }
        if !bytes.is_empty() {
            let pty = tabs
                .get_mut(event.target.tab)
                .and_then(|tab| tab.pty_mut())
                .filter(|pty| pty.tab() == event.target.tab)
                .ok_or(())?;
            pty.enqueue_write(&bytes).map_err(|_| ())?;
            if let Some(index) = accepted_button {
                self.buttons[index] = Some(true);
            }
        }
        Ok(true)
    }
}
fn encode(
    mode: TermMode,
    button: u8,
    (column, row): (usize, usize),
    release: bool,
    modifiers: Modifiers,
) -> Option<Vec<u8>> {
    let flags = u8::from(modifiers.shift()) * 4
        + u8::from(modifiers.alt()) * 8
        + u8::from(modifiers.ctrl()) * 16;
    if mode.contains(TermMode::SGR_MOUSE) {
        return Some(
            format!(
                "\x1b[<{};{};{}{}",
                button + flags,
                column + 1,
                row + 1,
                if release { 'm' } else { 'M' }
            )
            .into_bytes(),
        );
    }
    let limit = if mode.contains(TermMode::UTF8_MOUSE) {
        2015
    } else {
        223
    };
    // An accepted press must still end if its release moves past the encoding's range.
    let (column, row) = if release {
        (column.min(limit - 1), row.min(limit - 1))
    } else {
        if column >= limit || row >= limit {
            return None;
        }
        (column, row)
    };
    let mut bytes = vec![
        0x1b,
        b'[',
        b'M',
        32 + flags + if release { 3 } else { button },
    ];
    for value in [column + 33, row + 33] {
        if mode.contains(TermMode::UTF8_MOUSE) {
            bytes.extend_from_slice(
                char::from_u32(value as u32)?
                    .encode_utf8(&mut [0; 4])
                    .as_bytes(),
            );
        } else {
            bytes.push(value as u8);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TabId, WindowId,
        input::Point,
        pty::{PtyWriteError, PtyWriteFailure},
        tabs::TabPty,
    };
    #[derive(Default)]
    struct Fake {
        id: u64,
        bytes: Vec<u8>,
        reject: bool,
    }
    impl TabPty for Fake {
        fn tab(&self) -> TabId {
            TabId(self.id)
        }
    }
    impl TabPtyWrite for Fake {
        fn enqueue_write(&mut self, bytes: &[u8]) -> Result<(), PtyWriteError> {
            if self.reject {
                return Err(PtyWriteError {
                    tab: self.tab(),
                    pending: self.bytes.len(),
                    reason: PtyWriteFailure::QueueFull,
                });
            }
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }
    }
    fn target(id: u64) -> FocusTarget {
        FocusTarget {
            window: WindowId(1),
            tab: TabId(id),
        }
    }
    fn button(code: u32, pressed: bool) -> Kind {
        Kind::Button {
            button: code,
            pressed,
            serial: 1,
        }
    }
    fn mode(tabs: &mut TabRegistry<Fake>, bytes: &[u8]) {
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(TabId(1), bytes)
            .unwrap();
    }
    fn take(tabs: &mut TabRegistry<Fake>) -> Vec<u8> {
        std::mem::take(&mut tabs.get_mut(TabId(1)).unwrap().pty_mut().unwrap().bytes)
    }
    #[test]
    fn mouse_encoding_bounds_are_exact() {
        let none = Modifiers::default();
        for (mode, last) in [(TermMode::empty(), 222), (TermMode::UTF8_MOUSE, 2014)] {
            assert!(encode(mode, 0, (last, last), false, none).is_some());
            assert!(encode(mode, 0, (last + 1, 0), false, none).is_none());
            assert!(encode(mode, 0, (0, last + 1), false, none).is_none());
            assert_eq!(
                encode(mode, 0, (last + 100, last + 100), true, none),
                encode(mode, 0, (last, last), true, none)
            );
        }
        assert_eq!(
            encode(TermMode::empty(), 0, (222, 222), false, none).unwrap(),
            b"\x1b[M \xff\xff"
        );
        assert_eq!(
            encode(TermMode::UTF8_MOUSE, 0, (2014, 2014), false, none).unwrap(),
            "\x1b[M \u{7ff}\u{7ff}".as_bytes()
        );
        assert_eq!(
            encode(TermMode::SGR_MOUSE, 0, (299, 23), false, none).unwrap(),
            b"\x1b[<0;300;24M"
        );
    }
    #[test]
    fn runtime_mouse_reports_only_enabled_modes() {
        let mut tabs = TabRegistry::with_scrollback(10).unwrap();
        for id in [1, 2] {
            tabs.insert(TabId(id), 24, 300).unwrap();
            tabs.attach_pty(Fake {
                id,
                ..Default::default()
            })
            .ok()
            .unwrap();
        }
        let mut mouse = MouseInput::default();
        let geometry = Geometry {
            origin: 28,
            size: (3000, 480),
            cell: (10, 20),
        };
        let route =
            |mouse: &mut MouseInput, tabs: &mut TabRegistry<Fake>, kind, x, y, modifiers| {
                mouse.route(
                    tabs,
                    &Routed {
                        target: target(1),
                        position: Point::new(x, y).unwrap(),
                        kind,
                    },
                    geometry,
                    modifiers,
                    Some(target(1)),
                )
            };
        let none = Modifiers::default();
        let shift = Modifiers {
            depressed: 1,
            ..none
        };
        assert!(!route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).unwrap());
        assert!(take(&mut tabs).is_empty());
        mouse.reset();
        mode(&mut tabs, b"\x1b[?1000h\x1b[?1006h");
        assert!(route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).unwrap());
        assert_eq!(take(&mut tabs), b"\x1b[<0;2;2M");
        route(&mut mouse, &mut tabs, Kind::Motion, 25., 53., none).unwrap();
        assert!(take(&mut tabs).is_empty());
        route(&mut mouse, &mut tabs, button(272, false), 25., 53., none).unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[<0;3;2m");
        // Middle click belongs only to the app. Shift owns the whole local gesture.
        assert!(route(&mut mouse, &mut tabs, button(274, true), 15., 53., none).unwrap());
        route(&mut mouse, &mut tabs, button(274, false), 15., 53., none).unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[<1;2;2M\x1b[<1;2;2m");
        assert!(!route(&mut mouse, &mut tabs, button(272, true), 15., 53., shift).unwrap());
        assert!(!route(&mut mouse, &mut tabs, Kind::Motion, 25., 53., none).unwrap());
        assert!(!route(&mut mouse, &mut tabs, button(272, false), 25., 53., none).unwrap());
        assert!(take(&mut tabs).is_empty());
        // No app press in the strip, so release in content is not a new gesture.
        route(&mut mouse, &mut tabs, button(272, true), 15., 10., none).unwrap();
        route(&mut mouse, &mut tabs, button(272, false), 15., 53., none).unwrap();
        assert!(take(&mut tabs).is_empty());
        mode(&mut tabs, b"\x1b[?1002h");
        route(&mut mouse, &mut tabs, Kind::Motion, 15., 53., none).unwrap();
        assert!(take(&mut tabs).is_empty());
        route(&mut mouse, &mut tabs, button(273, true), 15., 53., none).unwrap();
        route(&mut mouse, &mut tabs, Kind::Motion, 16., 54., none).unwrap();
        route(&mut mouse, &mut tabs, Kind::Motion, 25., 53., none).unwrap();
        route(&mut mouse, &mut tabs, button(273, false), 25., 53., none).unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[<2;2;2M\x1b[<34;3;2M\x1b[<2;3;2m");
        mode(&mut tabs, b"\x1b[?1003h");
        route(&mut mouse, &mut tabs, Kind::Motion, 15., 53., none).unwrap();
        route(&mut mouse, &mut tabs, Kind::Motion, 16., 54., none).unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[<35;2;2M");
        let ctrl_alt = Modifiers {
            depressed: 12,
            ..none
        };
        route(&mut mouse, &mut tabs, button(272, true), 15., 53., ctrl_alt).unwrap();
        route(
            &mut mouse,
            &mut tabs,
            button(272, false),
            15.,
            53.,
            ctrl_alt,
        )
        .unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[<24;2;2M\x1b[<24;2;2m");
        for (axis, value) in [(0, -5.), (0, -5.), (0, 20.), (1, -10.), (1, 10.)] {
            route(
                &mut mouse,
                &mut tabs,
                Kind::Axis { axis, value },
                15.,
                53.,
                none,
            )
            .unwrap();
        }
        assert_eq!(
            take(&mut tabs),
            b"\x1b[<64;2;2M\x1b[<65;2;2M\x1b[<65;2;2M\x1b[<66;2;2M\x1b[<67;2;2M"
        );
        route(
            &mut mouse,
            &mut tabs,
            Kind::Axis {
                axis: 0,
                value: f64::MAX,
            },
            15.,
            53.,
            none,
        )
        .unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[<65;2;2M".repeat(64));
        assert!(
            !route(
                &mut mouse,
                &mut tabs,
                Kind::Axis {
                    axis: 0,
                    value: -10.
                },
                15.,
                53.,
                shift
            )
            .unwrap()
        );
        assert!(take(&mut tabs).is_empty());
        // Legacy and UTF-8 coordinates have different exact bounds; no wrapping.
        mode(&mut tabs, b"\x1b[?1000h\x1b[?1006l");
        route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).unwrap();
        route(&mut mouse, &mut tabs, button(272, false), 15., 53., none).unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[M \x22\x22\x1b[M#\x22\x22");
        route(&mut mouse, &mut tabs, button(272, true), 2235., 53., none).unwrap();
        route(&mut mouse, &mut tabs, button(272, false), 2235., 53., none).unwrap();
        assert!(take(&mut tabs).is_empty());
        mode(&mut tabs, b"\x1b[?1005h");
        route(&mut mouse, &mut tabs, button(272, true), 2235., 53., none).unwrap();
        route(&mut mouse, &mut tabs, button(272, false), 2235., 53., none).unwrap();
        assert_eq!(take(&mut tabs), "\x1b[M Ā\"\x1b[M#Ā\"".as_bytes());
        mode(&mut tabs, b"\x1b[?1006h");
        tabs.get_mut(TabId(1)).unwrap().pty_mut().unwrap().reject = true;
        assert!(route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).is_err());
        tabs.get_mut(TabId(1)).unwrap().pty_mut().unwrap().reject = false;
        route(&mut mouse, &mut tabs, button(272, false), 15., 53., none).unwrap();
        assert!(
            take(&mut tabs).is_empty(),
            "a rejected press cannot generate an orphan release"
        );
        // A content-owned release clips to the grid; fresh strip clicks never report.
        route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).unwrap();
        route(&mut mouse, &mut tabs, button(272, false), -5., 10., none).unwrap();
        assert_eq!(take(&mut tabs), b"\x1b[<0;2;2M\x1b[<0;1;1m");
        route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).unwrap();
        take(&mut tabs);
        mouse.reset();
        route(&mut mouse, &mut tabs, button(272, false), 15., 53., none).unwrap();
        assert!(take(&mut tabs).is_empty());
        let event = Routed {
            target: target(1),
            position: Point::new(15., 53.).unwrap(),
            kind: button(272, true),
        };
        assert!(
            mouse
                .route(&mut tabs, &event, geometry, none, None)
                .unwrap()
        );
        assert!(take(&mut tabs).is_empty());
        mode(&mut tabs, b"\x1b[?1000l\x1b[?9h\x1b[?1015h\x1b[?1016h");
        assert!(!route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).unwrap());
        assert!(
            take(&mut tabs).is_empty(),
            "unsupported X10/urxvt/pixel modes are not enabled"
        );
        mode(&mut tabs, b"\x1b[?1000h");
        tabs.activate(TabId(2));
        route(&mut mouse, &mut tabs, button(272, true), 15., 53., none).unwrap();
        assert!(take(&mut tabs).is_empty());
        assert!(tabs.get(TabId(2)).unwrap().pty().unwrap().bytes.is_empty());
    }
}
