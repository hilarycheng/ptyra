//! Linux PTY ownership, bounded I/O and Tab-scoped child exit events.
use crate::{
    AppEvent, TabId,
    error::{AppError, Component},
    shell::{apply_terminal_environment, shell_command},
};
use nix::{
    fcntl::{OFlag, open},
    libc,
    pty::{grantpt, posix_openpt, ptsname_r, unlockpt},
    sys::{
        signal::{self, SaFlags, SigAction, SigHandler, SigSet, SigmaskHow, Signal},
        stat::Mode,
    },
    unistd::setsid,
};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    os::{
        fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
        unix::{ffi::OsStrExt, process::CommandExt},
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
};

mod terminfo;
mod write;
use write::WriteQueue;
pub use write::{
    MAX_PTY_PENDING_BYTES, MAX_PTY_WRITE_ATTEMPTS, MAX_PTY_WRITE_BYTES, PtyEnqueueResult,
    PtyWriteError, PtyWriteFailure, PtyWriteResult, PtyWriteStatus,
};

pub const MAX_CWD_BYTES: usize = 4096;
/// Maximum bytes returned by one read, even with a larger caller buffer.
pub const MAX_PTY_READ_BYTES: usize = 16 * 1024;
/// Interrupted syscalls yield back to the caller after this many attempts.
pub const MAX_PTY_READ_ATTEMPTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyReadStatus {
    /// Only buffer[..length] contains valid new bytes; no decoding is performed.
    Data(usize),
    /// Wait for readability; do not busy-poll.
    WouldBlock,
    /// Retry budget exhausted; reschedule without a tight retry loop.
    Interrupted,
    /// Permanent end of this read stream, independent of child exit status.
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyReadResult {
    pub tab: TabId,
    pub status: PtyReadStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyReadError {
    pub tab: TabId,
    pub error: AppError,
}

#[derive(Default)]
struct ReadState {
    eof: bool,
}

impl ReadState {
    fn read<R: io::Read>(
        &mut self,
        tab: TabId,
        reader: Option<&mut R>,
        buffer: &mut [u8],
    ) -> Result<PtyReadResult, PtyReadError> {
        let result = |status| Ok(PtyReadResult { tab, status });
        let error = |error| PtyReadError {
            tab,
            error: AppError::from_io(Component::Pty, error),
        };
        if buffer.is_empty() {
            return Err(error(io::ErrorKind::InvalidInput.into()));
        }
        if self.eof || reader.is_none() {
            self.eof = true;
            return result(PtyReadStatus::Eof);
        }
        let reader = reader.expect("checked reader presence");
        let limit = buffer.len().min(MAX_PTY_READ_BYTES);
        for _ in 0..MAX_PTY_READ_ATTEMPTS {
            match reader.read(&mut buffer[..limit]) {
                Ok(0) => {
                    self.eof = true;
                    return result(PtyReadStatus::Eof);
                }
                Ok(n) if n <= limit => return result(PtyReadStatus::Data(n)),
                Ok(_) => return Err(error(io::ErrorKind::InvalidData.into())),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return result(PtyReadStatus::WouldBlock);
                }
                // This helper is private to the Linux PTY adapter: EIO on its
                // master after slave closure is EOF, not a general I/O policy.
                Err(e) if e.raw_os_error() == Some(libc::EIO) => {
                    self.eof = true;
                    return result(PtyReadStatus::Eof);
                }
                Err(e) => return Err(error(e)),
            }
        }
        result(PtyReadStatus::Interrupted)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtySize {
    pub rows: u16,
    pub columns: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyResizeStatus {
    Unchanged,
    Resized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyResizeResult {
    pub tab: TabId,
    pub size: PtySize,
    pub status: PtyResizeStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyResizeError {
    pub tab: TabId,
    pub error: AppError,
}

struct ResizeState {
    size: PtySize,
}
impl ResizeState {
    fn resize<F: FnOnce(PtySize) -> io::Result<()>>(
        &mut self,
        tab: TabId,
        size: PtySize,
        apply: Option<F>,
    ) -> Result<PtyResizeResult, PtyResizeError> {
        let error = |e| PtyResizeError {
            tab,
            error: AppError::from_io(Component::Pty, e),
        };
        if size.rows == 0 || size.columns == 0 {
            return Err(error(io::ErrorKind::InvalidInput.into()));
        }
        let Some(apply) = apply else {
            return Err(error(io::ErrorKind::NotConnected.into()));
        };
        if self.size == size {
            return Ok(PtyResizeResult {
                tab,
                size,
                status: PtyResizeStatus::Unchanged,
            });
        }
        apply(size).map_err(error)?;
        self.size = size;
        Ok(PtyResizeResult {
            tab,
            size,
            status: PtyResizeStatus::Resized,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtyExitError {
    pub tab: TabId,
    pub error: AppError,
}

#[derive(Default)]
struct ExitState {
    delivered: bool,
}

impl ExitState {
    fn take_event(
        &mut self,
        tab: TabId,
        wait: impl FnOnce() -> io::Result<Option<ExitStatus>>,
    ) -> Result<Option<AppEvent>, PtyExitError> {
        if self.delivered {
            return Ok(None);
        }
        let status = wait().map_err(|e| PtyExitError {
            tab,
            error: AppError::from_io(Component::Pty, e),
        })?;
        Ok(status.map(|status| {
            self.delivered = true;
            AppEvent::PtyExited { tab, status }
        }))
    }
}

/// Absolute program and cwd are explicit; no shell lookup or cwd fallback.
/// Deliberately has no Debug implementation: arguments may contain secrets.
pub struct PtySpawnRequest<'a> {
    pub tab: TabId,
    pub program: &'a OsStr,
    pub args: &'a [OsString],
    pub cwd: &'a Path,
    pub size: PtySize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnStage {
    Validate,
    Open,
    Spawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtySpawnError {
    pub tab: TabId,
    pub stage: SpawnStage,
    pub error: AppError,
}

trait SpawnBackend {
    type Master;
    type Slave;
    type Child;
    fn open(&mut self, size: PtySize) -> io::Result<(Self::Master, Self::Slave)>;
    fn spawn(&mut self, command: &mut Command, slave: Self::Slave) -> io::Result<Self::Child>;
}

struct Spawned<M, C> {
    tab: TabId,
    master: M,
    child: C,
}

fn spawn_with_backend<B: SpawnBackend>(
    request: PtySpawnRequest<'_>,
    backend: &mut B,
) -> Result<Spawned<B::Master, B::Child>, PtySpawnError> {
    let error = |stage, error| PtySpawnError {
        tab: request.tab,
        stage,
        error,
    };
    let cwd = request.cwd.as_os_str().as_bytes();
    if request.size.rows == 0
        || request.size.columns == 0
        || !Path::new(request.program).is_absolute()
        || !request.cwd.is_absolute()
        || cwd.len() > MAX_CWD_BYTES
        || cwd.contains(&0)
    {
        return Err(error(
            SpawnStage::Validate,
            AppError::new(Component::Pty, io::ErrorKind::InvalidInput),
        ));
    }
    let mut command =
        shell_command(request.program, request.args).map_err(|e| error(SpawnStage::Validate, e))?;
    command.current_dir(request.cwd);
    apply_terminal_environment(&mut command);
    let (master, slave) = backend
        .open(request.size)
        .map_err(|e| error(SpawnStage::Open, AppError::from_io(Component::Pty, e)))?;
    let child = backend
        .spawn(&mut command, slave)
        .map_err(|e| error(SpawnStage::Spawn, AppError::from_io(Component::Pty, e)))?;
    // Command and all parent slave duplicates are dropped before returning.
    Ok(Spawned {
        tab: request.tab,
        master,
        child,
    })
}

/// Owns exactly one Tab's master and direct child. No payload Debug/logging.
/// Drop closes the PTY, kills a still-running direct child, then reaps it.
/// Explicit termination reports errors; this is not a descendant-process sandbox.
pub struct PtySession {
    tab: TabId,
    master: Option<File>,
    child: OwnedChild,
    read_state: ReadState,
    write_queue: WriteQueue,
    resize_state: ResizeState,
    exit_state: ExitState,
    _terminfo: terminfo::Terminfo,
}

impl PtySession {
    pub fn spawn(request: PtySpawnRequest<'_>) -> Result<Self, PtySpawnError> {
        let size = request.size;
        let terminfo = terminfo::Terminfo::new().map_err(|error| PtySpawnError {
            tab: request.tab,
            stage: SpawnStage::Open,
            error: AppError::from_io(Component::Pty, error),
        })?;
        let spawned = spawn_with_backend(
            request,
            &mut LinuxBackend {
                terminfo: terminfo.path(),
            },
        )?;
        Ok(Self {
            tab: spawned.tab,
            master: Some(spawned.master),
            child: spawned.child,
            read_state: ReadState::default(),
            write_queue: WriteQueue::default(),
            resize_state: ResizeState { size },
            _terminfo: terminfo,
            exit_state: ExitState::default(),
        })
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }
    pub fn child_id(&self) -> u32 {
        self.child.0.id()
    }

    /// Last successfully applied row/column size, including initial spawn size.
    pub fn current_size(&self) -> PtySize {
        self.resize_state.size
    }

    /// Applies a valid changed size to this session's owned master, at most once.
    /// Errors leave the cached size unchanged; callers may retry explicitly.
    /// A terminated session returns NotConnected, even for an unchanged size.
    /// Pixel-to-cell calculation and GUI resize routing belong to later tasks.
    pub fn resize(&mut self, size: PtySize) -> Result<PtyResizeResult, PtyResizeError> {
        self.resize_state.resize(
            self.tab,
            size,
            self.master
                .as_ref()
                .map(|master| move |size| set_pty_size(master.as_fd(), size)),
        )
    }

    /// Borrowed, nonblocking master; no ownership transfer or stream buffering.
    pub fn master_fd(&self) -> Option<BorrowedFd<'_>> {
        self.master.as_ref().map(AsFd::as_fd)
    }

    /// Reads at most 16 KiB into the caller's nonempty buffer without allocation.
    /// Returns after the first successful partial read, or at most four EINTRs.
    /// EOF is latched only on read closure, never merely because a child exited:
    /// callers can drain remaining bytes after try_wait reports exit. EOF does
    /// not close the master, kill the child, or emit a Tab event.
    /// Do not consume this session's stream concurrently through master_fd.
    pub fn read_chunk(&mut self, buffer: &mut [u8]) -> Result<PtyReadResult, PtyReadError> {
        self.read_state.read(self.tab, self.master.as_mut(), buffer)
    }

    /// Copies literal bytes into this session's bounded queue, without I/O.
    /// QueueFull rejects the entire input; the caller retains responsibility for
    /// rejected bytes. No encoding, newline/paste processing, or logging occurs.
    pub fn enqueue_write(&mut self, bytes: &[u8]) -> Result<PtyEnqueueResult, PtyWriteError> {
        self.write_queue
            .enqueue(self.tab, self.master.is_some(), bytes)
    }

    /// Unwritten bytes still owned by this session, including after an error.
    pub fn pending_write_bytes(&self) -> usize {
        self.write_queue.pending()
    }

    /// Attempts at most 16 KiB / four syscalls, stopping at the first successful
    /// write. Only successfully written bytes leave the queue. Wait for writable
    /// readiness after WouldBlock; do not busy-poll Empty/Interrupted/errors.
    /// This does not call Write::flush or write_all, nor emit a Tab event.
    /// Do not interleave direct writes through master_fd with this queue.
    pub fn flush_write(&mut self) -> Result<PtyWriteResult, PtyWriteError> {
        self.write_queue.flush(self.tab, self.master.as_mut())
    }

    /// Lifecycle primitive; does not consume the event from take_exit_event.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, AppError> {
        self.child
            .0
            .try_wait()
            .map_err(|e| AppError::from_io(Component::Pty, e))
    }

    /// Checks the owned child once without blocking, returning its exit once.
    /// None means still running or already delivered; errors allow later retry.
    /// Call on child-lifecycle wakeups, not in a busy loop. Dispatch a returned
    /// event before discarding it. Prior try_wait/terminate calls do not consume it.
    /// The master and queued I/O remain owned so output can still be drained.
    pub fn take_exit_event(&mut self) -> Result<Option<AppEvent>, PtyExitError> {
        self.exit_state
            .take_event(self.tab, || self.child.0.try_wait())
    }

    /// Closes the master and synchronously kills/reaps this direct child.
    /// Waiting can block on kernel process teardown; no polling thread is added.
    /// Pending input stays owned until session drop; further enqueue/flush calls
    /// return Closed rather than transferring it to another session.
    pub fn terminate(&mut self) -> Result<(), AppError> {
        self.master.take();
        self.child
            .terminate()
            .map_err(|e| AppError::from_io(Component::Pty, e))
    }
}

impl crate::clipboard::PasteSink for PtySession {
    type Error = PtyWriteError;

    fn tab(&self) -> TabId {
        self.tab()
    }

    fn enqueue(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.enqueue_write(bytes).map(|_| ())
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.master.take();
    } // Child guard runs after master closes.
}

struct OwnedChild(Child);
impl OwnedChild {
    fn terminate(&mut self) -> io::Result<()> {
        if self.0.try_wait()?.is_none() {
            // Child retains its PID until reaped; never signal a re-used PID.
            if let Err(error) = self.0.kill()
                && self.0.try_wait()?.is_none()
            {
                return Err(error);
            }
            self.0.wait()?;
        }
        Ok(())
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

fn set_pty_size(fd: BorrowedFd<'_>, size: PtySize) -> io::Result<()> {
    let size = libc::winsize {
        ws_row: size.rows,
        ws_col: size.columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: fd is borrowed from the adapter's live owned PTY slave (spawn)
    // or master (resize). TIOCSWINSZ reads the fully initialized stack winsize
    // only during this call; it retains no pointer. No FD ownership is changed.
    // Kernel SIGWINCH delivery targets this PTY's foreground process group.
    if unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct LinuxBackend<'a> {
    terminfo: &'a Path,
}
impl SpawnBackend for LinuxBackend<'_> {
    type Master = File;
    type Slave = OwnedFd;
    type Child = OwnedChild;

    fn open(&mut self, size: PtySize) -> io::Result<(File, OwnedFd)> {
        // CLOEXEC is set atomically at open (including every slave duplicate).
        // NONBLOCK applies only to the master, never the child's stdio.
        let master =
            posix_openpt(OFlag::O_RDWR | OFlag::O_NOCTTY | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK)?;
        grantpt(&master)?;
        unlockpt(&master)?;
        let name = ptsname_r(&master)?;
        let slave = open(
            Path::new(&name),
            OFlag::O_RDWR | OFlag::O_NOCTTY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )?;
        set_pty_size(slave.as_fd(), size)?;
        Ok((File::from(OwnedFd::from(master)), slave))
    }

    fn spawn(&mut self, command: &mut Command, slave: OwnedFd) -> io::Result<OwnedChild> {
        command.env("TERMINFO", self.terminfo);
        command.stdin(Stdio::from(slave.try_clone()?));
        command.stdout(Stdio::from(slave.try_clone()?));
        command.stderr(Stdio::from(slave));
        let default = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());
        let mask = SigSet::empty();
        // SAFETY: std installs slave stdio before pre_exec. The callback only
        // performs async-signal-safe libc operations/syscalls, with stack data
        // prepared before fork. No allocation, locks, formatting, or logging.
        unsafe {
            command.pre_exec(move || {
                setsid()?;
                // fd 0 is the slave; zero does not steal another session's tty.
                if libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                for sig in [
                    Signal::SIGHUP,
                    Signal::SIGINT,
                    Signal::SIGQUIT,
                    Signal::SIGTERM,
                    Signal::SIGPIPE,
                    Signal::SIGCHLD,
                    Signal::SIGTSTP,
                    Signal::SIGTTIN,
                    Signal::SIGTTOU,
                ] {
                    signal::sigaction(sig, &default)?;
                }
                signal::sigprocmask(SigmaskHow::SIG_SETMASK, Some(&mask), None)?;
                Ok(())
            });
        }
        // All fallible setup happens before spawn. A successful child gets an
        // owning guard immediately; failed exec is reaped by std::Command.
        command.spawn().map(OwnedChild)
    }
}

#[cfg(test)]
mod tests;
