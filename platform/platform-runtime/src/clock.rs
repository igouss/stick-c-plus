//! Monotonic — the board's shared time base, and the [`Clock`] port's one adapter.
//!
//! Time-dependent rules (a reading's freshness, a creature's animation frame) are measured
//! against a [`Tick`]: the writer stamps, the readers ask "how old is it now?". For that to
//! mean anything, both sides must read the *same* monotonic clock, so a wall-clock
//! adjustment (NTP, an RTC sync) can never make a fresh value look ancient.
//!
//! [`Monotonic`] wraps [`Instant`] and hands out milliseconds since a fixed origin. It is
//! [`Copy`], so a composition root makes one and copies it to every thread, all sharing that
//! origin — and it is the concrete adapter for the [`Clock`] port, so the domain and the
//! shells take an injected `impl Clock` and never reach for wall time themselves.

use std::time::Instant;

use platform_core::{Clock, Tick};

/// A monotonic millisecond clock from a fixed origin.
///
/// Copying a `Monotonic` copies its origin, so every copy agrees on "now". Only differences
/// from the origin are exposed, never wall-clock time, so the reported tick is immune to
/// clock adjustments.
#[derive(Clone, Copy)]
pub struct Monotonic {
    origin: Instant,
}

impl Monotonic {
    /// Start a clock whose origin is the moment of the call.
    pub fn start() -> Self {
        Self {
            origin: Instant::now(),
        }
    }

    /// Milliseconds elapsed since the origin, as a [`Tick`].
    ///
    /// Saturates at [`u64::MAX`] rather than wrapping — a device would run for nearly six
    /// hundred million years before that mattered, but a saturating cast keeps the clock
    /// monotonic even at the impossible edge.
    pub fn now(&self) -> Tick {
        self.origin.elapsed().as_millis().min(u128::from(Tick::MAX)) as Tick
    }
}

impl Clock for Monotonic {
    fn now(&self) -> Tick {
        Monotonic::now(self)
    }
}
