use crate::{
    AppEffect, WindowId,
    clock::Clock,
    damage::{Damage, Rect},
    error::{AppError, Component},
    window::WindowSize,
};
use std::{io, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameToken(u64);

/// The adapter requests this callback with the same commit that presents damage.
pub struct FramePlan {
    pub window: WindowId,
    pub callback: FrameToken,
    pub damage: Rect,
    pub requested_at: Duration,
}

pub struct FrameScheduler {
    window: WindowId,
    dirty: Damage,
    pending: Option<(FrameToken, Rect)>,
    next_token: u64,
    stopped: bool,
}

impl FrameScheduler {
    pub fn new(window: WindowId, size: WindowSize) -> Self {
        Self {
            window,
            dirty: Damage::new(size),
            pending: None,
            next_token: 0,
            stopped: false,
        }
    }

    pub fn damage(&mut self, rect: Rect) {
        if !self.stopped {
            self.dirty.add(rect);
        }
    }
    pub fn resize(&mut self, size: WindowSize) {
        if !self.stopped {
            self.dirty.resize(size);
        }
    }

    pub fn schedule(&mut self, clock: &impl Clock) -> Result<Option<FramePlan>, AppError> {
        if self.stopped || self.pending.is_some() || !self.dirty.is_dirty() {
            return Ok(None);
        }
        let next = self
            .next_token
            .checked_add(1)
            .ok_or_else(|| AppError::new(Component::Renderer, io::ErrorKind::Other))?;
        let Some(damage) = self.dirty.take() else {
            return Ok(None);
        };
        self.next_token = next;
        let callback = FrameToken(next);
        self.pending = Some((callback, damage));
        Ok(Some(FramePlan {
            window: self.window,
            callback,
            damage,
            requested_at: clock.now(),
        }))
    }

    pub fn frame_done(&mut self, callback: FrameToken) -> bool {
        if self.pending.is_some_and(|(pending, _)| pending == callback) {
            self.pending = None;
            true
        } else {
            false
        }
    }

    /// A hidden view may never receive its pending callback. Repaint its last
    /// submitted region when shown again; any late callback becomes stale.
    pub fn suspend(&mut self) {
        if let Some((_, damage)) = self.pending.take() {
            self.dirty.add(damage);
        }
    }

    /// Stops this window permanently and returns one payload-free diagnostic.
    pub fn apply_effect(&mut self, effect: &AppEffect) -> Option<AppError> {
        if let AppEffect::StopRendering(error) = effect
            && !self.stopped
        {
            self.stopped = true;
            self.pending = None;
            self.dirty.take();
            return Some(*error);
        }
        None
    }

    /// Restore damage if the adapter cannot submit the planned frame.
    pub fn cancel(&mut self, callback: FrameToken) -> bool {
        if let Some((pending, damage)) = self.pending
            && pending == callback
        {
            self.pending = None;
            self.dirty.add(damage);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameScheduler, FrameToken};
    use crate::{WindowId, clock::FakeClock, damage::Rect, window::WindowSize};
    use std::time::Duration;

    #[test]
    fn frame_callback_does_not_busy_loop() {
        let clock = FakeClock::default();
        let mut scheduler = FrameScheduler::new(WindowId(1), WindowSize::new(800, 600).unwrap());
        let first_damage = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 10,
        };
        assert!(scheduler.schedule(&clock).unwrap().is_none());
        scheduler.damage(first_damage);
        let first = scheduler.schedule(&clock).unwrap().unwrap();
        assert_eq!(first.window, WindowId(1));
        assert_eq!(first.damage, first_damage);
        assert_eq!(first.requested_at, Duration::ZERO);
        assert!(scheduler.schedule(&clock).unwrap().is_none());
        clock.advance(Duration::from_secs(60)).unwrap();
        let later_damage = Rect {
            x: 40,
            y: 50,
            width: 30,
            height: 20,
        };
        scheduler.damage(later_damage);
        assert!(scheduler.schedule(&clock).unwrap().is_none());
        assert!(!scheduler.frame_done(FrameToken(999)));
        assert!(scheduler.frame_done(first.callback));
        let second = scheduler.schedule(&clock).unwrap().unwrap();
        assert_ne!(first.callback, second.callback);
        assert_eq!(second.damage, later_damage);
        assert_eq!(second.requested_at, Duration::from_secs(60));
        assert!(!scheduler.frame_done(first.callback));
        assert!(scheduler.cancel(second.callback));
        let retry = scheduler.schedule(&clock).unwrap().unwrap();
        assert_eq!(retry.damage, later_damage);
        assert!(scheduler.frame_done(retry.callback));
        clock.advance(Duration::from_secs(3600)).unwrap();
        assert!(scheduler.schedule(&clock).unwrap().is_none());
        scheduler.resize(WindowSize::new(100, 100).unwrap());
        let resized = scheduler.schedule(&clock).unwrap().unwrap();
        assert_eq!(
            resized.damage,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100
            }
        );
        assert!(scheduler.frame_done(resized.callback));
        scheduler.next_token = u64::MAX;
        scheduler.damage(first_damage);
        assert!(scheduler.schedule(&clock).is_err());
        assert!(scheduler.dirty.is_dirty());
    }
}
