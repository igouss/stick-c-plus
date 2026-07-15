//! The clock port: a monotonic millisecond time base, injected not read.

use crate::tick::Tick;

/// A monotonic millisecond clock.
///
/// The driven port for "what time is it?". The firmware supplies the adapter (a `Monotonic`
/// over `std::time::Instant` in `platform-runtime`); the shells and the render loop depend
/// only on this trait, so a clock is *injected* by the composition root and the domain
/// never reaches for wall time itself. That is what keeps the domain pure and every
/// time-dependent rule host-testable against explicit [`Tick`]s.
///
/// Monotonic and non-decreasing: successive calls never go backwards, so an elapsed
/// duration is always `now - earlier` without underflow.
pub trait Clock {
    /// The current time, in milliseconds since some fixed start.
    fn now(&self) -> Tick;
}
