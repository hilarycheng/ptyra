//! Native Wayland adapter. Only this module owns compositor objects and presentation I/O.
mod bar;
mod clipboard;
mod cursor;
mod damage;
mod notice;
mod osc52;
mod primary;
use crate::clipboard::SelectionKind;
mod clipboard_offers;
mod clipboard_receive;
mod clipboard_send;
mod focus_reports;
mod fonts;
mod hyperlink;
mod keyboard;
mod mouse;
mod paste;
mod pointer;
mod raster;
mod scale;
mod scroll;
mod selection;
mod selection_overlay;
mod sessions;
mod sync;
use crate::pty::{PtyReadStatus, PtySession, PtySize, PtySpawnRequest};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::memfd::{MFdFlags, memfd_create};
use std::{
    fs::File,
    os::{fd::AsFd, unix::fs::FileExt},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_registry, wl_seat,
        wl_shm, wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_PIXELS: usize = 4 * 1024 * 1024;

fn resize_frame_pixels(pixels: &mut Vec<u8>, bytes: usize) -> Result<()> {
    if bytes > MAX_PIXELS * 4 {
        return Err("frame exceeds pixel limit");
    }
    pixels
        .try_reserve_exact(bytes.saturating_sub(pixels.len()))
        .map_err(|_| "frame allocation failed")?;
    pixels.resize(bytes, 0);
    Ok(())
}

struct PaintState {
    size: (u32, u32), // Surface-local logical units.
    buffer_size: (u32, u32),
    scale: u32,
    geometry_presented: bool,
    configured: bool,
    pending_size: (i32, i32), // Latched only by xdg_surface.configure.
    configure_error: Option<&'static str>,
    configure_pending: bool,
    dirty: bool,
    frame_pending: bool,
}
impl Default for PaintState {
    fn default() -> Self {
        Self {
            size: (800, 600),
            buffer_size: (800, 600),
            scale: 1,
            geometry_presented: false,
            configured: false,
            pending_size: (0, 0),
            configure_error: None,
            configure_pending: false,
            dirty: false,
            frame_pending: false,
        }
    }
}
impl PaintState {
    fn configure(&mut self, width: i32, height: i32) -> Result<()> {
        // An acknowledged size must be exact (notably when maximized). On refusal,
        // retain the old buffer/grid but do not commit it against the new configure.
        self.configured = false;
        self.configure_pending = false;
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
        self.geometry_presented &= self.size == (width, height);
        self.size = (width, height);
        self.configured = true;
        self.configure_pending = true;
        self.dirty = true;
        Ok(())
    }
    fn ready(&self) -> bool {
        self.configured && self.dirty && (!self.frame_pending || self.configure_pending)
    }
    fn submitted(&mut self) {
        self.configure_pending = false;
        self.dirty = false;
        self.frame_pending = true;
        self.geometry_presented = true;
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
    output_scale: scale::State,
    cursor: cursor::State,
    damage: damage::State,
    outputs: std::collections::BTreeMap<u32, wl_output::WlOutput>,
    slots: [Option<Slot>; 2],
    pixels: Vec<u8>,
    running: bool,
    failure: Option<&'static str>,
    raster: raster::Raster,
    tabs: crate::tabs::TabRegistry<PtySession>,
    ready_pty: Vec<crate::TabId>,
    exit_history: sessions::ExitHistory,
    rejected_resize: Option<(u32, u32)>,
    presented_bar: Option<bar::Layout>,
    seat: Option<wl_seat::WlSeat>,
    seat_name: Option<u32>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wayland_client::protocol::wl_pointer::WlPointer>,
    pointer_input: pointer::PointerInput,
    selection_input: selection::SelectionInput,
    wheel: scroll::Wheel,
    hyperlinks: hyperlink::Opener,
    presented_hyperlink: Option<hyperlink::Presented>,
    clipboard: clipboard::Clipboard,
    primary: primary::Primary,
    paste: Option<paste::Paste>,
    clipboard_notice: Option<notice::Notice>,
    presented_paste: Option<std::sync::Arc<()>>,
    trace_pointer: bool,
    input: keyboard::KeyboardInput,
    clock: crate::clock::MonotonicClock,
    input_trace: Option<InputTrace>,
}

fn new_runtime(config: crate::config::Config, raster: raster::Raster) -> Result<Runtime> {
    let tabs = config
        .tab_registry()
        .map_err(|_| "invalid tab configuration")?;
    Ok(Runtime {
        compositor: None,
        shm: None,
        wm: None,
        globals: [0; 3],
        surface: None,
        paint: PaintState::default(),
        output_scale: scale::State::default(),
        cursor: cursor::State::default(),
        damage: damage::State::default(),
        outputs: std::collections::BTreeMap::new(),
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
        wheel: scroll::Wheel::default(),
        hyperlinks: hyperlink::Opener::default(),
        presented_hyperlink: None,
        clipboard: clipboard::Clipboard::new(config.clipboard_bytes),
        primary: primary::Primary::new(config.clipboard_bytes),
        paste: None,
        clipboard_notice: None,
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
    })
}

pub fn run(config: crate::config::Config) -> Result<()> {
    let connection =
        Connection::connect_to_env().map_err(|_| "cannot connect to Wayland compositor")?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let raster = raster::Raster::load(&config)?;
    let mut app = new_runtime(config, raster)?;
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
        app.sync_scale();
        sync::refresh(&mut app.tabs, &app.clock);
        app.sync_terminal()?;
        app.read_output(&qh)?;
        let sync_deadline = sync::refresh(&mut app.tabs, &app.clock);
        let copy_failed = app.clipboard.pump_sends();
        let primary_failed = app.primary.pump_sends();
        if copy_failed || primary_failed {
            app.show_clipboard_notice(notice::Notice::TransferFailed);
        }
        app.pump_paste();
        if app.hyperlinks.pump() {
            app.show_clipboard_notice(notice::Notice::HyperlinkFailed);
        }
        if !app.running {
            break;
        }
        let bytes = app.input.tick(
            app.application_cursor(),
            app.application_keypad(),
            &app.clock,
        )?;
        if bytes.is_some()
            && let Some(trace) = &mut app.input_trace
        {
            trace.add(2, 1);
        }
        app.enqueue_input(bytes);
        if let Some(id) = app.tabs.active() {
            app.flush_tab(id);
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
        app.paint.dirty |= app.cursor.refresh(
            &app.tabs,
            &app.input.focus,
            app.paint.configured,
            &app.clock,
        );
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
        for stream in app.hyperlinks.fds() {
            fds.push(PollFd::new(stream.as_fd(), PollFlags::POLLIN));
        }
        let timeout = app
            .input
            .map
            .as_ref()
            .and_then(|map| app.input.repeat.deadline(&app.input.focus, map))
            .into_iter()
            .chain(sync_deadline)
            .chain(app.cursor.deadline())
            .map(|deadline| deadline.remaining(&app.clock))
            .min()
            .map(|remaining| {
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
                            | PollFlags::POLLHUP
                            | PollFlags::POLLNVAL,
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
    fn application_keypad(&self) -> bool {
        self.tabs
            .active()
            .and_then(|id| self.tabs.get(id))
            .and_then(|tab| tab.terminal().mode())
            .is_some_and(|mode| mode.contains(alacritty_terminal::term::TermMode::APP_KEYPAD))
    }
    fn enqueue_input(&mut self, bytes: Option<Vec<u8>>) {
        let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty()) else {
            return;
        };
        let accepted = self
            .input
            .focus
            .target()
            .filter(|target| self.tabs.active() == Some(target.tab))
            .and_then(|target| self.tabs.get_mut(target.tab))
            .and_then(|tab| tab.pty_mut())
            .is_some_and(|pty| pty.enqueue_write(&bytes).is_ok());
        if !accepted {
            // Reject this whole event once; neither repeat nor a later Tab may replay it.
            self.input.repeat.cancel();
            if self.clipboard_notice != Some(notice::Notice::InputRejected) {
                self.show_clipboard_notice(notice::Notice::InputRejected);
            }
            return;
        }
        if self.clipboard_notice == Some(notice::Notice::InputRejected) {
            self.clear_clipboard_notice();
        }
        if let Some(trace) = &mut self.input_trace {
            trace.add(3, bytes.len());
        }
        self.wheel.reset();
        // Accepted keyboard input follows the live cursor; output alone preserves history.
        self.scroll_active(alacritty_terminal::grid::Scroll::Bottom);
    }
    fn tab_layout(&self) -> bar::Layout {
        bar::Layout::new(
            &self.tabs,
            self.paint.buffer_size,
            (self.raster.cell_width, self.raster.cell_height),
            self.paint.scale,
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
        let columns = (self.paint.buffer_size.0 / self.raster.cell_width).max(2);
        let requested = (rows, columns);
        let size = u16::try_from(rows)
            .ok()
            .zip(u16::try_from(columns).ok())
            .map(|(rows, columns)| PtySize { rows, columns });
        if self.tabs.active().is_none() {
            let (rows, columns) = self.tabs.initial_size(rows, columns);
            self.new_tab(PtySize { rows, columns })?;
        }
        self.tabs
            .set_cell_pixels((self.raster.cell_width, self.raster.cell_height));
        let resized = size.and_then(|size| {
            self.tabs
                .resize_all(size.rows, size.columns)
                .ok()
                .map(|changed| (size, changed))
        });
        let Some((size, changed)) = resized else {
            if self.rejected_resize != Some(requested) {
                eprintln!(
                    "Ptyra resize rejected: tab cell budget or unavailable grid; retaining bounded grid"
                );
            }
            self.rejected_resize = Some(requested);
            return Ok(());
        };
        self.paint.dirty |= changed;
        self.rejected_resize = None;
        let ids: Vec<_> = self.tabs.ids().collect();
        for id in ids {
            if let Some(pty) = self.tabs.get_mut(id).and_then(|tab| tab.pty_mut())
                && pty.resize(size).is_err()
            {
                self.stop_session(id, crate::tabs::SessionFailure::Io);
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
    fn read_output(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
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
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    self.stop_session(id, crate::tabs::SessionFailure::from_error(error));
                    continue;
                }
            };
            match result.status {
                PtyReadStatus::Data(_) => {
                    if self.tabs.enqueue_terminal_replies(id).is_err()
                        && self.clipboard_notice != Some(notice::Notice::ReplyRejected)
                    {
                        self.show_clipboard_notice(notice::Notice::ReplyRejected);
                    }
                    self.publish_osc52(id, qh);
                    self.paint.dirty |= self.tabs.active() == Some(id);
                }
                PtyReadStatus::Eof => {
                    let status = self
                        .tabs
                        .get_mut(id)
                        .and_then(|tab| tab.pty_mut())
                        .map(sessions::finish_child)
                        .transpose();
                    let status = match status {
                        Ok(status) => status.flatten(),
                        Err(_) => {
                            self.stop_session(id, crate::tabs::SessionFailure::Io);
                            continue;
                        }
                    };
                    self.exit_history.record(&mut self.tabs, id, status);
                    self.close_tab(id);
                }
                _ => {}
            }
            self.flush_tab(id);
        }
        self.paint.dirty |= previous_titles
            .iter()
            .any(|(id, old)| self.tabs.get(*id).is_some_and(|tab| tab.title() != old));
        self.ready_pty.clear();
        Ok(())
    }
    fn draw(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        if !self.paint.ready() || sync::held(&self.tabs) {
            return Ok(());
        }
        let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.as_ref().is_none_or(|slot| !slot.busy))
        else {
            return Ok(());
        };
        let (width, height) = self.paint.buffer_size;
        let bytes = width as usize * height as usize * 4;
        if self.slots[index]
            .as_ref()
            .is_none_or(|slot| slot.size != self.paint.buffer_size)
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
            self.damage.invalidate_slot(index, height);
            self.slots[index] = Some(Slot {
                file,
                buffer,
                size: self.paint.buffer_size,
                busy: false,
            });
        }
        resize_frame_pixels(&mut self.pixels, bytes)?;
        let (bar_height, content_size) = self.content_geometry();
        let content_offset = (bar_height * self.paint.buffer_size.0) as usize * 4;
        let active = self.tabs.active().ok_or("no active tab")?;
        let terminal = self
            .tabs
            .get_mut(active)
            .ok_or("terminal unavailable")?
            .terminal_mut();
        let rows = terminal.snapshot().ok_or("terminal unavailable")?.rows;
        let terminal_damage = terminal.damage_rows().and_then(|damage| {
            damage::Band::terminal(damage, rows, content_size.1, self.raster.cell_height)
        });
        let prompt = self
            .paste
            .as_ref()
            .and_then(|paste| paste.prompt_token(&self.input.focus));
        let notice = self
            .tabs
            .get(active)
            .and_then(|tab| tab.failure())
            .map(|failure| match failure {
                crate::tabs::SessionFailure::Stream => notice::Notice::StreamStopped,
                crate::tabs::SessionFailure::Io => notice::Notice::PtyStopped,
            })
            .or(self.clipboard_notice);
        let overlay_height = if prompt.is_some() {
            paste::prompt_height(
                self.paint.buffer_size,
                (self.raster.cell_width, self.raster.cell_height),
            )
        } else {
            notice.map_or(0, |notice| {
                notice::height(
                    self.paint.buffer_size,
                    (self.raster.cell_width, self.raster.cell_height),
                    notice,
                )
            })
        };
        let tab = self.tabs.get(active).ok_or("terminal unavailable")?;
        let mut overlays =
            selection_overlay::bounds(tab, self.raster.cell_height).map(|b| b.shifted(bar_height));
        damage::Band::add(
            &mut overlays,
            damage::Band::new(height - overlay_height, height),
        );
        let plan = self.damage.prepare(
            damage::View {
                tab: active,
                size: self.paint.buffer_size,
                cell: (self.raster.cell_width, self.raster.cell_height),
                scale: self.paint.scale,
                origin: bar_height,
                titles: self
                    .tabs
                    .ids()
                    .filter_map(|id| self.tabs.get(id).map(|tab| (id, tab.title().to_owned())))
                    .collect(),
            },
            terminal_damage,
            overlays,
        );
        if plan.surface.iter().all(Option::is_none) && !self.paint.configure_pending {
            self.tabs
                .get_mut(active)
                .ok_or("terminal unavailable")?
                .terminal_mut()
                .damage_presented();
            self.tabs.sync_presented();
            self.paint.dirty = false;
            return Ok(());
        }
        let terminal = tab.terminal();
        if let Some(rows) = plan.content {
            self.raster.draw_rows(
                terminal,
                content_size,
                &mut self.pixels[content_offset..],
                cursor::focused(&self.tabs, &self.input.focus),
                self.cursor.visible(),
                rows.range(),
            )?;
            selection_overlay::draw_rows(
                tab,
                content_size,
                (self.raster.cell_width, self.raster.cell_height),
                &mut self.pixels[content_offset..],
                rows.range(),
            )?;
        }
        let layout = self.tab_layout();
        if plan.bar {
            layout.draw(
                &self.tabs,
                &mut self.raster,
                self.paint.buffer_size,
                &mut self.pixels[..content_offset],
            )?;
        }
        if prompt.is_none()
            && let Some(notice) = notice
        {
            notice::draw(
                &mut self.raster,
                self.paint.buffer_size,
                &mut self.pixels,
                notice,
            )?;
        }
        let presented = if prompt.is_some()
            && paste::draw_prompt(&mut self.raster, self.paint.buffer_size, &mut self.pixels)?
        {
            prompt
        } else {
            None
        };
        let slot = self.slots[index].as_mut().ok_or("frame slot unavailable")?;
        if let Some(stale) = self.damage.stale(index) {
            let start = stale.start as usize * width as usize * 4;
            let end = stale.end as usize * width as usize * 4;
            slot.file
                .write_all_at(&self.pixels[start..end], start as u64)
                .map_err(|_| "frame write failed")?;
        }
        let surface = self.surface.as_ref().ok_or("surface unavailable")?;
        surface.set_buffer_scale(self.paint.scale as i32);
        surface.attach(Some(&slot.buffer), 0, 0);
        for band in plan.surface.into_iter().flatten() {
            surface.damage_buffer(
                0,
                band.start as i32,
                width as i32,
                (band.end - band.start) as i32,
            );
        }
        // A configure can require a response while the compositor withholds the old
        // frame callback. Reuse that one callback instead of accumulating more.
        if !self.paint.frame_pending {
            surface.frame(qh, ());
        }
        surface.commit();
        slot.busy = true;
        self.damage.submitted(index);
        self.tabs
            .get_mut(active)
            .ok_or("terminal unavailable")?
            .terminal_mut()
            .damage_presented();
        self.presented_hyperlink = hyperlink::Presented::capture(
            &self.tabs,
            selection::Geometry {
                origin: bar_height,
                size: content_size,
                cell: (self.raster.cell_width, self.raster.cell_height),
            },
            self.clipboard_notice.is_some() || presented.is_some(),
        );
        self.tabs.sync_presented();
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
        connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" if version >= 4 && state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()));
                    state.globals[0] = name;
                }
                "wl_output" if version >= 1 && state.output_scale.add(name, version.min(4)) => {
                    state
                        .outputs
                        .insert(name, registry.bind(name, version.min(4), qh, name));
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
            wl_registry::Event::GlobalRemove { name } if state.outputs.contains_key(&name) => {
                state.output_scale.remove(name);
                if let Some(output) = state.outputs.remove(&name) {
                    if output.version() >= 3 {
                        output.release();
                    } else {
                        let _ = connection.backend().destroy_object(&output.id());
                    }
                }
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
            let (width, height) = state.paint.pending_size;
            let error = state.paint.configure(width, height).err();
            if error != state.paint.configure_error
                && let Some(error) = error
            {
                eprintln!(
                    "Ptyra configure refused: {error}; presentation paused, Shells retained; resize smaller or leave maximized/fullscreen mode"
                );
            }
            state.paint.configure_error = error;
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
                state.paint.pending_size = (width, height);
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
    fn runtime_oversized_configure_keeps_valid_sessions() {
        use crate::{
            TabId,
            tabs::{TabPty, TabRegistry},
        };
        use std::{cell::Cell, rc::Rc};
        struct Fake(TabId, Rc<Cell<usize>>);
        impl TabPty for Fake {
            fn tab(&self) -> TabId {
                self.0
            }
        }
        impl Drop for Fake {
            fn drop(&mut self) {
                self.1.set(self.1.get() + 1);
            }
        }
        let dropped = Rc::new(Cell::new(0));
        let mut tabs = TabRegistry::with_scrollback(10_000).unwrap();
        for _ in 0..5 {
            sessions::spawn_tab(&mut tabs, 20, 80, |r| Ok(Fake(r.tab, dropped.clone()))).unwrap();
        }
        let reserved = tabs.reserved_cells();
        let mut state = PaintState::default();
        state.configure(800, 600).unwrap();
        state.submitted();
        // Staging a role event alone must not change the current presentation.
        state.pending_size = (i32::MAX, i32::MAX);
        assert!(state.configured);
        assert_eq!(state.size, (800, 600));
        for (width, height) in [(i32::MAX, i32::MAX), (-1, 600), (4096, 2048)] {
            assert!(state.configure(width, height).is_err());
            assert!(
                !state.ready(),
                "never commit an old-size buffer after refusal"
            );
            assert_eq!(state.size, (800, 600));
            assert_eq!(state.buffer_size, (800, 600));
            assert!(
                state.frame_pending,
                "keep ownership of the outstanding callback"
            );
            for id in tabs.ids().collect::<Vec<_>>() {
                let tab = tabs.get_mut(id).unwrap();
                assert!(tab.pty().is_some());
                tab.terminal_mut().feed(id, b"\x1b[Hstill alive").unwrap();
            }
            assert_eq!(dropped.get(), 0);
            assert_eq!(tabs.reserved_cells(), reserved);
        }
        // A new configure grants a response even if the old frame callback is withheld.
        state.configure(640, 480).unwrap();
        assert!(state.ready());
        state.submitted();
        state.dirty = true;
        assert!(
            !state.ready(),
            "ordinary redraws still wait for that callback"
        );
        state.frame_pending = false;
        assert!(state.ready());
        assert!(tabs.resize_all(10, 40).unwrap());
        assert_eq!(
            tabs.reserved_cells(),
            reserved,
            "retain high-water capacity"
        );
        assert!(
            tabs.resize_all(20, 150).is_err(),
            "combined Tab budget still applies"
        );
        for id in tabs.ids() {
            let snapshot = tabs.get(id).unwrap().terminal().snapshot().unwrap();
            assert_eq!((snapshot.rows, snapshot.columns), (10, 40));
            assert!(tabs.get(id).unwrap().pty().is_some());
        }
        // Pixel-valid wide/thin shapes and tiny fonts can still exceed u16/grid/history caps.
        for history in [0, 5000, 10_000] {
            let mut first = TabRegistry::<Fake>::with_scrollback(history).unwrap();
            for requested in [(1, 4_194_304), (4_194_304, 2), (1000, 1000), (24, 80)] {
                let bounded = first.initial_size(requested.0, requested.1);
                assert!(crate::terminal::cell_reservation(bounded.0, bounded.1, history).is_some());
                assert!(u32::from(bounded.0) <= requested.0 && u32::from(bounded.1) <= requested.1);
                let id = sessions::spawn_tab(&mut first, bounded.0, bounded.1, |r| {
                    Ok(Fake(r.tab, dropped.clone()))
                })
                .unwrap();
                assert!(first.resize_all(1000, 1000).is_err());
                let snapshot = first.get(id).unwrap().terminal().snapshot().unwrap();
                assert_eq!(
                    (snapshot.rows, snapshot.columns),
                    (usize::from(bounded.0), usize::from(bounded.1))
                );
                first.close(id);
            }
            assert_eq!(first.initial_size(24, 80), (24, 80));
        }
        for logical in [(2048, 2048), (4_194_304, 1), (1, 4_194_304)] {
            state.configure(logical.0, logical.1).unwrap();
            assert!(scale::buffer_size(state.size, 1).is_some());
            assert!(scale::buffer_size(state.size, 2).is_none());
        }
    }

    #[test]
    fn runtime_window_waits_for_configure() {
        let mut state = PaintState::default();
        assert!(!state.ready());
        state.pending_size = (800, 600);
        assert!(
            !state.ready(),
            "toplevel state waits for the surface configure"
        );
        let (width, height) = state.pending_size;
        state.configure(width, height).unwrap();
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
        assert!(state.geometry_presented);
        state.configure(801, 600).unwrap();
        assert!(!state.geometry_presented);
        state.configure(800, 600).unwrap();
        assert!(state.configure(-1, 600).is_err());
        assert!(state.configure(i32::MAX, i32::MAX).is_err());
        state.configure(0, 0).unwrap();
        assert_eq!(state.size, (800, 600));
    }
}

#[cfg(test)]
mod resource_tests;

#[cfg(test)]
mod failure_tests;
