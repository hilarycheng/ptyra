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

#[test]
fn runtime_paste_queue_preserves_payload_under_backpressure() {
    let tab = TabId(4);
    let mut queue = WriteQueue::default();
    queue.enqueue(tab, true, b"before").unwrap();
    let expected = [b"\x1b[200~".as_slice(), &vec![b'x'; 100_000], b"\x1b[201~"].concat();
    let mut payload = expected.clone();
    queue.enqueue_paste(tab, true, &mut payload, false).unwrap();
    assert!(payload.is_empty());
    queue.enqueue(tab, true, b"after").unwrap();
    let mut rejected = b"new".to_vec();
    assert_eq!(
        queue
            .enqueue_paste(tab, true, &mut rejected, false)
            .unwrap_err()
            .reason,
        PtyWriteFailure::QueueFull
    );
    assert_eq!(rejected, b"new");
    let mut writer = FakeWriter::new(
        [blocked(), interrupted(), Step::Accept(2)]
            .into_iter()
            .chain((0..100).map(|_| Step::Accept(4095))),
    );
    while queue.pending() > 0 {
        queue.flush(tab, Some(&mut writer)).unwrap();
    }
    assert_eq!(
        writer.output,
        [b"before".as_slice(), &expected, b"after"].concat()
    );
    assert!(writer.largest_write <= MAX_PTY_WRITE_BYTES);
    queue
        .enqueue_paste(tab, true, &mut rejected, false)
        .unwrap();
    assert!(rejected.is_empty());
}

#[test]
fn runtime_paste_queue_preserves_limits_and_closed_payload() {
    let tab = TabId(4);
    let mut queue = WriteQueue::default();
    let mut payload = b"paste".to_vec();
    assert_eq!(
        queue
            .enqueue_paste(tab, false, &mut payload, false)
            .unwrap_err()
            .reason,
        PtyWriteFailure::Closed
    );
    assert_eq!(payload, b"paste");
    let mut too_large = vec![0; crate::clipboard::read::MAX_PASTE_BYTES + 13];
    assert_eq!(
        queue
            .enqueue_paste(tab, true, &mut too_large, false)
            .unwrap_err()
            .reason,
        PtyWriteFailure::QueueFull
    );
    assert_eq!(
        too_large.len(),
        crate::clipboard::read::MAX_PASTE_BYTES + 13
    );
    assert_eq!(queue.pending(), 0);
    queue
        .enqueue(tab, true, &vec![b'a'; MAX_PTY_PENDING_BYTES])
        .unwrap();
    queue.enqueue_paste(tab, true, &mut payload, false).unwrap();
    assert_eq!(queue.pending(), MAX_PTY_PENDING_BYTES + 5);
    assert_eq!(
        queue.enqueue(tab, true, b"x").unwrap_err().reason,
        PtyWriteFailure::QueueFull
    );
    let mut writer = FakeWriter::new((0..10).map(|_| Step::Accept(usize::MAX)));
    while queue.pending() > 0 {
        queue.flush(tab, Some(&mut writer)).unwrap();
    }
    assert_eq!(
        writer.output,
        [vec![b'a'; MAX_PTY_PENDING_BYTES], b"paste".to_vec()].concat()
    );
    queue
        .enqueue_paste(tab, true, &mut Vec::new(), false)
        .unwrap();
    assert!(!queue.has_paste());
}

#[test]
fn runtime_paste_reservation_orders_keys_before_receive_eof() {
    let tab = TabId(4);
    let mut queue = WriteQueue::default();
    queue.enqueue(tab, true, b"before").unwrap();
    queue.reserve_paste(tab, true).unwrap();
    queue.enqueue(tab, true, b"after").unwrap();
    let mut writer = FakeWriter::new([
        Step::Accept(usize::MAX),
        Step::Accept(2),
        blocked(),
        interrupted(),
        Step::Accept(usize::MAX),
        Step::Accept(usize::MAX),
    ]);
    queue.flush(tab, Some(&mut writer)).unwrap();
    assert_eq!(writer.output, b"before");
    assert!(!queue.wants_write());
    assert_eq!(
        queue.flush(tab, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::WouldBlock
    );
    assert_eq!(writer.calls, 1);
    let mut payload = b"paste".to_vec();
    queue.enqueue_paste(tab, true, &mut payload, false).unwrap();
    assert!(queue.wants_write());
    queue.flush(tab, Some(&mut writer)).unwrap();
    assert_eq!(writer.output, b"beforepa");
    assert_eq!(queue.pending(), 8);
    assert_eq!(
        queue.flush(tab, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::WouldBlock
    );
    assert_eq!(queue.pending(), 8);
    while queue.pending() > 0 {
        queue.flush(tab, Some(&mut writer)).unwrap();
    }
    assert_eq!(writer.output, b"beforepasteafter");
    queue.reserve_paste(tab, true).unwrap();
    queue.enqueue(tab, true, b"released").unwrap();
    assert!(!queue.wants_write());
    queue.cancel_waiting_paste();
    assert!(queue.wants_write());
    let mut writer = FakeWriter::new([Step::Accept(usize::MAX)]);
    queue.flush(tab, Some(&mut writer)).unwrap();
    assert_eq!(writer.output, b"released");
    queue.reserve_paste(tab, true).unwrap();
    queue.enqueue(tab, true, b"empty tail").unwrap();
    queue
        .enqueue_paste(tab, true, &mut Vec::new(), false)
        .unwrap();
    assert!(!queue.has_paste());
    assert!(queue.wants_write());
    let mut writer = FakeWriter::new([Step::Accept(usize::MAX)]);
    queue.flush(tab, Some(&mut writer)).unwrap();
    assert_eq!(writer.output, b"empty tail");
}

#[test]
fn runtime_partial_paste_cancel_never_crosses_tabs() {
    let original = TabId(1);
    for bracketed in [false, true] {
        // A literal opening marker is still ordinary payload when framing is disabled.
        let payload = "\x1b[200~繁體e\u{301}\n".as_bytes();
        let framed = if bracketed {
            [b"\x1b[200~".as_slice(), payload, b"\x1b[201~"].concat()
        } else {
            payload.to_vec()
        };
        for sent in 0..=framed.len() {
            let mut queue = WriteQueue::default();
            queue.enqueue(original, true, b"before").unwrap();
            queue.reserve_paste(original, true).unwrap();
            queue.enqueue(original, true, b"after").unwrap();
            queue
                .enqueue_paste(original, true, &mut framed.clone(), bracketed)
                .unwrap();
            let mut writer = FakeWriter::new([Step::Accept(6), Step::Accept(sent)]);
            queue.flush(original, Some(&mut writer)).unwrap();
            if sent > 0 {
                queue.flush(original, Some(&mut writer)).unwrap();
            }
            queue.cancel_paste(true);
            queue.cancel_paste(true); // Repeated focus events cannot discard necessary framing.
            let expected = if bracketed && sent > 0 && sent < framed.len() {
                [
                    b"before".as_slice(),
                    &framed[..sent],
                    &b"\x1b[200~"[sent.min(6)..],
                    &b"\x1b[201~"[sent.saturating_sub(framed.len() - 6)..],
                    b"after",
                ]
                .concat()
            } else {
                [b"before".as_slice(), &framed[..sent], b"after"].concat()
            };
            writer.steps = [blocked(), interrupted()]
                .into_iter()
                .chain((0..32).map(|_| Step::Accept(1)))
                .collect();
            while queue.wants_write() {
                queue.flush(original, Some(&mut writer)).unwrap();
            }
            assert_eq!(writer.output, expected, "bracketed={bracketed} sent={sent}");
            assert!(!queue.has_paste());
            let mut other = WriteQueue::default();
            other.enqueue(TabId(2), true, b"other").unwrap();
            let mut other_writer = FakeWriter::new([Step::Accept(5)]);
            other.flush(TabId(2), Some(&mut other_writer)).unwrap();
            assert_eq!(other_writer.output, b"other");
        }
    }
    // Closing a Tab discards even partially emitted framing. No writer is invoked.
    for sent in 0..18 {
        let mut queue = WriteQueue::default();
        queue
            .enqueue_paste(
                original,
                true,
                &mut b"\x1b[200~secret\x1b[201~".to_vec(),
                true,
            )
            .unwrap();
        let mut writer = FakeWriter::new([Step::Accept(sent)]);
        if sent > 0 {
            queue.flush(original, Some(&mut writer)).unwrap();
        }
        queue.cancel_paste(false);
        assert!(!queue.has_paste());
        assert_eq!(queue.pending(), 0);
        assert_eq!(
            queue
                .flush::<FakeWriter>(original, None)
                .unwrap_err()
                .reason,
            PtyWriteFailure::Closed
        );
    }
    // Receive/confirmation is an unfilled barrier, which cancellation releases.
    let mut queue = WriteQueue::default();
    queue.reserve_paste(original, true).unwrap();
    queue.enqueue(original, true, b"tail").unwrap();
    queue.cancel_paste(true);
    let mut writer = FakeWriter::new([Step::Accept(4)]);
    queue.flush(original, Some(&mut writer)).unwrap();
    assert_eq!(writer.output, b"tail");
}

#[test]
fn runtime_terminal_replies_return_to_source_pty() {
    use crate::tabs::{TabPty, TabRegistry, output::TabPtyWrite};
    struct Session {
        id: TabId,
        queue: WriteQueue,
    }
    impl TabPty for Session {
        fn tab(&self) -> TabId {
            self.id
        }
    }
    impl TabPtyWrite for Session {
        fn enqueue_write(&mut self, bytes: &[u8]) -> Result<(), PtyWriteError> {
            self.queue.enqueue(self.id, true, bytes).map(|_| ())
        }
    }
    let mut tabs = TabRegistry::with_scrollback(0).unwrap();
    for id in [TabId(1), TabId(2)] {
        tabs.insert(id, 4, 16).unwrap();
        tabs.attach_pty(Session {
            id,
            queue: WriteQueue::default(),
        })
        .ok()
        .unwrap();
    }
    let id = TabId(2);
    let source = tabs.get_mut(id).unwrap();
    let queue = &mut source.pty_mut().unwrap().queue;
    queue.enqueue(id, true, b"before").unwrap();
    queue.reserve_paste(id, true).unwrap();
    // Every possible split must preserve DA/DSR/CPR order; no query changes cells.
    let query = b"\x1b[c\x1b[5n\x1b[3;7H\x1b[6n";
    for split in 0..=query.len() {
        let terminal = tabs.get_mut(id).unwrap().terminal_mut();
        terminal.feed(id, &query[..split]).unwrap();
        terminal.feed(id, &query[split..]).unwrap();
        tabs.enqueue_terminal_replies(id).unwrap();
        // A second drain must never duplicate bytes.
        tabs.enqueue_terminal_replies(id).unwrap();
    }
    assert_eq!(tabs.active(), Some(TabId(1)));
    assert_eq!(
        tabs.get(TabId(1)).unwrap().pty().unwrap().queue.pending(),
        0
    );
    let source = tabs.get_mut(id).unwrap();
    assert!(
        source
            .terminal()
            .snapshot()
            .unwrap()
            .content
            .display_iter
            .all(|c| c.c == ' ')
    );
    let queue = &mut source.pty_mut().unwrap().queue;
    let reply = b"\x1b[?6c\x1b[0n\x1b[3;7R".repeat(query.len() + 1);
    assert_eq!(queue.pending(), 6 + reply.len());
    // Reserved paste blocks subsequent replies, then delivers one intact paste first.
    let mut writer = FakeWriter::new([interrupted(), Step::Accept(2), blocked(), Step::Accept(99)]);
    assert_eq!(
        queue.flush(id, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::Written(2)
    );
    assert_eq!(
        queue.flush(id, Some(&mut writer)).unwrap().status,
        PtyWriteStatus::WouldBlock
    );
    queue.flush(id, Some(&mut writer)).unwrap();
    assert!(!queue.wants_write());
    let mut paste = b"\x1b[200~paste\x1b[201~".to_vec();
    queue.enqueue_paste(id, true, &mut paste, true).unwrap();
    queue.enqueue(id, true, b"after").unwrap();
    writer
        .steps
        .extend(std::iter::repeat_with(|| Step::Accept(1)).take(queue.pending()));
    while queue.wants_write() {
        queue.flush(id, Some(&mut writer)).unwrap();
    }
    let expected = [
        b"before\x1b[200~paste\x1b[201~".as_slice(),
        &reply,
        b"after",
    ]
    .concat();
    assert_eq!(writer.output, expected);
    // Full shared FIFO rejects an entire batch; previous bytes stay intact and aren't replayed.
    queue
        .enqueue(id, true, &vec![b'k'; MAX_PTY_PENDING_BYTES])
        .unwrap();
    tabs.get_mut(id)
        .unwrap()
        .terminal_mut()
        .feed(id, b"\x1b[5n")
        .unwrap();
    assert!(tabs.enqueue_terminal_replies(id).is_err());
    assert!(tabs.enqueue_terminal_replies(id).is_ok());
    assert_eq!(
        tabs.get(id).unwrap().pty().unwrap().queue.pending(),
        MAX_PTY_PENDING_BYTES
    );
    // Closing a source discards its queue rather than targeting the active successor.
    tabs.close(id);
    assert!(tabs.enqueue_terminal_replies(id).is_err());
    assert_eq!(
        tabs.get(TabId(1)).unwrap().pty().unwrap().queue.pending(),
        0
    );
}

#[test]
fn runtime_resource_caps_apply_to_actual_owners() {
    let mut queues: Vec<_> = (0..crate::tabs::MAX_TABS)
        .map(|_| WriteQueue::default())
        .collect();
    let regular = vec![b'k'; MAX_PTY_PENDING_BYTES];
    for (i, queue) in queues.iter_mut().enumerate() {
        queue.enqueue(TabId(i as u64), true, &regular).unwrap();
        assert!(queue.enqueue(TabId(i as u64), true, b"x").is_err());
        assert!(queue.bytes.capacity() <= MAX_PTY_PENDING_BYTES);
    }
    assert_eq!(
        queues.iter().map(|q| q.bytes.capacity()).sum::<usize>(),
        4 * 1024 * 1024
    );
    // Every formerly focused Tab may retain only short framing after cancellation.
    for (i, queue) in queues.iter_mut().enumerate() {
        let tab = TabId(i as u64);
        while !queue.bytes.is_empty() {
            queue.flush(tab, Some(&mut io::sink())).unwrap();
        }
        let mut paste = b"\x1b[200~payload\x1b[201~".to_vec();
        let ptr = paste.as_ptr();
        queue.enqueue_paste(tab, true, &mut paste, true).unwrap();
        assert_eq!(
            queue.paste.as_ref().unwrap().bytes.as_ptr(),
            ptr,
            "move, never copy"
        );
        assert_eq!(paste.capacity(), 0);
        let mut writer = FakeWriter::new([Step::Accept(1)]);
        queue.flush(tab, Some(&mut writer)).unwrap();
        queue.cancel_paste(true);
        assert!(queue.paste.as_ref().unwrap().bytes.capacity() <= 12);
    }
    assert!(
        queues
            .iter()
            .map(|q| q.paste.as_ref().unwrap().bytes.capacity())
            .sum::<usize>()
            <= 64 * 12
    );
    for (i, queue) in queues.iter_mut().enumerate() {
        queue.flush(TabId(i as u64), Some(&mut io::sink())).unwrap();
        assert!(!queue.has_paste());
    }
}
