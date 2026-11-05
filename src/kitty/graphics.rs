//! Bounded, inert Kitty APC parsing. No filesystem, process or renderer access.
use crate::error::{AppError, Component};
use std::io::ErrorKind;

pub const MAX_HEADER_BYTES: usize = 512;
pub const MAX_CHUNK_BYTES: usize = 4096;

fn invalid() -> AppError {
    AppError::new(Component::Terminal, ErrorKind::InvalidData)
}
fn index(key: u8) -> Option<usize> {
    match key {
        b'a'..=b'z' => Some((key - b'a') as usize),
        b'A'..=b'Z' => Some((key - b'A') as usize + 26),
        _ => None,
    }
}

#[derive(Clone)]
pub struct Header {
    fields: [Option<i64>; 52],
}
impl Default for Header {
    fn default() -> Self {
        Self { fields: [None; 52] }
    }
}
impl Header {
    pub fn get(&self, key: u8) -> Option<i64> {
        self.fields.get(index(key)?).copied().flatten()
    }
    pub fn value(&self, key: u8, default: i64) -> i64 {
        self.get(key).unwrap_or(default)
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, AppError> {
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(invalid());
        }
        let mut header = Self::default();
        if bytes.is_empty() {
            return Ok(header);
        }
        for pair in bytes.split(|b| *b == b',') {
            if pair.len() < 3 || pair[1] != b'=' {
                return Err(invalid());
            }
            let key = pair[0];
            let slot = index(key).ok_or_else(invalid)?;
            if header.fields[slot].is_some() {
                return Err(invalid());
            }
            let raw = &pair[2..];
            let value = match key {
                b'a' | b't' | b'd' => {
                    if raw.len() != 1 {
                        return Err(invalid());
                    }
                    let allowed: &[u8] = match key {
                        b'a' => b"tTpqd",
                        b't' => b"dfts",
                        _ => b"aAiI",
                    };
                    if !allowed.contains(&raw[0]) {
                        return Err(invalid());
                    }
                    i64::from(raw[0])
                }
                b'f' | b's' | b'v' | b'i' | b'p' | b'm' | b'q' | b'c' | b'r' | b'x' | b'y'
                | b'z' => {
                    let digits = if key == b'z' {
                        raw.strip_prefix(b"-").unwrap_or(raw)
                    } else {
                        raw
                    };
                    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                        return Err(invalid());
                    }
                    let value: i64 = std::str::from_utf8(raw)
                        .map_err(|_| invalid())?
                        .parse()
                        .map_err(|_| invalid())?;
                    if (key == b'z' && i32::try_from(value).is_err())
                        || (key != b'z' && u32::try_from(value).is_err())
                    {
                        return Err(invalid());
                    }
                    if key == b'm' && value > 1
                        || key == b'q' && value > 2
                        || key == b'i' && value == 0
                    {
                        return Err(invalid());
                    }
                    value
                }
                _ => return Err(AppError::new(Component::Terminal, ErrorKind::Unsupported)),
            };
            header.fields[slot] = Some(value);
        }
        Ok(header)
    }
}

pub struct GraphicsCommand {
    pub header: Header,
    pub payload: Vec<u8>,
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    Start,
    Body,
    End,
    Ignore(bool),
    IgnoreEscape(bool),
}

#[derive(Default)]
pub struct GraphicsParser {
    state: State,
    body: Vec<u8>,
    separator: Option<usize>,
    failed: bool,
}
impl GraphicsParser {
    pub fn push(&mut self, byte: u8) -> Result<Option<GraphicsCommand>, AppError> {
        if self.failed {
            return Err(invalid());
        }
        let result = self.step(byte);
        if result.is_err() {
            self.failed = true;
            self.body = Vec::new();
        }
        result
    }
    fn step(&mut self, byte: u8) -> Result<Option<GraphicsCommand>, AppError> {
        if matches!(byte, 0x18 | 0x1a) {
            self.state = State::Ground;
            self.body.clear();
            return Ok(None);
        }
        self.state = match self.state {
            State::Ground if byte == 0x1b => State::Escape,
            State::Ground => State::Ground,
            State::Escape => match byte {
                b'_' => State::Start,
                b']' => State::Ignore(true),
                b'P' | b'^' | b'X' => State::Ignore(false),
                0x1b => State::Escape,
                _ => State::Ground,
            },
            State::Start if byte == b'G' => {
                self.body.clear();
                self.separator = None;
                State::Body
            }
            State::Start => State::Ignore(false),
            State::Body if byte == 0x1b => State::End,
            State::Body => {
                if !(0x20..=0x7e).contains(&byte) {
                    return Err(invalid());
                }
                if byte == b';' && self.separator.is_none() {
                    self.separator = Some(self.body.len());
                }
                let max = self
                    .separator
                    .map_or(MAX_HEADER_BYTES, |offset| offset + 1 + MAX_CHUNK_BYTES);
                if self.body.len() >= max {
                    return Err(AppError::new(Component::Terminal, ErrorKind::FileTooLarge));
                }
                self.body
                    .try_reserve(1)
                    .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
                self.body.push(byte);
                State::Body
            }
            State::End => {
                if byte != b'\\' {
                    return Err(invalid());
                }
                let end = self.separator.unwrap_or(self.body.len());
                let header = Header::parse(&self.body[..end])?;
                let payload = self
                    .separator
                    .map_or(&[][..], |offset| &self.body[offset + 1..])
                    .to_vec();
                self.body.clear();
                self.state = State::Ground;
                return Ok(Some(GraphicsCommand { header, payload }));
            }
            State::Ignore(osc) => {
                if byte == 0x1b {
                    State::IgnoreEscape(osc)
                } else if osc && byte == 7 {
                    State::Ground
                } else {
                    State::Ignore(osc)
                }
            }
            State::IgnoreEscape(osc) => {
                if byte == b'\\' || osc && byte == 7 {
                    State::Ground
                } else if byte == 0x1b {
                    State::IgnoreEscape(osc)
                } else {
                    State::Ignore(osc)
                }
            }
        };
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kitty_apc_parser_rejects_invalid_header() {
        for header in [
            "a=bad",
            "i=-1",
            "i=4294967296",
            "m=2",
            "i=1,i=2",
            ",s=1",
            "s=1,",
            "s=",
            "unknown=1",
        ] {
            let mut parser = GraphicsParser::default();
            assert!(
                format!("\x1b_G{header};AAAA\x1b\\")
                    .bytes()
                    .try_for_each(|b| parser.push(b).map(|_| ()))
                    .is_err()
            );
        }
        let mut parser = GraphicsParser::default();
        let mut commands = Vec::new();
        for b in b"text\x1b]2;_Gi=9\x07\x1b_Gf=32,s=1,v=1,i=7;AAAAAA==\x1b\\" {
            if let Some(command) = parser.push(*b).unwrap() {
                commands.push(command);
            }
        }
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].header.get(b'i'), Some(7));
        assert_eq!(commands[0].payload, b"AAAAAA==");
        let mut parser = GraphicsParser::default();
        let huge = format!("\x1b_Gs={}", "1".repeat(513));
        assert!(
            huge.bytes()
                .try_for_each(|b| parser.push(b).map(|_| ()))
                .is_err()
        );
        let mut parser = GraphicsParser::default();
        let huge = format!("\x1b_G;{}\x1b\\", "A".repeat(4097));
        assert!(
            huge.bytes()
                .try_for_each(|b| parser.push(b).map(|_| ()))
                .is_err()
        );
    }
}
