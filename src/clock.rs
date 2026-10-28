use std::time::{Duration, Instant};

/// Elapsed time from a fixed origin; implementations must never go backwards.
pub trait Clock {
    fn now(&self) -> Duration;
}

pub struct MonotonicClock {
    origin: Instant,
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadline(Duration);

impl Deadline {
    pub fn after(clock: &impl Clock, delay: Duration) -> Option<Self> {
        clock.now().checked_add(delay).map(Self)
    }

    pub fn remaining(&self, clock: &impl Clock) -> Duration {
        self.0.saturating_sub(clock.now())
    }

    pub fn is_due(&self, clock: &impl Clock) -> bool {
        clock.now() >= self.0
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeClock {
    now: std::cell::Cell<Duration>,
}

#[cfg(test)]
impl FakeClock {
    pub(crate) fn advance(&self, duration: Duration) -> Option<()> {
        self.now.set(self.now.get().checked_add(duration)?);
        Some(())
    }
}

#[cfg(test)]
impl Clock for FakeClock {
    fn now(&self) -> Duration {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, Deadline, FakeClock};
    use std::time::Duration;

    #[test]
    fn fake_clock_advances_deterministically() {
        let clock = FakeClock::default();
        let deadline = Deadline::after(&clock, Duration::from_millis(500)).unwrap();
        assert_eq!(clock.now(), Duration::ZERO);
        assert!(!deadline.is_due(&clock));
        clock.advance(Duration::from_millis(499)).unwrap();
        assert_eq!(deadline.remaining(&clock), Duration::from_millis(1));
        assert!(!deadline.is_due(&clock));
        clock.advance(Duration::from_millis(1)).unwrap();
        assert!(deadline.is_due(&clock));
        clock.advance(Duration::from_secs(10)).unwrap();
        assert_eq!(deadline.remaining(&clock), Duration::ZERO);
        assert!(
            Deadline::after(&clock, Duration::ZERO)
                .unwrap()
                .is_due(&clock)
        );

        let limit = FakeClock::default();
        limit.advance(Duration::MAX).unwrap();
        assert!(Deadline::after(&limit, Duration::from_nanos(1)).is_none());
        assert!(limit.advance(Duration::from_nanos(1)).is_none());
        assert_eq!(limit.now(), Duration::MAX);
    }
}
