//! Bounded readiness-driven send jobs. The owner cancels by source identity.
use crate::clipboard::write::ClipboardSend;
use std::{fs::File, io::Write};
const MAX_SENDS: usize = 4;
struct Job<W> {
    id: u64,
    source: u64,
    writer: W,
    send: ClipboardSend,
}
pub(super) struct SendQueue<W> {
    jobs: Vec<Job<W>>,
    next: u64,
    limit: usize,
}
impl<W> Default for SendQueue<W> {
    fn default() -> Self {
        Self::new(crate::clipboard::write::MAX_CLIPBOARD_BYTES)
    }
}
impl<W> SendQueue<W> {
    pub fn new(limit: usize) -> Self {
        Self {
            jobs: Vec::new(),
            next: 0,
            limit,
        }
    }
}
impl<W: Write> SendQueue<W> {
    pub fn start(&mut self, source: u64, text: &str, writer: W) -> super::Result<u64> {
        if self.jobs.len() == MAX_SENDS {
            return Err("too many clipboard transfers");
        }
        if text.len() > self.limit.min(crate::clipboard::write::MAX_CLIPBOARD_BYTES) {
            return Err("clipboard transfer exceeds limit");
        }
        let id = self
            .next
            .checked_add(1)
            .ok_or("clipboard transfer identity exhausted")?;
        let mut owned = String::new();
        owned
            .try_reserve_exact(text.len())
            .map_err(|_| "clipboard transfer allocation failed")?;
        owned.push_str(text);
        let send = ClipboardSend::new(owned).map_err(|_| "clipboard transfer exceeds limit")?;
        self.jobs
            .try_reserve(1)
            .map_err(|_| "clipboard transfer allocation failed")?;
        self.jobs.push(Job {
            id,
            source,
            writer,
            send,
        });
        self.next = id;
        Ok(id)
    }
    pub fn fds(&self) -> impl Iterator<Item = (u64, &W)> {
        self.jobs.iter().map(|job| (job.id, &job.writer))
    }
    pub fn cancel_source(&mut self, source: u64) {
        self.jobs.retain(|job| job.source != source);
    }
    /// At most four writes of 16 KiB each. WouldBlock waits for the next readiness event.
    pub fn pump(&mut self, ready: &[u64]) -> usize {
        let mut failed = 0;
        self.jobs.retain_mut(|job| {
            if !ready.contains(&job.id) {
                return true;
            }
            if job.send.write_ready(&mut job.writer).is_err() {
                failed += 1;
                return false;
            }
            job.send.pending() > 0
        });
        failed
    }
}
pub(super) fn nonblocking(file: &File) -> super::Result<()> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let flags = fcntl(file, FcntlArg::F_GETFL).map_err(|_| "clipboard FD flags unavailable")?;
    fcntl(
        file,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|_| "clipboard FD cannot be nonblocking")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        collections::VecDeque,
        io::{self, Write},
        rc::Rc,
    };
    struct Writer {
        plan: VecDeque<io::Result<usize>>,
        bytes: Rc<RefCell<Vec<u8>>>,
        dropped: Rc<RefCell<usize>>,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let count = self.plan.pop_front().unwrap_or(Ok(bytes.len()))?;
            let count = count.min(bytes.len());
            self.bytes.borrow_mut().extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Drop for Writer {
        fn drop(&mut self) {
            *self.dropped.borrow_mut() += 1;
        }
    }
    #[test]
    fn runtime_resource_caps_apply_to_actual_owners() {
        let config = crate::config::Config::from_toml(Some("clipboard_bytes=20000")).unwrap();
        let mut queues = [
            SendQueue::new(config.clipboard_bytes),
            SendQueue::new(config.clipboard_bytes),
        ];
        let text = "x".repeat(config.clipboard_bytes);
        for queue in &mut queues {
            assert!(queue.start(1, &(text.clone() + "x"), io::sink()).is_err());
            for source in 1..=4 {
                queue.start(source, &text, io::sink()).unwrap();
            }
            assert!(queue.start(5, &text, io::sink()).is_err());
        }
        assert_eq!(
            queues
                .iter()
                .map(|q| q.jobs.iter().map(|j| j.send.pending()).sum::<usize>())
                .sum::<usize>(),
            8 * config.clipboard_bytes
        );
        for queue in &mut queues {
            let ids: Vec<_> = queue.fds().map(|(id, _)| id).collect();
            queue.pump(&ids); // Partial sends retain their full original allocation.
            assert_eq!(queue.fds().count(), 4);
            queue.cancel_source(1);
            assert_eq!(queue.fds().count(), 3);
            queue.start(5, &text, io::sink()).unwrap();
            for source in 1..=5 {
                queue.cancel_source(source);
            }
            assert_eq!(queue.fds().count(), 0);
        }
    }
    #[test]
    fn runtime_clipboard_send_uses_nonblocking_fds() {
        use nix::fcntl::{FcntlArg, OFlag, fcntl};
        use std::io::Read;
        let (read, write) = nix::unistd::pipe().unwrap();
        let mut writer = File::from(write);
        nonblocking(&writer).unwrap();
        assert!(
            OFlag::from_bits_truncate(fcntl(&writer, FcntlArg::F_GETFL).unwrap())
                .contains(OFlag::O_NONBLOCK)
        );
        let mut filled = 0;
        for _ in 0..128 {
            match writer.write(&[0; 4096]) {
                Ok(count) => filled += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                other => panic!("unexpected pipe result: {other:?}"),
            }
        }
        assert!(filled > 0 && filled < 128 * 4096);
        let mut queue = SendQueue::default();
        let id = queue.start(1, "繁體", writer).unwrap();
        assert_eq!(queue.pump(&[id]), 0);
        assert_eq!(queue.fds().count(), 1);
        let mut reader = File::from(read);
        reader.read_exact(&mut vec![0; filled]).unwrap();
        assert_eq!(queue.pump(&[id]), 0);
        assert_eq!(queue.fds().count(), 0);
        let mut received = Vec::new();
        reader.read_to_end(&mut received).unwrap();
        assert_eq!(received, "繁體".as_bytes());
    }

    #[test]
    fn runtime_clipboard_send_limits_work_per_dispatch() {
        let bytes = Rc::new(RefCell::new(Vec::new()));
        let dropped = Rc::new(RefCell::new(0));
        let mut queue = SendQueue::default();
        for source in 0..4 {
            queue
                .start(
                    source,
                    &"x".repeat(100_000),
                    Writer {
                        plan: VecDeque::new(),
                        bytes: bytes.clone(),
                        dropped: dropped.clone(),
                    },
                )
                .unwrap();
        }
        let ids: Vec<_> = queue.fds().map(|(id, _)| id).collect();
        queue.pump(&ids);
        assert_eq!(bytes.borrow().len(), 4 * 16 * 1024);
        assert_eq!(queue.fds().count(), 4);
    }

    #[test]
    fn runtime_clipboard_send_resumes_partial_writes() {
        let bytes = Rc::new(RefCell::new(Vec::new()));
        let dropped = Rc::new(RefCell::new(0));
        let writer = Writer {
            plan: VecDeque::from([
                Ok(1),
                Err(io::ErrorKind::Interrupted.into()),
                Err(io::ErrorKind::WouldBlock.into()),
                Ok(2),
            ]),
            bytes: bytes.clone(),
            dropped: dropped.clone(),
        };
        let mut queue = SendQueue::default();
        let id = queue.start(1, "繁體 text", writer).unwrap();
        assert_eq!(queue.pump(&[]), 0);
        assert!(bytes.borrow().is_empty());
        queue.pump(&[id]);
        assert_eq!(bytes.borrow().len(), 1);
        queue.pump(&[id]);
        assert_eq!(bytes.borrow().len(), 1);
        while queue.fds().next().is_some() {
            queue.pump(&[id]);
        }
        assert_eq!(*bytes.borrow(), "繁體 text".as_bytes());
        assert_eq!(*dropped.borrow(), 1);
        for source in 1..=4 {
            queue
                .start(
                    source,
                    "payload",
                    Writer {
                        plan: VecDeque::new(),
                        bytes: bytes.clone(),
                        dropped: dropped.clone(),
                    },
                )
                .unwrap();
        }
        assert!(
            queue
                .start(
                    5,
                    "payload",
                    Writer {
                        plan: VecDeque::new(),
                        bytes: bytes.clone(),
                        dropped: dropped.clone()
                    }
                )
                .is_err()
        );
        assert_eq!(queue.fds().count(), 4);
        queue.cancel_source(2);
        assert_eq!(queue.fds().count(), 3);
        let ids: Vec<_> = queue.fds().map(|(id, _)| id).collect();
        queue.pump(&ids);
        assert_eq!(queue.fds().count(), 0);
        let failed = queue
            .start(
                9,
                "payload",
                Writer {
                    plan: VecDeque::from([Err(io::ErrorKind::BrokenPipe.into())]),
                    bytes,
                    dropped: dropped.clone(),
                },
            )
            .unwrap();
        assert_eq!(queue.pump(&[failed]), 1);
        assert_eq!(queue.fds().count(), 0);
        assert_eq!(*dropped.borrow(), 7);
    }
}
