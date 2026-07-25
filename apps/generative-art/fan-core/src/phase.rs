//! The phase clock: wall-clock milliseconds in, an animation phase on the ring out.
//!
//! The fan ([`crate::fan`]) is a pure function of a phase `φ`; this is where `φ` comes from. The
//! original advances its fold by a small step every `draw()` call; here the fold is a
//! **continuous** function of time instead, so the render loop may repaint as fast as the hardware
//! allows and the fan neither speeds up nor slows down — a faster repaint only samples the same
//! fold more finely, which is the whole of what "smoother" means.
//!
//! ## Why it wraps to one turn
//!
//! Every use of `φ` in the fan is inside a sine, so `φ` and `φ + 2π` are the same picture — the
//! whole fan returns to its start after one turn. Left to grow, `φ` would reach thousands of
//! radians after an hour and an `f32` would start to lose the low bits of the phase. So [`phase`]
//! reduces the clock modulo one period ([`PERIOD_MS`]) *before* it ever becomes a float: the phase
//! handed to the fan is always in `[0, 2π)`, exact, however long the animation has been running.

use core::f32::consts::TAU;

/// One full fold cycle of the fan, in milliseconds: the time for the phase to sweep one turn and
/// the whole pattern to return to where it began.
///
/// A little over three and a half seconds a cycle — a touch calmer than the busy triangle field
/// wants to run. The clock is reduced modulo this before it becomes a float, so the phase never
/// leaves `[0, 2π)` and an `f32` never loses the low bits of a long-running phase.
pub const PERIOD_MS: u64 = 3_600;

/// The animation phase after `elapsed_ms` of wall-clock time — a *continuous* function of time on
/// the ring `[0, 2π)`.
///
/// A pure function of time: the same millisecond always yields the same phase, so the render loop
/// can compare two moments for equality, and a full period later reads exactly the same. The clock
/// is reduced modulo [`PERIOD_MS`] as an integer *before* the multiply, so the phase is on the ring
/// however long the animation has run.
pub fn phase(elapsed_ms: u64) -> f32 {
    let on_ring: u64 = elapsed_ms % PERIOD_MS;
    on_ring as f32 / PERIOD_MS as f32 * TAU
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero: at the start of time the phase is at the ring's origin.
    #[test]
    fn time_zero_is_phase_zero() {
        assert_eq!(phase(0), 0.0);
    }

    /// One: halfway through a period the phase is halfway round the ring — the clock is a linear
    /// sweep of time, so half the time is half the turn.
    #[test]
    fn half_a_period_is_half_a_turn() {
        assert!((phase(PERIOD_MS / 2) - TAU / 2.0).abs() < 1e-4);
    }

    /// Many: a full period later reads exactly the origin again, so a forever-running clock never
    /// leaves the ring and the pattern loops seamlessly.
    #[test]
    fn a_full_period_wraps_to_the_start() {
        assert_eq!(phase(0), phase(PERIOD_MS));
        assert_eq!(phase(PERIOD_MS / 4), phase(PERIOD_MS + PERIOD_MS / 4));
    }

    /// The phase never leaves the ring, however long the clock has run — the reason the count is
    /// reduced before it becomes a float.
    #[test]
    fn the_phase_stays_on_the_ring() {
        let a_day_ms: u64 = 24 * 60 * 60 * 1_000;
        let mut ms: u64 = 0;
        while ms < a_day_ms {
            let t: f32 = phase(ms);
            assert!((0.0..TAU).contains(&t), "phase {t} off the ring at {ms} ms");
            ms += 7_919;
        }
    }
}
