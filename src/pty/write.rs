use crate::{
    TabId,
    error::{AppError, Component},
};
use std::{collections::VecDeque, io};

/// Logical byte limit per session, independent of other sessions.
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
}

impl WriteQueue {
    pub(super) fn pending(&self) -> usize {
        self.bytes.len()
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
        if bytes.len() > MAX_PTY_PENDING_BYTES - pending {
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
        let (first, second) = self.bytes.as_slices();
        let front = if first.is_empty() { second } else { first };
        let chunk = &front[..front.len().min(MAX_PTY_WRITE_BYTES)];
        for _ in 0..MAX_PTY_WRITE_ATTEMPTS {
            match writer.write(chunk) {
                Ok(0) => return Err(io_error(io::ErrorKind::WriteZero.into())),
                Ok(n) if n <= chunk.len() => {
                    drop(self.bytes.drain(..n));
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
