//! Optional Primary Selection transport, independent from ordinary Clipboard ownership.
use super::{
    Runtime,
    clipboard::{Binding, DeviceId, prepare_copy},
};
use crate::clipboard::{DataSourceRequest, SeatId};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::primary_selection::zv1::client::{
    zwp_primary_selection_device_manager_v1 as manager, zwp_primary_selection_device_v1 as device,
    zwp_primary_selection_offer_v1 as offer, zwp_primary_selection_source_v1 as source,
};
struct Source {
    id: u64,
    proxy: source::ZwpPrimarySelectionSourceV1,
    tab: crate::TabId,
    text: String,
}
pub(super) struct Primary {
    pub manager: Option<(u32, manager::ZwpPrimarySelectionDeviceManagerV1)>,
    device: Option<device::ZwpPrimarySelectionDeviceV1>,
    pub binding: Binding,
    offers: super::clipboard_offers::Offers<offer::ZwpPrimarySelectionOfferV1>,
    source: Option<Source>,
    limit: usize,
    source_generation: u64,
    sends: super::clipboard_send::SendQueue<std::fs::File>,
    pub ready_sends: Vec<u64>,
    receive: super::clipboard_receive::Receive<std::fs::File>,
    pub ready_receive: Option<u64>,
}
impl Primary {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            offers: super::clipboard_offers::Offers::new_primary(),
            manager: None,
            device: None,
            binding: Binding::default(),
            source: None,
            source_generation: 0,
            sends: Default::default(),
            ready_sends: Vec::new(),
            receive: Default::default(),
            ready_receive: None,
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
            eprintln!("Ptyra primary: transfer failed");
        }
    }
    pub fn receive_fd(&self) -> Option<(u64, &std::fs::File)> {
        self.receive.fd()
    }
    pub fn cancel_receive(&mut self) {
        self.receive.cancel();
        self.ready_receive = None;
    }
    pub fn pump_receive(&mut self) -> super::Result<Option<String>> {
        self.receive.pump(self.ready_receive.take(), &self.offers)
    }
    pub fn start_receive(&mut self) -> super::Result<()> {
        use std::os::fd::AsFd;
        self.cancel_receive();
        if self.device.is_none() {
            return Err("primary selection unavailable");
        }
        let (proxy, offer) = self.offers.selected().ok_or("primary has no offer")?;
        let mime = offer
            .preferred_text_mime()
            .ok_or("primary has no text offer")?;
        let (read, write) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)
            .map_err(|_| "primary receive pipe unavailable")?;
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
            eprintln!("Ptyra primary: external offers cleared");
        }
        for offer in retired {
            offer.destroy();
        }
    }
    fn remove_device(&mut self) {
        self.remove_source();
        self.clear_offers();
        if let Some(device) = self.device.take() {
            device.destroy();
        }
        self.binding.sync(None);
    }
    pub fn remove_manager(&mut self) {
        self.remove_device();
        if let Some((_, manager)) = self.manager.take() {
            manager.destroy();
        }
    }
}
impl Runtime {
    pub(super) fn publish_primary(&mut self, qh: &QueueHandle<Self>) -> super::Result<()> {
        let target = self.input.focus.target().ok_or("no keyboard focus")?;
        let seat = SeatId(self.seat_name.ok_or("primary unavailable")?);
        let manager = self
            .primary
            .manager
            .as_ref()
            .map(|(_, manager)| manager.clone())
            .ok_or("primary unavailable")?;
        let device = self.primary.device.clone().ok_or("primary unavailable")?;
        let tab = self
            .tabs
            .get_mut(target.tab)
            .ok_or("selection unavailable")?;
        let plan = prepare_copy(
            tab,
            &mut self.primary.binding,
            seat,
            target,
            self.primary.limit,
        )?;
        let source_id = self
            .primary
            .source_generation
            .checked_add(1)
            .ok_or("primary source identity exhausted")?;
        self.primary.source_generation = source_id;
        self.primary.remove_source();
        self.primary.clear_offers();
        if !self.primary.offers.state.set_local_owner(target.tab) {
            return Err("primary identity exhausted");
        }
        let proxy = manager.create_source(qh, target.tab);
        for request in plan.requests {
            match request {
                DataSourceRequest::Create => {}
                DataSourceRequest::Offer(mime) => proxy.offer(mime.into()),
                DataSourceRequest::SetSelection { serial, .. } => {
                    device.set_selection(Some(&proxy), serial.0)
                }
            }
        }
        self.primary.source = Some(Source {
            id: source_id,
            proxy,
            tab: target.tab,
            text: plan.text,
        });
        eprintln!("Ptyra primary: published");
        Ok(())
    }
    pub(super) fn remove_primary_device(&mut self) {
        self.primary.remove_device();
    }
    pub(super) fn bind_primary(&mut self, qh: &QueueHandle<Self>) {
        let wanted = self
            .primary
            .manager
            .as_ref()
            .zip(self.seat_name)
            .map(|((name, _), seat)| (*name, SeatId(seat)));
        if !self.primary.binding.sync(wanted) {
            return;
        }
        self.primary.remove_source();
        self.primary.clear_offers();
        if let Some(device) = self.primary.device.take() {
            device.destroy();
        }
        if let (Some((_, manager)), Some(seat), Some(id)) = (
            &self.primary.manager,
            &self.seat,
            self.primary.binding.current,
        ) {
            self.primary.device = Some(manager.get_device(seat, qh, id));
            if std::env::var_os("PTYRA_TRACE_CLIPBOARD").as_deref()
                == Some(std::ffi::OsStr::new("1"))
            {
                eprintln!("Ptyra primary: device bound");
            }
        }
    }
}
impl Dispatch<device::ZwpPrimarySelectionDeviceV1, DeviceId> for Runtime {
    fn event(
        state: &mut Self,
        device: &device::ZwpPrimarySelectionDeviceV1,
        event: device::Event,
        id: &DeviceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let current = state.primary.device.as_ref() == Some(device)
            && state.primary.binding.current == Some(*id);
        if !current {
            if let device::Event::DataOffer { offer: id } = event {
                id.destroy();
            }
            return;
        }
        match event {
            device::Event::DataOffer { offer: id } => {
                if let Err(offer) = state.primary.offers.created(id) {
                    // Capacity/identity failure must not leave older text readable.
                    state.primary.clear_external_offers();
                    offer.destroy();
                }
            }
            device::Event::Selection { id } => {
                let (changed, retired) = state.primary.offers.select(id.as_ref());
                for offer in retired {
                    offer.destroy();
                }
                if changed
                    && std::env::var_os("PTYRA_TRACE_CLIPBOARD").as_deref()
                        == Some(std::ffi::OsStr::new("1"))
                {
                    let message = match state.primary.offers.selected() {
                        Some((_, offer)) if offer.preferred_text_mime().is_some() => {
                            "text offer selected"
                        }
                        Some(_) => "non-text offer selected",
                        None => "offer cleared",
                    };
                    eprintln!("Ptyra primary: {message}");
                }
            }
            _ => {}
        }
    }
    wayland_client::event_created_child!(Runtime,device::ZwpPrimarySelectionDeviceV1,[0 => (offer::ZwpPrimarySelectionOfferV1,())]);
}
impl Dispatch<source::ZwpPrimarySelectionSourceV1, crate::TabId> for Runtime {
    fn event(
        state: &mut Self,
        proxy: &source::ZwpPrimarySelectionSourceV1,
        event: source::Event,
        _: &crate::TabId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state
            .primary
            .source
            .as_ref()
            .is_none_or(|source| &source.proxy != proxy)
        {
            return;
        }
        match event {
            source::Event::Cancelled => {
                state.primary.remove_source();
                eprintln!("Ptyra primary: cancelled");
            }
            source::Event::Send { fd, mime_type } => {
                if !matches!(
                    mime_type.as_str(),
                    "text/plain;charset=utf-8" | "text/plain"
                ) {
                    return;
                }
                let file = std::fs::File::from(fd);
                if let Err(error) = super::clipboard_send::nonblocking(&file) {
                    eprintln!("Ptyra primary: {error}");
                    return;
                }
                let source = state.primary.source.as_ref().unwrap();
                if let Err(error) = state.primary.sends.start(source.id, &source.text, file) {
                    eprintln!("Ptyra primary: {error}");
                }
            }
            _ => {}
        }
    }
}
delegate_noop!(Runtime: ignore manager::ZwpPrimarySelectionDeviceManagerV1);
impl Dispatch<offer::ZwpPrimarySelectionOfferV1, ()> for Runtime {
    fn event(
        state: &mut Self,
        proxy: &offer::ZwpPrimarySelectionOfferV1,
        event: offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let offer::Event::Offer { mime_type } = event
            && state.primary.offers.advertise(proxy, &mime_type).is_err()
        {
            eprintln!("Ptyra primary: invalid offer");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, WindowId, clipboard::InputSerialEvent, input::FocusTarget};
    #[test]
    fn primary_missing_device_and_serials_are_independent() {
        let mut primary = Primary::new(1024);
        assert_eq!(
            primary.start_receive(),
            Err("primary selection unavailable")
        );
        assert!(primary.receive_fd().is_none());
        let mut clipboard = Binding::default();
        let seat = SeatId(1);
        let target = FocusTarget {
            window: WindowId(1),
            tab: TabId(2),
        };
        clipboard.sync(Some((3, seat)));
        primary.binding.sync(Some((4, seat)));
        for binding in [&mut clipboard, &mut primary.binding] {
            assert!(binding.record(seat, target, 42, InputSerialEvent::PointerButtonPress));
        }
        assert!(
            crate::clipboard::plan_data_source(&mut primary.binding.serials, seat, target)
                .is_some()
        );
        assert!(
            crate::clipboard::plan_data_source(&mut primary.binding.serials, seat, target)
                .is_none()
        );
        assert!(crate::clipboard::plan_data_source(&mut clipboard.serials, seat, target).is_some());
        primary.remove_manager();
        assert!(primary.binding.current.is_none());
        assert!(clipboard.current.is_some());
    }
}
