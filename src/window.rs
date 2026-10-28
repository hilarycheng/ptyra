use crate::{
    WindowId,
    error::{AppError, Component},
    wayland::{Capabilities, Protocol},
};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRequest {
    CreateSurface,
    GetXdgSurface,
    GetToplevel,
    SetTitle(&'static str),
    SetAppId(&'static str),
    CommitEmpty,
}

/// Requests target one new window. Buffer attachment awaits configure/ack.
#[derive(Debug, PartialEq, Eq)]
pub struct ToplevelPlan {
    pub window: WindowId,
    pub requests: [WindowRequest; 6],
}

impl ToplevelPlan {
    pub fn new(window: WindowId, capabilities: &Capabilities) -> Result<Self, AppError> {
        for protocol in [Protocol::Compositor, Protocol::XdgWmBase] {
            if capabilities.binding(protocol, 1, 1).is_none() {
                return Err(AppError::new(Component::Wayland, io::ErrorKind::NotFound));
            }
        }
        Ok(Self {
            window,
            requests: [
                WindowRequest::CreateSurface,
                WindowRequest::GetXdgSurface,
                WindowRequest::GetToplevel,
                WindowRequest::SetTitle(crate::APP_NAME),
                WindowRequest::SetAppId("ptyra"),
                WindowRequest::CommitEmpty,
            ],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSize {
    width: u32,
    height: u32,
}

impl WindowSize {
    pub fn new(width: u32, height: u32) -> Result<Self, AppError> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(AppError::new(
                Component::Wayland,
                io::ErrorKind::InvalidInput,
            ));
        }
        Ok(Self { width, height })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckConfigure {
    pub window: WindowId,
    pub serial: u32,
    pub size: WindowSize,
}

pub struct ConfigureState {
    window: WindowId,
    size: WindowSize,
    staged: Option<WindowSize>,
    pending: Option<AckConfigure>,
    configured: bool,
}

impl ConfigureState {
    pub fn new(window: WindowId, size: WindowSize) -> Self {
        Self {
            window,
            size,
            staged: None,
            pending: None,
            configured: false,
        }
    }

    pub fn size(&self) -> WindowSize {
        self.size
    }

    pub fn can_attach_buffer(&self) -> bool {
        self.configured
    }

    pub fn stage_toplevel(&mut self, width: i32, height: i32) -> Result<(), AppError> {
        if width < 0 || height < 0 {
            return Err(AppError::new(
                Component::Wayland,
                io::ErrorKind::InvalidInput,
            ));
        }
        self.staged = Some(WindowSize::new(
            if width == 0 {
                self.size.width
            } else {
                width as u32
            },
            if height == 0 {
                self.size.height
            } else {
                height as u32
            },
        )?);
        Ok(())
    }

    pub fn surface_configure(&mut self, serial: u32) {
        let size = self
            .staged
            .take()
            .unwrap_or_else(|| self.pending.map_or(self.size, |pending| pending.size));
        // Serial values are opaque; only arrival order decides the latest update.
        self.pending = Some(AckConfigure {
            window: self.window,
            serial,
            size,
        });
    }

    /// Consumes one ack effect. The adapter must send it before attaching a buffer.
    pub fn acknowledge(&mut self) -> Option<AckConfigure> {
        let ack = self.pending.take()?;
        self.size = ack.size;
        self.configured = true;
        Some(ack)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigureState, ToplevelPlan, WindowRequest, WindowSize};
    use crate::{
        WindowId,
        wayland::{Capabilities, Global},
    };

    #[test]
    fn toplevel_request_order() {
        let capabilities = Capabilities::from_globals(&[
            Global {
                name: 1,
                interface: "wl_compositor",
                version: 6,
            },
            Global {
                name: 2,
                interface: "xdg_wm_base",
                version: 7,
            },
        ]);
        let plan = ToplevelPlan::new(WindowId(7), &capabilities).unwrap();
        assert_eq!(plan.window, WindowId(7));
        assert_eq!(
            plan.requests,
            [
                WindowRequest::CreateSurface,
                WindowRequest::GetXdgSurface,
                WindowRequest::GetToplevel,
                WindowRequest::SetTitle("Ptyra"),
                WindowRequest::SetAppId("ptyra"),
                WindowRequest::CommitEmpty,
            ]
        );
        assert!(ToplevelPlan::new(WindowId(8), &Capabilities::default()).is_err());
        let compositor_only = Capabilities::from_globals(&[Global {
            name: 1,
            interface: "wl_compositor",
            version: 1,
        }]);
        assert!(ToplevelPlan::new(WindowId(8), &compositor_only).is_err());
    }

    #[test]
    fn configure_ack_matches_serial() {
        let initial = WindowSize::new(800, 600).unwrap();
        let mut state = ConfigureState::new(WindowId(7), initial);
        assert!(!state.can_attach_buffer());
        assert!(state.acknowledge().is_none());
        state.stage_toplevel(1024, 0).unwrap();
        assert_eq!(state.size(), initial);
        assert!(state.acknowledge().is_none());
        state.surface_configure(u32::MAX);
        let ack = state.acknowledge().unwrap();
        assert_eq!(ack.window, WindowId(7));
        assert_eq!(ack.serial, u32::MAX);
        assert_eq!(ack.size, WindowSize::new(1024, 600).unwrap());
        assert_eq!(state.size(), ack.size);
        assert!(state.can_attach_buffer());
        assert!(state.acknowledge().is_none());

        state.stage_toplevel(0, 768).unwrap();
        state.surface_configure(42);
        state.stage_toplevel(1280, 720).unwrap();
        state.surface_configure(0);
        let ack = state.acknowledge().unwrap();
        assert_eq!(ack.serial, 0);
        assert_eq!(ack.size, WindowSize::new(1280, 720).unwrap());
        assert!(state.acknowledge().is_none());

        state.stage_toplevel(900, 700).unwrap();
        state.surface_configure(7);
        state.surface_configure(6);
        let ack = state.acknowledge().unwrap();
        assert_eq!(ack.serial, 6);
        assert_eq!(ack.size, WindowSize::new(900, 700).unwrap());
        assert!(state.stage_toplevel(-1, 10).is_err());
        assert!(state.stage_toplevel(10, -1).is_err());
        state.stage_toplevel(0, 0).unwrap();
        state.surface_configure(8);
        assert_eq!(state.acknowledge().unwrap().size, ack.size);
        assert!(WindowSize::new(0, 600).is_err());
        assert!(WindowSize::new(800, 0).is_err());
        assert!(WindowSize::new(u32::MAX, 600).is_err());
    }
}
