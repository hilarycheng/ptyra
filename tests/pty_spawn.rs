#![cfg(target_os = "linux")]

use nix::{
    errno::Errno,
    fcntl::{FcntlArg, FdFlag, OFlag, fcntl},
    sys::wait::{WaitPidFlag, waitpid},
    unistd::{Pid, read},
};
use ptyra::{
    TabId,
    pty::{PtyReadResult, PtyReadStatus, PtySession, PtySize, PtySpawnRequest, SpawnStage},
};
use std::{
    ffi::{OsStr, OsString},
    fs, io,
    os::unix::ffi::OsStringExt,
    path::Path,
    process::ExitStatus,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

// Serialize process-FD accounting; test children inherit the runner's CPU affinity.
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());
const DEADLINE: Duration = Duration::from_secs(5);

fn request(args: &[OsString]) -> PtySpawnRequest<'_> {
    PtySpawnRequest {
        tab: TabId(42),
        program: OsStr::new("/bin/sh"),
        args,
        cwd: Path::new("/tmp"),
        size: PtySize {
            rows: 31,
            columns: 97,
        },
    }
}

fn spawn_script(script: &str) -> PtySession {
    let args = [
        OsString::from("-c"),
        OsString::from(script),
        OsString::from("ptyra-fixture"),
    ];
    PtySession::spawn(request(&args)).expect("Linux PTY fixture spawn")
}

fn collect(session: &mut PtySession) -> (Vec<u8>, ExitStatus) {
    let start = Instant::now();
    let mut output = Vec::new();
    let mut status = None;
    loop {
        let mut buffer = [0; 1024];
        let eof = match read(session.master_fd().unwrap(), &mut buffer) {
            Ok(0) | Err(Errno::EIO) => true, // Linux PTY slave closure.
            Ok(n) => {
                output.extend_from_slice(&buffer[..n]);
                false
            }
            Err(Errno::EAGAIN) | Err(Errno::EINTR) => false,
            Err(e) => panic!("fixture read failed: {e}"),
        };
        assert!(output.len() <= 16 * 1024, "fixture output bound");
        if status.is_none() {
            status = session.try_wait().unwrap();
        }
        if eof && let Some(status) = status {
            return (output, status);
        }
        if start.elapsed() >= DEADLINE {
            session.terminate().unwrap();
            panic!("PTY fixture exceeded five seconds");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_ready(session: &mut PtySession) {
    let start = Instant::now();
    loop {
        let mut byte = [0];
        match read(session.master_fd().unwrap(), &mut byte) {
            Ok(1) => {
                assert_eq!(byte, [b'R']);
                return;
            }
            Err(Errno::EAGAIN) | Err(Errno::EINTR) => {}
            other => panic!("fixture readiness failed: {other:?}"),
        }
        if start.elapsed() >= DEADLINE {
            session.terminate().unwrap();
            panic!("PTY readiness exceeded five seconds");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn pty_fd_count() -> usize {
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
        .filter(|path| path.starts_with("/dev/pts") || path == Path::new("/dev/ptmx"))
        .count()
}

#[test]
fn linux_pty_preserves_spawn_contract() {
    let _lock = FIXTURE_LOCK.lock().unwrap();
    // Fixed trusted fixture; all test values are separate positional arguments.
    let script = r#"set -eu
        [ -t 0 ] && [ -t 1 ] && [ -t 2 ]
        : </dev/tty
        [ "$TERM" = dumb ] && [ "${COLORTERM+x}" != x ]
        [ "$PWD" = /tmp ] && [ "$#" = 3 ]
        [ "$(/usr/bin/stty size)" = '31 97' ]
        /usr/bin/stty -onlcr
        printf 'OK|%s|%s|%s' "$1" "$2" "$3"
    "#;
    let literal = "spaces ; $(echo BAD) `echo BAD`\n繁體中文";
    let raw = vec![0xff, b'x'];
    let args = [
        OsString::from("-c"),
        OsString::from(script),
        OsString::from("ptyra-fixture"),
        OsString::from(literal),
        OsString::new(),
        OsString::from_vec(raw.clone()),
    ];
    let baseline = pty_fd_count();
    let mut session = PtySession::spawn(request(&args)).expect("PTY spawn");
    assert_eq!(session.tab(), TabId(42));
    assert_eq!(
        pty_fd_count(),
        baseline + 1,
        "only the master remains in parent"
    );
    let fd = session.master_fd().unwrap();
    assert!(
        FdFlag::from_bits_retain(fcntl(fd, FcntlArg::F_GETFD).unwrap())
            .contains(FdFlag::FD_CLOEXEC)
    );
    assert!(
        OFlag::from_bits_retain(fcntl(fd, FcntlArg::F_GETFL).unwrap()).contains(OFlag::O_NONBLOCK)
    );
    let (output, status) = collect(&mut session);
    assert!(status.success(), "fixture rejected PTY/env/cwd/size");
    let mut expected = format!("OK|{literal}||").into_bytes();
    expected.extend(raw);
    assert_eq!(output, expected);
    let pid = Pid::from_raw(session.child_id().try_into().unwrap());
    session.terminate().unwrap(); // Repeated cleanup must not signal a reused PID.
    session.terminate().unwrap();
    drop(session);
    assert_eq!(waitpid(pid, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD));
    assert_eq!(pty_fd_count(), baseline);
}

#[test]
fn linux_pty_spawn_failure_leaves_other_child_alive() {
    let _lock = FIXTURE_LOCK.lock().unwrap();
    let baseline = pty_fd_count();
    let mut first = spawn_script("printf R; read line");
    wait_for_ready(&mut first);
    for i in 0..12 {
        let mut r = request(&[]);
        r.tab = TabId(99);
        match i % 3 {
            0 => r.program = OsStr::new("/proc/self/ptyra-no-such-program"),
            1 => r.cwd = Path::new("/proc/self/ptyra-no-such-directory"),
            _ => r.program = OsStr::new("/"),
        }
        let error = PtySession::spawn(r)
            .err()
            .expect("invalid executable or cwd must fail");
        assert_eq!(error.tab, TabId(99));
        assert_eq!(error.stage, SpawnStage::Spawn);
        assert!(matches!(
            error.error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
        ));
        assert_eq!(first.try_wait().unwrap(), None);
        assert_eq!(
            pty_fd_count(),
            baseline + 1,
            "failed spawn leaked PTY descriptors"
        );
    }
    let pid = Pid::from_raw(first.child_id().try_into().unwrap());
    first.terminate().unwrap();
    assert!(first.master_fd().is_none());
    assert!(first.try_wait().unwrap().is_some());
    assert_eq!(waitpid(pid, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD));
    drop(first);
    assert_eq!(pty_fd_count(), baseline);
}

#[test]
fn linux_pty_drop_reaps_only_owned_child() {
    let _lock = FIXTURE_LOCK.lock().unwrap();
    let baseline = pty_fd_count();
    let mut first = spawn_script("printf R; read line");
    let mut second = spawn_script("printf R; read line");
    wait_for_ready(&mut first);
    wait_for_ready(&mut second);
    let pid = Pid::from_raw(first.child_id().try_into().unwrap());
    // A session leader owns the controlling tty and is its foreground group.
    let stat = fs::read_to_string(format!("/proc/{}/stat", first.child_id())).unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(") ")
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    assert_eq!(fields[2], first.child_id().to_string()); // pgrp (field 5)
    assert_eq!(fields[3], first.child_id().to_string()); // session (field 6)
    assert_ne!(fields[4], "0"); // tty_nr
    assert_eq!(fields[5], first.child_id().to_string()); // foreground pgrp
    drop(first);
    assert_eq!(waitpid(pid, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD));
    assert_eq!(second.try_wait().unwrap(), None);
    assert_eq!(pty_fd_count(), baseline + 1);
    second.terminate().unwrap();
    drop(second);
    assert_eq!(pty_fd_count(), baseline);
}

#[test]
fn linux_pty_read_preserves_bytes_after_child_exit() {
    let _lock = FIXTURE_LOCK.lock().unwrap();
    let baseline = pty_fd_count();
    // Fixed output is small enough to finish before the parent starts draining.
    // Disable only this fixture PTY's output processing so LF stays literal LF.
    let mut session = spawn_script(
        "set -eu; /usr/bin/stty -opost; printf '\\000\\377'; printf '\\347\\271\\201\\033[31m\\n  tail'",
    );
    let start = Instant::now();
    loop {
        if let Some(status) = session.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() >= DEADLINE {
            session.terminate().unwrap();
            panic!("read fixture child exceeded five seconds");
        }
        thread::sleep(Duration::from_millis(5));
    }
    let mut output = Vec::new();
    let mut buffer = [0; 2]; // Forces UTF-8 and ANSI sequences across chunks.
    let mut eof = false;
    for _ in 0..100 {
        let result = session.read_chunk(&mut buffer).unwrap();
        assert_eq!(result.tab, TabId(42));
        match result.status {
            PtyReadStatus::Data(n) => output.extend_from_slice(&buffer[..n]),
            PtyReadStatus::Eof => {
                eof = true;
                break;
            }
            PtyReadStatus::WouldBlock | PtyReadStatus::Interrupted => {
                thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(output.len() <= 1024);
    }
    assert!(eof);
    assert_eq!(output, b"\0\xff\xe7\xb9\x81\x1b[31m\n  tail");
    assert_eq!(
        session.read_chunk(&mut buffer).unwrap().status,
        PtyReadStatus::Eof
    );
    assert!(
        session.master_fd().is_some(),
        "read EOF does not close ownership"
    );
    drop(session);
    assert_eq!(pty_fd_count(), baseline);
}

#[test]
fn linux_pty_read_waits_and_isolates_sessions() {
    let _lock = FIXTURE_LOCK.lock().unwrap();
    let baseline = pty_fd_count();
    let mut first = spawn_script("printf R; read line");
    wait_for_ready(&mut first); // All emitted bytes consumed; child is waiting.
    let args = [
        OsString::from("-c"),
        OsString::from("printf second"),
        OsString::from("ptyra-fixture"),
    ];
    let mut second_request = request(&args);
    second_request.tab = TabId(43);
    let mut second = PtySession::spawn(second_request).unwrap();
    let mut buffer = [0; 3];
    assert_eq!(
        first.read_chunk(&mut buffer).unwrap(),
        PtyReadResult {
            tab: TabId(42),
            status: PtyReadStatus::WouldBlock
        }
    );
    assert!(
        first.try_wait().unwrap().is_none(),
        "empty read is not child exit"
    );
    first.terminate().unwrap();
    assert_eq!(
        first.read_chunk(&mut buffer).unwrap().status,
        PtyReadStatus::Eof
    );
    let start = Instant::now();
    let mut output = Vec::new();
    loop {
        let result = second.read_chunk(&mut buffer).unwrap();
        assert_eq!(result.tab, TabId(43));
        match result.status {
            PtyReadStatus::Data(n) => output.extend_from_slice(&buffer[..n]),
            PtyReadStatus::Eof => break,
            PtyReadStatus::WouldBlock | PtyReadStatus::Interrupted => {
                thread::sleep(Duration::from_millis(5));
            }
        }
        assert!(output.len() <= 1024);
        if start.elapsed() >= DEADLINE {
            second.terminate().unwrap();
            panic!("isolated read fixture exceeded five seconds");
        }
    }
    assert_eq!(output, b"second");
    drop(first);
    drop(second);
    assert_eq!(pty_fd_count(), baseline);
}
