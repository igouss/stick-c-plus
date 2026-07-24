//! The phase clock: wall-clock milliseconds in, an animation phase on the ring out.
//!
//! The field ([`crate::field`]) is a pure function of a phase `t`; this is where `t` comes
//! from. The original advances `t` by `π/240` on every `draw()` call — a per-frame step, not a
//! per-second rate — so the frond's speed is tied to the frame cadence. [`phase`] reproduces
//! that exactly: it counts whole frames elapsed and multiplies by the per-frame step, so the
//! motion is identical to the source at the cadence the source assumed.
//!
//! ## Why it wraps to one turn
//!
//! Every use of `t` in the field is inside a sine or a cosine, so `t` and `t + 2π` are the same
//! picture. Left to grow, `t` would reach thousands of radians after an hour and an `f32` would
//! start to lose the low bits of the phase. So [`phase`] reduces the frame count modulo one
//! period *before* it ever becomes a float: the phase handed to the field is always in
//! `[0, 2π)`, exact to the frame, however long the animation has been running.

/// Milliseconds of wall-clock time per animation frame — the cadence the per-frame phase step
/// is calibrated to.
///
/// Twenty frames a second. The render loop's animation cadence is set to match, so one repaint
/// advances the phase by exactly one frame and the frond moves at the speed the original
/// intended. A composition root that repaints faster or slower changes only the smoothness, not
/// the speed: the phase is a function of *time*, not of repaint count.
pub const FRAME_MS: u64 = 50;

/// How far the phase advances per frame — the original's `π/240`.
pub const PHASE_PER_FRAME: f32 = core::f32::consts::PI / 240.0;

/// How many frames complete one full turn of the phase: `2π / (π/240)` = 480.
///
/// The frame count is reduced modulo this before becoming a float, so the phase never leaves
/// `[0, 2π)` and never loses precision — see the module docs. A `u64` of milliseconds would
/// take years to overflow even without the reduction; the reduction is for float precision, not
/// for the counter.
pub const PERIOD_FRAMES: u64 = 480;

/// The animation phase after `elapsed_ms` of wall-clock time.
///
/// Whole frames elapsed, reduced onto one period, times the per-frame step: always in
/// `[0, 2π)`. A pure function of time — the same millisecond always yields the same phase — so
/// the render loop can compare two moments' pictures for equality and a dropped repaint costs
/// nothing but smoothness.
pub fn phase(elapsed_ms: u64) -> f32 {
    let frame: u64 = (elapsed_ms / FRAME_MS) % PERIOD_FRAMES;
    frame as f32 * PHASE_PER_FRAME
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::TAU;

    /// Zero: at the start of time the phase is at the ring's origin.
    #[test]
    fn time_zero_is_phase_zero() {
        assert_eq!(phase(0), 0.0);
    }

    /// One: one frame's worth of time advances the phase by exactly one step.
    #[test]
    fn one_frame_advances_by_one_step() {
        assert!((phase(FRAME_MS) - PHASE_PER_FRAME).abs() < 1e-6);
    }

    /// Many: the phase wraps to one turn — a full period later reads the same as the start, so
    /// a forever-running clock never leaves `[0, 2π)`.
    #[test]
    fn a_full_period_wraps_to_the_start() {
        let period_ms: u64 = PERIOD_FRAMES * FRAME_MS;
        assert_eq!(phase(0), phase(period_ms));
        assert_eq!(phase(FRAME_MS), phase(period_ms + FRAME_MS));
    }

    /// The phase never leaves the ring, however long the clock has run — the reason the count
    /// is reduced before it becomes a float.
    #[test]
    fn the_phase_stays_on_the_ring() {
        // A day of frames.
        let a_day_ms: u64 = 24 * 60 * 60 * 1_000;
        let mut ms: u64 = 0;
        while ms < a_day_ms {
            let t: f32 = phase(ms);
            assert!((0.0..TAU).contains(&t), "phase {t} off the ring at {ms} ms");
            ms += 7_919; // a prime stride, so the samples do not all land on a frame boundary
        }
    }

    /// Within a single frame the phase does not move: it is a per-frame step, not a continuous
    /// rate, so every millisecond of a frame shows the same picture as its first.
    #[test]
    fn the_phase_is_constant_within_a_frame() {
        assert_eq!(phase(0), phase(FRAME_MS - 1));
        assert_eq!(phase(FRAME_MS), phase(2 * FRAME_MS - 1));
    }
}
