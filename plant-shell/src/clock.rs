//! Monotonic — the shared time base for the sampler and its consumers.
//!
//! Freshness ([`plant_core::observe`]) is measured against a [`Tick`]: the writer
//! stamps each reading and the readers ask "how old is it now?". For that
//! comparison to mean anything, both sides must read the *same* clock — a
//! monotonic one, so a wall-clock adjustment (NTP, an RTC sync) can never make a
//! fresh reading look ancient or a stale one look fresh.
//!
//! [`Monotonic`] wraps [`Instant`] and hands out milliseconds since a fixed
//! origin. It is [`Copy`], so the composition root makes one and copies it to the
//! sampler thread and every consumer, all sharing that single origin.

use std::time::Instant;

use plant_core::Tick;

/// A monotonic millisecond clock from a fixed origin.
///
/// Copying a `Monotonic` copies its origin, so every copy agrees on "now" — the
/// property [`plant_core::observe`] relies on. Only differences from the origin are
/// exposed, never wall-clock time, so the reported tick is immune to clock
/// adjustments.
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
    /// Saturates at [`u64::MAX`] rather than wrapping — a monitor would run for
    /// nearly six hundred million years before that mattered, but a saturating
    /// cast keeps the clock monotonic even at the impossible edge.
    pub fn now(&self) -> Tick {
        self.origin.elapsed().as_millis().min(u128::from(Tick::MAX)) as Tick
    }
}
