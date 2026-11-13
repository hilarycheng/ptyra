//! Opt-in lifetime counter. No per-frame I/O or terminal/input payloads.
use std::io::Write;

pub(super) struct Frames(Option<u64>);

impl Frames {
    pub(super) fn from_env() -> Self {
        Self(
            (std::env::var_os("PTYRA_BENCHMARK_FRAMES").as_deref()
                == Some(std::ffi::OsStr::new("1")))
            .then_some(0),
        )
    }

    pub(super) fn submitted(&mut self) {
        if let Some(count) = &mut self.0 {
            *count = count.saturating_add(1);
        }
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        if let Some(count) = self.0 {
            let _ = writeln!(
                std::io::stderr().lock(),
                "Ptyra benchmark: version={} frames={count}",
                env!("CARGO_PKG_VERSION")
            );
        }
    }
}

#[test]
fn benchmark_frames_count_only_submissions_when_enabled() {
    let mut enabled = Frames(Some(0));
    let mut disabled = Frames(None);
    for _ in 0..3 {
        enabled.submitted();
        disabled.submitted();
    }
    assert_eq!(enabled.0.take(), Some(3));
    assert_eq!(disabled.0, None);
}
