//! The animation contract the generic render loop reads.

use crate::tick::Tick;

/// An app state that knows its own animation policy.
///
/// The generic render loop (`platform-runtime`) is written once against this trait, so a
/// plant `Observation` and a pomodoro timer render through the same code. It answers three
/// questions, all pure:
///
/// - [`anchor`](Self::anchor) — a *coarse* identity that changes only on a real transition
///   (a fault arriving, a phase ending), never on every tick. The loop restarts the
///   creature's animation clock exactly when the anchor changes. This is the load-bearing
///   distinction: a pomodoro screen repaints every second as its `mm:ss` counts down, but
///   the creature must keep animating on the *phase's* clock rather than restarting each
///   second — so the second-by-second value is *not* part of the anchor.
/// - [`is_animated`](Self::is_animated) — whether the creature moves, which the loop uses to
///   choose its cadence: a fast tick while something moves, a slow one while it is still, so
///   a healthy device is not kept awake by a motionless creature.
/// - [`frame_index`](Self::frame_index) — which animation frame shows after the state has
///   been current for `elapsed_ms`. A still state returns a constant, so the loop finds
///   nothing to repaint.
///
/// The loop repaints iff the pair `(state, frame_index)` changes, so `Eq` must compare
/// everything drawn — including the second-by-second value that is not in the anchor.
pub trait Animated: Copy + Eq {
    /// The coarse identity whose change restarts the animation clock.
    type Anchor: Copy + Eq;

    /// The current animation anchor — see the trait docs.
    fn anchor(&self) -> Self::Anchor;

    /// Whether this state's creature moves (a fast render cadence) or is still (a slow one).
    fn is_animated(&self) -> bool;

    /// Which animation frame shows after this state has been current for `elapsed_ms`. A
    /// still state returns a constant frame whatever the clock says.
    fn frame_index(&self, elapsed_ms: Tick) -> usize;
}
