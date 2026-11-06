//! Integer surface/buffer scale, with bounded output state and allocation checks.
use super::{MAX_PIXELS, Runtime};
use crate::input::Point;
use std::collections::BTreeMap;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_output, wl_surface},
};

// Legacy outputs lack release; cap their lifetime bindings as well as live outputs.
const MAX_OUTPUTS: usize = 64;
const MAX_SCALE: u32 = 4;
#[derive(Default)]
pub(super) struct State {
    outputs: BTreeMap<u32, Output>,
    preferred: Option<u32>,
    legacy_bindings: usize,
}
struct Output {
    current: u32,
    pending: u32,
    entered: bool,
}
impl State {
    pub fn add(&mut self, name: u32, version: u32) -> bool {
        if self.outputs.len() == MAX_OUTPUTS
            || self.outputs.contains_key(&name)
            || (version < 3 && self.legacy_bindings == MAX_OUTPUTS)
        {
            return false;
        }
        self.legacy_bindings += usize::from(version < 3);
        self.outputs.insert(
            name,
            Output {
                current: 1,
                pending: 1,
                entered: false,
            },
        );
        true
    }
    pub fn remove(&mut self, name: u32) {
        if self
            .outputs
            .remove(&name)
            .is_some_and(|output| output.entered)
        {
            // Some compositors omit a replacement preference after output removal.
            self.preferred = None;
        }
    }
    fn stage(&mut self, name: u32, factor: i32) {
        if factor > 0
            && let Some(output) = self.outputs.get_mut(&name)
        {
            output.pending = factor as u32;
        }
    }
    fn done(&mut self, name: u32) {
        if let Some(output) = self.outputs.get_mut(&name) {
            output.current = output.pending;
        }
    }
    fn enter(&mut self, name: u32, entered: bool) {
        if let Some(output) = self.outputs.get_mut(&name) {
            output.entered = entered;
        }
    }
    fn prefer(&mut self, factor: i32) {
        if factor > 0 {
            self.preferred = Some(factor as u32);
        }
    }
    fn requested(&self) -> u32 {
        self.preferred.unwrap_or_else(|| {
            self.outputs
                .values()
                .filter(|o| o.entered)
                .map(|o| o.current)
                .max()
                .unwrap_or(1)
        })
    }
}

pub(super) fn buffer_size(logical: (u32, u32), factor: u32) -> Option<(u32, u32)> {
    let size = crate::window::WindowSize::new(logical.0, logical.1).ok()?;
    let scale = crate::geometry::Scale::new(factor.try_into().ok()?).ok()?;
    let size = scale.to_physical(size).ok()?;
    (u64::from(size.width()) * u64::from(size.height()) <= MAX_PIXELS as u64)
        .then_some((size.width(), size.height()))
}
fn bounded_scale(logical: (u32, u32), cell: (u32, u32), requested: u32) -> u32 {
    // Lower-resolution integer buffers remain valid when the preferred scale exceeds our caps.
    (1..=requested.clamp(1, MAX_SCALE))
        .rev()
        .find(|&factor| {
            buffer_size(logical, factor).is_some()
                && cell.0.checked_mul(factor).is_some_and(|v| v <= 256)
                && cell.1.checked_mul(factor).is_some_and(|v| v <= 512)
        })
        .unwrap_or(1)
}
pub(super) fn buffer_point(point: Point, factor: u32) -> Option<Point> {
    Point::new(point.x() * f64::from(factor), point.y() * f64::from(factor)).ok()
}
impl Runtime {
    pub(super) fn sync_scale(&mut self) {
        if !self.paint.configured {
            return;
        }
        let factor = bounded_scale(
            self.paint.size,
            self.raster.logical_cell(),
            self.output_scale.requested(),
        );
        let size = buffer_size(self.paint.size, factor).expect("validated logical surface size");
        if self.paint.scale != factor || self.paint.buffer_size != size {
            self.raster.set_scale(factor);
            self.paint.scale = factor;
            self.paint.buffer_size = size;
            self.paint.geometry_presented = false;
            self.paint.dirty = true;
            self.presented_bar = None;
            self.presented_hyperlink = None;
            self.pointer_input.cancel_drag();
            self.selection_input.cancel(&mut self.tabs);
            self.selection_input.set_scale(factor);
        }
    }
}
impl Dispatch<wl_output::WlOutput, u32> for Runtime {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        name: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.outputs.get(name) != Some(output) {
            return;
        }
        match event {
            wl_output::Event::Scale { factor } => state.output_scale.stage(*name, factor),
            wl_output::Event::Done => state.output_scale.done(*name),
            _ => {}
        }
    }
}
impl Dispatch<wl_surface::WlSurface, ()> for Runtime {
    fn event(
        state: &mut Self,
        surface: &wl_surface::WlSurface,
        event: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if state.surface.as_ref() != Some(surface) {
            return;
        }
        match event {
            wl_surface::Event::Enter { output } => {
                if let Some(name) = output.data::<u32>()
                    && state.outputs.get(name) == Some(&output)
                {
                    state.output_scale.enter(*name, true);
                }
            }
            wl_surface::Event::Leave { output } => {
                if let Some(name) = output.data::<u32>()
                    && state.outputs.get(name) == Some(&output)
                {
                    state.output_scale.enter(*name, false);
                }
            }
            wl_surface::Event::PreferredBufferScale { factor } => state.output_scale.prefer(factor),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_scale_keeps_logical_and_buffer_coordinates_consistent() {
        let mut state = State::default();
        assert_eq!(state.requested(), 1);
        assert!(state.add(10, 4));
        assert!(state.add(20, 4));
        state.stage(10, 2);
        state.enter(10, true);
        assert_eq!(state.requested(), 1, "output changes are atomic at done");
        state.done(10);
        assert_eq!(state.requested(), 2);
        state.stage(20, 3);
        state.done(20);
        assert_eq!(state.requested(), 2, "an unentered output has no effect");
        state.enter(20, true);
        assert_eq!(state.requested(), 3);
        state.enter(20, false);
        assert_eq!(state.requested(), 2);
        state.prefer(1);
        assert_eq!(
            state.requested(),
            1,
            "surface preference overrides output maximum"
        );
        state.prefer(0);
        state.stage(10, -1);
        state.done(10);
        assert_eq!(state.requested(), 1);
        state.prefer(2);
        state.remove(10);
        assert_eq!(
            state.requested(),
            1,
            "removed output invalidates a stale surface preference"
        );
        state.enter(20, true);
        assert_eq!(state.requested(), 3);
        state.prefer(2);
        assert_eq!(
            state.requested(),
            2,
            "fresh surface preference overrides output maximum"
        );
        state.enter(20, false);
        assert_eq!(
            state.requested(),
            2,
            "ordinary enter/leave retains the surface preference"
        );
        state.enter(20, true);
        assert_eq!(state.requested(), 2);
        for name in 0..1000 {
            state.add(name, 4);
        }
        assert_eq!(state.outputs.len(), MAX_OUTPUTS);
        let mut legacy = State::default();
        for name in 0..MAX_OUTPUTS as u32 {
            assert!(legacy.add(name, 2));
            legacy.remove(name);
        }
        assert!(
            !legacy.add(1000, 2),
            "legacy output IDs cannot be released to the server"
        );
        assert!(
            legacy.add(1000, 3),
            "releasable modern outputs remain available"
        );
        assert_eq!(buffer_size((801, 603), 2), Some((1602, 1206)));
        for scale in [0, u32::MAX] {
            assert!(buffer_size((801, 603), scale).is_none());
        }
        assert!(buffer_size((u32::MAX, 2), 2).is_none());
        assert!(buffer_size((2048, 1025), 2).is_none());
        assert_eq!(bounded_scale((800, 600), (8, 16), 2), 2);
        assert_eq!(bounded_scale((1600, 1200), (8, 16), 2), 1);
        assert_eq!(bounded_scale((160, 80), (8, 16), u32::MAX), 4);
        assert_eq!(bounded_scale((160, 80), (200, 300), 2), 1);
        let point = crate::input::Point::new(12.5, 30.25).unwrap();
        let physical = buffer_point(point, 2).unwrap();
        assert_eq!((physical.x(), physical.y()), (25., 60.5));
    }
}
