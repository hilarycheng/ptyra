use crate::{
    TabId, WindowId,
    error::{AppError, Component},
    geometry::Scale,
};
use std::{io, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusTarget {
    pub window: WindowId,
    pub tab: TabId,
}

// Key payloads deliberately have no Debug implementation.
pub struct KeyRoute {
    pub target: FocusTarget,
    pub code: u32,
    pub pressed: bool,
    pub(crate) focus_epoch: Arc<()>,
}

/// One instance per seat keyboard. Enter synchronizes focus without replaying keys.
#[derive(Default)]
pub struct KeyboardFocus {
    target: Option<FocusTarget>,
    epoch: Arc<()>,
}

impl KeyboardFocus {
    pub fn enter(&mut self, window: WindowId, tab: TabId) {
        self.target = Some(FocusTarget { window, tab });
        self.epoch = Arc::new(());
    }

    pub fn leave(&mut self, window: WindowId) {
        if self.target.is_some_and(|target| target.window == window) {
            self.clear();
        }
    }

    /// Also used when a seat or its keyboard capability disappears.
    pub fn clear(&mut self) {
        self.target = None;
    }

    // Live tokens cannot be reused while a pending input still holds the old one.
    pub(crate) fn matches(&self, target: FocusTarget, epoch: &Arc<()>) -> bool {
        self.target == Some(target) && Arc::ptr_eq(&self.epoch, epoch)
    }

    pub fn route_key(&self, code: u32, pressed: bool) -> Option<KeyRoute> {
        Some(KeyRoute {
            target: self.target?,
            code,
            pressed,
            focus_epoch: self.epoch.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    x: f64,
    y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Result<Self, AppError> {
        let bounds = i32::MIN as f64..=i32::MAX as f64;
        if !bounds.contains(&x) || !bounds.contains(&y) {
            return Err(AppError::new(
                Component::Wayland,
                io::ErrorKind::InvalidInput,
            ));
        }
        Ok(Self { x, y })
    }
    pub fn x(&self) -> f64 {
        self.x
    }
    pub fn y(&self) -> f64 {
        self.y
    }
}

pub struct PointerClick {
    pub window: WindowId,
    pub surface: Point,
    pub buffer: Point,
    pub button: u32,
    pub pressed: bool,
}

/// Tracks the last surface-local position for one seat pointer.
#[derive(Default)]
pub struct PointerFocus {
    location: Option<(WindowId, Point)>,
}

impl PointerFocus {
    pub fn enter(&mut self, window: WindowId, position: Point) {
        self.location = Some((window, position));
    }

    pub fn motion(&mut self, position: Point) {
        if let Some((_, previous)) = &mut self.location {
            *previous = position;
        }
    }

    pub fn leave(&mut self, window: WindowId) {
        if self.location.is_some_and(|(focused, _)| focused == window) {
            self.clear();
        }
    }

    pub fn clear(&mut self) {
        self.location = None;
    }

    pub fn button(
        &self,
        button: u32,
        pressed: bool,
        scale: Scale,
    ) -> Result<Option<PointerClick>, AppError> {
        let Some((window, surface)) = self.location else {
            return Ok(None);
        };
        // wl_pointer positions are already surface-local; only buffer coordinates scale.
        let buffer = Point::new(
            surface.x * f64::from(scale.factor()),
            surface.y * f64::from(scale.factor()),
        )?;
        Ok(Some(PointerClick {
            window,
            surface,
            buffer,
            button,
            pressed,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{FocusTarget, KeyboardFocus, Point, PointerFocus};
    use crate::geometry::Scale;
    use crate::{TabId, WindowId};

    #[test]
    fn pointer_coordinates_are_scaled() {
        let scale = Scale::new(2).unwrap();
        let mut pointer = PointerFocus::default();
        assert!(pointer.button(272, true, scale).unwrap().is_none());
        pointer.enter(WindowId(1), Point::new(10.25, 20.5).unwrap());
        let click = pointer.button(272, true, scale).unwrap().unwrap();
        assert_eq!(click.window, WindowId(1));
        assert_eq!(click.surface, Point::new(10.25, 20.5).unwrap());
        assert_eq!(click.buffer, Point::new(20.5, 41.0).unwrap());
        assert_eq!(click.button, 272);
        assert!(click.pressed);
        pointer.motion(Point::new(-1.5, 30.25).unwrap());
        pointer.leave(WindowId(2));
        let release = pointer.button(272, false, scale).unwrap().unwrap();
        assert!(!release.pressed);
        assert_eq!(release.buffer, Point::new(-3.0, 60.5).unwrap());
        pointer.leave(WindowId(1));
        pointer.motion(Point::new(1.0, 1.0).unwrap());
        assert!(pointer.button(272, true, scale).unwrap().is_none());
        assert!(Point::new(f64::NAN, 0.0).is_err());
        assert!(Point::new(0.0, f64::INFINITY).is_err());
        pointer.enter(WindowId(2), Point::new(i32::MAX as f64, 1.0).unwrap());
        assert!(pointer.button(272, true, scale).is_err());
        pointer.clear();
        assert!(pointer.button(272, true, scale).unwrap().is_none());
    }

    #[test]
    fn keyboard_focus_transitions() {
        let mut focus = KeyboardFocus::default();
        assert!(focus.route_key(30, true).is_none());
        focus.enter(WindowId(1), TabId(10));
        let key = focus.route_key(30, true).unwrap();
        assert_eq!(
            key.target,
            FocusTarget {
                window: WindowId(1),
                tab: TabId(10)
            }
        );
        assert_eq!(key.code, 30);
        assert!(key.pressed);
        assert!(!focus.route_key(30, false).unwrap().pressed);
        focus.leave(WindowId(2));
        assert!(focus.route_key(30, true).is_some());
        focus.leave(WindowId(1));
        assert!(focus.route_key(30, true).is_none());
        assert!(focus.route_key(30, false).is_none());

        focus.enter(WindowId(2), TabId(20));
        focus.leave(WindowId(1));
        assert_eq!(focus.route_key(48, true).unwrap().target.tab, TabId(20));
        let mut other_seat = KeyboardFocus::default();
        other_seat.enter(WindowId(3), TabId(30));
        focus.clear();
        assert!(focus.route_key(48, true).is_none());
        assert_eq!(
            other_seat.route_key(48, true).unwrap().target.tab,
            TabId(30)
        );
    }
}
