//! Native Wayland adapter. Only this module owns compositor objects and presentation I/O.
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
}

pub fn run(_config: crate::config::Config) -> Result<()> {
    let connection =
        Connection::connect_to_env().map_err(|_| "cannot connect to Wayland compositor")?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
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
        app.draw(&qh)?;
        queue
            .blocking_dispatch(&mut app)
            .map_err(|_| "Wayland connection lost")?;
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
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&0xff181818u32.to_ne_bytes());
        }
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
