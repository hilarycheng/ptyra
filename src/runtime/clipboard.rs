//! Seat-owned Wayland Clipboard adapter. Payloads never appear in diagnostics.
use super::Runtime;
use crate::{
    clipboard::{InputSerialEvent, SeatId, SeatSerialState},
    input::FocusTarget,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_data_device, wl_data_device_manager, wl_data_offer},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DeviceId {
    manager: u32,
    seat: SeatId,
    generation: u64,
}
#[derive(Default)]
pub(super) struct Binding {
    current: Option<DeviceId>,
    generation: u64,
    target: Option<FocusTarget>,
    pub serials: SeatSerialState,
}
impl Binding {
    fn sync(&mut self, wanted: Option<(u32, SeatId)>) -> bool {
        if self.current.map(|id| (id.manager, id.seat)) == wanted {
            return false;
        }
        self.current = None;
        self.focus(None);
        if let Some((manager, seat)) = wanted
            && let Some(generation) = self.generation.checked_add(1)
        {
            self.generation = generation;
            self.current = Some(DeviceId {
                manager,
                seat,
                generation,
            });
        }
        true
    }
    pub fn focus(&mut self, target: Option<FocusTarget>) {
        self.serials.clear();
        self.target = target;
        if let (Some(id), Some(target)) = (self.current, target) {
            self.serials.focus(id.seat, target);
        }
    }
    pub fn record(
        &mut self,
        seat: SeatId,
        target: FocusTarget,
        serial: u32,
        event: InputSerialEvent,
    ) -> bool {
        if self.current.is_none_or(|id| id.seat != seat) {
            return false;
        }
        if self.target != Some(target) {
            self.focus(Some(target));
        }
        self.serials.record_event(seat, target, serial, event)
    }
}
#[derive(Default)]
pub(super) struct Clipboard {
    pub manager: Option<(u32, wl_data_device_manager::WlDataDeviceManager)>,
    device: Option<wl_data_device::WlDataDevice>,
    pub binding: Binding,
}
impl Clipboard {
    fn remove_device(&mut self) {
        if let Some(device) = self.device.take() {
            device.release();
        }
        self.binding.sync(None);
    }
    pub fn remove_manager(&mut self) {
        self.remove_device();
        self.manager = None; // wl_data_device_manager has no destructor request.
    }
}
impl Runtime {
    pub(super) fn remove_clipboard_device(&mut self) {
        self.clipboard.remove_device();
    }
    pub(super) fn bind_clipboard(&mut self, qh: &QueueHandle<Self>) {
        let wanted = self
            .clipboard
            .manager
            .as_ref()
            .zip(self.seat_name)
            .map(|((name, _), seat)| (*name, SeatId(seat)));
        if !self.clipboard.binding.sync(wanted) {
            return;
        }
        if let Some(device) = self.clipboard.device.take() {
            device.release();
        }
        if let (Some((_, manager)), Some(seat), Some(id)) = (
            &self.clipboard.manager,
            &self.seat,
            self.clipboard.binding.current,
        ) {
            self.clipboard.device = Some(manager.get_data_device(seat, qh, id));
            if std::env::var_os("PTYRA_TRACE_CLIPBOARD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                eprintln!("Ptyra clipboard: device bound");
            }
        }
    }
}
impl Dispatch<wl_data_device::WlDataDevice, DeviceId> for Runtime {
    fn event(
        state: &mut Self,
        device: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        id: &DeviceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let current = state.clipboard.device.as_ref() == Some(device)
            && state.clipboard.binding.current == Some(*id);
        if !current {
            if let wl_data_device::Event::DataOffer { id } = event {
                id.destroy();
            }
            return;
        }
        // Offer consumption is TASK-148. Retire announcements until that lifecycle is connected.
        if let wl_data_device::Event::DataOffer { id } = event {
            id.destroy();
        }
    }
    wayland_client::event_created_child!(Runtime,wl_data_device::WlDataDevice,[0 => (wl_data_offer::WlDataOffer,())]);
}
delegate_noop!(Runtime: ignore wl_data_device_manager::WlDataDeviceManager);
delegate_noop!(Runtime: ignore wl_data_offer::WlDataOffer);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, WindowId};
    #[test]
    fn runtime_data_device_tracks_seat_lifecycle() {
        let seat = SeatId(7);
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(2),
        };
        let mut binding = Binding::default();
        assert!(!binding.sync(None));
        assert!(!binding.record(seat, target, 1, InputSerialEvent::KeyPress));
        assert!(binding.sync(Some((4, seat))));
        let first = binding.current.unwrap();
        assert!(!binding.sync(Some((4, seat))));
        assert!(!binding.record(SeatId(8), target, 2, InputSerialEvent::KeyPress));
        assert!(binding.record(seat, target, 0, InputSerialEvent::KeyPress));
        assert_eq!(
            binding.serials.take_for_selection(seat, target).unwrap().0,
            0
        );
        assert!(binding.record(seat, target, 3, InputSerialEvent::PointerButtonPress));
        binding.focus(None);
        assert!(binding.serials.take_for_selection(seat, target).is_none());
        assert!(binding.sync(None));
        assert!(binding.current.is_none());
        assert!(binding.sync(Some((4, seat))));
        assert_ne!(binding.current.unwrap(), first);
        assert!(binding.sync(Some((5, seat))));
        assert_eq!(binding.current.unwrap().manager, 5);
        assert!(binding.serials.take_for_selection(seat, target).is_none());
    }
}
