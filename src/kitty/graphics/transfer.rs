use super::{GraphicsCommand, Header, MAX_CHUNK_BYTES, invalid};
use crate::error::{AppError, Component};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::ErrorKind;

pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_CHUNKS: usize = 8192;

pub struct Transmission {
    pub header: Header,
    pub bytes: Vec<u8>,
    chunks: usize,
}
#[derive(Default)]
pub struct Assembler {
    pending: Option<Transmission>,
}
impl Assembler {
    pub fn push(&mut self, command: GraphicsCommand) -> Result<Option<Transmission>, AppError> {
        let result = self.append(command);
        if result.is_err() {
            self.pending = None;
        }
        result
    }
    fn append(&mut self, command: GraphicsCommand) -> Result<Option<Transmission>, AppError> {
        let GraphicsCommand { header, payload } = command;
        if header.value(b't', i64::from(b'd')) != i64::from(b'd')
            || !(*b"tTq")
                .map(i64::from)
                .contains(&header.value(b'a', i64::from(b't')))
        {
            return Err(AppError::new(Component::Terminal, ErrorKind::Unsupported));
        }
        let more = header.value(b'm', 0) == 1;
        if payload.len() > MAX_CHUNK_BYTES
            || more && (payload.len() % 4 != 0 || payload.contains(&b'='))
        {
            return Err(invalid());
        }
        if self.pending.is_some()
            && header.fields.iter().enumerate().any(|(i, value)| {
                value.is_some()
                    && ![super::index(b'm').unwrap(), super::index(b'q').unwrap()].contains(&i)
            })
        {
            return Err(invalid());
        }
        let decoded = STANDARD.decode(payload).map_err(|_| invalid())?;
        let pending = self.pending.get_or_insert_with(|| Transmission {
            header,
            bytes: Vec::new(),
            chunks: 0,
        });
        if pending.chunks == MAX_CHUNKS || decoded.len() > MAX_IMAGE_BYTES - pending.bytes.len() {
            return Err(AppError::new(Component::Terminal, ErrorKind::FileTooLarge));
        }
        pending
            .bytes
            .try_reserve(decoded.len())
            .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
        pending.bytes.extend_from_slice(&decoded);
        pending.chunks += 1;
        if more {
            Ok(None)
        } else {
            Ok(self.pending.take())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitty::graphics::{GraphicsCommand, Header};
    fn chunk(header: &[u8], bytes: &[u8]) -> GraphicsCommand {
        GraphicsCommand {
            header: Header::parse(header).unwrap(),
            payload: bytes.to_vec(),
        }
    }
    #[test]
    fn kitty_chunks_reassemble_in_order() {
        let mut assembler = Assembler::default();
        assert!(
            assembler
                .push(chunk(b"f=32,s=1,v=1,i=7,m=1", b"AQID"))
                .unwrap()
                .is_none()
        );
        let complete = assembler.push(chunk(b"m=0", b"BA==")).unwrap().unwrap();
        assert_eq!(complete.bytes, [1, 2, 3, 4]);
        assert_eq!(complete.header.get(b'i'), Some(7));
        assert!(assembler.push(chunk(b"m=1", b"AAAA")).unwrap().is_none());
        assert!(assembler.push(chunk(b"i=9,m=0", b"AAAA")).is_err());
        assert!(assembler.push(chunk(b"m=1", b"AA==")).is_err());
        assert!(assembler.push(chunk(b"", b"%%%!")).is_err());
        assert!(assembler.push(chunk(b"", b"AQID")).unwrap().is_some());
        assert!(assembler.push(chunk(b"m=1", b"AAAA")).unwrap().is_none());
        assert!(assembler.push(chunk(b"m=0", b"")).unwrap().is_some());
    }
}
