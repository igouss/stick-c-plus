//! `SharedTimer` — the timer state shared between the input thread and the render loop.

use std::sync::{Arc, Mutex};

use platform_core::Tick;
use pomodoro_core::{step, Durations, Event, Jingle, Timer};

/// The latest [`Timer`], shared between the input thread (the one writer) and the render loop
/// (the reader). Poison-tolerant: a panic in any holder recovers the inner value rather than
/// propagating, so one wedged thread cannot take the others down.
#[derive(Clone)]
pub struct SharedTimer {
    inner: Arc<Mutex<Timer>>,
}

impl SharedTimer {
    /// A fresh, idle timer.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Timer::new())),
        }
    }

    /// The current timer, copied out — what the render loop's source reads each tick.
    pub fn snapshot(&self) -> Timer {
        *self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Fold `event` into the shared timer, returning any jingle the transition makes.
    pub fn apply(&self, event: Event, now: Tick, durations: Durations) -> Option<Jingle> {
        let mut timer = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stepped = step(*timer, event, now, durations);
        *timer = stepped.timer;
        stepped.jingle
    }
}

impl Default for SharedTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomodoro_core::{Phase, Status};

    const D: Durations = Durations {
        focus_ms: 1_000,
        short_break_ms: 500,
        long_break_ms: 1_500,
        long_break_every: 4,
    };

    #[test]
    fn a_fresh_shared_timer_is_idle() {
        let shared: SharedTimer = SharedTimer::new();
        assert_eq!(shared.snapshot().status(), Status::Idle);
    }

    #[test]
    fn applying_an_event_advances_the_shared_timer_and_returns_its_jingle() {
        let shared: SharedTimer = SharedTimer::new();
        let jingle = shared.apply(Event::StartPause, 0, D);
        assert_eq!(jingle, Some(Jingle::FocusStart));
        assert_eq!(shared.snapshot().status(), Status::Running);
        assert_eq!(shared.snapshot().phase(), Phase::Focus);
    }

    #[test]
    fn a_clone_sees_the_same_timer() {
        // The writer and the reader hold clones of one cache.
        let writer: SharedTimer = SharedTimer::new();
        let reader: SharedTimer = writer.clone();
        writer.apply(Event::StartPause, 0, D);
        assert_eq!(reader.snapshot().status(), Status::Running);
    }
}
