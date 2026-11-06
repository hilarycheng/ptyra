//! Seat-owned Wayland Clipboard adapter. Payloads never appear in diagnostics.
use super::Runtime;
use crate::{
    clipboard::{DataSourceRequest, InputSerialEvent, SeatId, SeatSerialState},
    input::FocusTarget,
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source},
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
struct CopyPlan {
    text: String,
    requests: [DataSourceRequest; 4],
}
fn prepare_copy<P>(
    tab: &mut crate::tabs::Tab<P>,
    binding: &mut Binding,
    seat: SeatId,
    target: FocusTarget,
    limit: usize,
) -> super::Result<CopyPlan> {
    if tab.id() != target.tab || binding.current.is_none_or(|id| id.seat != seat) {
        return Err("clipboard unavailable");
    }
    let range = tab.selection_range().ok_or("no valid selection")?;
    let text = tab
        .terminal_mut()
        .copy_range_text(range)
        .ok_or("selection unavailable")?;
    if text.is_empty() {
        return Err("no valid selection");
    }
    if text.len() > limit {
        return Err("selection exceeds clipboard limit");
    }
    let requests = crate::clipboard::plan_data_source(&mut binding.serials, seat, target)
        .ok_or("no valid input serial")?;
    Ok(CopyPlan { text, requests })
}
struct Source {
    id: u64,
    proxy: wl_data_source::WlDataSource,
    tab: crate::TabId,
    text: String,
}
#[derive(Default)]
pub(super) struct Clipboard {
    pub manager: Option<(u32, wl_data_device_manager::WlDataDeviceManager)>,
    device: Option<wl_data_device::WlDataDevice>,
    pub binding: Binding,
    offers: super::clipboard_offers::Offers<wl_data_offer::WlDataOffer>,
    source: Option<Source>,
    limit: usize,
    source_generation: u64,
    sends: super::clipboard_send::SendQueue<std::fs::File>,
    pub ready_sends: Vec<u64>,
    receive: super::clipboard_receive::Receive<std::fs::File>,
    pub ready_receive: Option<u64>,
}
impl Clipboard {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }
    fn remove_source(&mut self) {
        if let Some(source) = self.source.take() {
            self.sends.cancel_source(source.id);
            source.proxy.destroy();
            self.offers.state.cancel_local_owner(source.tab);
        }
    }
    pub fn send_fds(&self) -> impl Iterator<Item = (u64, &std::fs::File)> {
        self.sends.fds()
    }
    pub fn pump_sends(&mut self) {
        if self.sends.pump(&std::mem::take(&mut self.ready_sends)) > 0 {
            eprintln!("Ptyra copy: transfer failed");
        }
    }
    pub fn receive_fd(&self) -> Option<(u64, &std::fs::File)> {
        self.receive.fd()
    }
    pub fn cancel_receive(&mut self) {
        self.receive.cancel();
        self.ready_receive = None;
    }
    pub fn pump_receive(&mut self) {
        match self.receive.pump(self.ready_receive.take(), &self.offers) {
            Ok(Some(text)) => {
                // PTY delivery and multiline safety are connected by TASK-150/151.
                eprintln!(
                    "Ptyra paste: received {} bytes; delivery unavailable",
                    text.len()
                );
            }
            Err(error) => eprintln!("Ptyra paste: {error}"),
            Ok(None) => {}
        }
    }
    fn start_receive(&mut self) -> super::Result<()> {
        use std::os::fd::AsFd;
        self.cancel_receive();
        let (proxy, offer) = self.offers.selected().ok_or("clipboard has no offer")?;
        let mime = offer
            .preferred_text_mime()
            .ok_or("clipboard has no text offer")?;
        let (read, write) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)
            .map_err(|_| "clipboard receive pipe unavailable")?;
        let file = std::fs::File::from(read);
        super::clipboard_send::nonblocking(&file)?;
        self.receive.start(offer.id(), file, self.limit)?;
        proxy.receive(mime.to_owned(), write.as_fd());
        // The connection owns its queued SCM_RIGHTS copy; close our writer now.
        Ok(())
    }
    fn clear_offers(&mut self) {
        self.cancel_receive();
        for offer in self.offers.clear() {
            offer.destroy();
        }
    }
    pub fn clear_external_offers(&mut self) {
        self.cancel_receive();
        let retired = self.offers.clear_external();
        if !retired.is_empty()
            && std::env::var_os("PTYRA_TRACE_CLIPBOARD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            eprintln!("Ptyra clipboard: external offers cleared");
        }
        for offer in retired {
            offer.destroy();
        }
    }
    fn remove_device(&mut self) {
        self.remove_source();
        self.clear_offers();
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
    pub(super) fn receive_clipboard(&mut self) -> super::Result<()> {
        self.input.focus.target().ok_or("no keyboard focus")?;
        self.clipboard.start_receive()
    }

    pub(super) fn copy_selection(&mut self, qh: &QueueHandle<Self>) -> super::Result<()> {
        let target = self.input.focus.target().ok_or("no keyboard focus")?;
        let seat = SeatId(self.seat_name.ok_or("clipboard unavailable")?);
        let manager = self
            .clipboard
            .manager
            .as_ref()
            .map(|(_, manager)| manager.clone())
            .ok_or("clipboard unavailable")?;
        let device = self
            .clipboard
            .device
            .clone()
            .ok_or("clipboard unavailable")?;
        let tab = self
            .tabs
            .get_mut(target.tab)
            .ok_or("selection unavailable")?;
        let plan = prepare_copy(
            tab,
            &mut self.clipboard.binding,
            seat,
            target,
            self.clipboard.limit,
        )?;
        let source_id = self
            .clipboard
            .source_generation
            .checked_add(1)
            .ok_or("clipboard source identity exhausted")?;
        self.clipboard.source_generation = source_id;
        self.clipboard.remove_source();
        self.clipboard.clear_offers();
        if !self.clipboard.offers.state.set_local_owner(target.tab) {
            return Err("clipboard identity exhausted");
        }
        let proxy = manager.create_data_source(qh, target.tab);
        for request in plan.requests {
            match request {
                DataSourceRequest::Create => {}
                DataSourceRequest::Offer(mime) => proxy.offer(mime.into()),
                DataSourceRequest::SetSelection { serial, .. } => {
                    device.set_selection(Some(&proxy), serial.0)
                }
            }
        }
        self.clipboard.source = Some(Source {
            id: source_id,
            proxy,
            tab: target.tab,
            text: plan.text,
        });
        eprintln!("Ptyra copy: published");
        Ok(())
    }
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
        self.clipboard.remove_source();
        self.clipboard.clear_offers();
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
        match event {
            wl_data_device::Event::DataOffer { id } => {
                if let Err(offer) = state.clipboard.offers.created(id) {
                    // Capacity/identity failure must not leave older text readable.
                    state.clipboard.clear_external_offers();
                    offer.destroy();
                }
            }
            wl_data_device::Event::Selection { id } => {
                let (changed, retired) = state.clipboard.offers.select(id.as_ref());
                for offer in retired {
                    offer.destroy();
                }
                if changed
                    && std::env::var_os("PTYRA_TRACE_CLIPBOARD").as_deref()
                        == Some(std::ffi::OsStr::new("1"))
                {
                    let message = match state.clipboard.offers.selected() {
                        Some((_, offer)) if offer.preferred_text_mime().is_some() => {
                            "text offer selected"
                        }
                        Some(_) => "non-text offer selected",
                        None => "offer cleared",
                    };
                    eprintln!("Ptyra clipboard: {message}");
                }
            }
            wl_data_device::Event::Enter {
                id: Some(offer), ..
            } => {
                if state
                    .clipboard
                    .offers
                    .selected()
                    .is_none_or(|(current, _)| current != &offer)
                    && let Some(offer) = state.clipboard.offers.discard(&offer)
                {
                    offer.destroy();
                }
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(Runtime,wl_data_device::WlDataDevice,[0 => (wl_data_offer::WlDataOffer,())]);
}
impl Dispatch<wl_data_source::WlDataSource, crate::TabId> for Runtime {
    fn event(
        state: &mut Self,
        proxy: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        _: &crate::TabId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state
            .clipboard
            .source
            .as_ref()
            .is_none_or(|source| &source.proxy != proxy)
        {
            return;
        }
        match event {
            wl_data_source::Event::Cancelled => {
                state.clipboard.remove_source();
                eprintln!("Ptyra copy: cancelled");
            }
            wl_data_source::Event::Send { fd, mime_type } => {
                if !matches!(
                    mime_type.as_str(),
                    "text/plain;charset=utf-8" | "text/plain"
                ) {
                    return;
                }
                let file = std::fs::File::from(fd);
                if let Err(error) = super::clipboard_send::nonblocking(&file) {
                    eprintln!("Ptyra copy: {error}");
                    return;
                }
                let source = state.clipboard.source.as_ref().unwrap();
                if let Err(error) = state.clipboard.sends.start(source.id, &source.text, file) {
                    eprintln!("Ptyra copy: {error}");
                }
            }
            _ => {}
        }
    }
}
delegate_noop!(Runtime: ignore wl_data_device_manager::WlDataDeviceManager);
impl Dispatch<wl_data_offer::WlDataOffer, ()> for Runtime {
    fn event(
        state: &mut Self,
        proxy: &wl_data_offer::WlDataOffer,
        event: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event
            && state.clipboard.offers.advertise(proxy, &mime_type).is_err()
        {
            eprintln!("Ptyra clipboard: invalid offer");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, WindowId};
    #[test]
    fn runtime_copy_publishes_selected_text_only() {
        use crate::{
            selection::Screen,
            tabs::{SelectionAction, TabPty, TabRegistry},
        };
        use alacritty_terminal::index::{Column, Line, Point};
        struct Fake;
        impl TabPty for Fake {
            fn tab(&self) -> TabId {
                TabId(1)
            }
        }
        let mut tabs = TabRegistry::<Fake>::with_scrollback(0).unwrap();
        tabs.insert(TabId(1), 3, 8).unwrap();
        let tab = tabs.get_mut(TabId(1)).unwrap();
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(1),
        };
        let seat = SeatId(7);
        let mut binding = Binding::default();
        binding.sync(Some((4, seat)));
        binding.record(seat, target, 50, InputSerialEvent::KeyPress);
        assert!(prepare_copy(tab, &mut binding, seat, target, 1024).is_err());
        tab.terminal_mut()
            .feed(TabId(1), "a中e\u{301} Z\r\nB".as_bytes())
            .unwrap();
        tab.select(
            Screen::Main,
            SelectionAction::Press,
            Point::new(Line(0), Column(0)),
        );
        tab.select(
            Screen::Main,
            SelectionAction::Release,
            Point::new(Line(1), Column(0)),
        );
        let plan = prepare_copy(tab, &mut binding, seat, target, 1024).unwrap();
        assert_eq!(plan.text, "a中e\u{301} Z  \nB");
        assert!(matches!(
            plan.requests[3],
            crate::clipboard::DataSourceRequest::SetSelection {
                serial: crate::clipboard::SelectionSerial(50),
                ..
            }
        ));
        assert!(prepare_copy(tab, &mut binding, seat, target, 1024).is_err());
        binding.record(seat, target, 51, InputSerialEvent::KeyPress);
        assert!(prepare_copy(tab, &mut binding, seat, target, 2).is_err());
        assert!(prepare_copy(tab, &mut binding, seat, target, 1024).is_ok());
        tab.terminal_mut()
            .feed(TabId(1), b"\x1b[2J\x1b[H12345678Q")
            .unwrap();
        tab.select(
            Screen::Main,
            SelectionAction::Press,
            Point::new(Line(0), Column(0)),
        );
        tab.select(
            Screen::Main,
            SelectionAction::Release,
            Point::new(Line(1), Column(0)),
        );
        binding.record(seat, target, 52, InputSerialEvent::KeyPress);
        assert_eq!(
            prepare_copy(tab, &mut binding, seat, target, 1024)
                .unwrap()
                .text,
            "12345678Q"
        );
    }
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
