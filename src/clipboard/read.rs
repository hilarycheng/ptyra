//! Clipboard FD reader. Payload state deliberately has no Debug.

use crate::error::{AppError, Component};
use std::io::{self, Read};

const READ_CHUNK_BYTES: usize = 16 * 1024;
const MAX_READ_ATTEMPTS: usize = 4;
const MAX_RECEIVE_BYTES: usize = 16 * 1024 * 1024;

/// Complete carries sensitive text and deliberately has no Debug.
pub enum ReadProgress {
    Read(usize),
    WouldBlock,
    Interrupted,
    Complete(String),
    Closed,
}

#[derive(Default)]
pub struct ClipboardReceive {
    bytes: Vec<u8>,
    closed: bool,
}

impl ClipboardReceive {
    pub fn new() -> Self {
        Self::default()
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
                        .is_none_or(|size| size > MAX_RECEIVE_BYTES)
                    {
                        self.close();
                        return Err(AppError::new(
                            Component::Clipboard,
                            io::ErrorKind::InvalidData,
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

#[cfg(test)]
mod tests;
