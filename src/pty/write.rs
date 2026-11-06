use crate::{
    TabId,
    error::{AppError, Component},
};
use std::{collections::VecDeque, io};

/// Regular-input byte limit per session, plus one separately bounded owned paste.
pub const MAX_PTY_PENDING_BYTES: usize = 64 * 1024;
/// Maximum submitted bytes per flush; a short write ends the call immediately.
pub const MAX_PTY_WRITE_BYTES: usize = 16 * 1024;
/// Maximum syscall attempts, including interrupted attempts, per flush.
pub const MAX_PTY_WRITE_ATTEMPTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyEnqueueResult {
    pub tab: TabId,
    pub pending: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyWriteStatus {
    Empty,
    Written(usize),
    /// Wait for writable readiness rather than busy-polling.
    WouldBlock,
    /// Retry budget exhausted; reschedule later without spinning.
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyWriteResult {
    pub tab: TabId,
    pub status: PtyWriteStatus,
    pub pending: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyWriteFailure {
    QueueFull,
    Closed,
    Io(AppError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyWriteError {
    pub tab: TabId,
    pub pending: usize,
    pub reason: PtyWriteFailure,
}

/// Contains sensitive input bytes; deliberately has no Debug implementation.
#[derive(Default)]
pub(super) struct WriteQueue {
    bytes: VecDeque<u8>,
    paste: Option<PasteSegment>,
}

struct PasteSegment {
    bytes: Vec<u8>,
    sent: usize,
    before: usize,
    ready: bool,
    bracketed: bool,
    finishing: bool,
}

impl WriteQueue {
    pub(super) fn reserve_paste(&mut self, tab: TabId, open: bool) -> Result<(), PtyWriteError> {
        if !open || self.paste.is_some() {
            return Err(PtyWriteError {
                tab,
                pending: self.pending(),
                reason: if open {
                    PtyWriteFailure::QueueFull
                } else {
                    PtyWriteFailure::Closed
                },
            });
        }
        self.paste = Some(PasteSegment {
            bytes: Vec::new(),
            sent: 0,
            before: self.bytes.len(),
            ready: false,
            bracketed: false,
            finishing: false,
        });
        Ok(())
    }
    pub(super) fn cancel_waiting_paste(&mut self) {
        if self.paste.as_ref().is_some_and(|paste| !paste.ready) {
            self.paste = None;
        }
    }
    /// Cancel undelivered payload, retaining only required framing on this live PTY.
    pub(super) fn cancel_paste(&mut self, open: bool) {
        let Some(paste) = &mut self.paste else {
            return;
        };
        if !open
            || !paste.ready
            || paste.sent == 0 && !paste.finishing
            || !paste.bracketed && !paste.finishing
        {
            self.paste = None;
        } else if !paste.finishing {
            let end = paste.bytes.len() - 6;
            let mut framing = Vec::with_capacity(12);
            if paste.sent < 6 {
                framing.extend_from_slice(&paste.bytes[paste.sent..6]);
            }
            framing.extend_from_slice(&paste.bytes[paste.sent.max(end)..]);
            paste.bytes = framing;
            paste.sent = 0;
            paste.finishing = true;
        }
    }
    pub(super) fn wants_write(&self) -> bool {
        self.paste.as_ref().map_or(!self.bytes.is_empty(), |paste| {
            paste.before > 0 || (paste.ready && paste.sent < paste.bytes.len())
        })
    }

    /// Move one bounded paste into the FIFO without copying or permitting key interleaving.
    pub(super) fn enqueue_paste(
        &mut self,
        tab: TabId,
        open: bool,
        bytes: &mut Vec<u8>,
        bracketed: bool,
    ) -> Result<PtyEnqueueResult, PtyWriteError> {
        let reason = if bracketed
            && (bytes.len() < 12
                || !bytes.starts_with(b"\x1b[200~")
                || !bytes.ends_with(b"\x1b[201~"))
        {
            Some(PtyWriteFailure::Io(AppError::new(
                Component::Pty,
                io::ErrorKind::InvalidData,
            )))
        } else if !open {
            Some(PtyWriteFailure::Closed)
        } else if self.paste.as_ref().is_some_and(|paste| paste.ready)
            || bytes.len() > crate::clipboard::read::MAX_PASTE_BYTES + 12
        {
            Some(PtyWriteFailure::QueueFull)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(PtyWriteError {
                tab,
                pending: self.pending(),
                reason,
            });
        }
        if bytes.is_empty() {
            self.cancel_waiting_paste();
        } else if let Some(paste) = &mut self.paste {
            paste.bytes = std::mem::take(bytes);
            paste.ready = true;
            paste.bracketed = bracketed;
        } else {
            self.paste = Some(PasteSegment {
                bytes: std::mem::take(bytes),
                sent: 0,
                before: self.bytes.len(),
                ready: true,
                bracketed,
                finishing: false,
            });
        }
        Ok(PtyEnqueueResult {
            tab,
            pending: self.pending(),
        })
    }
    pub(super) fn has_paste(&self) -> bool {
        self.paste.is_some()
    }
    pub(super) fn pending(&self) -> usize {
        self.bytes.len()
            + self
                .paste
                .as_ref()
                .map_or(0, |paste| paste.bytes.len() - paste.sent)
    }

    pub(super) fn enqueue(
        &mut self,
        tab: TabId,
        open: bool,
        bytes: &[u8],
    ) -> Result<PtyEnqueueResult, PtyWriteError> {
        let pending = self.pending();
        let error = |reason| PtyWriteError {
            tab,
            pending,
            reason,
        };
        if !open {
            return Err(error(PtyWriteFailure::Closed));
        }
        if bytes.len() > MAX_PTY_PENDING_BYTES - self.bytes.len() {
            return Err(error(PtyWriteFailure::QueueFull));
        }
        // Reserve before copying: failure rejects the entire enqueue and leaves
        // all previously accepted bytes intact. No silent prefix acceptance.
        self.bytes.try_reserve_exact(bytes.len()).map_err(|_| {
            error(PtyWriteFailure::Io(AppError::new(
                Component::Pty,
                io::ErrorKind::OutOfMemory,
            )))
        })?;
        self.bytes.extend(bytes);
        Ok(PtyEnqueueResult {
            tab,
            pending: self.pending(),
        })
    }

    pub(super) fn flush<W: io::Write>(
        &mut self,
        tab: TabId,
        writer: Option<&mut W>,
    ) -> Result<PtyWriteResult, PtyWriteError> {
        let pending = self.pending();
        let result = |status, pending| {
            Ok(PtyWriteResult {
                tab,
                status,
                pending,
            })
        };
        let error = |reason| PtyWriteError {
            tab,
            pending,
            reason,
        };
        let io_error = |e| error(PtyWriteFailure::Io(AppError::from_io(Component::Pty, e)));
        let Some(writer) = writer else {
            return Err(error(PtyWriteFailure::Closed));
        };
        if pending == 0 {
            return result(PtyWriteStatus::Empty, 0);
        }
        if !self.wants_write() {
            return result(PtyWriteStatus::WouldBlock, pending);
        }
        let pasting = self.paste.as_ref().is_some_and(|paste| paste.before == 0);
        let front = if pasting {
            let paste = self.paste.as_ref().unwrap();
            &paste.bytes[paste.sent..]
        } else {
            let (first, second) = self.bytes.as_slices();
            let front = if first.is_empty() { second } else { first };
            let limit = self
                .paste
                .as_ref()
                .map_or(front.len(), |paste| paste.before.min(front.len()));
            &front[..limit]
        };
        let chunk = &front[..front.len().min(MAX_PTY_WRITE_BYTES)];
        for _ in 0..MAX_PTY_WRITE_ATTEMPTS {
            match writer.write(chunk) {
                Ok(0) => return Err(io_error(io::ErrorKind::WriteZero.into())),
                Ok(n) if n <= chunk.len() => {
                    if pasting {
                        let paste = self.paste.as_mut().unwrap();
                        paste.sent += n;
                        if paste.sent == paste.bytes.len() {
                            self.paste = None;
                        }
                    } else {
                        drop(self.bytes.drain(..n));
                        if let Some(paste) = &mut self.paste {
                            paste.before -= n;
                        }
                    }
                    return result(PtyWriteStatus::Written(n), self.pending());
                }
                Ok(_) => return Err(io_error(io::ErrorKind::InvalidData.into())),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return result(PtyWriteStatus::WouldBlock, pending);
                }
                Err(e) => return Err(io_error(e)),
            }
        }
        result(PtyWriteStatus::Interrupted, pending)
    }
}

#[cfg(test)]
mod tests;
