use super::{TabPty, TabRegistry};
use crate::{
    TabId,
    error::{AppError, Component},
    pty::{PtyReadError, PtyReadResult, PtyReadStatus, PtySession},
    terminal::MAX_FEED_BYTES,
};
use std::io::ErrorKind;

pub trait TabPtyRead: TabPty {
    fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError>;
}

impl TabPtyRead for PtySession {
    fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError> {
        self.read_chunk(buffer)
    }
}

impl<P: TabPtyRead> TabRegistry<P> {
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
                .feed(id, &buffer[..length])
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
