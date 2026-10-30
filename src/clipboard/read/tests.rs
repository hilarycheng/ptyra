use super::{ClipboardReceive, MAX_PASTE_BYTES, ReadProgress};
use crate::error::{AppError, Component};
use std::{
    collections::VecDeque,
    io::{self, Read},
};

enum Step {
    Data(Vec<u8>),
    Error(io::ErrorKind),
    Eof,
}
struct FakeReader {
    steps: VecDeque<Step>,
}
impl Read for FakeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.steps.pop_front().expect("unexpected read") {
            Step::Data(bytes) => {
                assert!(bytes.len() <= buf.len());
                buf[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
            Step::Error(kind) => Err(kind.into()),
            Step::Eof => Ok(0),
        }
    }
}

#[test]
fn clipboard_read_handles_partial_utf8() {
    let mut reader = FakeReader {
        steps: [
            Step::Data(vec![0xe7]),
            Step::Error(io::ErrorKind::Interrupted),
            Step::Data(vec![0xb9]),
            Step::Error(io::ErrorKind::WouldBlock),
            Step::Data(vec![0x81, b'\n', b' ']),
            Step::Eof,
        ]
        .into(),
    };
    let mut receive = ClipboardReceive::new();
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::Read(1)
    ));
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::Read(1)
    ));
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::WouldBlock
    ));
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::Read(3)
    ));
    match receive.read_ready(&mut reader).unwrap() {
        ReadProgress::Complete(text) => assert_eq!(text, "繁\n "),
        _ => panic!("expected UTF-8 completion"),
    }
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::Closed
    ));

    let mut invalid = ClipboardReceive::new();
    let mut reader = FakeReader {
        steps: [Step::Data(vec![0xe7]), Step::Eof].into(),
    };
    assert!(matches!(
        invalid.read_ready(&mut reader).unwrap(),
        ReadProgress::Read(1)
    ));
    assert!(
        matches!(invalid.read_ready(&mut reader), Err(error) if error == AppError::new(Component::Clipboard, io::ErrorKind::InvalidData))
    );
    assert!(matches!(
        invalid.read_ready(&mut reader).unwrap(),
        ReadProgress::Closed
    ));
}

#[test]
fn clipboard_read_limit_is_explicit() {
    let mut receive = ClipboardReceive::with_limit(4).unwrap();
    let mut reader = FakeReader {
        steps: [
            Step::Data("繁".as_bytes().to_vec()),
            Step::Data("體".as_bytes().to_vec()),
        ]
        .into(),
    };
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::Read(3)
    ));
    let error = receive.read_ready(&mut reader).err().unwrap();
    assert_eq!(
        error,
        AppError::new(Component::Clipboard, io::ErrorKind::FileTooLarge)
    );
    assert!(!format!("{error:?}").contains("繁"));
    assert!(matches!(
        receive.read_ready(&mut reader).unwrap(),
        ReadProgress::Closed
    ));

    let mut exact = ClipboardReceive::with_limit(3).unwrap();
    let mut reader = FakeReader {
        steps: [Step::Data("繁".as_bytes().to_vec()), Step::Eof].into(),
    };
    assert!(matches!(
        exact.read_ready(&mut reader).unwrap(),
        ReadProgress::Read(3)
    ));
    assert!(
        matches!(exact.read_ready(&mut reader).unwrap(), ReadProgress::Complete(text) if text == "繁")
    );
    for invalid in [0, MAX_PASTE_BYTES + 1] {
        assert!(
            matches!(ClipboardReceive::with_limit(invalid), Err(error) if error == AppError::new(Component::Clipboard, io::ErrorKind::InvalidInput))
        );
    }
}
