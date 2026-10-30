//! Clipboard FD reader. Payload state deliberately has no Debug.

use crate::clipboard::{ClipboardOwner, ClipboardState, DataOffer, OfferId};
use crate::error::{AppError, Component};
use std::io::{self, Read};

const READ_CHUNK_BYTES: usize = 16 * 1024;
const MAX_READ_ATTEMPTS: usize = 4;
pub const MAX_PASTE_BYTES: usize = 16 * 1024 * 1024;

/// Complete carries sensitive text and deliberately has no Debug.
pub enum ReadProgress {
    Read(usize),
    WouldBlock,
    Interrupted,
    Complete(String),
    Closed,
}

pub struct ClipboardReceive {
    bytes: Vec<u8>,
    closed: bool,
    limit: usize,
}

impl Default for ClipboardReceive {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardReceive {
    pub fn new() -> Self {
        Self {
            bytes: Vec::new(),
            closed: false,
            limit: MAX_PASTE_BYTES,
        }
    }

    pub fn with_limit(limit: usize) -> Result<Self, AppError> {
        if limit == 0 || limit > MAX_PASTE_BYTES {
            return Err(AppError::new(
                Component::Clipboard,
                io::ErrorKind::InvalidInput,
            ));
        }
        Ok(Self {
            bytes: Vec::new(),
            closed: false,
            limit,
        })
    }

    pub fn cancel(&mut self) {
        self.close();
    }

    pub fn read_ready<R: Read>(&mut self, reader: &mut R) -> Result<ReadProgress, AppError> {
        if self.closed {
            return Ok(ReadProgress::Closed);
        }
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        for _ in 0..MAX_READ_ATTEMPTS {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    self.closed = true;
                    return String::from_utf8(std::mem::take(&mut self.bytes))
                        .map(ReadProgress::Complete)
                        .map_err(|_| {
                            AppError::new(Component::Clipboard, io::ErrorKind::InvalidData)
                        });
                }
                Ok(n) if n <= chunk.len() => {
                    if self
                        .bytes
                        .len()
                        .checked_add(n)
                        .is_none_or(|size| size > self.limit)
                    {
                        self.close();
                        return Err(AppError::new(
                            Component::Clipboard,
                            io::ErrorKind::FileTooLarge,
                        ));
                    }
                    if self.bytes.try_reserve(n).is_err() {
                        self.close();
                        return Err(AppError::new(
                            Component::Clipboard,
                            io::ErrorKind::OutOfMemory,
                        ));
                    }
                    self.bytes.extend_from_slice(&chunk[..n]);
                    return Ok(ReadProgress::Read(n));
                }
                Ok(_) => {
                    self.close();
                    return Err(AppError::new(
                        Component::Clipboard,
                        io::ErrorKind::InvalidData,
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(ReadProgress::WouldBlock);
                }
                Err(error) => {
                    self.close();
                    return Err(AppError::from_io(Component::Clipboard, error));
                }
            }
        }
        Ok(ReadProgress::Interrupted)
    }

    fn close(&mut self) {
        self.closed = true;
        self.bytes = Vec::new();
    }
}

/// Owns the receive FD and bytes for exactly one offer identity.
pub struct ClipboardTransfer<R: Read> {
    offer: OfferId,
    reader: Option<R>,
    receive: ClipboardReceive,
}

impl<R: Read> ClipboardTransfer<R> {
    pub fn new(offer: OfferId, reader: R, limit: usize) -> Result<Self, AppError> {
        Ok(Self {
            offer,
            reader: Some(reader),
            receive: ClipboardReceive::with_limit(limit)?,
        })
    }

    pub fn cancel(&mut self) {
        self.reader = None;
        self.receive.cancel();
    }

    pub fn read_ready(
        &mut self,
        offer: &DataOffer,
        clipboard: &ClipboardState,
    ) -> Result<ReadProgress, AppError> {
        if offer.id() != self.offer
            || offer.is_cancelled()
            || clipboard.owner() != Some(ClipboardOwner::External(self.offer))
        {
            self.cancel();
            return Ok(ReadProgress::Closed);
        }
        let Some(reader) = self.reader.as_mut() else {
            return Ok(ReadProgress::Closed);
        };
        let result = self.receive.read_ready(reader);
        if matches!(
            &result,
            Ok(ReadProgress::Complete(_) | ReadProgress::Closed) | Err(_)
        ) {
            self.reader = None;
        }
        result
    }
}

#[cfg(test)]
mod tests;
