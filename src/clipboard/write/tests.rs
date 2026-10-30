use super::{ClipboardSend, SendProgress};
use crate::error::{AppError, Component};
use std::{
    collections::VecDeque,
    io::{self, Write},
};

enum Step {
    Accept(usize),
    Error(io::ErrorKind),
}

struct FakeWriter {
    steps: VecDeque<Step>,
    output: Vec<u8>,
}

impl Write for FakeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.steps.pop_front().expect("unexpected write") {
            Step::Accept(limit) => {
                let count = limit.min(bytes.len());
                self.output.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            Step::Error(kind) => Err(kind.into()),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("FD writer must not call flush")
    }
}

#[test]
fn clipboard_write_handles_short_write() {
    let text = "繁體\n tail";
    let mut send = ClipboardSend::new(text.to_owned()).unwrap();
    let mut writer = FakeWriter {
        steps: [
            Step::Accept(2),
            Step::Error(io::ErrorKind::Interrupted),
            Step::Accept(3),
            Step::Error(io::ErrorKind::WouldBlock),
        ]
        .into_iter()
        .chain((0..text.len()).map(|_| Step::Accept(1)))
        .collect(),
        output: Vec::new(),
    };
    assert_eq!(
        send.write_ready(&mut writer).unwrap(),
        SendProgress::Written(2)
    );
    assert_eq!(
        send.write_ready(&mut writer).unwrap(),
        SendProgress::Written(3)
    );
    assert_eq!(
        send.write_ready(&mut writer).unwrap(),
        SendProgress::WouldBlock
    );
    for _ in 0..text.len() + 1 {
        if send.write_ready(&mut writer).unwrap() == SendProgress::Done {
            break;
        }
    }
    assert_eq!(writer.output, text.as_bytes());
    assert_eq!(send.pending(), 0);
    assert_eq!(send.write_ready(&mut writer).unwrap(), SendProgress::Done);

    let mut closed = ClipboardSend::new("secret".into()).unwrap();
    let mut broken = FakeWriter {
        steps: [Step::Error(io::ErrorKind::BrokenPipe)].into(),
        output: Vec::new(),
    };
    assert_eq!(
        closed.write_ready(&mut broken),
        Err(AppError::new(
            Component::Clipboard,
            io::ErrorKind::BrokenPipe
        ))
    );
    assert_eq!(closed.pending(), 0);
    assert_eq!(closed.write_ready(&mut broken).unwrap(), SendProgress::Done);
    assert!(format!("{:?}", SendProgress::Done).is_ascii());
}
