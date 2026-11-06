use super::*;
use std::{cell::Cell, os::unix::ffi::OsStrExt, rc::Rc};

#[test]
fn pty_exit_does_not_exit_app() {
    use crate::{
        AppEffect, EventSource, WindowId, clock::FakeClock, frame::FrameScheduler, reduce,
        window::WindowSize,
    };
    use std::os::unix::process::ExitStatusExt;

    let mut first = ExitState::default();
    let mut second = ExitState::default();
    let mut frames = FrameScheduler::new(WindowId(1), WindowSize::new(80, 24).unwrap());
    assert!(first.take_event(TabId(7), || Ok(None)).unwrap().is_none());
    let status = ExitStatus::from_raw(23 << 8);
    let event = first
        .take_event(TabId(7), || Ok(Some(status)))
        .unwrap()
        .unwrap();
    assert_eq!(event.source(), EventSource::Pty);
    let effect = reduce(&event);
    assert_eq!(
        effect,
        AppEffect::PtyExited {
            tab: TabId(7),
            status
        }
    );
    assert_eq!(frames.apply_effect(&effect), None);
    frames.resize(WindowSize::new(100, 30).unwrap());
    assert!(frames.schedule(&FakeClock::default()).unwrap().is_some());
    assert!(
        first
            .take_event(TabId(7), || panic!("exit already delivered"))
            .unwrap()
            .is_none()
    );
    assert!(second.take_event(TabId(8), || Ok(None)).unwrap().is_none());
    let status = ExitStatus::from_raw(libc::SIGTERM);
    let event = second
        .take_event(TabId(8), || Ok(Some(status)))
        .unwrap()
        .unwrap();
    assert_eq!(
        reduce(&event),
        AppEffect::PtyExited {
            tab: TabId(8),
            status
        }
    );
}

#[test]
fn pty_exit_wait_error_can_retry_without_payload() {
    use crate::{AppEffect, reduce};
    use std::os::unix::process::ExitStatusExt;

    let mut state = ExitState::default();
    for kind in [io::ErrorKind::Interrupted, io::ErrorKind::PermissionDenied] {
        let error = state
            .take_event(TabId(9), || Err(io::Error::new(kind, "secret child text")))
            .err()
            .unwrap();
        assert_eq!(
            error,
            PtyExitError {
                tab: TabId(9),
                error: AppError::new(Component::Pty, kind)
            }
        );
        assert!(!format!("{error:?}").contains("secret"));
    }
    let status = ExitStatus::from_raw(0);
    let event = state
        .take_event(TabId(9), || Ok(Some(status)))
        .unwrap()
        .unwrap();
    assert_eq!(
        reduce(&event),
        AppEffect::PtyExited {
            tab: TabId(9),
            status
        }
    );
}

#[test]
fn pty_resize_sends_correct_dimensions() {
    let initial = PtySize {
        rows: 31,
        columns: 97,
    };
    let mut state = ResizeState { size: initial };
    let mut calls = Vec::new();
    let result = state
        .resize(
            TabId(5),
            initial,
            Some(|size| {
                calls.push(size);
                Ok(())
            }),
        )
        .unwrap();
    assert_eq!(
        result,
        PtyResizeResult {
            tab: TabId(5),
            size: initial,
            status: PtyResizeStatus::Unchanged
        }
    );
    assert!(calls.is_empty());
    for size in [
        PtySize {
            rows: 42,
            columns: 109,
        },
        PtySize {
            rows: 1,
            columns: 1,
        },
        PtySize {
            rows: u16::MAX,
            columns: u16::MAX,
        },
    ] {
        let result = state
            .resize(
                TabId(5),
                size,
                Some(|s| {
                    calls.push(s);
                    Ok(())
                }),
            )
            .unwrap();
        assert_eq!(
            result,
            PtyResizeResult {
                tab: TabId(5),
                size,
                status: PtyResizeStatus::Resized
            }
        );
        assert_eq!(state.size, size);
        assert_eq!(calls.last(), Some(&size));
        assert_eq!(
            state
                .resize(TabId(5), size, Some(|_| panic!("duplicate resize I/O")))
                .unwrap()
                .status,
            PtyResizeStatus::Unchanged
        );
    }
    assert_eq!(calls.len(), 3);
}

#[test]
fn pty_resize_invalid_and_closed_do_not_call_adapter() {
    let initial = PtySize {
        rows: 31,
        columns: 97,
    };
    let mut state = ResizeState { size: initial };
    for size in [
        PtySize {
            rows: 0,
            columns: 97,
        },
        PtySize {
            rows: 31,
            columns: 0,
        },
        PtySize {
            rows: 0,
            columns: 0,
        },
    ] {
        let error = state
            .resize(TabId(1), size, Some(|_| panic!("invalid resize I/O")))
            .unwrap_err();
        assert_eq!(
            error,
            PtyResizeError {
                tab: TabId(1),
                error: AppError::new(Component::Pty, io::ErrorKind::InvalidInput)
            }
        );
        assert_eq!(state.size, initial);
    }
    for size in [
        initial,
        PtySize {
            rows: 42,
            columns: 109,
        },
    ] {
        let error = state
            .resize::<fn(PtySize) -> io::Result<()>>(TabId(1), size, None)
            .unwrap_err();
        assert_eq!(error.error.kind(), io::ErrorKind::NotConnected);
        assert_eq!(state.size, initial);
    }
}

#[test]
fn pty_resize_failure_retry_is_isolated() {
    let initial = PtySize {
        rows: 31,
        columns: 97,
    };
    let next = PtySize {
        rows: 42,
        columns: 109,
    };
    let mut first = ResizeState { size: initial };
    let mut second = ResizeState { size: initial };
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Interrupted] {
        let mut calls = 0;
        let error = first
            .resize(
                TabId(1),
                next,
                Some(|size| {
                    calls += 1;
                    assert_eq!(size, next);
                    Err(io::Error::new(kind, "secret metadata"))
                }),
            )
            .unwrap_err();
        assert_eq!(
            error,
            PtyResizeError {
                tab: TabId(1),
                error: AppError::new(Component::Pty, kind)
            }
        );
        assert!(!format!("{error:?}").contains("secret"));
        assert_eq!(calls, 1, "no unbounded retry loop");
        assert_eq!(first.size, initial);
        assert_eq!(second.size, initial);
    }
    assert_eq!(
        first
            .resize(TabId(1), next, Some(|_| Ok(())))
            .unwrap()
            .status,
        PtyResizeStatus::Resized
    );
    assert_eq!(first.size, next);
    assert_eq!(
        second
            .resize(TabId(2), initial, Some(|_| panic!("unrelated session I/O")))
            .unwrap()
            .status,
        PtyResizeStatus::Unchanged
    );
    let other = PtySize {
        rows: 17,
        columns: 53,
    };
    assert_eq!(
        second
            .resize(TabId(2), other, Some(|_| Ok(())))
            .unwrap()
            .tab,
        TabId(2)
    );
    assert_eq!(second.size, other);
    assert_eq!(first.size, next);
}

enum ReadStep {
    Bytes(Vec<u8>),
    Error(io::Error),
    Eof,
}

struct FakeReader {
    steps: std::collections::VecDeque<ReadStep>,
    calls: usize,
    largest_buffer: usize,
}

impl FakeReader {
    fn new(steps: impl IntoIterator<Item = ReadStep>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            calls: 0,
            largest_buffer: 0,
        }
    }
}

impl io::Read for FakeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        self.largest_buffer = self.largest_buffer.max(buffer.len());
        match self
            .steps
            .pop_front()
            .expect("unexpected read after script ended")
        {
            ReadStep::Bytes(bytes) => {
                let n = bytes.len().min(buffer.len());
                buffer[..n].copy_from_slice(&bytes[..n]);
                if n < bytes.len() {
                    self.steps.push_front(ReadStep::Bytes(bytes[n..].to_vec()));
                }
                Ok(n)
            }
            ReadStep::Error(error) => Err(error),
            ReadStep::Eof => Ok(0),
        }
    }
}

fn interrupted() -> ReadStep {
    ReadStep::Error(io::Error::from(io::ErrorKind::Interrupted))
}
fn would_block() -> ReadStep {
    ReadStep::Error(io::Error::from(io::ErrorKind::WouldBlock))
}

#[test]
fn pty_read_preserves_byte_order() {
    let payload = b"a\0\xe7\xb9\x81\xe9\xab\x94\x1b[31m\xff\n  z";
    for chunk_size in [1, 2, 5, 64] {
        let mut steps = Vec::new();
        for fragment in payload.chunks(3) {
            steps.extend([
                interrupted(),
                ReadStep::Bytes(fragment.to_vec()),
                would_block(),
            ]);
        }
        steps.push(ReadStep::Eof);
        let mut reader = FakeReader::new(steps);
        let mut state = ReadState::default();
        let mut output = Vec::new();
        let mut buffer = vec![0xa5; chunk_size];
        for _ in 0..100 {
            buffer.fill(0xa5);
            let result = state
                .read(TabId(12), Some(&mut reader), &mut buffer)
                .unwrap();
            assert_eq!(result.tab, TabId(12));
            match result.status {
                PtyReadStatus::Data(n) => {
                    assert!(n > 0 && n <= buffer.len());
                    output.extend_from_slice(&buffer[..n]);
                    assert!(buffer[n..].iter().all(|b| *b == 0xa5));
                }
                PtyReadStatus::WouldBlock => assert!(!state.eof),
                PtyReadStatus::Eof => break,
                other => panic!("unexpected status: {other:?}"),
            }
        }
        assert!(state.eof);
        assert_eq!(output, payload);
        let calls = reader.calls;
        assert_eq!(
            state
                .read(TabId(12), Some(&mut reader), &mut buffer)
                .unwrap()
                .status,
            PtyReadStatus::Eof
        );
        assert_eq!(reader.calls, calls, "EOF is sticky without more I/O");
    }
}

#[test]
fn pty_read_work_and_buffer_are_bounded() {
    let mut state = ReadState::default();
    let mut reader = FakeReader::new((0..MAX_PTY_READ_ATTEMPTS + 1).map(|_| interrupted()).chain(
        [
            ReadStep::Bytes(vec![7; MAX_PTY_READ_BYTES + 3]),
            ReadStep::Eof,
        ],
    ));
    let mut buffer = vec![0xa5; MAX_PTY_READ_BYTES + 100];
    assert_eq!(
        state
            .read(TabId(1), Some(&mut reader), &mut buffer)
            .unwrap()
            .status,
        PtyReadStatus::Interrupted
    );
    assert_eq!(reader.calls, MAX_PTY_READ_ATTEMPTS);
    assert!(!state.eof);
    assert_eq!(
        state
            .read(TabId(1), Some(&mut reader), &mut buffer)
            .unwrap()
            .status,
        PtyReadStatus::Data(MAX_PTY_READ_BYTES)
    );
    assert_eq!(reader.largest_buffer, MAX_PTY_READ_BYTES);
    assert!(buffer[MAX_PTY_READ_BYTES..].iter().all(|b| *b == 0xa5));
    assert_eq!(
        state
            .read(TabId(1), Some(&mut reader), &mut buffer)
            .unwrap()
            .status,
        PtyReadStatus::Data(3)
    );
    assert_eq!(reader.calls, MAX_PTY_READ_ATTEMPTS + 3);
    let calls = reader.calls;
    let error = state
        .read(TabId(1), Some(&mut reader), &mut [])
        .unwrap_err();
    assert_eq!(
        error,
        PtyReadError {
            tab: TabId(1),
            error: AppError::new(Component::Pty, io::ErrorKind::InvalidInput)
        }
    );
    assert_eq!(
        reader.calls, calls,
        "zero-sized buffer cannot cause false EOF"
    );
    assert!(!state.eof);
}

#[test]
fn pty_read_eof_and_errors_are_per_session() {
    let mut first_state = ReadState::default();
    let mut second_state = ReadState::default();
    let mut first = FakeReader::new([
        would_block(),
        ReadStep::Error(io::Error::from_raw_os_error(libc::EIO)),
    ]);
    let mut second = FakeReader::new([
        ReadStep::Error(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "secret PTY payload",
        )),
        ReadStep::Bytes(b"second".to_vec()),
        ReadStep::Eof,
    ]);
    let mut buffer = [0; 64];
    assert_eq!(
        first_state
            .read(TabId(1), Some(&mut first), &mut buffer)
            .unwrap()
            .status,
        PtyReadStatus::WouldBlock
    );
    let error = second_state
        .read(TabId(2), Some(&mut second), &mut buffer)
        .unwrap_err();
    assert_eq!(
        error,
        PtyReadError {
            tab: TabId(2),
            error: AppError::new(Component::Pty, io::ErrorKind::PermissionDenied)
        }
    );
    assert!(!format!("{error:?}").contains("secret"));
    assert!(!first_state.eof && !second_state.eof);
    assert_eq!(
        first_state
            .read(TabId(1), Some(&mut first), &mut buffer)
            .unwrap()
            .status,
        PtyReadStatus::Eof
    );
    let result = second_state
        .read(TabId(2), Some(&mut second), &mut buffer)
        .unwrap();
    assert_eq!(
        result,
        PtyReadResult {
            tab: TabId(2),
            status: PtyReadStatus::Data(6)
        }
    );
    assert_eq!(&buffer[..6], b"second");
    assert!(!second_state.eof);
    let calls = second.calls;
    assert_eq!(
        second_state
            .read::<FakeReader>(TabId(2), None, &mut buffer)
            .unwrap()
            .status,
        PtyReadStatus::Eof
    );
    assert_eq!(
        second_state
            .read(TabId(2), Some(&mut second), &mut buffer)
            .unwrap()
            .status,
        PtyReadStatus::Eof
    );
    assert_eq!(
        second.calls, calls,
        "closed session cannot resume a stale stream"
    );
}

struct Resource(Rc<Cell<usize>>);
impl Resource {
    fn new(count: &Rc<Cell<usize>>) -> Self {
        count.set(count.get() + 1);
        Self(count.clone())
    }
}
impl Drop for Resource {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

#[derive(Default)]
struct FakeBackend {
    resources: Rc<Cell<usize>>,
    calls: usize,
    fail: Option<SpawnStage>,
}
impl SpawnBackend for FakeBackend {
    type Master = Resource;
    type Slave = Resource;
    type Child = Resource;

    fn open(&mut self, size: PtySize) -> io::Result<(Resource, Resource)> {
        self.calls += 1;
        assert_eq!(
            size,
            PtySize {
                rows: 24,
                columns: 80
            }
        );
        let master = Resource::new(&self.resources);
        if self.fail == Some(SpawnStage::Open) {
            return Err(io::Error::other("secret path"));
        }
        Ok((master, Resource::new(&self.resources)))
    }
    fn spawn(&mut self, command: &mut Command, _slave: Resource) -> io::Result<Resource> {
        self.calls += 1;
        assert_eq!(command.get_program(), OsStr::new("/bin/sh"));
        assert_eq!(command.get_current_dir(), Some(Path::new("/tmp")));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("literal ; $() `x` 繁體")]
        );
        assert!(
            command
                .get_envs()
                .any(|(k, v)| k == "TERM" && v == Some(OsStr::new("ptyra")))
        );
        assert!(
            command
                .get_envs()
                .any(|(k, v)| k == "COLORTERM" && v.is_none())
        );
        if self.fail == Some(SpawnStage::Spawn) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "secret argv",
            ));
        }
        Ok(Resource::new(&self.resources))
    }
}

fn request(args: &[OsString]) -> PtySpawnRequest<'_> {
    PtySpawnRequest {
        tab: TabId(7),
        program: OsStr::new("/bin/sh"),
        args,
        cwd: Path::new("/tmp"),
        size: PtySize {
            rows: 24,
            columns: 80,
        },
    }
}

#[test]
fn pty_spawn_failure_is_isolated() {
    let args = [OsString::from("literal ; $() `x` 繁體")];
    let mut backend = FakeBackend::default();
    let first = spawn_with_backend(request(&args), &mut backend).expect("spawn success");
    assert_eq!(first.tab, TabId(7));
    assert_eq!(backend.resources.get(), 2, "parent slave must be closed");
    for (stage, kind) in [
        (SpawnStage::Open, io::ErrorKind::Other),
        (SpawnStage::Spawn, io::ErrorKind::PermissionDenied),
    ] {
        backend.fail = Some(stage);
        let mut second = request(&args);
        second.tab = TabId(8);
        let error = spawn_with_backend(second, &mut backend)
            .err()
            .expect("injected failure");
        assert_eq!(
            error,
            PtySpawnError {
                tab: TabId(8),
                stage,
                error: AppError::new(Component::Pty, kind)
            }
        );
        assert!(!format!("{error:?}").contains("secret"));
        assert_eq!(
            backend.resources.get(),
            2,
            "failure cannot leak or close first session"
        );
    }
    backend.fail = None;
    let second = spawn_with_backend(request(&args), &mut backend)
        .ok()
        .unwrap();
    assert_eq!(backend.resources.get(), 4);
    drop(first);
    assert_eq!(backend.resources.get(), 2);
    drop(second);
    assert_eq!(backend.resources.get(), 0);
}

#[test]
fn pty_spawn_rejects_invalid_request_before_opening() {
    let mut backend = FakeBackend::default();
    let nul_cwd = Path::new(OsStr::from_bytes(b"/tmp/bad\0cwd"));
    let long_cwd = "/".repeat(MAX_CWD_BYTES + 1);
    for i in 0..9 {
        let mut r = request(&[]);
        match i {
            0 => r.size.rows = 0,
            1 => r.size.columns = 0,
            2 => r.cwd = Path::new("relative"),
            3 => r.cwd = nul_cwd,
            4 => r.cwd = Path::new(&long_cwd),
            5 => r.program = OsStr::new("relative"),
            6 => r.program = OsStr::new(""),
            7 => r.program = OsStr::from_bytes(b"/bad\0program"),
            _ => r.cwd = Path::new(""),
        }
        let error = spawn_with_backend(r, &mut backend).err().unwrap();
        assert_eq!(error.stage, SpawnStage::Validate);
        assert_eq!(error.error.kind(), io::ErrorKind::InvalidInput);
    }
    let too_many = vec![OsString::new(); crate::shell::MAX_ARGS + 1];
    assert_eq!(
        spawn_with_backend(request(&too_many), &mut backend)
            .err()
            .unwrap()
            .stage,
        SpawnStage::Validate
    );
    assert_eq!(backend.calls, 0);
    assert_eq!(backend.resources.get(), 0);
}

#[test]
fn runtime_bad_stream_does_not_stop_other_tabs_owned_io() {
    use crate::tabs::{SessionFailure, TabRegistry};
    for operation in ["read", "write", "resize"] {
        let mut tabs = TabRegistry::with_scrollback(0).unwrap();
        for id in [TabId(1), TabId(2)] {
            tabs.insert(id, 2, 40).unwrap();
            let pty = PtySession::spawn(PtySpawnRequest {
                tab: id,
                program: OsStr::new("/bin/sleep"),
                args: &["60".into()],
                cwd: Path::new("/tmp"),
                size: PtySize {
                    rows: 2,
                    columns: 40,
                },
            })
            .unwrap();
            tabs.attach_pty(pty).ok().unwrap();
        }
        let good = tabs.get(TabId(2)).unwrap().pty().unwrap();
        let good_pid = good.child_id();
        let good_fd = std::fs::read_link(format!(
            "/proc/self/fd/{}",
            good.master_fd().unwrap().as_raw_fd()
        ))
        .unwrap();
        let bad = tabs.get_mut(TabId(1)).unwrap().pty_mut().unwrap();
        let bad_pid = bad.child_id();
        // Keep the real PTY master alive until after this dispatch, so the child
        // remains owned; inject an OS descriptor with deterministic I/O failure.
        let original = bad.master.take().unwrap();
        let (read_end, write_end) = nix::unistd::pipe().unwrap();
        let (fault, peer) = if operation == "read" {
            (write_end, read_end)
        } else {
            (read_end, write_end)
        };
        bad.master = Some(File::from(fault));
        let bad_fd = format!("/proc/self/fd/{}", bad.master_fd().unwrap().as_raw_fd());
        let bad_link = std::fs::read_link(&bad_fd).unwrap();
        bad.enqueue_write(b"pending-private-input").unwrap();
        match operation {
            "read" => {
                let mut result = None;
                tabs.drain_ready(&[TabId(1)], &mut [0; MAX_PTY_READ_BYTES], |_, read| {
                    result = Some(read)
                })
                .unwrap();
                assert!(result.unwrap().is_err());
            }
            "write" => {
                assert!(bad.flush_write().is_err());
                assert_eq!(bad.pending_write_bytes(), b"pending-private-input".len());
                tabs.stop_session(TabId(1), SessionFailure::Io);
            }
            "resize" => {
                assert!(
                    bad.resize(PtySize {
                        rows: 3,
                        columns: 40
                    })
                    .is_err()
                );
                tabs.stop_session(TabId(1), SessionFailure::Io);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            tabs.get(TabId(1)).unwrap().failure(),
            Some(SessionFailure::Io)
        );
        assert_ne!(std::fs::read_link(&bad_fd).ok(), Some(bad_link));
        drop(peer);
        assert!(
            !Path::new(&format!("/proc/{bad_pid}")).exists(),
            "failed direct child was not reaped"
        );
        drop(original);
        assert_eq!(
            std::fs::read_link(format!(
                "/proc/self/fd/{}",
                tabs.get(TabId(2))
                    .unwrap()
                    .pty()
                    .unwrap()
                    .master_fd()
                    .unwrap()
                    .as_raw_fd()
            ))
            .unwrap(),
            good_fd
        );
        assert!(Path::new(&format!("/proc/{good_pid}")).exists());
        let good = tabs.get_mut(TabId(2)).unwrap().pty_mut().unwrap();
        good.enqueue_write(b"healthy").unwrap();
        assert!(matches!(
            good.flush_write().unwrap().status,
            PtyWriteStatus::Written(7)
        ));
        assert!(
            !tabs.stop_session(TabId(1), SessionFailure::Stream),
            "stale failure is inert"
        );
        tabs.close(TabId(1));
        tabs.close(TabId(2));
        assert_eq!(tabs.reserved_cells(), 0);
        assert_eq!(tabs.allocated_cells(), 0);
        assert!(!Path::new(&format!("/proc/{good_pid}")).exists());
    }
}
