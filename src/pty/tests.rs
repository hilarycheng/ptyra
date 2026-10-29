use super::*;
use std::{cell::Cell, os::unix::ffi::OsStrExt, rc::Rc};

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
