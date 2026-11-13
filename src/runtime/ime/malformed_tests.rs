//! Fixed-seed adapter-boundary corpus; not a claim of exhaustive wire fuzzing.

use super::super::{
    PaintState, clipboard::Binding, clipboard_offers::Offers, clipboard_receive::Receive,
    paste::Paste, scale,
};
use super::{ImeEvent, ImeInput, KeyboardFocus};
use crate::{
    TabId, WindowId,
    clipboard::{InputSerialEvent, PasteSink, SeatId},
    input::FocusTarget,
    pty::{
        MAX_PTY_PENDING_BYTES, PtyReadError, PtyReadResult, PtyReadStatus, PtyWriteError,
        PtyWriteFailure,
    },
    tabs::{
        TabPty, TabRegistry,
        output::{TabPtyRead, TabPtyWrite},
    },
    terminal::{MAX_FEED_BYTES, MAX_OSC_BYTES, Terminal},
};
use std::{
    cell::Cell,
    collections::VecDeque,
    io::{self, Read},
    rc::Rc,
};

fn next(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

struct Reader {
    calls: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
    mode: u32,
    bytes: &'static [u8],
}
impl Read for Reader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.calls.set(self.calls.get() + 1);
        match self.mode {
            1 => Err(io::ErrorKind::Interrupted.into()),
            2 => Err(io::ErrorKind::WouldBlock.into()),
            _ => {
                let count = self.bytes.len().min(output.len()).min(1);
                output[..count].copy_from_slice(&self.bytes[..count]);
                self.bytes = &self.bytes[count..];
                Ok(count)
            }
        }
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

fn clipboard_events(value: u32) -> usize {
    let mut binding = Binding::default();
    let seat = SeatId(7);
    let target = FocusTarget {
        window: WindowId(1),
        tab: TabId(1),
    };
    binding.sync(Some((3, seat)));
    let old = binding.current;
    binding.focus(Some(target));
    assert!(!binding.record(SeatId(8), target, value, InputSerialEvent::KeyPress));
    assert!(!binding.record(seat, target, value, InputSerialEvent::Other));
    assert!(binding.record(seat, target, value, InputSerialEvent::KeyPress));
    assert_eq!(
        binding.serials.take_for_selection(seat, target).unwrap().0,
        value
    );
    assert!(binding.serials.take_for_selection(seat, target).is_none());
    binding.record(
        seat,
        target,
        value.wrapping_add(1),
        InputSerialEvent::PointerButtonPress,
    );
    binding.sync(None);
    assert!(binding.serials.take_for_selection(seat, target).is_none());
    binding.sync(Some((3, seat)));
    assert_ne!(old, binding.current); // Same numeric global name, different bound generation.

    let mut offers = Offers::default();
    let old = (value % 4, 1u64);
    let new = (value % 4, 2u64);
    offers.created(old).unwrap();
    offers.advertise(&old, "text/plain;charset=utf-8").unwrap();
    offers.select(Some(&old));
    let id = offers.selected().unwrap().1.id();
    let calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let reader = |mode, bytes| Reader {
        calls: calls.clone(),
        drops: drops.clone(),
        mode,
        bytes,
    };
    let mut receive = Receive::default();
    let ready = receive
        .start(id, reader(value % 4, "中".as_bytes()), 8)
        .unwrap();
    assert!(receive.pump(Some(ready + 1), &offers).unwrap().is_none());
    assert_eq!(calls.get(), 0);
    assert!(receive.pump(Some(ready), &offers).unwrap().is_none());
    assert!((1..=4).contains(&calls.get()));
    offers.created(new).unwrap();
    offers.advertise(&new, "text/plain").unwrap();
    assert_eq!(offers.select(Some(&new)).1, [old]);
    let before = calls.get();
    assert!(receive.pump(None, &offers).is_err());
    assert_eq!(calls.get(), before); // Stale ownership closes without another FD read.
    assert_eq!(drops.get(), 1);
    assert!(receive.fd().is_none());
    assert!(offers.advertise(&old, "text/plain").is_err());
    assert!(!offers.select(Some(&old)).0);
    let id = offers.selected().unwrap().1.id();
    let replacement = receive.start(id, reader(0, b"ok"), 8).unwrap();
    assert!(receive.pump(Some(ready), &offers).unwrap().is_none());
    assert_eq!(calls.get(), before);
    for _ in 0..2 {
        assert!(receive.pump(Some(replacement), &offers).unwrap().is_none());
    }
    assert_eq!(
        receive.pump(Some(replacement), &offers).unwrap().as_deref(),
        Some("ok")
    );
    assert_eq!(drops.get(), 2);
    let invalid: &[u8] = if value & 1 == 0 { b"\xff" } else { b"ab" };
    let invalid_job = receive.start(id, reader(0, invalid), 1).unwrap();
    assert!(receive.pump(Some(invalid_job), &offers).unwrap().is_none());
    assert!(receive.pump(Some(invalid_job), &offers).is_err());
    assert!(receive.fd().is_none());
    assert_eq!(drops.get(), 3);
    let mut accepted = 1;
    for generation in 3..100 {
        accepted += usize::from(offers.created((value % 4, generation)).is_ok());
    }
    assert_eq!(accepted, 4);
    assert_eq!(offers.clear().len(), 4);
    calls.get()
}

struct Sink(TabId, Vec<u8>);
impl PasteSink for Sink {
    type Error = std::convert::Infallible;
    fn tab(&self) -> TabId {
        self.0
    }
    fn enqueue(&mut self, bytes: &[u8]) -> std::result::Result<(), Self::Error> {
        assert!(self.1.len() + bytes.len() <= 4096);
        self.1.extend_from_slice(bytes);
        Ok(())
    }
}

fn ime_events(value: u32) -> usize {
    let mut focus = KeyboardFocus::default();
    focus.enter(WindowId(1), TabId(1));
    let mut ime = ImeInput::new(7, 3);
    ime.state.enter(SeatId(7), WindowId(1));
    ime.sync_focus(&focus);
    let serial = ime.state.commit_serial();
    let mut repeat = crate::repeat::RepeatScheduler::default();
    let mut sink = Sink(TabId(1), Vec::new());
    let mut pending = Paste::capture(&focus).unwrap();
    let terminal = Terminal::with_scrollback(TabId(1), 2, 8, 0).unwrap();
    let case = value % 6;
    if case == 3 {
        ime.stage(
            &focus,
            ImeEvent::Preedit {
                text: Some("中"),
                cursor_begin: 1,
                cursor_end: 2,
            },
        );
    }
    let oversized = "x".repeat(4097);
    ime.stage(
        &focus,
        ImeEvent::Commit {
            text: Some(if case == 4 { &oversized } else { "中" }),
        },
    );
    let done_serial = match case {
        1 | 5 => {
            focus.enter(WindowId(1), TabId(2));
            ime.sync_focus(&focus);
            sink.0 = TabId(2);
            if case == 5 {
                focus.enter(WindowId(1), TabId(1));
                ime.sync_focus(&focus);
                sink.0 = TabId(1);
                ime.stage(
                    &focus,
                    ImeEvent::Commit {
                        text: Some("遲到")
                    },
                );
            }
            assert!(
                pending
                    .complete(&focus, &terminal, "cancelled".into())
                    .is_err()
            );
            serial
        }
        2 => serial.wrapping_add(1),
        _ => serial,
    };
    let result = ime
        .done(&focus, done_serial, &mut sink, &mut repeat)
        .unwrap();
    assert_eq!(result.committed, case == 0);
    assert!(
        !ime.done(&focus, done_serial, &mut sink, &mut repeat)
            .unwrap()
            .committed
    );
    assert_eq!(
        sink.1.as_slice(),
        if case == 0 { "中".as_bytes() } else { b"" }
    );
    sink.1.len()
}

enum Source {
    Bytes(VecDeque<Vec<u8>>),
    Queries,
    WrongTab,
    InvalidLength,
    Interrupted,
}
struct Pty {
    tab: TabId,
    source: Source,
    writes: Vec<u8>,
    reads: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
}
impl TabPty for Pty {
    fn tab(&self) -> TabId {
        self.tab
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
impl TabPtyRead for Pty {
    fn read_chunk(
        &mut self,
        buffer: &mut [u8],
    ) -> std::result::Result<PtyReadResult, PtyReadError> {
        self.reads.set(self.reads.get() + 1);
        let mut tab = self.tab;
        let status = match &mut self.source {
            Source::Queries => {
                for chunk in buffer.chunks_exact_mut(4) {
                    chunk.copy_from_slice(b"\x1b[6n");
                }
                PtyReadStatus::Data(buffer.len() / 4 * 4)
            }
            Source::Bytes(chunks) => match chunks.pop_front() {
                Some(bytes) => {
                    assert!(bytes.len() <= buffer.len());
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    PtyReadStatus::Data(bytes.len())
                }
                None => PtyReadStatus::WouldBlock,
            },
            Source::WrongTab => {
                tab = TabId(2);
                buffer[0] = b'X';
                PtyReadStatus::Data(1)
            }
            Source::InvalidLength => PtyReadStatus::Data(buffer.len() + 1),
            Source::Interrupted => PtyReadStatus::Interrupted,
        };
        Ok(PtyReadResult { tab, status })
    }
}
impl TabPtyWrite for Pty {
    fn enqueue_write(&mut self, bytes: &[u8]) -> std::result::Result<(), PtyWriteError> {
        if self.writes.len() + bytes.len() > MAX_PTY_PENDING_BYTES {
            return Err(PtyWriteError {
                tab: self.tab,
                pending: self.writes.len(),
                reason: PtyWriteFailure::QueueFull,
            });
        }
        self.writes.extend_from_slice(bytes);
        Ok(())
    }
}

fn streams(seed: &mut u32) -> usize {
    let reads = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let mut tabs = TabRegistry::with_scrollback(0).unwrap();
    let bad_osc = [
        b"\x1b]2;".as_slice(),
        &vec![b'x'; MAX_OSC_BYTES + 1],
        b"\x07",
    ]
    .concat();
    for (index, source) in [
        Source::Queries,
        Source::Bytes(VecDeque::from([b"healthy".to_vec()])),
        Source::WrongTab,
        Source::InvalidLength,
        Source::Interrupted,
        Source::Bytes(VecDeque::from([bad_osc])),
    ]
    .into_iter()
    .enumerate()
    {
        let tab = TabId(index as u64 + 1);
        tabs.insert(tab, 2, 40).unwrap();
        tabs.attach_pty(Pty {
            tab,
            source,
            writes: Vec::new(),
            reads: reads.clone(),
            drops: drops.clone(),
        })
        .ok()
        .unwrap();
    }
    let mut ready = vec![
        TabId(1),
        TabId(1),
        TabId(999),
        TabId(2),
        TabId(3),
        TabId(4),
        TabId(5),
        TabId(6),
    ];
    let mut buffer = [0; MAX_FEED_BYTES];
    let mut rejected = 0;
    for _ in 0..48 {
        let length = ready.len();
        ready.rotate_left(next(seed) as usize % length);
        let before = reads.get();
        let count = tabs.drain_ready(&ready, &mut buffer, |_, _| {}).unwrap();
        assert!(count <= crate::tabs::output::MAX_READS_PER_TICK);
        assert_eq!(reads.get() - before, count);
        rejected += usize::from(tabs.enqueue_terminal_replies(TabId(1)).is_err());
        assert!(tabs.get(TabId(1)).unwrap().pty().unwrap().writes.len() <= MAX_PTY_PENDING_BYTES);
        assert!(tabs.get(TabId(2)).unwrap().pty().unwrap().writes.is_empty());
        assert!(tabs.allocated_cells() <= crate::tabs::MAX_TOTAL_TAB_ALLOCATION_CELLS);
    }
    assert!(rejected > 0);
    for id in [3, 4, 6] {
        assert!(tabs.get(TabId(id)).unwrap().pty().is_none());
        assert!(tabs.get(TabId(id)).unwrap().terminal().snapshot().is_some());
    }
    assert_eq!(drops.get(), 3);
    assert!(tabs.get(TabId(1)).unwrap().failure().is_none());
    assert!(tabs.get(TabId(2)).unwrap().failure().is_none());
    assert!(tabs.get(TabId(5)).unwrap().failure().is_none());
    let healthy = |tabs: &TabRegistry<Pty>| {
        tabs.get(TabId(2))
            .unwrap()
            .terminal()
            .snapshot()
            .unwrap()
            .content
            .display_iter
            .map(|cell| cell.c)
            .collect::<String>()
    };
    let expected = healthy(&tabs);
    assert!(expected.starts_with("healthy"));
    // Fixed malformed bytes traverse the actual ready-PTY adapter, not just Terminal::feed.
    let mut corpus: Vec<Vec<u8>> = include_str!("../../../tests/fixtures/fuzz.hex")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split_whitespace()
                .map(|hex| u8::from_str_radix(hex, 16).unwrap())
                .collect()
        })
        .collect();
    for _ in 0..128 {
        corpus.push((0..128).map(|_| next(seed) as u8).collect());
    }
    for (index, bytes) in corpus.into_iter().enumerate() {
        let tab = TabId(index as u64 + 7);
        tabs.insert(tab, 2, 40).unwrap();
        let size = (next(seed) as usize % 31) + 1;
        let chunks: VecDeque<_> = bytes.chunks(size).map(<[u8]>::to_vec).collect();
        let limit = chunks.len() + 1;
        tabs.attach_pty(Pty {
            tab,
            source: Source::Bytes(chunks),
            writes: Vec::new(),
            reads: reads.clone(),
            drops: drops.clone(),
        })
        .ok()
        .unwrap();
        for _ in 0..limit {
            assert!(
                tabs.drain_ready(&[tab, tab, TabId(999)], &mut buffer, |_, _| {})
                    .unwrap()
                    <= 1
            );
        }
        assert!(tabs.get(TabId(2)).unwrap().failure().is_none());
        assert!(tabs.get(TabId(2)).unwrap().pty().unwrap().writes.is_empty());
        assert_eq!(healthy(&tabs), expected);
        tabs.close(tab).unwrap();
        assert!(tabs.insert(tab, 2, 40).is_err());
        assert_eq!(
            tabs.drain_ready(&[tab], &mut buffer, |_, _| panic!("closed Tab dispatched"))
                .unwrap(),
            0
        );
    }
    for tab in (1..=6).map(TabId) {
        tabs.close(tab).unwrap();
    }
    assert_eq!(tabs.allocated_cells(), 0);
    reads.get()
}

fn run_corpus(mut seed: u32) -> (usize, usize, usize) {
    let mut fd_calls = clipboard_events(0) + clipboard_events(u32::MAX);
    let mut committed = 0;
    let mut outputs = scale::State::default();
    let mut live = Vec::new();
    for _ in 0..256 {
        let value = next(&mut seed);
        fd_calls += clipboard_events(value);
        committed += ime_events(value);
        let mut paint = PaintState::default();
        assert!(paint.configure(i32::MAX, i32::MAX).is_err());
        assert!(!paint.ready());
        assert_eq!(paint.size, (800, 600));
        assert!(paint.configure(-1, 1).is_err());
        paint.configure(800, 600).unwrap();
        paint.submitted();
        assert!(!paint.ready());
        assert!(super::super::scale::buffer_size((u32::MAX, value), u32::MAX).is_none());
        let name = value % 100;
        if (value >> 8) & 1 == 0 {
            if outputs.add(name, 4) {
                live.push(name);
            }
        } else {
            outputs.remove(name);
            live.retain(|old| *old != name);
        }
        assert!(live.len() <= 64);
    }
    (fd_calls, committed, streams(&mut seed))
}

#[test]
fn runtime_malformed_events_remain_bounded_and_isolated() {
    assert_eq!(run_corpus(0x5054_5952), run_corpus(0x5054_5952));
}
