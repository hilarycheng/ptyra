use super::{TabPty, TabRegistry};
use crate::{
    TabId,
    error::{AppError, Component},
    pty::{PtyReadError, PtyReadResult, PtyReadStatus, PtySession},
    terminal::MAX_FEED_BYTES,
};
use std::io::ErrorKind;

pub const MAX_READS_PER_TICK: usize = 4;

pub trait TabPtyRead: TabPty {
    fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError>;
}

impl TabPtyRead for PtySession {
    fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError> {
        self.read_chunk(buffer)
    }
}

/// Every producer uses the session's existing bounded FIFO, including paste barriers.
pub trait TabPtyWrite: TabPty {
    fn enqueue_write(&mut self, bytes: &[u8]) -> Result<(), crate::pty::PtyWriteError>;
}
impl TabPtyWrite for PtySession {
    fn enqueue_write(&mut self, bytes: &[u8]) -> Result<(), crate::pty::PtyWriteError> {
        self.enqueue_write(bytes).map(|_| ())
    }
}
impl<P: TabPtyWrite> TabRegistry<P> {
    /// Source-bound, not focus-bound. Rejected replies are reported, never retried or rerouted.
    pub fn enqueue_terminal_replies(&mut self, id: TabId) -> Result<(), AppError> {
        let rejected = || AppError::new(Component::Pty, ErrorKind::WouldBlock);
        let tab = self.get_mut(id).ok_or_else(rejected)?;
        let (bytes, overflow) = tab.terminal.take_core_replies();
        if !bytes.is_empty() {
            let pty = tab
                .pty
                .as_mut()
                .filter(|pty| pty.tab() == id)
                .ok_or_else(rejected)?;
            pty.enqueue_write(&bytes).map_err(|_| rejected())?;
        }
        if overflow { Err(rejected()) } else { Ok(()) }
    }
}

impl<P: TabPtyRead> TabRegistry<P> {
    /// Service at most four ready sessions once each, then return to input dispatch.
    /// Keep readable sessions registered; Interrupted must be retried on a later tick.
    pub fn drain_ready(
        &mut self,
        ready: &[TabId],
        buffer: &mut [u8],
        mut report: impl FnMut(TabId, Result<PtyReadResult, AppError>),
    ) -> Result<usize, AppError> {
        if ready.len() > super::MAX_TABS || buffer.is_empty() {
            return Err(AppError::new(Component::Pty, ErrorKind::InvalidInput));
        }
        let mut reads = 0;
        for _ in 0..self.entries.len() {
            self.output_cursor %= self.entries.len();
            let id = self.entries[self.output_cursor].id;
            self.output_cursor += 1;
            if ready.contains(&id) && self.entries[self.output_cursor - 1].pty.is_some() {
                let result = self.drain_output(id, buffer);
                if let Err(error) = result {
                    self.stop_session(id, super::SessionFailure::from_error(error));
                }
                report(id, result);
                reads += 1;
                if reads == MAX_READS_PER_TICK {
                    break;
                }
            }
        }
        Ok(reads)
    }

    /// One bounded read per readiness dispatch, for active and hidden Tabs alike.
    /// Wait for readiness after WouldBlock; reschedule Interrupted without spinning.
    pub fn drain_output(
        &mut self,
        id: TabId,
        buffer: &mut [u8],
    ) -> Result<PtyReadResult, AppError> {
        if buffer.is_empty() {
            return Err(AppError::new(Component::Pty, ErrorKind::InvalidInput));
        }
        let tab = self
            .get_mut(id)
            .ok_or_else(|| AppError::new(Component::Pty, ErrorKind::NotFound))?;
        let pty = tab
            .pty
            .as_mut()
            .ok_or_else(|| AppError::new(Component::Pty, ErrorKind::NotConnected))?;
        let limit = buffer.len().min(MAX_FEED_BYTES);
        let result = pty
            .read_chunk(&mut buffer[..limit])
            .map_err(|error| error.error)?;
        if result.tab != id {
            return Err(AppError::new(Component::Pty, ErrorKind::InvalidData));
        }
        if let PtyReadStatus::Data(length) = result.status {
            if length == 0 || length > limit {
                return Err(AppError::new(Component::Pty, ErrorKind::InvalidData));
            }
            tab.dirty = true;
            tab.terminal
                .feed_with_replies(id, &buffer[..length])
                .map_err(|error| error.error)?;
            tab.refresh_title()?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    #[test]
    fn runtime_multi_pty_poll_is_fair() {
        struct Flood(TabId);
        impl TabPty for Flood {
            fn tab(&self) -> TabId {
                self.0
            }
        }
        impl TabPtyRead for Flood {
            fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError> {
                buffer.fill(b'x');
                Ok(PtyReadResult {
                    tab: self.0,
                    status: PtyReadStatus::Data(buffer.len()),
                })
            }
        }
        let mut tabs = TabRegistry::with_scrollback(0).unwrap();
        let ready: Vec<_> = (1..=10).map(TabId).collect();
        for &id in &ready {
            tabs.insert(id, 2, 16).unwrap();
            tabs.attach_pty(Flood(id)).ok().unwrap();
        }
        let mut buffer = vec![0; MAX_FEED_BYTES * 2];
        let mut visited = Vec::new();
        for _ in 0..3 {
            let mut bytes = 0;
            let count = tabs
                .drain_ready(&ready, &mut buffer, |id, result| {
                    visited.push(id);
                    if let PtyReadStatus::Data(n) = result.unwrap().status {
                        bytes += n;
                    }
                })
                .unwrap();
            assert_eq!(count, 4);
            assert_eq!(bytes, 4 * MAX_FEED_BYTES);
            // Returning here lets the dispatcher service input between batches.
        }
        assert_eq!(&visited[..10], ready.as_slice());
        assert_eq!(
            tabs.drain_ready(&[], &mut buffer, |_, _| panic!("not ready"))
                .unwrap(),
            0
        );
        assert!(
            tabs.drain_ready(&vec![TabId(1); 65], &mut buffer, |_, _| {})
                .is_err()
        );
    }
    #[derive(Debug)]
    struct FakePty {
        tab: TabId,
        steps: VecDeque<Option<Vec<u8>>>,
    }
    impl TabPty for FakePty {
        fn tab(&self) -> TabId {
            self.tab
        }
    }
    impl TabPtyRead for FakePty {
        fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError> {
            assert!(buffer.len() <= MAX_FEED_BYTES);
            let status = match self.steps.pop_front() {
                Some(Some(bytes)) => {
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    PtyReadStatus::Data(bytes.len())
                }
                Some(None) => PtyReadStatus::WouldBlock,
                None => PtyReadStatus::Eof,
            };
            Ok(PtyReadResult {
                tab: self.tab,
                status,
            })
        }
    }

    #[test]
    fn inactive_tab_drains_output() {
        let mut tabs = TabRegistry::default();
        for id in [1, 2] {
            tabs.insert(TabId(id), 2, 16).unwrap();
        }
        tabs.attach_pty(FakePty {
            tab: TabId(2),
            steps: [
                Some(vec![0xe4]),
                None,
                Some(vec![0xb8, 0xad]),
                Some(b"\x1b]2;hidden\x07".to_vec()),
            ]
            .into(),
        })
        .unwrap();
        assert_eq!(tabs.active(), Some(TabId(1)));
        let mut buffer = vec![0; MAX_FEED_BYTES * 2];
        assert_eq!(
            tabs.drain_output(TabId(2), &mut buffer).unwrap().status,
            PtyReadStatus::Data(1)
        );
        assert_eq!(
            tabs.drain_output(TabId(2), &mut buffer).unwrap().status,
            PtyReadStatus::WouldBlock
        );
        assert_eq!(
            tabs.drain_output(TabId(2), &mut buffer).unwrap().status,
            PtyReadStatus::Data(2)
        );
        tabs.drain_output(TabId(2), &mut buffer).unwrap();
        assert_eq!(
            tabs.drain_output(TabId(2), &mut buffer).unwrap().status,
            PtyReadStatus::Eof
        );
        assert_eq!(tabs.get(TabId(2)).unwrap().title(), "hidden");
        assert_eq!(
            tabs.get(TabId(2))
                .unwrap()
                .terminal()
                .snapshot()
                .unwrap()
                .content
                .display_iter
                .next()
                .unwrap()
                .c,
            '中'
        );
        assert_eq!(
            tabs.get(TabId(1))
                .unwrap()
                .terminal()
                .grid()
                .unwrap()
                .cursor
                .point
                .column
                .0,
            0
        );
        assert!(tabs.drain_output(TabId(1), &mut buffer).is_err());
        assert!(tabs.drain_output(TabId(99), &mut buffer).is_err());
        assert!(tabs.drain_output(TabId(2), &mut []).is_err());
        tabs.get_mut(TabId(2))
            .unwrap()
            .pty
            .as_mut()
            .unwrap()
            .steps
            .push_back(Some(Vec::new()));
        assert!(tabs.drain_output(TabId(2), &mut buffer).is_err());
        tabs.get_mut(TabId(2)).unwrap().pty.as_mut().unwrap().tab = TabId(1);
        assert!(tabs.drain_output(TabId(2), &mut buffer).is_err());
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct Source {
        id: TabId,
        bad: bool,
        calls: Rc<RefCell<Vec<TabId>>>,
        dropped: Rc<RefCell<Vec<TabId>>>,
    }
    impl TabPty for Source {
        fn tab(&self) -> TabId {
            self.id
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            self.dropped.borrow_mut().push(self.id);
        }
    }
    impl TabPtyRead for Source {
        fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError> {
            self.calls.borrow_mut().push(self.id);
            let bytes = if self.bad {
                let mut bytes = b"prefix\x1b]2;".to_vec();
                bytes.extend(std::iter::repeat_n(
                    b'x',
                    crate::terminal::MAX_OSC_BYTES + 1,
                ));
                bytes.extend_from_slice(b"\x07never replay\x1b[6n");
                bytes
            } else {
                b"ok".to_vec()
            };
            buffer[..bytes.len()].copy_from_slice(&bytes);
            Ok(PtyReadResult {
                tab: self.id,
                status: PtyReadStatus::Data(bytes.len()),
            })
        }
    }
    #[test]
    fn runtime_bad_stream_does_not_stop_other_tabs() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let dropped = Rc::new(RefCell::new(Vec::new()));
        let mut tabs = TabRegistry::with_scrollback(10).unwrap();
        for id in 1..=6 {
            tabs.insert(TabId(id), 3, 80).unwrap();
            tabs.attach_pty(Source {
                id: TabId(id),
                bad: id == 1 || id == 4,
                calls: calls.clone(),
                dropped: dropped.clone(),
            })
            .ok()
            .unwrap();
        }
        let ready: Vec<_> = (1..=6).map(TabId).collect();
        let mut buffer = [0; MAX_FEED_BYTES];
        let mut results = Vec::new();
        assert_eq!(
            tabs.drain_ready(&ready, &mut buffer, |id, result| results.push((id, result)))
                .unwrap(),
            4
        );
        assert!(results[0].1.is_err());
        assert!(results[1].1.is_ok());
        assert!(
            tabs.get(TabId(1)).unwrap().pty().is_none(),
            "failed source must release its PTY"
        );
        assert!(
            tabs.get(TabId(1)).unwrap().terminal().snapshot().is_some(),
            "failed active Tab stays drawable"
        );
        assert_eq!(*dropped.borrow(), [TabId(1), TabId(4)]);
        for _ in 0..4 {
            tabs.drain_ready(&ready, &mut buffer, |_, _| {}).unwrap();
        }
        assert_eq!(
            calls.borrow().iter().filter(|id| **id == TabId(1)).count(),
            1
        );
        assert_eq!(
            calls.borrow().iter().filter(|id| **id == TabId(4)).count(),
            1
        );
        assert!(
            calls.borrow().contains(&TabId(6)),
            "healthy sources remain fair"
        );
        assert!(tabs.resize_all(4, 60).unwrap());
        assert_eq!(tabs.active(), Some(TabId(1)));
        for id in ready {
            tabs.close(id);
        }
        assert_eq!(tabs.reserved_cells(), 0);
        assert_eq!(tabs.allocated_cells(), 0);
        assert_eq!(dropped.borrow().len(), 6);
    }
}
