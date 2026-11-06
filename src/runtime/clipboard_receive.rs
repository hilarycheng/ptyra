//! One bounded, readiness-driven receive. Replacing it closes the previous FD.
use super::clipboard_offers::Offers;
use crate::clipboard::{
    OfferId,
    read::{ClipboardTransfer, ReadProgress},
};
use std::io::Read;
struct Job<R: Read> {
    id: u64,
    offer: OfferId,
    transfer: ClipboardTransfer<R>,
}
pub(super) struct Receive<R: Read> {
    job: Option<Job<R>>,
    next: u64,
}
impl<R: Read> Default for Receive<R> {
    fn default() -> Self {
        Self { job: None, next: 0 }
    }
}
impl<R: Read> Receive<R> {
    pub fn start(&mut self, offer: OfferId, reader: R, limit: usize) -> super::Result<u64> {
        self.cancel();
        let id = self
            .next
            .checked_add(1)
            .ok_or("clipboard receive identity exhausted")?;
        let transfer = ClipboardTransfer::new(offer, reader, limit)
            .map_err(|_| "invalid clipboard receive limit")?;
        self.next = id;
        self.job = Some(Job {
            id,
            offer,
            transfer,
        });
        Ok(id)
    }
    pub fn cancel(&mut self) {
        self.job = None;
    }
    pub fn fd(&self) -> Option<(u64, &R)> {
        let job = self.job.as_ref()?;
        Some((job.id, job.transfer.reader()?))
    }
    /// Validate ownership even without readiness; a stale FD must not survive replacement.
    /// A call reads at most 16 KiB or retries four interruptions, then yields to the UI.
    pub fn pump<K: Eq>(
        &mut self,
        ready: Option<u64>,
        offers: &Offers<K>,
    ) -> super::Result<Option<String>> {
        let Some(job) = &mut self.job else {
            return Ok(None);
        };
        let selected = offers
            .selected()
            .filter(|(_, offer)| offer.id() == job.offer && !offer.is_cancelled());
        let Some((_, offer)) = selected else {
            self.cancel();
            return Err("clipboard receive cancelled");
        };
        if ready != Some(job.id) {
            return Ok(None);
        }
        match job.transfer.read_ready(offer, &offers.state) {
            Ok(ReadProgress::Complete(text)) => {
                self.cancel();
                Ok(Some(text))
            }
            Ok(ReadProgress::Closed) => {
                self.cancel();
                Err("clipboard receive cancelled")
            }
            Ok(_) => Ok(None),
            Err(error) => {
                self.cancel();
                Err(match error.kind() {
                    std::io::ErrorKind::FileTooLarge => "clipboard receive exceeds limit",
                    std::io::ErrorKind::InvalidData => "clipboard receive is not UTF-8",
                    _ => "clipboard receive failed",
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        collections::VecDeque,
        io::{self, Read},
        rc::Rc,
    };
    struct Reader {
        chunks: VecDeque<io::Result<Vec<u8>>>,
        drops: Rc<Cell<usize>>,
    }
    impl Read for Reader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let chunk = self.chunks.pop_front().unwrap_or(Ok(Vec::new()))?;
            assert!(chunk.len() <= bytes.len());
            bytes[..chunk.len()].copy_from_slice(&chunk);
            Ok(chunk.len())
        }
    }
    impl Drop for Reader {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    fn offers() -> Offers<u32> {
        let mut offers = Offers::default();
        offers.created(1).unwrap();
        offers.advertise(&1, "text/plain;charset=utf-8").unwrap();
        offers.select(Some(&1));
        offers
    }
    #[test]
    fn runtime_clipboard_receive_is_bounded_and_cancellable() {
        let mut offers = offers();
        let offer = offers.selected().unwrap().1.id();
        let drops = Rc::new(Cell::new(0));
        let mut receive = Receive::default();
        let reader = |chunks: Vec<io::Result<Vec<u8>>>| Reader {
            chunks: chunks.into(),
            drops: drops.clone(),
        };
        let id = receive
            .start(
                offer,
                reader(vec![
                    Ok(vec![0xe7]),
                    Err(io::ErrorKind::Interrupted.into()),
                    Err(io::ErrorKind::WouldBlock.into()),
                    Ok(vec![0xb9, 0x81, b' ', b'\n']),
                    Ok(Vec::new()),
                ]),
                5,
            )
            .unwrap();
        assert!(receive.pump(None, &offers).unwrap().is_none());
        assert!(receive.pump(Some(id + 1), &offers).unwrap().is_none());
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert_eq!(
            receive.pump(Some(id), &offers).unwrap().as_deref(),
            Some("繁 \n")
        );
        assert!(receive.fd().is_none());
        assert_eq!(drops.get(), 1);
        let id = receive
            .start(offer, reader(vec![Ok(b"12345".to_vec())]), 4)
            .unwrap();
        assert_eq!(
            receive.pump(Some(id), &offers).unwrap_err(),
            "clipboard receive exceeds limit"
        );
        assert!(receive.fd().is_none());
        let id = receive
            .start(offer, reader(vec![Ok(vec![0xff])]), 4)
            .unwrap();
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert_eq!(
            receive.pump(Some(id), &offers).unwrap_err(),
            "clipboard receive is not UTF-8"
        );
        assert!(receive.fd().is_none());
        receive
            .start(offer, reader(vec![Ok(b"x".to_vec())]), 4)
            .unwrap();
        offers.clear();
        assert_eq!(
            receive.pump(None, &offers).unwrap_err(),
            "clipboard receive cancelled"
        );
        assert!(receive.fd().is_none());
        assert_eq!(drops.get(), 4);
        receive.start(offer, reader(vec![]), 4).unwrap();
        receive.cancel();
        assert_eq!(drops.get(), 5);
    }
    #[test]
    fn runtime_clipboard_receive_rejects_stale_readiness_and_owner() {
        let mut offers = offers();
        let offer = offers.selected().unwrap().1.id();
        let drops = Rc::new(Cell::new(0));
        let reader = || Reader {
            chunks: VecDeque::from([Ok(b"ok".to_vec())]),
            drops: drops.clone(),
        };
        let mut receive = Receive::default();
        let old = receive.start(offer, reader(), 8).unwrap();
        let new = receive.start(offer, reader(), 8).unwrap();
        assert_eq!(drops.get(), 1);
        assert_ne!(old, new);
        assert!(receive.pump(Some(old), &offers).unwrap().is_none());
        assert_eq!(
            receive
                .job
                .as_ref()
                .unwrap()
                .transfer
                .reader()
                .unwrap()
                .chunks
                .len(),
            1
        );
        offers.created(2).unwrap();
        offers.advertise(&2, "text/plain").unwrap();
        offers.select(Some(&2));
        assert!(receive.pump(None, &offers).is_err());
        assert_eq!(drops.get(), 2);
        let second = offers.selected().unwrap().1.id();
        receive.start(second, reader(), 8).unwrap();
        assert!(offers.advertise(&2, &"x".repeat(257)).is_err());
        assert!(receive.pump(None, &offers).is_err());
        assert_eq!(drops.get(), 3);
    }
    #[test]
    fn runtime_clipboard_receive_limits_dispatch_work() {
        let offers = offers();
        let offer = offers.selected().unwrap().1.id();
        let drops = Rc::new(Cell::new(0));
        let mut receive = Receive::default();
        let id = receive
            .start(
                offer,
                Reader {
                    chunks: VecDeque::from([
                        Err(io::ErrorKind::Interrupted.into()),
                        Err(io::ErrorKind::Interrupted.into()),
                        Err(io::ErrorKind::Interrupted.into()),
                        Err(io::ErrorKind::Interrupted.into()),
                        Ok(vec![b'x'; 16384]),
                        Ok(vec![b'x'; 16384]),
                    ]),
                    drops: drops.clone(),
                },
                32768,
            )
            .unwrap();
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert_eq!(receive.fd().unwrap().1.chunks.len(), 2);
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert_eq!(receive.fd().unwrap().1.chunks.len(), 1);
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert_eq!(
            receive.pump(Some(id), &offers).unwrap().unwrap(),
            "x".repeat(32768)
        );
        assert_eq!(drops.get(), 1);
    }
    #[test]
    fn runtime_clipboard_receive_owns_nonblocking_pipe() {
        use std::{fs::File, io::Write};
        let offers = offers();
        let offer = offers.selected().unwrap().1.id();
        let (read, write) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC).unwrap();
        let file = File::from(read);
        super::super::clipboard_send::nonblocking(&file).unwrap();
        let mut writer = File::from(write);
        let mut receive = Receive::default();
        let id = receive.start(offer, file, 8).unwrap();
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        writer.write_all(&[0xe7]).unwrap();
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        writer.write_all(&[0xb9, 0x81]).unwrap();
        drop(writer);
        assert!(receive.pump(Some(id), &offers).unwrap().is_none());
        assert_eq!(
            receive.pump(Some(id), &offers).unwrap().as_deref(),
            Some("繁")
        );
        assert!(receive.fd().is_none());
        let (read, write) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC).unwrap();
        let mut writer = File::from(write);
        receive.start(offer, File::from(read), 8).unwrap();
        receive.cancel();
        assert_eq!(
            writer.write(b"x").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
