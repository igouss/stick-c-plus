//! `PlumeView` — the app state the render loop shows, which for an ambient animation is
//! almost nothing at all.
//!
//! Every other app on this platform has a *state* the glass reflects: a pose, a timer, a
//! moisture reading. The plume has none — it is a clock-driven ornament, the same frond
//! forever, only ever a little further along its breath. So the view carries no data. What it
//! carries is the *animation policy* the generic render loop reads: that this app is always
//! moving, and which frame is due at a given elapsed time.

use platform_core::{Animated, Tick};
use plume_core::FRAME_MS;

/// The plume's view: a marker with no data, only an animation policy.
///
/// It is [`Animated`] like every other app's view, so the plume rides the *same* generic
/// render loop the pomodoro timer and the orientation readout do — it simply answers "always
/// animating" where they answer "still until something happens".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlumeView;

impl Animated for PlumeView {
    /// There is no coarse identity to reset on — the animation clock runs unbroken from
    /// boot, because the frond has no transitions, only its one continuous breath. A constant
    /// anchor is what tells the loop never to restart that clock.
    type Anchor = ();

    fn anchor(&self) -> Self::Anchor {}

    /// Always: the frond never rests, so the loop always paints at its animation cadence.
    fn is_animated(&self) -> bool {
        true
    }

    /// Which frame is due after `elapsed_ms`: one per [`FRAME_MS`]. The index strictly
    /// increases with time, so the loop's `(state, frame)` pair changes every frame and the
    /// panel repaints — which for a continuous animation is exactly what is wanted.
    fn frame_index(&self, elapsed_ms: Tick) -> usize {
        (elapsed_ms / FRAME_MS) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frond is always animating — the property that keeps the loop on its fast cadence.
    #[test]
    fn the_plume_is_always_animated() {
        assert!(PlumeView.is_animated());
    }

    /// Zero: at the start the first frame is due.
    #[test]
    fn the_first_frame_is_index_zero() {
        assert_eq!(PlumeView.frame_index(0), 0);
    }

    /// One, then many: the frame index advances once per frame period and keeps climbing, so
    /// consecutive frames never suppress each other.
    #[test]
    fn the_frame_index_advances_once_per_frame() {
        assert_eq!(PlumeView.frame_index(FRAME_MS - 1), 0);
        assert_eq!(PlumeView.frame_index(FRAME_MS), 1);
        assert_eq!(PlumeView.frame_index(2 * FRAME_MS), 2);
    }
}
