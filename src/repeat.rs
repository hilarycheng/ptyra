use crate::{
    clock::{Clock, Deadline},
    error::{AppError, Component},
    input::{FocusTarget, KeyboardFocus},
    keymap::{KeyState, Keymap, TranslatedKey},
};
use std::{io, sync::Arc, time::Duration};

pub const MAX_REPEAT_RATE: i32 = 1000;
pub const MAX_REPEAT_DELAY_MS: i32 = 60_000;

#[derive(Clone, Copy)]
struct Timing {
    delay: Duration,
    interval: Duration,
}

struct Pending {
    code: u32,
    target: FocusTarget,
    focus_epoch: Arc<()>,
    keymap_epoch: Arc<()>,
    deadline: Deadline,
}

impl Pending {
    fn matches(&self, key: &TranslatedKey) -> bool {
        self.code == key.physical_code
            && self.target == key.target
            && Arc::ptr_eq(&self.focus_epoch, &key.focus_epoch)
            && Arc::ptr_eq(&self.keymap_epoch, &key.keymap_epoch)
    }
}

/// One instance per seat keyboard, driven by the owner's event loop and monotonic clock.
/// No timer thread or I/O; at most one held key and one deadline are retained.
#[derive(Default)]
pub struct RepeatScheduler {
    timing: Option<Timing>,
    pending: Option<Pending>,
}

fn invalid() -> AppError {
    AppError::new(Component::Wayland, io::ErrorKind::InvalidInput)
}

impl RepeatScheduler {
    /// Configuration changes cancel the current hold; invalid settings disable repeating.
    /// Rate zero disables client repeat, including when the compositor owns repetition.
    pub fn configure(&mut self, rate: i32, delay_ms: i32) -> Result<(), AppError> {
        self.cancel();
        self.timing = None;
        if rate < 0 || delay_ms < 0 {
            return Err(invalid());
        }
        if rate == 0 {
            return Ok(());
        }
        if rate > MAX_REPEAT_RATE || delay_ms > MAX_REPEAT_DELAY_MS {
            return Err(invalid());
        }
        self.timing = Some(Timing {
            delay: Duration::from_millis(delay_ms as u64),
            interval: Duration::from_nanos(1_000_000_000u64.div_ceil(rate as u64)),
        });
        Ok(())
    }

    /// Observe routed, translated input in dispatch order. The original event is not emitted here.
    /// Non-repeatable keys leave the existing hold alone; repeated events never arm it.
    pub fn on_key(&mut self, key: &TranslatedKey, clock: &impl Clock) -> Result<(), AppError> {
        let matches = self.pending.as_ref().is_some_and(|p| p.matches(key));
        match key.state {
            KeyState::Released => {
                if matches {
                    self.cancel();
                }
            }
            KeyState::Pressed if key.repeatable && !matches => {
                let Some(timing) = self.timing else {
                    return Ok(());
                };
                self.cancel();
                let deadline = Deadline::after(clock, timing.delay).ok_or_else(invalid)?;
                self.pending = Some(Pending {
                    code: key.physical_code,
                    target: key.target,
                    focus_epoch: key.focus_epoch.clone(),
                    keymap_epoch: key.keymap_epoch.clone(),
                    deadline,
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// Query before waiting or handling a wakeup; stale focus/keymap state revokes the deadline.
    pub fn deadline(&mut self, focus: &KeyboardFocus, keymap: &Keymap) -> Option<Deadline> {
        let pending = self.pending.as_ref()?;
        if !focus.matches(pending.target, &pending.focus_epoch)
            || !keymap.matches(&pending.keymap_epoch)
        {
            self.cancel();
            return None;
        }
        Some(pending.deadline)
    }

    /// Return at most one repeat, translated with current modifiers. Missed ticks are skipped.
    pub fn poll(
        &mut self,
        clock: &impl Clock,
        focus: &KeyboardFocus,
        keymap: &Keymap,
    ) -> Result<Option<TranslatedKey>, AppError> {
        if !self
            .deadline(focus, keymap)
            .is_some_and(|d| d.is_due(clock))
        {
            return Ok(None);
        }
        // Taking first ensures errors cannot leave a due deadline spinning indefinitely.
        let Some(mut pending) = self.pending.take() else {
            return Ok(None);
        };
        let Some(timing) = self.timing else {
            return Ok(None);
        };
        let Some(route) = focus.route_key(pending.code, true) else {
            return Ok(None);
        };
        let Some(key) = keymap.translate(route, true)? else {
            return Ok(None);
        };
        if !key.repeatable {
            return Ok(None);
        }
        pending.deadline = Deadline::after(clock, timing.interval).ok_or_else(invalid)?;
        self.pending = Some(pending);
        Ok(Some(key))
    }

    /// Also available to the owner when input is cancelled or consumed by another handler.
    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests;
