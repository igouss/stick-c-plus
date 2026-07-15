//! `PomodoroView` — a snapshot of the timer as an [`Animated`] view the render loop shows.

use platform_core::{Animated, Tick};
use pomodoro_core::{Durations, Phase, Status, Timer};

use crate::scene;

/// What the pomodoro glass shows at one instant: the phase, the status, and the whole seconds
/// remaining.
///
/// Built from a [`Timer`] and the clock with [`of`](Self::of), then handed to the render loop
/// and [`render`](crate::render). It carries whole *seconds*, not milliseconds, so two ticks
/// inside the same second are the same view and the loop suppresses the redundant repaint —
/// only the creature's frame and each new second redraw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PomodoroView {
    /// The current phase — chooses the label and the creature.
    pub phase: Phase,
    /// The current status — chooses the label and whether the creature moves.
    pub status: Status,
    /// Whole seconds left in the phase, rounded up.
    pub remaining_secs: u32,
}

impl PomodoroView {
    /// The view of `timer` at `now`.
    pub fn of(timer: &Timer, now: Tick, durations: Durations) -> Self {
        let remaining_ms: u64 = timer.remaining(now, durations);
        // Round up: a fresh 25-minute focus reads 25:00, and the final second reads 00:01 then
        // 00:00 exactly as the phase ends.
        let remaining_secs: u32 = remaining_ms.div_ceil(1_000).min(u64::from(u32::MAX)) as u32;
        PomodoroView {
            phase: timer.phase(),
            status: timer.status(),
            remaining_secs,
        }
    }

    /// The minutes and seconds the clock shows.
    pub fn minutes_seconds(&self) -> (u32, u32) {
        (self.remaining_secs / 60, self.remaining_secs % 60)
    }
}

impl Animated for PomodoroView {
    /// The phase and status — but *not* the seconds. So the creature keeps animating on the
    /// phase's clock while the mm:ss counts down, rather than restarting each second.
    type Anchor = (Phase, Status);

    fn anchor(&self) -> (Phase, Status) {
        (self.phase, self.status)
    }

    fn is_animated(&self) -> bool {
        scene::is_animated(self.phase, self.status)
    }

    fn frame_index(&self, elapsed_ms: Tick) -> usize {
        scene::frame_index(self.phase, self.status, elapsed_ms)
    }
}
