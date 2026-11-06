//! Seat-scoped pointer adapter. Routed events capture their original target.
use super::Runtime;
use crate::{
    TabId, WindowId,
    clipboard::{InputSerialEvent, SeatId, SeatSerialState},
    geometry::Scale,
    input::{FocusTarget, Point, PointerFocus},
};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, protocol::wl_pointer};

pub(super) enum Kind {
    Enter,
    Leave,
    Motion,
    Button {
        button: u32,
        pressed: bool,
        serial: u32,
    },
    Axis {
        axis: u32,
        value: f64,
    },
}
pub(super) struct Routed {
    pub target: FocusTarget,
    pub position: Point,
    pub kind: Kind,
}
#[derive(Default)]
pub(super) struct PointerInput {
    focus: PointerFocus,
    target: Option<(SeatId, FocusTarget)>,
    pub serials: SeatSerialState,
    enter_serial: Option<u32>,
    drag: Option<TabId>,
    mouse: super::mouse::MouseInput,
}
impl PointerInput {
    fn route(&self, kind: Kind) -> Option<Routed> {
        let target = self.target?.1;
        let click = self.focus.button(0, false, Scale::new(1).ok()?).ok()??;
        Some(Routed {
            target,
            position: click.buffer,
            kind,
        })
    }
    fn enter(
        &mut self,
        seat: SeatId,
        target: FocusTarget,
        point: Point,
        serial: u32,
    ) -> Option<Routed> {
        self.clear();
        self.target = Some((seat, target));
        self.focus.enter(target.window, point);
        self.serials.focus(seat, target);
        self.enter_serial = Some(serial);
        self.route(Kind::Enter)
    }
    fn motion(&mut self, point: Point) -> Option<Routed> {
        self.focus.motion(point);
        self.route(Kind::Motion)
    }
    fn button(&mut self, button: u32, pressed: bool, serial: u32) -> Option<Routed> {
        let (seat, target) = self.target?;
        if pressed {
            self.serials
                .record_event(seat, target, serial, InputSerialEvent::PointerButtonPress);
            self.drag = Some(target.tab);
        } else {
            self.drag = None;
        }
        self.route(Kind::Button {
            button,
            pressed,
            serial,
        })
    }
    // Viewport scrolling cancels selection, but keeps ownership until each button releases.
    pub fn cancel_drag(&mut self) {
        self.drag = None;
        self.serials.clear();
        if let Some((seat, target)) = self.target {
            self.serials.focus(seat, target);
        }
    }
    pub fn retarget(&mut self, tab: TabId) {
        self.mouse.reset();
        self.cancel_drag();
        if let Some((seat, target)) = &mut self.target {
            target.tab = tab;
            self.serials.focus(*seat, *target);
        }
    }
    pub fn clear(&mut self) {
        self.mouse.reset();
        self.focus.clear();
        self.target = None;
        self.enter_serial = None;
        self.cancel_drag();
    }
}
impl Runtime {
    pub(super) fn cancel_pointer_drag(&mut self) {
        self.pointer_input.mouse.reset();
        self.clear_clipboard_notice();
        self.wheel.reset();
        self.clipboard.binding.focus(None);
        self.primary.binding.focus(None);
        self.pointer_input.cancel_drag();
        self.paint.dirty |= self.selection_input.cancel(&mut self.tabs);
    }
    pub(super) fn remove_pointer(&mut self) {
        self.cancel_pointer_drag();
        if let Some(pointer) = self.pointer.take() {
            pointer.release();
        }
        self.pointer_input.clear();
    }
    pub(super) fn route_pointer(&mut self, event: Routed, qh: &QueueHandle<Self>) {
        if self.tabs.active() != Some(event.target.tab) {
            return;
        }
        if matches!(event.kind, Kind::Enter | Kind::Leave) {
            self.wheel.reset();
        }
        if let Kind::Button {
            pressed: true,
            serial,
            ..
        } = event.kind
            && let Some(seat) = self.seat_name
        {
            self.clipboard.binding.record(
                SeatId(seat),
                event.target,
                serial,
                InputSerialEvent::PointerButtonPress,
            );
            self.primary.binding.record(
                SeatId(seat),
                event.target,
                serial,
                InputSerialEvent::PointerButtonPress,
            );
        }
        let link_frame = if self.clipboard_notice.is_none() && self.presented_paste.is_none() {
            self.presented_hyperlink
        } else {
            None
        };
        if matches!(event.kind, Kind::Button { pressed: true, .. }) {
            self.clear_clipboard_notice();
        }
        // Only the last submitted layout is clickable, and only at its current size.
        if self.trace_pointer {
            let kind = match event.kind {
                Kind::Enter => "enter",
                Kind::Leave => "leave",
                Kind::Motion => "motion",
                Kind::Button { pressed: true, .. } => "press",
                Kind::Button { .. } => "release",
                Kind::Axis { .. } => "axis",
            };
            eprintln!("Ptyra pointer: {kind}");
        }
        if let Kind::Button {
            button: 272,
            pressed: true,
            ..
        } = event.kind
            && let Some(id) = self
                .presented_bar
                .as_ref()
                .and_then(|layout| layout.tab_at(&self.tabs, self.paint.size, event.position))
        {
            self.cancel_pointer_drag();
            self.activate_tab(id);
            return;
        }
        if self
            .paste
            .as_ref()
            .and_then(|paste| paste.prompt_token(&self.input.focus))
            .is_some()
        {
            return;
        }
        let (origin, size) = self.content_geometry();
        let geometry = super::selection::Geometry {
            origin,
            size,
            cell: (self.raster.cell_width, self.raster.cell_height),
        };
        let modifiers = self
            .input
            .map
            .as_ref()
            .map(|map| map.modifiers())
            .unwrap_or_default();
        match self.pointer_input.mouse.route(
            &mut self.tabs,
            &event,
            geometry,
            modifiers,
            self.input.focus.target(),
        ) {
            Ok(false) => {}
            result => {
                self.wheel.reset();
                self.paint.dirty |= self.selection_input.cancel(&mut self.tabs);
                if result.is_err()
                    && self.clipboard_notice != Some(super::notice::Notice::MouseRejected)
                {
                    self.show_clipboard_notice(super::notice::Notice::MouseRejected);
                }
                return;
            }
        }
        // Application mouse owns its gestures. Shift+Ctrl+click uses the local override.
        match super::hyperlink::route(
            &self.tabs,
            &event,
            geometry,
            link_frame.as_ref(),
            modifiers,
            self.input.focus.target(),
            |request| self.hyperlinks.open(request),
        ) {
            Ok(false) => {}
            result => {
                self.paint.dirty |= self.selection_input.cancel(&mut self.tabs);
                if result.is_err() {
                    self.show_clipboard_notice(super::notice::Notice::HyperlinkFailed);
                }
                return;
            }
        }
        if let Kind::Axis { axis, value } = event.kind {
            if event.position.x() >= 0.
                && event.position.x() < f64::from(size.0)
                && event.position.y() >= f64::from(origin)
                && event.position.y() < f64::from(origin + size.1)
                && let Some(mode) = self
                    .tabs
                    .get(event.target.tab)
                    .and_then(|tab| tab.terminal().mode())
                && let Some(scroll) = self.wheel.input(
                    event.target.tab,
                    *mode & !alacritty_terminal::term::TermMode::MOUSE_MODE,
                    axis,
                    value,
                )
            {
                self.scroll_active(scroll);
            }
            return;
        }
        if matches!(
            event.kind,
            Kind::Button {
                button: 274,
                pressed: true,
                ..
            }
        ) && event.position.x() >= 0.
            && event.position.x() < f64::from(size.0)
            && event.position.y() >= f64::from(origin)
            && event.position.y() < f64::from(origin + size.1)
            && self.input.focus.target() == Some(event.target)
        {
            if let Err(error) = self.receive_selection(crate::clipboard::SelectionKind::Primary) {
                eprintln!("Ptyra paste: {error}");
                self.clipboard_error(error);
            }
            return;
        }
        let selection_changed =
            self.selection_input
                .route(&mut self.tabs, &event, geometry, &self.clock);
        if self.selection_input.take_publication()
            && self.primary.manager.is_some()
            && let Err(error) = self.publish_primary(qh)
        {
            eprintln!("Ptyra primary: {error}");
            self.clipboard_error(error);
        }
        self.paint.dirty |= selection_changed;
        if selection_changed && self.trace_pointer {
            eprintln!("Ptyra selection: updated");
            if let Some(range) = self
                .tabs
                .get(event.target.tab)
                .and_then(|tab| tab.selection_range())
            {
                eprintln!(
                    "Ptyra selection geometry: {}:{}-{}:{} cell={}:{} origin={}",
                    range.start.line.0,
                    range.start.column.0,
                    range.end.line.0,
                    range.end.column.0,
                    self.raster.cell_width,
                    self.raster.cell_height,
                    origin
                );
            }
        }
        match event.kind {
            Kind::Button { button, serial, .. } => {
                let _ = (button, serial, event.position);
            }
            Kind::Axis { axis, value } => {
                let _ = (axis, value, event.position);
            }
            _ => {}
        }
    }
}
impl Dispatch<wl_pointer::WlPointer, ()> for Runtime {
    fn event(
        state: &mut Self,
        pointer: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if state.pointer.as_ref() != Some(pointer) {
            return;
        }
        let routed = match event {
            wl_pointer::Event::Enter {
                serial,
                surface,
                surface_x,
                surface_y,
            } if state.surface.as_ref() == Some(&surface) => {
                match (
                    state.seat_name,
                    state.tabs.active(),
                    Point::new(surface_x, surface_y),
                ) {
                    (Some(seat), Some(tab), Ok(point)) => state.pointer_input.enter(
                        SeatId(seat),
                        FocusTarget {
                            window: WindowId(1),
                            tab,
                        },
                        point,
                        serial,
                    ),
                    _ => None,
                }
            }
            wl_pointer::Event::Leave { surface, .. }
                if state.surface.as_ref() == Some(&surface) =>
            {
                let event = state.pointer_input.route(Kind::Leave);
                state.pointer_input.clear();
                event
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => Point::new(surface_x, surface_y)
                .ok()
                .and_then(|point| state.pointer_input.motion(point)),
            wl_pointer::Event::Button {
                serial,
                button,
                state: WEnum::Value(pressed),
                ..
            } => state.pointer_input.button(
                button,
                pressed == wl_pointer::ButtonState::Pressed,
                serial,
            ),
            wl_pointer::Event::Axis {
                axis: WEnum::Value(axis),
                value,
                ..
            } if value.is_finite() => state.pointer_input.route(Kind::Axis {
                axis: axis as u32,
                value,
            }),
            _ => None,
        };
        if let Some(event) = routed {
            state.route_pointer(event, qh);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn viewport_scroll_preserves_mouse_press_ownership() {
        use crate::{
            keymap::Modifiers,
            tabs::{TabPty, TabRegistry, output::TabPtyWrite},
        };
        #[derive(Default)]
        struct Sink(Vec<u8>);
        impl TabPty for Sink {
            fn tab(&self) -> TabId {
                TabId(1)
            }
        }
        impl TabPtyWrite for Sink {
            fn enqueue_write(&mut self, bytes: &[u8]) -> Result<(), crate::pty::PtyWriteError> {
                self.0.extend_from_slice(bytes);
                Ok(())
            }
        }
        let mut tabs = TabRegistry::with_scrollback(20).unwrap();
        tabs.insert(TabId(1), 4, 20).unwrap();
        tabs.attach_pty(Sink::default()).ok().unwrap();
        tabs.get_mut(TabId(1))
            .unwrap()
            .terminal_mut()
            .feed(
                TabId(1),
                b"line\r\nline\r\nline\r\nline\r\nline\r\n\x1b[?1003h\x1b[?1006h",
            )
            .unwrap();
        let mut pointer = PointerInput::default();
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(1),
        };
        let geometry = super::super::selection::Geometry {
            origin: 20,
            size: (200, 80),
            cell: (10, 20),
        };
        let route = |pointer: &mut PointerInput, tabs: &mut TabRegistry<Sink>, kind, modifiers| {
            pointer
                .mouse
                .route(
                    tabs,
                    &Routed {
                        target,
                        position: Point::new(15., 45.).unwrap(),
                        kind,
                    },
                    geometry,
                    modifiers,
                    Some(target),
                )
                .unwrap()
        };
        let press = || Kind::Button {
            button: 272,
            pressed: true,
            serial: 1,
        };
        let release = || Kind::Button {
            button: 272,
            pressed: false,
            serial: 2,
        };
        let none = Modifiers::default();
        for local in [false, true] {
            assert_eq!(
                route(
                    &mut pointer,
                    &mut tabs,
                    press(),
                    Modifiers {
                        depressed: u32::from(local),
                        ..none
                    }
                ),
                !local
            );
            assert!(
                tabs.get_mut(TabId(1))
                    .unwrap()
                    .terminal_mut()
                    .scroll_viewport(alacritty_terminal::grid::Scroll::Delta(1))
                    .unwrap()
            );
            pointer.cancel_drag(); // The viewport path, unlike focus/tab/pointer removal.
            if local {
                assert!(!route(&mut pointer, &mut tabs, Kind::Motion, none));
            }
            assert_eq!(route(&mut pointer, &mut tabs, release(), none), !local);
        }
        assert_eq!(
            tabs.get(TabId(1)).unwrap().pty().unwrap().0,
            b"\x1b[<0;2;2M\x1b[<0;2;2m"
        );
    }
    #[test]
    fn runtime_pointer_lifecycle_preserves_serials() {
        let mut pointer = PointerInput::default();
        let seat = SeatId(7);
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(2),
        };
        let point = Point::new(12.5, 30.25).unwrap();
        assert!(pointer.button(272, true, 1).is_none());
        let enter = pointer.enter(seat, target, point, 50).unwrap();
        assert_eq!(enter.target, target);
        assert_eq!(enter.position, point);
        assert_eq!(pointer.enter_serial, Some(50));
        assert!(pointer.serials.take_for_selection(seat, target).is_none());
        let pressed = pointer.button(272, true, 0).unwrap();
        assert_eq!(pressed.target, target);
        assert_eq!(
            pointer.serials.take_for_selection(seat, target).unwrap().0,
            0
        );
        assert_eq!(pointer.drag, Some(TabId(2)));
        pointer.cancel_drag();
        pointer.button(272, true, 51).unwrap();
        assert_eq!(
            pointer.serials.take_for_selection(seat, target).unwrap().0,
            51
        );
        pointer.retarget(TabId(3));
        assert!(pointer.drag.is_none());
        assert!(pointer.serials.take_for_selection(seat, target).is_none());
        assert_eq!(pressed.target.tab, TabId(2)); // Existing events never retarget.
        assert_eq!(pointer.motion(point).unwrap().target.tab, TabId(3));
        pointer.clear();
        assert!(pointer.enter_serial.is_none());
        assert!(pointer.motion(point).is_none());
        assert!(pointer.button(272, false, 52).is_none());
    }
}
