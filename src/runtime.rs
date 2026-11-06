//! Native Wayland adapter. Only this module owns compositor objects and presentation I/O.
mod bar;
mod clipboard;
mod primary;
use crate::clipboard::SelectionKind;
mod clipboard_offers;
mod clipboard_receive;
mod clipboard_send;
mod keyboard;
mod paste;
mod pointer;
mod raster;
mod selection;
mod selection_overlay;
mod sessions;
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
        wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
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
/// Opt-in counts only; never records keycodes, key text or PTY payloads.
#[derive(Default)]
struct InputTrace {
    counts: [u64; 5], // press, release, repeat, queued bytes, written bytes
    previous: Option<([u64; 5], i32, usize)>,
}
impl InputTrace {
    fn add(&mut self, index: usize, count: usize) {
        self.counts[index] = self.counts[index].saturating_add(count as u64);
    }
    fn report(&mut self, terminal: &crate::terminal::Terminal) {
        if self.counts[0] == 0 && self.counts[1] == 0 {
            return;
        }
        let Some(grid) = terminal.grid() else {
            return;
        };
        let point = grid.cursor.point;
        let snapshot = (self.counts, point.line.0, point.column.0);
        if self.previous == Some(snapshot) {
            return;
        }
        self.previous = Some(snapshot);
        let [press, release, repeat, queued, written] = self.counts;
        eprintln!(
            "Ptyra input counts: press={press} release={release} repeat={repeat} queued={queued} written={written} cursor={}:{}",
            point.line.0, point.column.0
        );
    }
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
    tabs: crate::tabs::TabRegistry<PtySession>,
    ready_pty: Vec<crate::TabId>,
    exit_history: sessions::ExitHistory,
    rejected_resize: Option<PtySize>,
    presented_bar: Option<bar::Layout>,
    seat: Option<wl_seat::WlSeat>,
    seat_name: Option<u32>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wayland_client::protocol::wl_pointer::WlPointer>,
    pointer_input: pointer::PointerInput,
    selection_input: selection::SelectionInput,
    clipboard: clipboard::Clipboard,
    primary: primary::Primary,
    paste: Option<paste::Paste>,
    presented_paste: Option<std::sync::Arc<()>>,
    trace_pointer: bool,
    input: keyboard::KeyboardInput,
    clock: crate::clock::MonotonicClock,
    input_trace: Option<InputTrace>,
}

pub fn run(config: crate::config::Config) -> Result<()> {
    let connection =
        Connection::connect_to_env().map_err(|_| "cannot connect to Wayland compositor")?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let raster = raster::Raster::load(&config)?;
    let tabs = config
        .tab_registry()
        .map_err(|_| "invalid tab configuration")?;
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
        tabs,
        ready_pty: Vec::new(),
        exit_history: sessions::ExitHistory::default(),
        rejected_resize: None,
        presented_bar: None,
        seat: None,
        seat_name: None,
        keyboard: None,
        pointer: None,
        pointer_input: pointer::PointerInput::default(),
        selection_input: selection::SelectionInput::default(),
        clipboard: clipboard::Clipboard::new(config.clipboard_bytes),
        primary: primary::Primary::new(config.clipboard_bytes),
        paste: None,
        presented_paste: None,
        trace_pointer: std::env::var_os("PTYRA_TRACE_POINTER").as_deref()
            == Some(std::ffi::OsStr::new("1")),
        input: keyboard::KeyboardInput {
            bindings: config.bindings,
            ..Default::default()
        },
        clock: crate::clock::MonotonicClock::default(),
        input_trace: (std::env::var_os("PTYRA_TRACE_INPUT").as_deref()
            == Some(std::ffi::OsStr::new("1")))
        .then(InputTrace::default),
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
    let mut window_title = crate::APP_NAME.to_owned();
    while app.running {
        queue
            .dispatch_pending(&mut app)
            .map_err(|_| "Wayland dispatch failed")?;
        if !app.running {
            break;
        }
        app.sync_terminal()?;
        app.read_output()?;
        app.clipboard.pump_sends();
        app.primary.pump_sends();
        app.pump_paste();
        if !app.running {
            break;
        }
        let bytes = app.input.tick(app.application_cursor(), &app.clock)?;
        if bytes.is_some()
            && let Some(trace) = &mut app.input_trace
        {
            trace.add(2, 1);
        }
        app.enqueue_input(bytes)?;
        if let Some(pty) = app
            .tabs
            .active()
            .and_then(|id| app.tabs.get_mut(id))
            .and_then(|tab| tab.pty_mut())
            && pty.wants_write()
        {
            let result = pty.flush_write().map_err(|_| "PTY write failed")?;
            if let crate::pty::PtyWriteStatus::Written(count) = result.status
                && let Some(trace) = &mut app.input_trace
            {
                trace.add(4, count);
            }
        }
        if let Some(trace) = &mut app.input_trace
            && let Some(tab) = app.tabs.active().and_then(|id| app.tabs.get(id))
        {
            trace.report(tab.terminal());
        }
        let title = sessions::active_title(&app.tabs);
        if window_title != title {
            top.set_title(title.to_owned());
            window_title = title.to_owned();
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
        let mut polled_tabs = Vec::new();
        for id in app.tabs.ids() {
            if let Some(pty) = app.tabs.get(id).and_then(|tab| tab.pty())
                && let Some(fd) = pty.master_fd()
            {
                polled_tabs.push(id);
                fds.push(PollFd::new(
                    fd,
                    PollFlags::POLLIN
                        | if pty.wants_write() {
                            PollFlags::POLLOUT
                        } else {
                            PollFlags::empty()
                        },
                ));
            }
        }
        let send_offset = fds.len();
        let mut polled_sends = Vec::new();
        for (selection, id, file) in app
            .clipboard
            .send_fds()
            .map(|(id, file)| (SelectionKind::Clipboard, id, file))
            .chain(
                app.primary
                    .send_fds()
                    .map(|(id, file)| (SelectionKind::Primary, id, file)),
            )
        {
            polled_sends.push((selection, id));
            fds.push(PollFd::new(file.as_fd(), PollFlags::POLLOUT));
        }
        let mut polled_receives = Vec::new();
        for (selection, receive) in [
            (SelectionKind::Clipboard, app.clipboard.receive_fd()),
            (SelectionKind::Primary, app.primary.receive_fd()),
        ] {
            if let Some((id, file)) = receive {
                polled_receives.push((selection, id, fds.len()));
                fds.push(PollFd::new(file.as_fd(), PollFlags::POLLIN));
            }
        }
        let timeout = app
            .input
            .map
            .as_ref()
            .and_then(|map| app.input.repeat.deadline(&app.input.focus, map))
            .map(|deadline| {
                let remaining = deadline.remaining(&app.clock);
                PollTimeout::try_from(
                    remaining
                        .as_millis()
                        .saturating_add(u128::from(remaining.subsec_nanos() % 1_000_000 != 0)),
                )
                .unwrap_or(PollTimeout::MAX)
            })
            .unwrap_or(PollTimeout::NONE);
        match poll(&mut fds, timeout) {
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(_) => return Err("runtime poll failed"),
        }
        let wayland_ready = fds[0]
            .revents()
            .unwrap_or(PollFlags::empty())
            .intersects(PollFlags::POLLIN | PollFlags::POLLERR | PollFlags::POLLHUP);
        app.ready_pty = polled_tabs
            .into_iter()
            .zip(fds.iter().skip(1))
            .filter_map(|(id, fd)| {
                fd.revents()
                    .unwrap_or(PollFlags::empty())
                    .intersects(
                        PollFlags::POLLIN
                            | PollFlags::POLLOUT
                            | PollFlags::POLLERR
                            | PollFlags::POLLHUP,
                    )
                    .then_some(id)
            })
            .collect();
        let ready_sends: Vec<_> = polled_sends
            .into_iter()
            .zip(fds.iter().skip(send_offset))
            .filter_map(|(id, fd)| {
                fd.revents()
                    .unwrap_or(PollFlags::empty())
                    .intersects(
                        PollFlags::POLLOUT
                            | PollFlags::POLLERR
                            | PollFlags::POLLHUP
                            | PollFlags::POLLNVAL,
                    )
                    .then_some(id)
            })
            .collect();
        let ready_receives: Vec<_> = polled_receives
            .into_iter()
            .filter_map(|(selection, id, index)| {
                fds[index]
                    .revents()
                    .unwrap_or(PollFlags::empty())
                    .intersects(
                        PollFlags::POLLIN
                            | PollFlags::POLLERR
                            | PollFlags::POLLHUP
                            | PollFlags::POLLNVAL,
                    )
                    .then_some((selection, id))
            })
            .collect();
        drop(fds);
        for (selection, id) in ready_receives {
            match selection {
                SelectionKind::Clipboard => app.clipboard.ready_receive = Some(id),
                SelectionKind::Primary => app.primary.ready_receive = Some(id),
            }
        }
        for (selection, id) in ready_sends {
            match selection {
                SelectionKind::Clipboard => app.clipboard.ready_sends.push(id),
                SelectionKind::Primary => app.primary.ready_sends.push(id),
            }
        }
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
    fn application_cursor(&self) -> bool {
        self.tabs
            .active()
            .and_then(|id| self.tabs.get(id))
            .and_then(|tab| tab.terminal().mode())
            .is_some_and(|mode| mode.contains(alacritty_terminal::term::TermMode::APP_CURSOR))
    }
    fn enqueue_input(&mut self, bytes: Option<Vec<u8>>) -> Result<()> {
        if let Some(bytes) = bytes
            && let Some(pty) = self
                .tabs
                .active()
                .and_then(|id| self.tabs.get_mut(id))
                .and_then(|tab| tab.pty_mut())
        {
            pty.enqueue_write(&bytes)
                .map_err(|_| "PTY input queue full or closed")?;
            if let Some(trace) = &mut self.input_trace {
                trace.add(3, bytes.len());
            }
        }
        Ok(())
    }
    fn tab_layout(&self) -> bar::Layout {
        bar::Layout::new(
            &self.tabs,
            self.paint.size,
            (self.raster.cell_width, self.raster.cell_height),
        )
    }
    fn content_geometry(&self) -> (u32, (u32, u32)) {
        let layout = self.tab_layout();
        (layout.height, layout.content)
    }
    fn sync_terminal(&mut self) -> Result<()> {
        if !self.paint.configured {
            return Ok(());
        }
        let rows = (self.content_geometry().1.1 / self.raster.cell_height).max(1);
        let columns = (self.paint.size.0 / self.raster.cell_width).max(2);
        let size = PtySize {
            rows: rows.try_into().map_err(|_| "too many rows")?,
            columns: columns.try_into().map_err(|_| "too many columns")?,
        };
        if self.tabs.active().is_none() {
            self.new_tab(size)?;
        }
        match self.tabs.resize_all(size.rows, size.columns) {
            Ok(changed) => {
                self.paint.dirty |= changed;
                self.rejected_resize = None;
            }
            Err(_) => {
                if self.rejected_resize != Some(size) {
                    eprintln!("Ptyra resize rejected: tab cell budget or unavailable grid");
                }
                self.rejected_resize = Some(size);
                return Ok(());
            }
        }
        let ids: Vec<_> = self.tabs.ids().collect();
        for id in ids {
            if let Some(pty) = self.tabs.get_mut(id).and_then(|tab| tab.pty_mut()) {
                pty.resize(size).map_err(|_| "PTY resize failed")?;
            }
        }
        Ok(())
    }
    fn new_tab(&mut self, size: PtySize) -> Result<crate::TabId> {
        let program = std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let cwd = std::env::current_dir().map_err(|_| "working directory unavailable")?;
        sessions::spawn_tab(&mut self.tabs, size.rows, size.columns, |request| {
            PtySession::spawn(PtySpawnRequest {
                tab: request.tab,
                program: &program,
                args: &[],
                cwd: &cwd,
                size,
            })
            .map_err(|_| "cannot start Shell PTY")
        })
    }
    fn read_output(&mut self) -> Result<()> {
        let previous_titles: Vec<_> = self
            .ready_pty
            .iter()
            .filter_map(|id| self.tabs.get(*id).map(|tab| (*id, tab.title().to_owned())))
            .collect();
        let mut bytes = [0; crate::pty::MAX_PTY_READ_BYTES];
        let mut results = Vec::new();
        self.tabs
            .drain_ready(&self.ready_pty, &mut bytes, |id, result| {
                results.push((id, result))
            })
            .map_err(|_| "PTY dispatch failed")?;
        for (id, result) in results {
            match result.map_err(|_| "PTY read failed")?.status {
                PtyReadStatus::Data(_) if self.tabs.active() == Some(id) => self.paint.dirty = true,
                PtyReadStatus::Eof => {
                    let status = if let Some(pty) =
                        self.tabs.get_mut(id).and_then(|tab| tab.pty_mut())
                    {
                        let mut status = pty.try_wait().map_err(|_| "child status unavailable")?;
                        if status.is_none() {
                            pty.terminate().map_err(|_| "child cleanup failed")?;
                            status = pty.try_wait().map_err(|_| "child status unavailable")?;
                        }
                        status
                    } else {
                        None
                    };
                    self.exit_history.record(&mut self.tabs, id, status);
                    self.close_tab(id);
                }
                _ => {}
            }
            if let Some(pty) = self.tabs.get_mut(id).and_then(|tab| tab.pty_mut())
                && pty.wants_write()
            {
                pty.flush_write().map_err(|_| "PTY write failed")?;
            }
        }
        self.paint.dirty |= previous_titles
            .iter()
            .any(|(id, old)| self.tabs.get(*id).is_some_and(|tab| tab.title() != old));
        self.ready_pty.clear();
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
        let (bar_height, content_size) = self.content_geometry();
        let content_offset = (bar_height * self.paint.size.0) as usize * 4;
        let tab = self
            .tabs
            .get(self.tabs.active().ok_or("no active tab")?)
            .ok_or("terminal unavailable")?;
        let terminal = tab.terminal();
        self.raster.draw(
            terminal,
            content_size,
            &mut self.pixels[content_offset..],
            self.input.focus.target().is_some(),
        )?;
        selection_overlay::draw(
            tab,
            content_size,
            (self.raster.cell_width, self.raster.cell_height),
            &mut self.pixels[content_offset..],
        )?;
        let layout = self.tab_layout();
        layout.draw(
            &self.tabs,
            &mut self.raster,
            self.paint.size,
            &mut self.pixels[..content_offset],
        )?;
        let prompt = self
            .paste
            .as_ref()
            .and_then(|paste| paste.prompt_token(&self.input.focus));
        let presented = if prompt.is_some()
            && paste::draw_prompt(&mut self.raster, self.paint.size, &mut self.pixels)?
        {
            prompt
        } else {
            None
        };
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
        self.presented_bar = Some(layout);
        if let Some(token) = &presented
            && self
                .presented_paste
                .as_ref()
                .is_none_or(|old| !std::sync::Arc::ptr_eq(old, token))
        {
            eprintln!("Ptyra paste: confirmation shown");
        }
        self.presented_paste = presented;
        Ok(())
    }
    fn fail(&mut self, error: &'static str) {
        self.failure = Some(error);
        self.running = false;
    }
}

#[cfg(test)]
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
                "wl_seat" if version >= 5 && state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(7), qh, ()));
                    state.seat_name = Some(name);
                }
                "wl_data_device_manager" if version >= 2 && state.clipboard.manager.is_none() => {
                    state.clipboard.manager =
                        Some((name, registry.bind(name, version.min(3), qh, ())));
                }
                "zwp_primary_selection_device_manager_v1"
                    if version >= 1 && state.primary.manager.is_none() =>
                {
                    state.primary.manager = Some((name, registry.bind(name, 1, qh, ())));
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } if state.globals.contains(&name) => {
                state.fail("required Wayland global removed")
            }
            wl_registry::Event::GlobalRemove { name } if state.seat_name == Some(name) => {
                state.remove_clipboard_device();
                state.remove_primary_device();
                state.remove_keyboard();
                state.remove_pointer();
                if let Some(seat) = state.seat.take() {
                    seat.release();
                }
                state.seat_name = None;
            }
            wl_registry::Event::GlobalRemove { name }
                if state
                    .clipboard
                    .manager
                    .as_ref()
                    .is_some_and(|(id, _)| *id == name) =>
            {
                state.clipboard.remove_manager();
            }
            wl_registry::Event::GlobalRemove { name }
                if state
                    .primary
                    .manager
                    .as_ref()
                    .is_some_and(|(id, _)| *id == name) =>
            {
                state.primary.remove_manager();
            }
            _ => {}
        }
        state.bind_clipboard(qh);
        state.bind_primary(qh);
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
