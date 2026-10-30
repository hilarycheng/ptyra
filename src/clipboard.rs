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

/// Return the exact advertised MIME string required by wl_data_offer.receive.
/// Prefer explicit UTF-8, then legacy UTF8_STRING, then plain text whose
/// charset will be validated by the later reader.
pub fn select_text_mime<'a>(offered: &'a [&'a str]) -> Option<&'a str> {
    if offered.len() > 64 {
        return None;
    }
    let mut best = None;
    let mut rank = 3;
    for mime in offered {
        if mime.len() > 256 {
            continue;
        }
        let Some(candidate) = text_mime_rank(mime) else {
            continue;
        };
        if candidate < rank {
            best = Some(*mime);
            rank = candidate;
            if rank == 0 {
                break;
            }
        }
    }
    best
}

fn text_mime_rank(mime: &str) -> Option<u8> {
    if mime.eq_ignore_ascii_case("UTF8_STRING") {
        return Some(1);
    }
    let mut parts = mime.split(';');
    if !parts.next()?.trim().eq_ignore_ascii_case("text/plain") {
        return None;
    }
    let Some(parameter) = parts.next() else {
        return Some(2);
    };
    if parts.next().is_some() {
        return None;
    }
    let (name, value) = parameter.split_once('=')?;
    (name.trim().eq_ignore_ascii_case("charset")
        && value.trim().trim_matches('"').eq_ignore_ascii_case("utf-8"))
    .then_some(0)
}

#[cfg(test)]
mod tests;
