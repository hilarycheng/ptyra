//! Linux PTY process ownership. Stream queues and exit routing are separate tasks.
use crate::{
    TabId,
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

pub const MAX_CWD_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtySize {
    pub rows: u16,
    pub columns: u16,
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
}

impl PtySession {
    pub fn spawn(request: PtySpawnRequest<'_>) -> Result<Self, PtySpawnError> {
        let spawned = spawn_with_backend(request, &mut LinuxBackend)?;
        Ok(Self {
            tab: spawned.tab,
            master: Some(spawned.master),
            child: spawned.child,
        })
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }
    pub fn child_id(&self) -> u32 {
        self.child.0.id()
    }

    /// Borrowed, nonblocking master; no ownership transfer or stream buffering.
    pub fn master_fd(&self) -> Option<BorrowedFd<'_>> {
        self.master.as_ref().map(AsFd::as_fd)
    }

    /// Lifecycle primitive only: no Tab event is emitted (TASK-025).
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, AppError> {
        self.child
            .0
            .try_wait()
            .map_err(|e| AppError::from_io(Component::Pty, e))
    }

    /// Closes the master and synchronously kills/reaps this direct child.
    /// Waiting can block on kernel process teardown; no polling thread is added.
    pub fn terminate(&mut self) -> Result<(), AppError> {
        self.master.take();
        self.child
            .terminate()
            .map_err(|e| AppError::from_io(Component::Pty, e))
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

struct LinuxBackend;
impl SpawnBackend for LinuxBackend {
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
        let size = libc::winsize {
            ws_row: size.rows,
            ws_col: size.columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: slave is owned/live and TIOCSWINSZ reads this initialized winsize
        // during the call. This only sets the new PTY's initial dimensions.
        if unsafe { libc::ioctl(slave.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok((File::from(OwnedFd::from(master)), slave))
    }

    fn spawn(&mut self, command: &mut Command, slave: OwnedFd) -> io::Result<OwnedChild> {
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
