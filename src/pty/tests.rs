use super::*;
use std::{cell::Cell, os::unix::ffi::OsStrExt, rc::Rc};

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
                .any(|(k, v)| k == "TERM" && v == Some(OsStr::new("dumb")))
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
