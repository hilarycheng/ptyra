use super::*;

enum Step {
    Accept(usize),
    Error(io::Error),
}
struct FakeWriter {
    steps: VecDeque<Step>,
    output: Vec<u8>,
    calls: usize,
    largest_write: usize,
}
impl FakeWriter {
    fn new(steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            output: Vec::new(),
            calls: 0,
            largest_write: 0,
        }
    }
}
impl io::Write for FakeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        self.largest_write = self.largest_write.max(bytes.len());
        match self.steps.pop_front().expect("unexpected write") {
            Step::Accept(max) => {
                let n = max.min(bytes.len());
                self.output.extend_from_slice(&bytes[..n]);
                Ok(n)
            }
            Step::Error(error) => Err(error),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("queue must not use Write::flush/write_all")
    }
}
fn interrupted() -> Step {
    Step::Error(io::Error::from(io::ErrorKind::Interrupted))
}
fn blocked() -> Step {
    Step::Error(io::Error::from(io::ErrorKind::WouldBlock))
}

#[test]
fn ime_commit_is_written_once() {
    use crate::{
        WindowId,
        clipboard::{PasteSink, SeatId},
        ime::{ImeEvent, ImeState},
        input::KeyboardFocus,
        wayland::{Capabilities, Global},
    };

    struct Sink {
        tab: TabId,
        open: bool,
        queue: WriteQueue,
    }
    impl PasteSink for Sink {
        type Error = PtyWriteError;

        fn tab(&self) -> TabId {
            self.tab
        }

        fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.queue.enqueue(self.tab, self.open, bytes).map(|_| ())
        }
    }

    let seat = SeatId(1);
    let window = WindowId(1);
    let tab = TabId(7);
    let capabilities = Capabilities::from_globals(&[Global {
        name: 3,
        interface: "zwp_text_input_manager_v3",
        version: 1,
    }]);
    let mut keyboard = KeyboardFocus::default();
    keyboard.enter(window, tab);
    let mut ime = ImeState::new(seat, &capabilities);
    ime.enter(seat, window);
    ime.sync_enabled(seat, &keyboard, true);
    let serial = ime.commit_serial();
    let text = " 繁體中文 e\u{301}🙂\t\n ";
    let mut sink = Sink {
        tab,
        open: true,
        queue: WriteQueue::default(),
    };
    for value in ["replaced", text] {
        ime.stage(
            &ime.capture_focus(seat, window, &keyboard),
            &keyboard,
            ImeEvent::Commit { text: Some(value) },
        )
        .unwrap();
    }
    ime.stage(
        &ime.capture_focus(seat, window, &keyboard),
        &keyboard,
        ImeEvent::Preedit {
            text: Some("候選"),
            cursor_begin: 0,
            cursor_end: 6,
        },
    )
    .unwrap();
    assert_eq!(sink.queue.pending(), 0, "nothing is queued before done");
    let mut update = ime
        .done(
            &ime.capture_focus(seat, window, &keyboard),
            &keyboard,
            serial,
        )
        .unwrap();

    sink.tab = TabId(8);
    assert!(!update.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
    assert_eq!(sink.queue.pending(), 0);
    assert_eq!(update.commit, text);
    sink.tab = tab;
    sink.open = false;
    assert_eq!(
        update
            .enqueue_commit(&ime, &keyboard, &mut sink)
            .unwrap_err()
            .reason,
        PtyWriteFailure::Closed
    );
    assert_eq!(update.commit, text);
    sink.open = true;
    sink.enqueue(&vec![b'x'; MAX_PTY_PENDING_BYTES]).unwrap();
    assert_eq!(
        update
            .enqueue_commit(&ime, &keyboard, &mut sink)
            .unwrap_err()
            .reason,
        PtyWriteFailure::QueueFull
    );
    assert_eq!(update.commit, text);
    let mut discard = io::sink();
    for _ in 0..8 {
        sink.queue.flush(tab, Some(&mut discard)).unwrap();
    }
    assert_eq!(sink.queue.pending(), 0);
    assert!(update.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
    assert!(update.commit.is_empty());
    assert_eq!(update.preedit.text, "候選");
    assert!(!update.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
    assert_eq!(sink.queue.pending(), text.len());

    let mut writer = FakeWriter::new(
        [Step::Accept(2), interrupted(), blocked()]
            .into_iter()
            .chain((0..text.len()).map(|_| Step::Accept(1))),
    );
    for _ in 0..text.len() + 3 {
        sink.queue.flush(tab, Some(&mut writer)).unwrap();
        assert!(!update.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
    }
    assert_eq!(sink.queue.pending(), 0);
    assert_eq!(writer.output, text.as_bytes());

    // The serial identifies client state, not unique Commit events.
    let mut output = Vec::new();
    for _ in 0..2 {
        ime.stage(
            &ime.capture_focus(seat, window, &keyboard),
            &keyboard,
            ImeEvent::Commit { text: Some(text) },
        )
        .unwrap();
        let mut next = ime
            .done(
                &ime.capture_focus(seat, window, &keyboard),
                &keyboard,
                serial,
            )
            .unwrap();
        assert!(next.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
        assert!(!next.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
        sink.queue.flush(tab, Some(&mut output)).unwrap();
    }
    assert_eq!(output, text.repeat(2).as_bytes());
    for text in [Some(""), None] {
        ime.stage(
            &ime.capture_focus(seat, window, &keyboard),
            &keyboard,
            ImeEvent::Commit { text },
        )
        .unwrap();
        let mut empty = ime
            .done(
                &ime.capture_focus(seat, window, &keyboard),
                &keyboard,
                serial,
            )
            .unwrap();
        assert!(!empty.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
    }
    let mut empty = ime
        .done(
            &ime.capture_focus(seat, window, &keyboard),
            &keyboard,
            serial,
        )
        .unwrap();
    assert!(!empty.enqueue_commit(&ime, &keyboard, &mut sink).unwrap());
    assert_eq!(sink.queue.pending(), 0);
}

#[test]
fn pty_write_handles_partial_writes() {
    let tab = TabId(9);
    let payload = b"\xe7\xb9\x81\x1b[31m\0\xff\n  tail";
    let mut queue = WriteQueue::default();
    assert_eq!(
        queue.enqueue(tab, true, &payload[..3]).unwrap(),
        PtyEnqueueResult { tab, pending: 3 }
    );
    queue.enqueue(tab, true, &payload[3..]).unwrap();
    let mut writer = FakeWriter::new(
        [Step::Accept(2), interrupted(), blocked()]
            .into_iter()
            .chain((0..payload.len()).map(|_| Step::Accept(1))),
    );
    let first = queue.flush(tab, Some(&mut writer)).unwrap();
    assert_eq!(
        first,
        PtyWriteResult {
            tab,
            status: PtyWriteStatus::Written(2),
            pending: payload.len() - 2
        }
    );
    let pending = queue.pending();
    assert_eq!(
        queue.flush(tab, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::WouldBlock
    );
    assert_eq!(queue.pending(), pending);
    queue.enqueue(tab, true, b"extra").unwrap();
    writer.steps.extend((0..5).map(|_| Step::Accept(1)));
    for _ in 0..100 {
        let result = queue.flush(tab, Some(&mut writer)).unwrap();
        assert_eq!(result.tab, tab);
        if result.status == PtyWriteStatus::Empty {
            break;
        }
    }
    assert_eq!(queue.pending(), 0);
    assert_eq!(writer.output, [payload.as_slice(), b"extra"].concat());
    let calls = writer.calls;
    assert_eq!(
        queue.flush(tab, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::Empty
    );
    assert_eq!(writer.calls, calls, "empty queue performs no I/O");
}

#[test]
fn pty_write_queue_is_bounded_and_atomic() {
    let tab = TabId(1);
    let mut queue = WriteQueue::default();
    assert_eq!(
        queue
            .enqueue(tab, true, &vec![0; MAX_PTY_PENDING_BYTES + 1])
            .unwrap_err()
            .reason,
        PtyWriteFailure::QueueFull
    );
    assert_eq!(queue.pending(), 0);
    let original: Vec<_> = (0..MAX_PTY_PENDING_BYTES)
        .map(|i| (i % 251) as u8)
        .collect();
    queue.enqueue(tab, true, &original).unwrap();
    let error = queue.enqueue(tab, true, b"rejected").unwrap_err();
    assert_eq!(
        error,
        PtyWriteError {
            tab,
            pending: MAX_PTY_PENDING_BYTES,
            reason: PtyWriteFailure::QueueFull
        }
    );
    assert_eq!(
        queue.enqueue(tab, true, b"").unwrap().pending,
        MAX_PTY_PENDING_BYTES
    );
    let mut writer = FakeWriter::new((0..10).map(|_| Step::Accept(usize::MAX)));
    for _ in 0..2 {
        queue.flush(tab, Some(&mut writer)).unwrap();
    }
    let tail = vec![0xff; 2 * MAX_PTY_WRITE_BYTES];
    queue.enqueue(tab, true, &tail).unwrap(); // Reuses drained space, potentially wrapping storage.
    assert_eq!(queue.pending(), MAX_PTY_PENDING_BYTES);
    for _ in 0..8 {
        if queue.flush(tab, Some(&mut writer)).unwrap().status == PtyWriteStatus::Empty {
            break;
        }
    }
    assert_eq!(queue.pending(), 0);
    assert_eq!(writer.output, [original, tail].concat());
    assert!(writer.largest_write <= MAX_PTY_WRITE_BYTES);
}

#[test]
fn pty_write_retries_and_flush_size_are_bounded() {
    let mut queue = WriteQueue::default();
    let tab = TabId(1);
    let data = vec![0x23; MAX_PTY_WRITE_BYTES + 7];
    queue.enqueue(tab, true, &data).unwrap();
    let mut writer = FakeWriter::new(
        (0..MAX_PTY_WRITE_ATTEMPTS + 1)
            .map(|_| interrupted())
            .chain([Step::Accept(usize::MAX), Step::Accept(usize::MAX)]),
    );
    assert_eq!(
        queue.flush(tab, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::Interrupted
    );
    assert_eq!(writer.calls, MAX_PTY_WRITE_ATTEMPTS);
    assert_eq!(queue.pending(), data.len());
    assert_eq!(
        queue.flush(tab, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::Written(MAX_PTY_WRITE_BYTES)
    );
    assert_eq!(writer.calls, MAX_PTY_WRITE_ATTEMPTS + 2);
    assert_eq!(queue.pending(), 7);
    assert_eq!(
        queue.flush(tab, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::Written(7)
    );
    assert_eq!(writer.output, data);
    assert_eq!(writer.largest_write, MAX_PTY_WRITE_BYTES);
}

#[test]
fn pty_write_errors_preserve_pending_and_isolate_sessions() {
    let mut first = WriteQueue::default();
    let mut second = WriteQueue::default();
    first.enqueue(TabId(1), true, b"first").unwrap();
    second.enqueue(TabId(2), true, b"second").unwrap();
    let mut writer = FakeWriter::new([
        Step::Accept(0),
        Step::Error(io::Error::new(io::ErrorKind::BrokenPipe, "secret input")),
    ]);
    for kind in [io::ErrorKind::WriteZero, io::ErrorKind::BrokenPipe] {
        let error = first.flush(TabId(1), Some(&mut writer)).unwrap_err();
        assert_eq!(
            error,
            PtyWriteError {
                tab: TabId(1),
                pending: 5,
                reason: PtyWriteFailure::Io(AppError::new(Component::Pty, kind))
            }
        );
        assert!(!format!("{error:?}").contains("secret"));
        assert_eq!(first.pending(), 5);
        assert_eq!(second.pending(), 6);
    }
    assert_eq!(writer.calls, 2, "zero/error writes do not retry");
    assert!(writer.output.is_empty());
    assert_eq!(
        first.enqueue(TabId(1), false, b"x").unwrap_err().reason,
        PtyWriteFailure::Closed
    );
    assert_eq!(
        first
            .flush::<FakeWriter>(TabId(1), None)
            .unwrap_err()
            .reason,
        PtyWriteFailure::Closed
    );
    assert_eq!(
        first.pending(),
        5,
        "closed-session queue remains owned until dropped"
    );
    let mut second_writer = FakeWriter::new([Step::Accept(usize::MAX)]);
    assert_eq!(
        second.flush(TabId(2), Some(&mut second_writer)).unwrap(),
        PtyWriteResult {
            tab: TabId(2),
            status: PtyWriteStatus::Written(6),
            pending: 0
        }
    );
    assert_eq!(second_writer.output, b"second");
    assert_eq!(first.pending(), 5);
}
