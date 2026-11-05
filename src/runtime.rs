//! Native Wayland adapter. Only this module owns compositor objects and presentation I/O.
mod raster;
use crate::pty::{PtyReadStatus, PtySession, PtySize, PtySpawnRequest};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::memfd::{MFdFlags, memfd_create};
use std::{
    fs::File,
    os::{fd::AsFd, unix::fs::FileExt},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_PIXELS: usize = 4 * 1024 * 1024;

struct PaintState {
    size: (u32, u32),
    configured: bool,
    dirty: bool,
    frame_pending: bool,
}
impl Default for PaintState {
    fn default() -> Self {
        Self {
            size: (800, 600),
            configured: false,
            dirty: false,
            frame_pending: false,
        }
    }
}
impl PaintState {
    fn configure(&mut self, width: i32, height: i32) -> Result<()> {
        if width < 0 || height < 0 {
            return Err("invalid Wayland dimensions");
        }
        let width = if width == 0 {
            self.size.0
        } else {
            width as u32
        };
        let height = if height == 0 {
            self.size.1
        } else {
            height as u32
        };
        if u64::from(width) * u64::from(height) > MAX_PIXELS as u64 {
            return Err("Wayland surface exceeds pixel limit");
        }
        self.size = (width, height);
        self.dirty = true;
        Ok(())
    }
    fn ready(&self) -> bool {
        self.configured && self.dirty && !self.frame_pending
    }
    fn submitted(&mut self) {
        self.dirty = false;
        self.frame_pending = true;
    }
}

struct Slot {
    file: File,
    buffer: wl_buffer::WlBuffer,
    size: (u32, u32),
    busy: bool,
}
struct Runtime {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm: Option<xdg_wm_base::XdgWmBase>,
    globals: [u32; 3],
    surface: Option<wl_surface::WlSurface>,
    paint: PaintState,
    slots: [Option<Slot>; 2],
    pixels: Vec<u8>,
    running: bool,
    failure: Option<&'static str>,
    raster: raster::Raster,
    terminal: crate::terminal::Terminal,
    pty: Option<PtySession>,
}

pub fn run(config: crate::config::Config) -> Result<()> {
    let connection =
        Connection::connect_to_env().map_err(|_| "cannot connect to Wayland compositor")?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let raster = raster::Raster::load(&config)?;
    let terminal = crate::terminal::Terminal::with_scrollback(
        crate::TabId(1),
        24,
        80,
        config.scrollback_lines,
    )
    .map_err(|_| "terminal allocation failed")?;
    let mut app = Runtime {
        compositor: None,
        shm: None,
        wm: None,
        globals: [0; 3],
        surface: None,
        paint: PaintState::default(),
        slots: [None, None],
        pixels: Vec::new(),
        running: true,
        failure: None,
        raster,
        terminal,
        pty: None,
    };
    connection.display().get_registry(&qh, ());
    queue
        .roundtrip(&mut app)
        .map_err(|_| "Wayland registry failed")?;
    let compositor = app
        .compositor
        .as_ref()
        .ok_or("Wayland compositor v4 unavailable")?;
    let wm = app.wm.as_ref().ok_or("xdg-shell unavailable")?;
    app.shm
        .as_ref()
        .ok_or("Wayland shared memory unavailable")?;
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm.get_xdg_surface(&surface, &qh, ());
    let top = xdg.get_toplevel(&qh, ());
    top.set_title(crate::APP_NAME.into());
    top.set_app_id("ptyra".into());
    top.set_min_size(160, 80);
    surface.commit();
    app.surface = Some(surface);
    while app.running {
        queue
            .dispatch_pending(&mut app)
            .map_err(|_| "Wayland dispatch failed")?;
        if !app.running {
            break;
        }
        app.sync_terminal()?;
        app.read_output()?;
        if !app.running {
            break;
        }
        app.draw(&qh)?;
        let writable = match connection.flush() {
            Ok(()) => false,
            Err(wayland_client::backend::WaylandError::Io(error))
                if error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                true
            }
            Err(_) => return Err("Wayland flush failed"),
        };
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut fds = vec![PollFd::new(
            guard.connection_fd(),
            PollFlags::POLLIN
                | if writable {
                    PollFlags::POLLOUT
                } else {
                    PollFlags::empty()
                },
        )];
        if let Some(fd) = app.pty.as_ref().and_then(PtySession::master_fd) {
            fds.push(PollFd::new(fd, PollFlags::POLLIN));
        }
        match poll(&mut fds, PollTimeout::NONE) {
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(_) => return Err("runtime poll failed"),
        }
        let wayland_ready = fds[0]
            .revents()
            .unwrap_or(PollFlags::empty())
            .intersects(PollFlags::POLLIN | PollFlags::POLLERR | PollFlags::POLLHUP);
        drop(fds);
        if wayland_ready {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(error))
                    if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err("Wayland connection lost"),
            }
        }
    }
    top.destroy();
    xdg.destroy();
    if let Some(error) = app.failure {
        Err(error)
    } else {
        Ok(())
    }
}

impl Runtime {
    fn sync_terminal(&mut self) -> Result<()> {
        if !self.paint.configured {
            return Ok(());
        }
        let rows = (self.paint.size.1 / self.raster.cell_height).max(1);
        let columns = (self.paint.size.0 / self.raster.cell_width).max(2);
        let size = PtySize {
            rows: rows.try_into().map_err(|_| "too many rows")?,
            columns: columns.try_into().map_err(|_| "too many columns")?,
        };
        let snapshot = self.terminal.snapshot().ok_or("terminal unavailable")?;
        if snapshot.rows != rows as usize || snapshot.columns != columns as usize {
            self.terminal
                .resize(size.rows, size.columns)
                .map_err(|_| "terminal size exceeds allocation budget")?;
            self.paint.dirty = true;
        }
        if let Some(pty) = &mut self.pty {
            pty.resize(size).map_err(|_| "PTY resize failed")?;
        } else {
            let program = std::env::var_os("SHELL")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/bin/sh".into());
            let cwd = std::env::current_dir().map_err(|_| "working directory unavailable")?;
            self.pty = Some(
                PtySession::spawn(PtySpawnRequest {
                    tab: crate::TabId(1),
                    program: &program,
                    args: &[],
                    cwd: &cwd,
                    size,
                })
                .map_err(|_| "cannot start Shell PTY")?,
            );
        }
        Ok(())
    }
    fn read_output(&mut self) -> Result<()> {
        let Some(pty) = &mut self.pty else {
            return Ok(());
        };
        let mut bytes = [0; crate::pty::MAX_PTY_READ_BYTES];
        for _ in 0..4 {
            let status = pty
                .read_chunk(&mut bytes)
                .map_err(|_| "PTY read failed")?
                .status;
            if !apply_output(&mut self.terminal, &mut self.paint, status, &bytes)? {
                self.running = false;
                break;
            }
            if !matches!(status, PtyReadStatus::Data(_)) {
                break;
            }
        }
        // EOF drains output before shutdown; Drop reaps the child and closes the master.
        Ok(())
    }
    fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        if !self.paint.ready() {
            return Ok(());
        }
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.as_ref().is_none_or(|slot| !slot.busy))
        else {
            return Ok(());
        };
        let (width, height) = self.paint.size;
        let bytes = width as usize * height as usize * 4;
        if self.slots[index]
            .as_ref()
            .is_none_or(|slot| slot.size != self.paint.size)
        {
            if let Some(old) = self.slots[index].take() {
                old.buffer.destroy();
            }
            let file = File::from(
                memfd_create(c"ptyra-frame", MFdFlags::MFD_CLOEXEC)
                    .map_err(|_| "cannot create frame memory")?,
            );
            file.set_len(bytes as u64)
                .map_err(|_| "cannot size frame memory")?;
            let shm = self.shm.as_ref().ok_or("shared memory unavailable")?;
            let pool = shm.create_pool(file.as_fd(), bytes as i32, qh, ());
            let buffer = pool.create_buffer(
                0,
                width as i32,
                height as i32,
                width as i32 * 4,
                wl_shm::Format::Argb8888,
                qh,
                index,
            );
            pool.destroy();
            self.slots[index] = Some(Slot {
                file,
                buffer,
                size: self.paint.size,
                busy: false,
            });
        }
        self.pixels.clear();
        self.pixels
            .try_reserve(bytes)
            .map_err(|_| "frame allocation failed")?;
        self.pixels.resize(bytes, 0);
        self.raster
            .draw(&self.terminal, self.paint.size, &mut self.pixels)?;
        let slot = self.slots[index].as_mut().ok_or("frame slot unavailable")?;
        slot.file
            .write_all_at(&self.pixels, 0)
            .map_err(|_| "frame write failed")?;
        let surface = self.surface.as_ref().ok_or("surface unavailable")?;
        surface.attach(Some(&slot.buffer), 0, 0);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        surface.frame(qh, ());
        surface.commit();
        slot.busy = true;
        self.paint.submitted();
        Ok(())
    }
    fn fail(&mut self, error: &'static str) {
        self.failure = Some(error);
        self.running = false;
    }
}

fn apply_output(
    terminal: &mut crate::terminal::Terminal,
    paint: &mut PaintState,
    status: PtyReadStatus,
    bytes: &[u8],
) -> Result<bool> {
    match status {
        PtyReadStatus::Data(length) => {
            let bytes = bytes.get(..length).ok_or("invalid PTY read length")?;
            terminal
                .feed(terminal.tab(), bytes)
                .map_err(|_| "terminal rejected PTY output")?;
            paint.dirty = true;
        }
        PtyReadStatus::Eof => return Ok(false),
        _ => {}
    }
    Ok(true)
}

impl Dispatch<wl_registry::WlRegistry, ()> for Runtime {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" if version >= 4 && state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, 4, qh, ()));
                    state.globals[0] = name;
                }
                "wl_shm" if state.shm.is_none() => {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                    state.globals[1] = name;
                }
                "xdg_wm_base" if state.wm.is_none() => {
                    state.wm = Some(registry.bind(name, 1, qh, ()));
                    state.globals[2] = name;
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } if state.globals.contains(&name) => {
                state.fail("required Wayland global removed")
            }
            _ => {}
        }
    }
}
impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Runtime {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}
impl Dispatch<xdg_surface::XdgSurface, ()> for Runtime {
    fn event(
        state: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
            state.paint.configured = true;
            state.paint.dirty = true;
        }
    }
}
impl Dispatch<xdg_toplevel::XdgToplevel, ()> for Runtime {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                if let Err(error) = state.paint.configure(width, height) {
                    state.fail(error);
                }
            }
            xdg_toplevel::Event::Close => state.running = false,
            _ => {}
        }
    }
}
impl Dispatch<wl_buffer::WlBuffer, usize> for Runtime {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(slot) = &mut state.slots[*index] {
            slot.busy = false;
        }
    }
}
impl Dispatch<wl_callback::WlCallback, ()> for Runtime {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.paint.frame_pending = false;
    }
}
delegate_noop!(Runtime: ignore wl_compositor::WlCompositor);
delegate_noop!(Runtime: ignore wl_surface::WlSurface);
delegate_noop!(Runtime: ignore wl_shm::WlShm);
delegate_noop!(Runtime: ignore wl_shm_pool::WlShmPool);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_pty_output_updates_grid() {
        use crate::pty::PtyReadStatus;
        use alacritty_terminal::index::{Column, Line};
        let mut terminal = crate::terminal::Terminal::new(crate::TabId(1), 4, 16).unwrap();
        let mut paint = PaintState::default();
        for chunk in [b"\xe7\xb9".as_slice(), b"\x81\x1b[31mA"] {
            assert!(
                apply_output(
                    &mut terminal,
                    &mut paint,
                    PtyReadStatus::Data(chunk.len()),
                    chunk
                )
                .unwrap()
            );
        }
        assert!(paint.dirty);
        assert_eq!(terminal.grid().unwrap()[Line(0)][Column(0)].c, '繁');
        assert_eq!(terminal.grid().unwrap()[Line(0)][Column(2)].c, 'A');
        terminal.resize(6, 20).unwrap();
        assert_eq!(terminal.grid().unwrap()[Line(0)][Column(0)].c, '繁');
        assert_eq!(terminal.snapshot().unwrap().columns, 20);
        assert!(terminal.resize(0, 20).is_err());
        assert_eq!(terminal.snapshot().unwrap().rows, 6);
        paint.dirty = false;
        assert!(apply_output(&mut terminal, &mut paint, PtyReadStatus::WouldBlock, &[]).unwrap());
        assert!(!paint.dirty);
        assert!(!apply_output(&mut terminal, &mut paint, PtyReadStatus::Eof, &[]).unwrap());
    }
    #[test]
    fn runtime_window_waits_for_configure() {
        let mut state = PaintState::default();
        assert!(!state.ready());
        state.configure(800, 600).unwrap();
        assert!(!state.ready());
        state.configured = true;
        assert!(state.ready());
        state.submitted();
        assert!(!state.ready());
        state.dirty = true;
        assert!(!state.ready());
        state.frame_pending = false;
        assert!(state.ready());
        state.submitted();
        state.frame_pending = false;
        assert!(!state.ready());
        assert!(state.configure(-1, 600).is_err());
        assert!(state.configure(i32::MAX, i32::MAX).is_err());
        state.configure(0, 0).unwrap();
        assert_eq!(state.size, (800, 600));
    }
}
