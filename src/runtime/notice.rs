//! Fixed recoverable outcomes only. No caller-supplied diagnostic or payload is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    StreamStopped,
    PtyStopped,
    Empty,
    NonText,
    TooLarge,
    InvalidText,
    OwnerChanged,
    TransferFailed,
    Unavailable,
    NoSelection,
    Selecting,
    SelectionSaved,
    SelectionSavedCustom,
    Copied,
    NoSerial,
    TargetChanged,
    Busy,
    ReplyRejected,
    InputRejected,
    MouseRejected,
    FocusRejected,
    HyperlinkFailed,
}
impl Notice {
    pub fn selection_saved(bindings: &crate::config::bindings::Bindings) -> Self {
        use crate::config::bindings::{BindingAction, Bindings};
        if bindings.get(BindingAction::Copy) == Bindings::default().get(BindingAction::Copy) {
            Self::SelectionSaved
        } else {
            Self::SelectionSavedCustom
        }
    }
    pub fn from_error(error: &str) -> Self {
        match error {
            "clipboard has no offer" | "primary has no offer" => Self::Empty,
            "clipboard has no text offer" | "primary has no text offer" => Self::NonText,
            "selection exceeds clipboard limit"
            | "clipboard receive exceeds limit"
            | "paste payload exceeds limit"
            | "clipboard transfer exceeds limit"
            | "write exceeds clipboard limit" => Self::TooLarge,
            "clipboard receive is not UTF-8" => Self::InvalidText,
            "clipboard receive cancelled" => Self::OwnerChanged,
            "clipboard unavailable"
            | "primary unavailable"
            | "primary selection unavailable"
            | "selection unavailable" => Self::Unavailable,
            "no valid selection" => Self::NoSelection,
            "no valid input serial" => Self::NoSerial,
            "paste target changed" | "paste target unavailable" | "no keyboard focus" => {
                Self::TargetChanged
            }
            "paste already in progress" | "too many clipboard transfers" => Self::Busy,
            _ => Self::TransferFailed,
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Self::StreamStopped => {
                "Session stopped: invalid or oversized output. Close this Tab or open a new one."
            }
            Self::PtyStopped => {
                "Session stopped: PTY I/O failed. Close this Tab or open a new one."
            }
            Self::Empty => "Clipboard is empty.",
            Self::NonText => "Clipboard has no text.",
            Self::TooLarge => "Clipboard exceeds limit.",
            Self::InvalidText => "Clipboard is not UTF-8.",
            Self::OwnerChanged => "Clipboard owner changed.",
            Self::TransferFailed => "Clipboard transfer failed.",
            Self::Unavailable => "Selection unavailable.",
            Self::NoSelection => "Nothing selected.",
            Self::Selecting => "Selecting. View paused; output continues.",
            Self::SelectionSaved => "Selection saved. Ctrl+Shift+C to copy.",
            Self::SelectionSavedCustom => "Selection saved. Use your Copy shortcut.",
            Self::Copied => "Copied.",
            Self::NoSerial => "Copy needs an input event.",
            Self::TargetChanged => "Paste target changed.",
            Self::Busy => "Clipboard transfer busy.",
            Self::HyperlinkFailed => "Link could not be opened: unavailable or busy.",
            Self::FocusRejected => "Focus report rejected: input unavailable.",
            Self::MouseRejected => "Mouse report rejected: input unavailable.",
            Self::InputRejected => "Input rejected: terminal busy or unavailable. Try again.",
            Self::ReplyRejected => "Terminal reply rejected: queue limit.",
        }
    }
}

fn geometry(size: (u32, u32), cell: (u32, u32), notice: Notice) -> Option<(u16, u16, u32)> {
    let columns = (size.0 / cell.0.max(1)).min(1024);
    if columns < 2 {
        return None;
    }
    let rows = (notice.message().len() as u32).div_ceil(columns);
    let height = rows.checked_mul(cell.1.max(1))?;
    (height <= size.1).then_some((rows as u16, columns as u16, height))
}

pub(super) fn height(size: (u32, u32), cell: (u32, u32), notice: Notice) -> u32 {
    geometry(size, cell, notice).map_or(size.1.min(cell.1), |(_, _, height)| height)
}

pub(super) fn draw(
    raster: &mut super::raster::Raster,
    size: (u32, u32),
    pixels: &mut [u8],
    notice: Notice,
) -> super::Result<()> {
    let Some((rows, columns, height)) =
        geometry(size, (raster.cell_width, raster.cell_height), notice)
    else {
        // When text cannot fit, keep a visible indicator; resizing reveals the metadata.
        let height = size.1.min(raster.cell_height);
        let offset = (size.1 - height) as usize * size.0 as usize * 4;
        for pixel in pixels
            .get_mut(offset..)
            .ok_or("invalid notice buffer")?
            .chunks_exact_mut(4)
        {
            pixel.copy_from_slice(&(0xff000000 | raster.palette.ansi[3]).to_ne_bytes());
        }
        return Ok(());
    };
    let mut terminal =
        crate::terminal::Terminal::with_scrollback(crate::TabId(0), rows, columns, 0)
            .map_err(|_| "notice allocation failed")?;
    let text = format!("\x1b[30;43m\x1b[2J\x1b[H\x1b[?25l{}", notice.message());
    terminal
        .feed(crate::TabId(0), text.as_bytes())
        .map_err(|_| "notice text rejected")?;
    let offset = (size.1 - height) as usize * size.0 as usize * 4;
    raster.draw(
        &terminal,
        (size.0, height),
        pixels.get_mut(offset..).ok_or("invalid notice buffer")?,
        false,
        true,
    )
}

impl super::Runtime {
    pub(super) fn show_clipboard_notice(&mut self, notice: Notice) {
        if self.clipboard_notice != Some(notice) {
            self.clipboard_notice = Some(notice);
            self.paint.dirty = true;
        }
        eprintln!("Ptyra clipboard notice: {}", notice.message());
    }
    pub(super) fn clipboard_error(&mut self, error: &str) {
        self.show_clipboard_notice(Notice::from_error(error));
    }
    pub(super) fn clear_clipboard_notice(&mut self) {
        self.paint.dirty |= self.clipboard_notice.take().is_some();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_clipboard_errors_do_not_leak_or_exit() {
        use crate::config::{Config, bindings::Bindings};
        assert_eq!(
            Notice::selection_saved(&Bindings::default()).message(),
            "Selection saved. Ctrl+Shift+C to copy."
        );
        let custom = Config::from_toml(Some("[bindings]\ncopy='Ctrl+Shift+a'")).unwrap();
        assert_eq!(
            Notice::selection_saved(&custom.bindings),
            Notice::SelectionSavedCustom
        );
        assert_eq!(Notice::Copied.message(), "Copied.");
        assert_eq!(Notice::from_error("clipboard has no offer"), Notice::Empty);
        assert_eq!(
            Notice::from_error("primary has no text offer"),
            Notice::NonText
        );
        assert_eq!(
            Notice::from_error("clipboard receive exceeds limit"),
            Notice::TooLarge
        );
        assert_eq!(
            Notice::from_error("clipboard receive is not UTF-8"),
            Notice::InvalidText
        );
        assert_eq!(
            Notice::from_error("clipboard receive cancelled"),
            Notice::OwnerChanged
        );
        assert_eq!(
            Notice::from_error("clipboard receive failed"),
            Notice::TransferFailed
        );
        let secret = "clipboard=私人內容; key=secret; pty=password";
        let notice = Notice::from_error(secret);
        assert_eq!(notice, Notice::TransferFailed);
        assert!(!format!("{notice:?} {}", notice.message()).contains(secret));
        for notice in [
            Notice::StreamStopped,
            Notice::PtyStopped,
            Notice::Empty,
            Notice::NonText,
            Notice::TooLarge,
            Notice::InvalidText,
            Notice::OwnerChanged,
            Notice::TransferFailed,
            Notice::Unavailable,
            Notice::NoSelection,
            Notice::NoSerial,
            Notice::TargetChanged,
            Notice::Busy,
            Notice::InputRejected,
            Notice::Selecting,
            Notice::SelectionSaved,
            Notice::SelectionSavedCustom,
            Notice::Copied,
        ] {
            assert!(notice.message().is_ascii());
            let (rows, columns, height) = geometry((160, 80), (8, 16), notice).unwrap();
            assert!(usize::from(rows) * usize::from(columns) >= notice.message().len());
            assert!(height <= 80);
            assert!(geometry((1, 1), (256, 512), notice).is_none());
        }
        use super::super::{
            clipboard_offers::Offers, clipboard_receive::Receive, clipboard_send::SendQueue,
        };
        use std::io::{self, Read, Write};
        enum Reader {
            Good(io::Cursor<Vec<u8>>),
            Bad,
        }
        impl Read for Reader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                match self {
                    Self::Good(reader) => reader.read(buffer),
                    Self::Bad => Err(io::Error::new(io::ErrorKind::BrokenPipe, "secret payload")),
                }
            }
        }
        let mut offers = Offers::default();
        offers.created(1).unwrap();
        offers.advertise(&1, "text/plain;charset=utf-8").unwrap();
        offers.select(Some(&1));
        let offer = offers.selected().unwrap().1.id();
        let mut receive = Receive::default();
        let id = receive.start(offer, Reader::Bad, 8).unwrap();
        assert_eq!(
            Notice::from_error(receive.pump(Some(id), &offers).unwrap_err()),
            Notice::TransferFailed
        );
        assert!(receive.fd().is_none());
        let id = receive
            .start(offer, Reader::Good(io::Cursor::new(Vec::new())), 8)
            .unwrap();
        assert_eq!(
            receive.pump(Some(id), &offers).unwrap(),
            Some(String::new())
        );
        let id = receive
            .start(offer, Reader::Good(io::Cursor::new(b"ok".to_vec())), 8)
            .unwrap();
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert_eq!(receive.pump(Some(id), &offers).unwrap(), Some("ok".into()));
        receive.start(offer, Reader::Bad, 8).unwrap();
        offers.select(None);
        assert_eq!(
            Notice::from_error(receive.pump(None, &offers).unwrap_err()),
            Notice::OwnerChanged
        );
        // Explicit cancellation is silent and releases its owned endpoint.
        receive.cancel();
        assert!(receive.pump(None, &offers).unwrap().is_none());
        struct Writer(bool, Vec<u8>);
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0 {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                self.1.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut sends = SendQueue::default();
        let bad = sends.start(1, "secret", Writer(true, vec![])).unwrap();
        let good = sends.start(2, "other", Writer(false, vec![])).unwrap();
        assert_eq!(sends.pump(&[bad]), 1);
        assert_eq!(sends.fds().map(|(id, _)| id).collect::<Vec<_>>(), [good]);
        assert_eq!(sends.pump(&[good]), 0);
        let retry = sends.start(1, "recovered", Writer(false, vec![])).unwrap();
        assert_eq!(sends.pump(&[retry]), 0);
        assert_eq!(sends.fds().count(), 0);
    }
}
