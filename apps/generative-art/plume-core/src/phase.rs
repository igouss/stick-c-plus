//! The phase clock: wall-clock milliseconds in, an animation phase on the ring out.
//!
//! The field ([`crate::field`]) is a pure function of a phase `t`; this is where `t` comes
//! from. The original advances `t` by `π/240` on every `draw()` call — a per-frame step at a
//! roughly fixed cadence, so the step and that cadence together are the frond's *speed*: one
//! per-frame step every [`FRAME_MS`] milliseconds. [`phase`] keeps that speed but makes `t` a
//! **continuous** function of time — fractional frames elapsed, not whole ones — so it lands
//! exactly on the source's values at every whole frame yet fills in the motion between them.
//!
//! ## Speed is calibrated; smoothness is free
//!
//! Because the phase is a function of *time* and not of repaint count, the render loop may
//! repaint as fast as the hardware allows and the frond neither speeds up nor slows down — a
//! faster repaint only samples the same sweep more finely, which is the whole of what "smoother"
//! means here. The 20-frames-a-second the source assumed is [`FRAME_MS`]; the glass is free to
//! run at three times that and show three times the distinct pictures of the *same* motion.
//!
//! ## Why it wraps to one turn
//!
//! Every use of `t` in the field is inside a sine or a cosine, so `t` and `t + 2π` are the same
//! picture. Left to grow, `t` would reach thousands of radians after an hour and an `f32` would
//! start to lose the low bits of the phase. So [`phase`] reduces the clock modulo one period
//! ([`PERIOD_MS`]) *before* it ever becomes a float: the phase handed to the field is always in
//! `[0, 2π)`, exact, however long the animation has been running.

/// Milliseconds of wall-clock time per animation frame — the cadence the per-frame phase step
/// is calibrated to, and so half of the frond's *speed*.
///
/// Twenty frames a second: one [`PHASE_PER_FRAME`] step per this many milliseconds. It is **not**
/// the repaint cadence — [`phase`] is continuous in time, so a composition root that repaints
/// faster or slower changes only the smoothness, never the speed. See the module docs.
pub const FRAME_MS: u64 = 50;

/// How far the phase advances per frame — the original's `π/240`.
pub const PHASE_PER_FRAME: f32 = core::f32::consts::PI / 240.0;

/// How many frames complete one full turn of the phase: `2π / (π/240)` = 480.
pub const PERIOD_FRAMES: u64 = 480;

/// One full turn of the phase, in milliseconds: [`PERIOD_FRAMES`] × [`FRAME_MS`] = 24 000.
///
/// The clock is reduced modulo this before it becomes a float, so the phase never leaves
/// `[0, 2π)` and an `f32` never loses the low bits of a long-running phase — see the module docs.
/// A `u64` of milliseconds would take years to overflow even without the reduction; the reduction
/// is for float precision, not for the counter.
pub const PERIOD_MS: u64 = PERIOD_FRAMES * FRAME_MS;

/// The animation phase after `elapsed_ms` of wall-clock time — a *continuous* function of time.
///
/// Fractional frames elapsed times the per-frame step, reduced onto one period: always in
/// `[0, 2π)`. A pure function of time — the same millisecond always yields the same phase, so the
/// render loop can still compare two moments for equality — and it lands exactly on the source's
/// per-frame values whenever `elapsed_ms` is a whole multiple of [`FRAME_MS`], interpolating
/// between them the rest of the time. A dropped repaint costs nothing but smoothness; an extra one
/// buys exactly that.
pub fn phase(elapsed_ms: u64) -> f32 {
    let on_ring: u64 = elapsed_ms % PERIOD_MS;
    on_ring as f32 / FRAME_MS as f32 * PHASE_PER_FRAME
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

    /// Many: every whole frame lands on the source's discrete per-frame value — the phase is
    /// continuous, but at the frame boundaries it is *exactly* what the original stepped to. This
    /// is what keeps the goldens (sampled on frame boundaries) the canonical frames of the motion.
    #[test]
    fn whole_frames_land_on_the_source_steps() {
        for k in [0_u64, 1, 2, 7] {
            let expected: f32 = k as f32 * PHASE_PER_FRAME;
            assert!(
                (phase(k * FRAME_MS) - expected).abs() < 1e-6,
                "frame {k}: {} vs {expected}",
                phase(k * FRAME_MS)
            );
        }
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

    /// Within a single frame the phase now *moves*: it is a continuous rate, not a per-frame
    /// step, so a sub-frame moment sits strictly between its bounding frames — and, being time,
    /// halfway through a frame is halfway through a step. This is the smoothness a faster repaint
    /// buys, stated as a property: no repaint between two frames is a duplicate of either.
    #[test]
    fn the_phase_moves_within_a_frame() {
        let start: f32 = phase(0);
        let half: f32 = phase(FRAME_MS / 2);
        let whole: f32 = phase(FRAME_MS);
        assert!(start < half && half < whole, "{start} < {half} < {whole}");
        assert!(
            (half - PHASE_PER_FRAME / 2.0).abs() < 1e-6,
            "half a frame is half a step: {half}"
        );
    }
}
