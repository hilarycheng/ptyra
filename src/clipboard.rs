//! Pure clipboard owner and offer identity state. No Wayland or FD I/O.

use crate::TabId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfferId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardOwner {
    Local(TabId),
    External(OfferId),
}

/// One instance per Wayland clipboard data device.
#[derive(Default)]
pub struct ClipboardState {
    next_epoch: u64,
    current_epoch: u64,
    latest_offer: Option<OfferId>,
    owner: Option<ClipboardOwner>,
}

impl ClipboardState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn owner(&self) -> Option<ClipboardOwner> {
        self.owner
    }

    /// Create an app-owned identity for an arriving wl_data_offer object.
    pub fn offer_created(&mut self) -> Option<OfferId> {
        self.next_epoch = self.next_epoch.checked_add(1)?;
        let id = OfferId(self.next_epoch);
        self.latest_offer = Some(id);
        Some(id)
    }

    /// Accept only the newest live offer, never one older than current ownership.
    pub fn select_offer(&mut self, id: OfferId) -> bool {
        if self.latest_offer != Some(id) || id.0 <= self.current_epoch {
            return false;
        }
        self.current_epoch = id.0;
        self.owner = Some(ClipboardOwner::External(id));
        true
    }

    pub fn set_local_owner(&mut self, tab: TabId) -> bool {
        let Some(epoch) = self.next_epoch.checked_add(1) else {
            return false;
        };
        self.next_epoch = epoch;
        self.current_epoch = epoch;
        self.latest_offer = None;
        self.owner = Some(ClipboardOwner::Local(tab));
        true
    }

    pub fn cancel_offer(&mut self, id: OfferId) {
        if self.latest_offer == Some(id) {
            self.latest_offer = None;
        }
        if self.owner == Some(ClipboardOwner::External(id)) {
            self.owner = None;
        }
    }
}

#[cfg(test)]
mod tests;
