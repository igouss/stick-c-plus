//! Which creature the glass shows for a pomodoro state, and whether it moves.
//!
//! The whole animation policy, as pure functions of the [`Phase`] and [`Status`]. No clock,
//! no timer — the render loop supplies the elapsed milliseconds and this answers which frame.
//!
//! ## The creature *is* the phase
//!
//! | State | Creature | Motion |
//! |---|---|---|
//! | Running a focus | coding | animated |
//! | Running a break | dancing | animated |
//! | Finished (either) | winking | animated |
//! | Paused | looking around | still |
//! | Idle (not started) | breathing | still |
//!
//! A running creature moves — it is *doing* the thing (coding, resting) — so an operator
//! across the room can see a session is live. A paused or idle creature holds still: the
//! render loop then repaints nothing and the clock is frozen anyway.

use platform_display::sprite::{
    Sprite, DANCE_BOUNCE, EXPRESSION_WINK, IDLE_BREATHE, IDLE_LOOK_AROUND, WORK_CODING,
};
use pomodoro_core::{Phase, Status};

/// The creature shown for a pomodoro state.
pub fn creature(phase: Phase, status: Status) -> &'static Sprite {
    match status {
        // Not started: a calm, breathing creature waiting for the first tap.
        Status::Idle => &IDLE_BREATHE,
        // Working or resting: the creature does the thing.
        Status::Running if phase == Phase::Focus => &WORK_CODING,
        Status::Running => &DANCE_BOUNCE,
        // Held: the creature waits, looking around.
        Status::Paused => &IDLE_LOOK_AROUND,
        // A phase just ended: a wink — tap to begin the next one.
        Status::Finished => &EXPRESSION_WINK,
    }
}

/// Whether the creature for this state moves.
///
/// A running or finished creature animates (it draws the eye to a live session or a just-
/// ended phase); an idle or paused one is still, so the render loop can rest.
pub fn is_animated(_phase: Phase, status: Status) -> bool {
    matches!(status, Status::Running | Status::Finished)
}

/// Which animation frame shows after this state has been current for `elapsed_ms`.
///
/// A still state is always frame 0, whatever the clock says — the same power rule the render
/// loop reads through [`Animated`](platform_core::Animated). The render loop compares this
/// against what it last painted; two equal indices mean nothing to redraw.
pub fn frame_index(phase: Phase, status: Status, elapsed_ms: u64) -> usize {
    if is_animated(phase, status) {
        creature(phase, status).frame_index_at(elapsed_ms)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A running focus and a running break show *different* creatures — the whole point is
    /// that a glance tells work from rest.
    #[test]
    fn focus_and_break_show_different_creatures() {
        let focus: &str = creature(Phase::Focus, Status::Running).slug();
        let short: &str = creature(Phase::ShortBreak, Status::Running).slug();
        assert_ne!(focus, short);
    }

    /// Running and finished animate; idle and paused are still.
    #[test]
    fn only_running_and_finished_animate() {
        assert!(is_animated(Phase::Focus, Status::Running));
        assert!(is_animated(Phase::Focus, Status::Finished));
        assert!(!is_animated(Phase::Focus, Status::Idle));
        assert!(!is_animated(Phase::Focus, Status::Paused));
    }

    /// A still state never advances its frame; an animated one does.
    #[test]
    fn a_still_state_holds_frame_zero_and_an_animated_one_advances() {
        assert_eq!(frame_index(Phase::Focus, Status::Idle, u64::MAX), 0);
        let sprite: &Sprite = creature(Phase::Focus, Status::Running);
        let first_hold: u64 = u64::from(sprite.frames()[0].hold_ms());
        assert_eq!(frame_index(Phase::Focus, Status::Running, first_hold), 1);
    }
}
