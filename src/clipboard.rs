//! Pure clipboard owner and offer identity state. No Wayland or FD I/O.

use crate::{
    TabId,
    error::{AppError, Component},
    input::{FocusTarget, KeyRoute, KeyboardFocus},
    keymap::{KeyState, Modifiers},
    selection::copy_text,
};
use alacritty_terminal::{grid::Grid, selection::SelectionRange, term::cell::Cell};
use std::io::ErrorKind;

pub mod read;
pub mod write;

/// Owned Paste bytes; no normalization, trimming, or payload Debug/logging.
pub struct PastePayload {
    bytes: Vec<u8>,
}

impl PastePayload {
    pub fn new(text: String) -> Result<Self, AppError> {
        if text.len() > read::MAX_PASTE_BYTES {
            return Err(AppError::new(Component::Clipboard, ErrorKind::FileTooLarge));
        }
        Ok(Self {
            bytes: text.into_bytes(),
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Sensitive Copy text deliberately has no Debug implementation.
pub struct CopyIntent {
    target: FocusTarget,
    text: String,
}

impl CopyIntent {
    pub fn target(&self) -> FocusTarget {
        self.target
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn into_text(self) -> String {
        self.text
    }
}

/// Only a pressed Ctrl+Shift+C with a valid nonempty selection creates Copy.
pub fn copy_shortcut(
    target: FocusTarget,
    modifiers: Modifiers,
    keysym: u32,
    state: KeyState,
    selection: Option<SelectionRange>,
    grid: &Grid<Cell>,
) -> Option<CopyIntent> {
    if state != KeyState::Pressed
        || !modifiers.ctrl()
        || !modifiers.shift()
        || modifiers.alt()
        || !matches!(keysym, 0x63 | 0x43)
    {
        return None;
    }
    let text = copy_text(grid, selection?)?;
    (!text.is_empty()).then_some(CopyIntent { target, text })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PasteIntent {
    target: FocusTarget,
}

impl PasteIntent {
    pub fn target(self) -> FocusTarget {
        self.target
    }

    /// Bind completed Clipboard data to the Tab that initiated Paste.
    pub fn with_payload(self, payload: PastePayload) -> PendingPaste {
        PendingPaste {
            target: self.target,
            payload,
            sent: 0,
        }
    }
}

/// A target-identified enqueue endpoint; implementations may use a fake queue
/// or a real Tab-owned PTY write queue.
pub trait PasteSink {
    type Error;

    fn tab(&self) -> TabId;
    fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Owns Paste bytes while chunks are queued. Sensitive bytes have no Debug.
pub struct PendingPaste {
    target: FocusTarget,
    payload: PastePayload,
    sent: usize,
}

impl PendingPaste {
    pub fn target(&self) -> FocusTarget {
        self.target
    }

    pub fn is_complete(&self) -> bool {
        self.sent == self.payload.bytes.len()
    }

    /// A different Tab never receives a chunk, even after a focus switch.
    /// Errors leave this chunk pending for a later retry.
    pub fn enqueue_next<S: PasteSink>(&mut self, sink: &mut S) -> Result<bool, S::Error> {
        if sink.tab() != self.target.tab || self.is_complete() {
            return Ok(false);
        }
        let end = (self.sent + 16 * 1024).min(self.payload.bytes.len());
        sink.enqueue(&self.payload.bytes[self.sent..end])?;
        self.sent = end;
        Ok(true)
    }
}

/// `Consumed` keeps a matched shortcut out of ordinary Shell text even when
/// its focus epoch is stale or it is a release/repeat event.
pub enum PasteKeyAction {
    Shell,
    Consumed,
    Paste(PasteIntent),
}

pub fn paste_shortcut(
    focus: &KeyboardFocus,
    route: &KeyRoute,
    modifiers: Modifiers,
    keysym: u32,
    state: KeyState,
) -> PasteKeyAction {
    if !modifiers.ctrl() || !modifiers.shift() || modifiers.alt() || !matches!(keysym, 0x76 | 0x56)
    {
        return PasteKeyAction::Shell;
    }
    if state != KeyState::Pressed
        || !route.pressed
        || !focus.matches(route.target, &route.focus_epoch)
    {
        return PasteKeyAction::Consumed;
    }
    PasteKeyAction::Paste(PasteIntent {
        target: route.target,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSerial(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSerialEvent {
    KeyPress,
    PointerButtonPress,
    Other,
}

/// A serial is usable once only for the focused seat, window and Tab.
#[derive(Default)]
pub struct SeatSerialState {
    focused: Option<(SeatId, FocusTarget)>,
    pending: Option<SelectionSerial>,
}

impl SeatSerialState {
    pub fn focus(&mut self, seat: SeatId, target: FocusTarget) {
        self.focused = Some((seat, target));
        self.pending = None;
    }

    pub fn clear(&mut self) {
        self.focused = None;
        self.pending = None;
    }

    /// Called by the seat adapter only for a real input event's serial.
    pub fn record_event(
        &mut self,
        seat: SeatId,
        target: FocusTarget,
        serial: u32,
        event: InputSerialEvent,
    ) -> bool {
        if self.focused != Some((seat, target)) || event == InputSerialEvent::Other {
            return false;
        }
        self.pending = Some(SelectionSerial(serial));
        true
    }

    /// Returns no request serial without a fresh matching input event.
    pub fn take_for_selection(
        &mut self,
        seat: SeatId,
        target: FocusTarget,
    ) -> Option<SelectionSerial> {
        if self.focused != Some((seat, target)) {
            return None;
        }
        self.pending.take()
    }
}

/// Pure request plan for one wl_data_source and its wl_data_device selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSourceRequest {
    Create,
    Offer(&'static str),
    SetSelection {
        seat: SeatId,
        serial: SelectionSerial,
    },
}

pub fn plan_data_source(
    serials: &mut SeatSerialState,
    seat: SeatId,
    target: FocusTarget,
) -> Option<[DataSourceRequest; 4]> {
    let serial = serials.take_for_selection(seat, target)?;
    Some([
        DataSourceRequest::Create,
        DataSourceRequest::Offer("text/plain;charset=utf-8"),
        DataSourceRequest::Offer("text/plain"),
        DataSourceRequest::SetSelection { seat, serial },
    ])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfferId(u64);

/// One received wl_data_offer object's advertised formats and lifetime.
pub struct DataOffer {
    id: OfferId,
    formats: Vec<String>,
    cancelled: bool,
}

impl DataOffer {
    pub fn new(id: OfferId) -> Self {
        Self {
            id,
            formats: Vec::new(),
            cancelled: false,
        }
    }

    pub fn id(&self) -> OfferId {
        self.id
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn formats(&self) -> &[String] {
        &self.formats
    }

    pub fn advertise(&mut self, mime: &str) -> Result<bool, AppError> {
        if self.cancelled
            || mime.is_empty()
            || mime.len() > 256
            || mime.contains('\0')
            || self.formats.len() >= 64
        {
            return Err(AppError::new(Component::Clipboard, ErrorKind::InvalidInput));
        }
        if self
            .formats
            .iter()
            .any(|format| format.eq_ignore_ascii_case(mime))
        {
            return Ok(false);
        }
        self.formats
            .try_reserve(1)
            .map_err(|_| AppError::new(Component::Clipboard, ErrorKind::OutOfMemory))?;
        let mut format = String::new();
        format
            .try_reserve_exact(mime.len())
            .map_err(|_| AppError::new(Component::Clipboard, ErrorKind::OutOfMemory))?;
        format.push_str(mime);
        self.formats.push(format);
        Ok(true)
    }

    pub fn preferred_text_mime(&self) -> Option<&str> {
        if self.cancelled {
            return None;
        }
        let formats: Vec<_> = self.formats.iter().map(String::as_str).collect();
        select_text_mime(&formats)
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.formats = Vec::new();
    }
}

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
pub fn select_text_mime<'a>(offered: &[&'a str]) -> Option<&'a str> {
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
