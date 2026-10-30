//! Bounded Clipboard FD writer. Payload state deliberately has no Debug.

use crate::error::{AppError, Component};
use std::io::{self, Write};

pub const MAX_CLIPBOARD_BYTES: usize = 16 * 1024 * 1024;
const WRITE_CHUNK_BYTES: usize = 16 * 1024;
const MAX_WRITE_ATTEMPTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendProgress {
    Written(usize),
    WouldBlock,
    Interrupted,
    Done,
}

/// Owns one transfer; pass a nonblocking FD writer when used by an event loop.
pub struct ClipboardSend {
    bytes: Vec<u8>,
    offset: usize,
    done: bool,
}

impl ClipboardSend {
    pub fn new(text: String) -> Result<Self, AppError> {
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err(AppError::new(
                Component::Clipboard,
                io::ErrorKind::InvalidInput,
            ));
        }
        let bytes = text.into_bytes();
        let done = bytes.is_empty();
        Ok(Self {
            bytes,
            offset: 0,
            done,
        })
    }

    pub fn pending(&self) -> usize {
        self.bytes.len() - self.offset
    }

    pub fn write_ready<W: Write>(&mut self, writer: &mut W) -> Result<SendProgress, AppError> {
        if self.done {
            return Ok(SendProgress::Done);
        }
        let end = (self.offset + WRITE_CHUNK_BYTES).min(self.bytes.len());
        let chunk = &self.bytes[self.offset..end];
        for _ in 0..MAX_WRITE_ATTEMPTS {
            match writer.write(chunk) {
                Ok(0) => {
                    self.close();
                    return Err(AppError::new(
                        Component::Clipboard,
                        io::ErrorKind::WriteZero,
                    ));
                }
                Ok(n) if n <= chunk.len() => {
                    self.offset += n;
                    if self.offset == self.bytes.len() {
                        self.close();
                    }
                    return Ok(SendProgress::Written(n));
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
                    return Ok(SendProgress::WouldBlock);
                }
                Err(error) => {
                    self.close();
                    return Err(AppError::from_io(Component::Clipboard, error));
                }
            }
        }
        Ok(SendProgress::Interrupted)
    }

    fn close(&mut self) {
        self.bytes = Vec::new();
        self.offset = 0;
        self.done = true;
    }
}

#[cfg(test)]
mod tests;
