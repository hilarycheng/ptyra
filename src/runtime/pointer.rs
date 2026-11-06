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
    pub fn cancel_drag(&mut self) {
        self.drag = None;
        self.serials.clear();
        if let Some((seat, target)) = self.target {
            self.serials.focus(seat, target);
        }
    }
    pub fn retarget(&mut self, tab: TabId) {
        self.cancel_drag();
        if let Some((seat, target)) = &mut self.target {
            target.tab = tab;
            self.serials.focus(*seat, *target);
        }
    }
    pub fn clear(&mut self) {
        self.focus.clear();
        self.target = None;
        self.enter_serial = None;
        self.cancel_drag();
    }
}
impl Runtime {
    pub(super) fn cancel_pointer_drag(&mut self) {
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
        if let Kind::Axis { axis, value } = event.kind {
            if event.position.x() >= 0.
                && event.position.x() < f64::from(size.0)
                && event.position.y() >= f64::from(origin)
                && event.position.y() < f64::from(origin + size.1)
                && let Some(mode) = self
                    .tabs
                    .get(event.target.tab)
                    .and_then(|tab| tab.terminal().mode())
                && let Some(scroll) = self.wheel.input(event.target.tab, *mode, axis, value)
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
        let selection_changed = self.selection_input.route(
            &mut self.tabs,
            &event,
            super::selection::Geometry {
                origin,
                size,
                cell: (self.raster.cell_width, self.raster.cell_height),
            },
            &self.clock,
        );
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
