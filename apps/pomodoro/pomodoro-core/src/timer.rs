//! The pomodoro state machine: a pure `step` over phase, status, and a pause-correct clock.
//!
//! The whole timer, as a value object and one pure function. [`step`] folds a [`Timer`] and an
//! [`Event`] (a button gesture, or the passage of time) into the next [`Timer`] and whatever
//! sound the transition makes — no clock, no thread, no I/O. The shell supplies `now` and the
//! events; every rule below is decided on the host.
//!
//! ## The countdown, across pauses
//!
//! Time spent *running* is what counts down. The timer banks the elapsed run time in
//! `accrued_ms` whenever it pauses, and while running adds the live span since
//! `running_since`. So [`remaining`](Timer::remaining) is `duration - (accrued + live span)`,
//! which is why pausing and resuming leaves the remaining time untouched — the property the
//! whole design turns on.

use platform_core::Tick;

use crate::config::Durations;
use crate::jingle::Jingle;
use crate::phase::Phase;

/// Whether the timer is idle, counting, paused, or waiting at a finished phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// Never started: the first focus is shown at full length, not counting.
    Idle,
    /// Counting down.
    Running,
    /// Held mid-phase; the countdown is frozen at its remaining time.
    Paused,
    /// The phase's countdown reached zero; waiting for a tap to begin the next phase.
    Finished,
}

/// What drives the timer: a tick of the clock, or one of the three button gestures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// The clock advanced — check whether the current phase has run out.
    Tick,
    /// The front button tap: start, pause, or resume — or begin the next phase when finished.
    StartPause,
    /// The front button hold: reset the current phase to its full length.
    Reset,
    /// The side button tap: skip immediately to the next phase.
    Skip,
}

/// The timer state. [`Copy`], so [`step`] takes and returns it by value — the FSM is pure.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timer {
    phase: Phase,
    status: Status,
    /// Focus pomodoros completed so far — drives the long-break cadence.
    completed_focus: u32,
    /// Run time banked in the current phase, excluding any live running span.
    accrued_ms: u64,
    /// When the current running span began; meaningful only while [`Running`](Status::Running).
    running_since: Tick,
}

/// The outcome of a [`step`]: the next timer, and any sound the transition makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stepped {
    /// The timer after the event.
    pub timer: Timer,
    /// The jingle this transition plays, if any.
    pub jingle: Option<Jingle>,
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

impl Timer {
    /// A fresh timer: idle at the first focus, nothing completed.
    pub const fn new() -> Self {
        Timer {
            phase: Phase::Focus,
            status: Status::Idle,
            completed_focus: 0,
            accrued_ms: 0,
            running_since: 0,
        }
    }

    /// The current phase.
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The current status.
    pub const fn status(&self) -> Status {
        self.status
    }

    /// How many focus pomodoros have completed.
    pub const fn completed_focus(&self) -> u32 {
        self.completed_focus
    }

    /// Milliseconds left in the current phase; never negative, saturating at 0.
    pub fn remaining(&self, now: Tick, durations: Durations) -> u64 {
        self.phase
            .duration_ms(durations)
            .saturating_sub(self.elapsed_in_phase(now))
    }

    /// Milliseconds elapsed in the current phase: banked run time plus any live running span.
    fn elapsed_in_phase(&self, now: Tick) -> u64 {
        let live: u64 = match self.status {
            Status::Running => now.saturating_sub(self.running_since),
            _ => 0,
        };
        self.accrued_ms.saturating_add(live)
    }

    /// Start / pause / resume, or begin the next phase when finished.
    fn start_pause(self, now: Tick, durations: Durations) -> Stepped {
        match self.status {
            // Pause: bank the live span so the remaining time is frozen exactly.
            Status::Running => Stepped {
                timer: Timer {
                    status: Status::Paused,
                    accrued_ms: self.elapsed_in_phase(now),
                    ..self
                },
                jingle: None,
            },
            // Resume: open a new running span; the banked time stays.
            Status::Paused => Stepped {
                timer: Timer {
                    status: Status::Running,
                    running_since: now,
                    ..self
                },
                jingle: None,
            },
            // Start the very first focus.
            Status::Idle => Stepped {
                timer: Timer {
                    status: Status::Running,
                    running_since: now,
                    accrued_ms: 0,
                    ..self
                },
                jingle: Some(Jingle::FocusStart),
            },
            // Begin the next phase, counting from now.
            Status::Finished => self.advance(now, durations, Status::Running),
        }
    }

    /// Reset the current phase to its full length, without changing which phase it is.
    fn reset(self) -> Stepped {
        // A never-started timer stays idle; anything else parks at the full phase, paused.
        let status: Status = match self.status {
            Status::Idle => Status::Idle,
            _ => Status::Paused,
        };
        Stepped {
            timer: Timer {
                status,
                accrued_ms: 0,
                running_since: 0,
                ..self
            },
            jingle: None,
        }
    }

    /// Skip immediately to the next phase, preserving whether the timer was running.
    fn skip(self, now: Tick, durations: Durations) -> Stepped {
        // A skipped (abandoned) focus is not a completed one, so the cadence is unaffected.
        let status: Status = match self.status {
            Status::Running => Status::Running,
            _ => Status::Paused,
        };
        self.advance(now, durations, status)
    }

    /// A clock tick: finish the phase if its countdown has run out, otherwise do nothing.
    fn tick(self, now: Tick, durations: Durations) -> Stepped {
        if self.status != Status::Running || self.remaining(now, durations) > 0 {
            // Not counting, or still counting: a tick changes nothing and makes no sound.
            return Stepped {
                timer: self,
                jingle: None,
            };
        }
        // Reached zero. Bank the elapsed so `remaining` reads 0 while finished, and count a
        // completed focus (which drives the long-break cadence). Emit one completion jingle;
        // later ticks find the timer finished and stay silent.
        let completed_focus: u32 = self.completed_focus + u32::from(self.phase == Phase::Focus);
        Stepped {
            timer: Timer {
                status: Status::Finished,
                accrued_ms: self.elapsed_in_phase(now),
                completed_focus,
                ..self
            },
            jingle: Some(Jingle::PhaseComplete),
        }
    }

    /// Move to the phase after the current one at `now`, with the given resume status.
    fn advance(self, now: Tick, durations: Durations, status: Status) -> Stepped {
        let (phase, jingle): (Phase, Jingle) =
            next_phase(self.phase, self.completed_focus, durations);
        Stepped {
            timer: Timer {
                phase,
                status,
                accrued_ms: 0,
                running_since: now,
                ..self
            },
            jingle: Some(jingle),
        }
    }
}

/// The phase after `current`, and the jingle that announces it.
///
/// A long break replaces the short one every `long_break_every` *completed* focus pomodoros —
/// but never on zero completed (skipping an unstarted first focus advances to a *short* break,
/// not a long one), and never when the cadence is zero (which means "no long breaks").
fn next_phase(current: Phase, completed_focus: u32, durations: Durations) -> (Phase, Jingle) {
    match current {
        Phase::Focus => {
            let earns_long: bool = durations.long_break_every != 0
                && completed_focus != 0
                && completed_focus.is_multiple_of(durations.long_break_every);
            if earns_long {
                (Phase::LongBreak, Jingle::LongBreakStart)
            } else {
                (Phase::ShortBreak, Jingle::BreakStart)
            }
        }
        _ => (Phase::Focus, Jingle::FocusStart),
    }
}

/// Fold one event into the timer, returning the next timer and any sound it makes.
///
/// The whole use case, as a pure function of its inputs. `now` is the current
/// [`Tick`]; the shell reads it from the injected clock and hands it in.
pub fn step(timer: Timer, event: Event, now: Tick, durations: Durations) -> Stepped {
    match event {
        Event::StartPause => timer.start_pause(now, durations),
        Event::Reset => timer.reset(),
        Event::Skip => timer.skip(now, durations),
        Event::Tick => timer.tick(now, durations),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Short, readable test lengths: 1 s focus, 0.5 s short break, 1.5 s long break, long every
    /// 4th. Chosen so a reader can do the countdown arithmetic in their head.
    const D: Durations = Durations {
        focus_ms: 1_000,
        short_break_ms: 500,
        long_break_ms: 1_500,
        long_break_every: 4,
    };

    /// Complete a focus: start it at `start`, then tick it to zero. Works from a fresh/idle
    /// focus or from a finished break (StartPause advances into the focus first).
    fn complete_focus(timer: Timer, start: Tick) -> Timer {
        let running: Timer = step(timer, Event::StartPause, start, D).timer;
        step(running, Event::Tick, start + D.focus_ms, D).timer
    }

    /// Complete the break after a finished focus: begin it, then tick it to zero.
    fn complete_break(finished_focus: Timer, start: Tick) -> Timer {
        let running: Timer = step(finished_focus, Event::StartPause, start, D).timer;
        let length: u64 = running.phase().duration_ms(D);
        step(running, Event::Tick, start + length, D).timer
    }

    // ---- Zero: a fresh timer ----

    #[test]
    fn a_new_timer_is_idle_at_a_full_focus() {
        let t: Timer = Timer::new();
        assert_eq!(t.phase(), Phase::Focus);
        assert_eq!(t.status(), Status::Idle);
        assert_eq!(t.remaining(0, D), D.focus_ms);
    }

    #[test]
    fn an_idle_timer_does_not_count_down() {
        // Time passes, but an unstarted timer still shows the full focus.
        let t: Timer = Timer::new();
        assert_eq!(t.remaining(10_000, D), D.focus_ms);
    }

    // ---- One: a single focus ----

    #[test]
    fn starting_runs_the_countdown_and_announces_focus() {
        let stepped: Stepped = step(Timer::new(), Event::StartPause, 0, D);
        assert_eq!(stepped.timer.status(), Status::Running);
        assert_eq!(stepped.jingle, Some(Jingle::FocusStart));
        assert_eq!(
            stepped.timer.remaining(400, D),
            600,
            "counts down while running"
        );
    }

    #[test]
    fn a_focus_reaching_zero_finishes_with_one_completion_jingle() {
        let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
        let finish: Stepped = step(running, Event::Tick, D.focus_ms, D);
        assert_eq!(finish.timer.status(), Status::Finished);
        assert_eq!(finish.timer.completed_focus(), 1);
        assert_eq!(finish.jingle, Some(Jingle::PhaseComplete));
        assert_eq!(finish.timer.remaining(D.focus_ms, D), 0);

        // A later tick past zero is silent — a phase completes exactly once.
        let again: Stepped = step(finish.timer, Event::Tick, D.focus_ms + 500, D);
        assert_eq!(again.jingle, None);
    }

    #[test]
    fn pausing_then_resuming_preserves_the_remaining_time() {
        let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
        let paused: Stepped = step(running, Event::StartPause, 300, D);
        assert_eq!(paused.timer.status(), Status::Paused);
        assert_eq!(paused.jingle, None);
        // Frozen: time passing while paused does not count down.
        assert_eq!(paused.timer.remaining(9_000, D), 700);
        // Resume much later; the remaining time is exactly where it was left.
        let resumed: Timer = step(paused.timer, Event::StartPause, 9_000, D).timer;
        assert_eq!(resumed.status(), Status::Running);
        assert_eq!(resumed.remaining(9_000, D), 700);
    }

    #[test]
    fn a_tick_while_idle_or_paused_is_inert() {
        let idle_tick: Stepped = step(Timer::new(), Event::Tick, 5_000, D);
        assert_eq!(idle_tick.timer, Timer::new());
        assert_eq!(idle_tick.jingle, None);

        let paused: Timer = step(
            step(Timer::new(), Event::StartPause, 0, D).timer,
            Event::StartPause,
            300,
            D,
        )
        .timer;
        let paused_tick: Stepped = step(paused, Event::Tick, 9_000, D);
        assert_eq!(paused_tick.timer, paused);
        assert_eq!(paused_tick.jingle, None);
    }

    // ---- Transitions: reset and skip ----

    #[test]
    fn resetting_restores_the_full_phase_without_changing_it() {
        let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
        let reset: Stepped = step(running, Event::Reset, 400, D);
        assert_eq!(reset.timer.phase(), Phase::Focus, "same phase");
        assert_eq!(reset.timer.remaining(400, D), D.focus_ms, "back to full");
        assert_eq!(reset.jingle, None);
    }

    #[test]
    fn skipping_advances_immediately_with_its_jingle() {
        let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
        let skip: Stepped = step(running, Event::Skip, 300, D);
        assert_eq!(skip.timer.phase(), Phase::ShortBreak);
        assert_eq!(
            skip.timer.status(),
            Status::Running,
            "skip keeps it running"
        );
        assert_eq!(skip.jingle, Some(Jingle::BreakStart));
        assert_eq!(
            skip.timer.remaining(300, D),
            D.short_break_ms,
            "the break is full"
        );
    }

    #[test]
    fn skipping_an_unstarted_first_focus_is_a_short_break_not_a_long_one() {
        // The load-bearing guard: completed_focus is 0, and 0 % 4 == 0 — but a long break must
        // be *earned*, so an abandoned first focus advances to a short break.
        let skip: Stepped = step(Timer::new(), Event::Skip, 0, D);
        assert_eq!(skip.timer.phase(), Phase::ShortBreak);
        assert_eq!(skip.jingle, Some(Jingle::BreakStart));
    }

    #[test]
    fn after_a_finished_focus_starting_advances_to_a_short_break() {
        let finished: Timer = complete_focus(Timer::new(), 0);
        let begin_break: Stepped = step(finished, Event::StartPause, 2_000, D);
        assert_eq!(begin_break.timer.phase(), Phase::ShortBreak);
        assert_eq!(begin_break.timer.status(), Status::Running);
        assert_eq!(begin_break.jingle, Some(Jingle::BreakStart));
    }

    // ---- Many: the long-break cadence ----

    #[test]
    fn the_fourth_completed_focus_earns_a_long_break() {
        // Run four full focus/break cycles; the break after the fourth focus is the long one.
        let f1: Timer = complete_focus(Timer::new(), 0);
        assert_eq!(
            step(f1, Event::StartPause, 100, D).timer.phase(),
            Phase::ShortBreak,
            "the 1st break is short"
        );
        let f2: Timer = complete_focus(complete_break(f1, 2_000), 4_000);
        let f3: Timer = complete_focus(complete_break(f2, 6_000), 8_000);
        let f4: Timer = complete_focus(complete_break(f3, 10_000), 12_000);
        assert_eq!(f4.completed_focus(), 4);

        let after_fourth: Stepped = step(f4, Event::StartPause, 14_000, D);
        assert_eq!(
            after_fourth.timer.phase(),
            Phase::LongBreak,
            "the 4th break is long"
        );
        assert_eq!(after_fourth.jingle, Some(Jingle::LongBreakStart));
    }

    #[test]
    fn the_cycle_after_a_long_break_starts_short_again() {
        // Complete four focuses, take the long break, complete a fifth focus: its break is short.
        let f1: Timer = complete_focus(Timer::new(), 0);
        let f2: Timer = complete_focus(complete_break(f1, 2_000), 4_000);
        let f3: Timer = complete_focus(complete_break(f2, 6_000), 8_000);
        let f4: Timer = complete_focus(complete_break(f3, 10_000), 12_000);
        let f5: Timer = complete_focus(complete_break(f4, 14_000), 18_000); // long break, then focus 5
        assert_eq!(f5.completed_focus(), 5);
        assert_eq!(
            step(f5, Event::StartPause, 20_000, D).timer.phase(),
            Phase::ShortBreak,
            "the 5th break is short again"
        );
    }

    // ---- Property tests ----

    proptest! {
        /// Remaining is always within `[0, phase duration]`, however long a running timer runs.
        #[test]
        fn remaining_is_bounded(now in 0u64..1_000_000) {
            let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
            prop_assert!(running.remaining(now, D) <= D.focus_ms);
        }

        /// The countdown never goes back up while running: for any two ordered instants, the
        /// later one has no more time left than the earlier.
        #[test]
        fn the_countdown_is_monotone_while_running(a in 0u64..2_000, b in 0u64..2_000) {
            let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
            let (early, late): (u64, u64) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(running.remaining(early, D) >= running.remaining(late, D));
        }

        /// Pausing then resuming conserves the remaining time, for any pause instant and any
        /// gap spent paused — the property the whole pause design turns on.
        #[test]
        fn pausing_conserves_remaining(pause_at in 0u64..1_000, gap in 0u64..10_000) {
            let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
            let before: u64 = running.remaining(pause_at, D);
            let paused: Timer = step(running, Event::StartPause, pause_at, D).timer;
            prop_assert_eq!(paused.remaining(pause_at + gap, D), before, "frozen while paused");
            let resumed: Timer = step(paused, Event::StartPause, pause_at + gap, D).timer;
            prop_assert_eq!(resumed.remaining(pause_at + gap, D), before, "conserved on resume");
        }

        /// StartPause is an involution on Running: two toggles return to Running with the same
        /// remaining time.
        #[test]
        fn two_toggles_return_to_running(pause_at in 0u64..1_000, gap in 0u64..1_000) {
            let running: Timer = step(Timer::new(), Event::StartPause, 0, D).timer;
            let before: u64 = running.remaining(pause_at, D);
            let paused: Timer = step(running, Event::StartPause, pause_at, D).timer;
            let running_again: Timer = step(paused, Event::StartPause, pause_at + gap, D).timer;
            prop_assert_eq!(running_again.status(), Status::Running);
            prop_assert_eq!(running_again.remaining(pause_at + gap, D), before);
        }
    }
}
