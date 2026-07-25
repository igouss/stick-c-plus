//! The clock: elapsed milliseconds to the source's virtual frame counter.
//!
//! The *Dwitter* is driven by `f`, a counter that ticks once per drawn frame at roughly `60` Hz.
//! Tying the animation to a repaint counter would make its speed a hostage to the board's frame
//! rate; instead the sketch is a pure function of *elapsed time*, and this maps the clock onto the
//! same virtual `f` the source's motion was tuned against — so the comet drifts at the author's
//! pace whether the panel paints at 30 fps or 50.

/// Milliseconds per virtual frame — the source's `~60` Hz, so a virtual frame is `1000 / 60` ms.
/// The whole motion (the sweep rates, the per-orbit lag) is calibrated in these frames, so keeping
/// the unit is what keeps the ported speed.
pub const FRAME_MS: f32 = 1000.0 / 60.0;

/// The virtual frame at `elapsed_ms` milliseconds: the source's `f`, made continuous.
///
/// A plain scaling of the clock, so the frame advances smoothly rather than in whole steps — the
/// centres are a continuous function of it, so a fractional frame is a real in-between position, not
/// an interpolation. Takes bare milliseconds, like the other sketches' clocks, so the domain stays
/// off the platform kernel. Over a long uptime the `f32` loses low-order precision as the count
/// grows, a sub-pixel drift the gallery's short exhibitions never reach.
pub fn frame(elapsed_ms: u64) -> f32 {
    elapsed_ms as f32 / FRAME_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero: the clock starts the motion at frame zero.
    #[test]
    fn the_clock_starts_at_zero() {
        assert_eq!(frame(0), 0.0);
    }

    /// One: one frame's worth of milliseconds is one virtual frame — the unit the source's motion is
    /// tuned in.
    #[test]
    fn one_frame_of_time_is_one_frame() {
        assert!((frame(16) - 0.96).abs() < 0.01, "frame(16) = {}", frame(16));
    }

    /// Many: the frame grows with the clock — a later moment is a later frame, so the comet keeps
    /// moving rather than stalling.
    #[test]
    fn a_later_moment_is_a_later_frame() {
        assert!(frame(1_000) > frame(500));
    }
}
