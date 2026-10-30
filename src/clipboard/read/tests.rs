use super::{ClipboardReceive, ClipboardTransfer, MAX_PASTE_BYTES, ReadProgress};
use crate::clipboard::{ClipboardState, DataOffer, PastePayload};
use crate::error::{AppError, Component};
use std::{
    cell::Cell,
    collections::VecDeque,
    io::{self, Read},
    rc::Rc,
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

struct TrackedReader {
    inner: FakeReader,
    calls: Rc<Cell<usize>>,
    dropped: Rc<Cell<bool>>,
}
impl Read for TrackedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.calls.set(self.calls.get() + 1);
        self.inner.read(buf)
    }
}
impl Drop for TrackedReader {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

#[test]
fn clipboard_cancel_drops_pending_transfer() {
    let mut state = ClipboardState::new();
    let id = state.offer_created().unwrap();
    assert!(state.select_offer(id));
    let mut offer = DataOffer::new(id);
    let calls = Rc::new(Cell::new(0));
    let dropped = Rc::new(Cell::new(false));
    let reader = TrackedReader {
        inner: FakeReader {
            steps: [Step::Data(b"part".to_vec()), Step::Eof].into(),
        },
        calls: calls.clone(),
        dropped: dropped.clone(),
    };
    let mut transfer = ClipboardTransfer::new(id, reader, 16).unwrap();
    assert!(matches!(
        transfer.read_ready(&offer, &state).unwrap(),
        ReadProgress::Read(4)
    ));
    offer.cancel();
    state.cancel_offer(id);
    assert!(matches!(
        transfer.read_ready(&offer, &state).unwrap(),
        ReadProgress::Closed
    ));
    assert!(dropped.get());
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        transfer.read_ready(&offer, &state).unwrap(),
        ReadProgress::Closed
    ));

    let old = state.offer_created().unwrap();
    assert!(state.select_offer(old));
    let old_offer = DataOffer::new(old);
    let reader = TrackedReader {
        inner: FakeReader {
            steps: [Step::Data(b"stale".to_vec())].into(),
        },
        calls: calls.clone(),
        dropped: Rc::new(Cell::new(false)),
    };
    let mut stale = ClipboardTransfer::new(old, reader, 16).unwrap();
    let newer = state.offer_created().unwrap();
    assert!(state.select_offer(newer));
    assert!(matches!(
        stale.read_ready(&old_offer, &state).unwrap(),
        ReadProgress::Closed
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn paste_preserves_utf8_and_newlines() {
    let original = "繁體\t  \n第二行 \r\n";
    let mut steps: VecDeque<_> = original
        .as_bytes()
        .chunks(2)
        .map(|part| Step::Data(part.to_vec()))
        .collect();
    steps.push_back(Step::Eof);
    let mut reader = FakeReader { steps };
    let mut receive = ClipboardReceive::new();
    let text = loop {
        match receive.read_ready(&mut reader).unwrap() {
            ReadProgress::Read(_) => continue,
            ReadProgress::Complete(text) => break text,
            _ => panic!("unexpected fixture state"),
        }
    };
    let payload = PastePayload::new(text).unwrap();
    assert_eq!(payload.bytes(), original.as_bytes());
}
