//! The input thread — poll the buttons, debounce, step the timer, sound the jingles.
//!
//! The imperative shell's one background loop: every [`POLL_PERIOD`] it reads the two buttons,
//! folds each through its pure [`Debounce`], maps a resulting gesture to a timer [`Event`],
//! and also fires a [`Tick`](Event::Tick) so the countdown finishes on time. Each `step` may
//! return a [`Jingle`](pomodoro_core::Jingle), which the loop plays on the injected
//! [`Tone`](platform_core::Tone). The controls:
//!
//! - **front tap** → start / pause / resume (or begin the next phase when finished);
//! - **front double-tap** → restart the whole session (back to a fresh, idle first focus);
//! - **front hold** → reset the current phase;
//! - **side tap** → skip to the next phase.
//!
//! The front button is routed through a [`TapCadence`] so a double-tap can be told from a
//! single one — which means a lone front tap is reported a
//! [`DOUBLE_TAP_MS`](platform_core::DOUBLE_TAP_MS) window later than the side tap, whose skip
//! stays immediate. It owns the timing and the shared cache; every
//! gesture rule and the whole FSM stay pure inward, so the mapping and the wiring are exercised
//! on the host with fake peripherals.

use std::fmt::Display;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use platform_core::{Button, ButtonEvent, Clock, Debounce, TapCadence, Tone};
use pomodoro_core::{Durations, Event};

use crate::shared::SharedTimer;

/// How often the buttons are polled and a tick is fired.
///
/// Ten milliseconds is comfortably faster than the debounce window, so a real press settles
/// within a couple of polls, and firing a `Tick` this often costs only a mutex lock and a pure
/// `step` — the countdown then finishes within 10 ms of its true zero.
pub const POLL_PERIOD: Duration = Duration::from_millis(10);

/// The input thread's stack, in bytes. Sized explicitly (it becomes a FreeRTOS task stack on
/// device); the loop polls, steps, and blocks briefly playing a jingle, well within 8 KiB.
pub const INPUT_STACK_SIZE: usize = 8 * 1024;

/// The event a front-button gesture maps to: a tap starts / pauses, a double-tap restarts the
/// session, a hold resets the current phase.
fn front_event(gesture: ButtonEvent) -> Event {
    match gesture {
        ButtonEvent::Tap => Event::StartPause,
        ButtonEvent::DoubleTap => Event::RestartSession,
        ButtonEvent::Hold => Event::Reset,
    }
}

/// The event a side-button gesture maps to: a tap skips; a hold, and a double-tap it is never
/// routed to produce, are unused.
fn side_event(gesture: ButtonEvent) -> Option<Event> {
    match gesture {
        ButtonEvent::Tap => Some(Event::Skip),
        ButtonEvent::Hold | ButtonEvent::DoubleTap => None,
    }
}

/// A running input thread — a handle to stop and join it.
pub struct InputTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl InputTask {
    /// Ask the input loop to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the input thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// The state one poll cycle carries between calls: a debounce per button, plus the front
/// button's tap-vs-double-tap cadence.
struct Debouncers {
    front: Debounce,
    front_cadence: TapCadence,
    side: Debounce,
}

impl Debouncers {
    /// Fresh debouncers, everything released and nothing pending.
    fn new() -> Self {
        Debouncers {
            front: Debounce::new(),
            front_cadence: TapCadence::new(),
            side: Debounce::new(),
        }
    }
}

/// One poll cycle: read both buttons, fold each gesture into the timer, then fire a tick — and
/// sound whatever jingles the transitions make. The whole shell↔domain seam, testable without
/// a thread.
fn cycle<F, S, T>(
    front: &mut F,
    side: &mut S,
    tone: &mut T,
    debouncers: &mut Debouncers,
    shared: &SharedTimer,
    clock: &impl Clock,
    durations: Durations,
) where
    F: Button,
    S: Button,
    T: Tone,
    T::Error: Display,
{
    let now: u64 = clock.now();

    // Front: debounce the level, then fold it through the tap-vs-double-tap cadence. The cadence
    // is ticked every cycle — even when the debounce produced nothing — so a lone tap's window
    // can time out and release its single tap.
    let front_raw: Option<ButtonEvent> = debouncers.front.update(now, front.poll());
    if let Some(gesture) = debouncers.front_cadence.update(now, front_raw) {
        sound(shared.apply(front_event(gesture), now, durations), tone);
    }
    if let Some(gesture) = debouncers.side.update(now, side.poll()) {
        if let Some(event) = side_event(gesture) {
            sound(shared.apply(event, now, durations), tone);
        }
    }
    // Fire a tick every cycle: a no-op until the running phase reaches zero, then it finishes.
    sound(shared.apply(Event::Tick, now, durations), tone);
}

/// Play a jingle if the transition produced one, logging (not propagating) a buzzer failure —
/// a flaky speaker must not take the timer down.
fn sound<T>(jingle: Option<pomodoro_core::Jingle>, tone: &mut T)
where
    T: Tone,
    T::Error: Display,
{
    if let Some(jingle) = jingle {
        if let Err(err) = tone.play(jingle.notes()) {
            warn!("pomodoro-input: buzzer failed to play {jingle:?}: {err}");
        }
    }
}

/// Spawn the input thread: poll the `front`/`side` buttons and the `clock`, drive the pure FSM
/// in `shared`, and sound the jingles on `buzzer`, every [`POLL_PERIOD`].
///
/// All four peripherals move into the thread, so each must be [`Send`] + `'static`; the
/// buzzer's error must be [`Display`] so a failed play can be logged. Returns the [`InputTask`]
/// handle, or the [`io::Error`] from failing to spawn the thread.
pub fn spawn_input<F, S, T, C>(
    mut front: F,
    mut side: S,
    mut buzzer: T,
    shared: SharedTimer,
    clock: C,
    durations: Durations,
) -> io::Result<InputTask>
where
    F: Button + Send + 'static,
    S: Button + Send + 'static,
    T: Tone + Send + 'static,
    T::Error: Display,
    C: Clock + Send + 'static,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("pomodoro-input".to_string())
        .stack_size(INPUT_STACK_SIZE)
        .spawn(move || {
            let mut debouncers: Debouncers = Debouncers::new();
            while !stop_in_thread.load(Ordering::Relaxed) {
                cycle(
                    &mut front,
                    &mut side,
                    &mut buzzer,
                    &mut debouncers,
                    &shared,
                    &clock,
                    durations,
                );
                thread::sleep(POLL_PERIOD);
            }
        })?;
    Ok(InputTask { handle, stop })
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::Note;
    use pomodoro_core::{Jingle, Phase, Status, Timer};
    use std::cell::Cell;

    const D: Durations = Durations {
        focus_ms: 1_000,
        short_break_ms: 500,
        long_break_ms: 1_500,
        long_break_every: 4,
    };

    /// A button whose raw level is scripted by a shared cell, so a test drives presses by hand.
    struct FakeButton<'a> {
        pressed: &'a Cell<bool>,
    }

    impl Button for FakeButton<'_> {
        fn poll(&mut self) -> bool {
            self.pressed.get()
        }
    }

    /// A clock whose `now` a test advances by hand.
    struct ManualClock<'a> {
        now: &'a Cell<u64>,
    }

    impl Clock for ManualClock<'_> {
        fn now(&self) -> u64 {
            self.now.get()
        }
    }

    /// A buzzer that records every jingle's notes, so a test can assert what sounded.
    #[derive(Default)]
    struct RecordingTone {
        played: Vec<Note>,
    }

    impl Tone for RecordingTone {
        type Error = core::convert::Infallible;
        fn play(&mut self, notes: &[Note]) -> Result<(), Self::Error> {
            self.played.extend_from_slice(notes);
            Ok(())
        }
    }

    /// Drive `n` poll cycles at 1 ms steps, holding the buttons at their current levels — enough
    /// for the debounce to accept a settled level.
    fn settle(
        front: &mut FakeButton,
        side: &mut FakeButton,
        tone: &mut RecordingTone,
        debouncers: &mut Debouncers,
        shared: &SharedTimer,
        now: &Cell<u64>,
        n: u64,
    ) {
        let clock: ManualClock = ManualClock { now };
        (0..n).for_each(|_| {
            cycle(front, side, tone, debouncers, shared, &clock, D);
            now.set(now.get() + 1);
        });
    }

    #[test]
    fn a_front_tap_starts_the_timer_and_sounds_the_focus_jingle() {
        let front_level: Cell<bool> = Cell::new(false);
        let side_level: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let shared: SharedTimer = SharedTimer::new();
        let mut front: FakeButton = FakeButton {
            pressed: &front_level,
        };
        let mut side: FakeButton = FakeButton {
            pressed: &side_level,
        };
        let mut tone: RecordingTone = RecordingTone::default();
        let mut db: Debouncers = Debouncers::new();

        // Press and hold the front button briefly, then release: a tap. The release is held
        // well past the double-tap window so the cadence releases the lone tap (no second one).
        front_level.set(true);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 40);
        front_level.set(false);
        settle(
            &mut front, &mut side, &mut tone, &mut db, &shared, &now, 340,
        );

        assert_eq!(shared.snapshot().status(), Status::Running);
        assert_eq!(tone.played, Jingle::FocusStart.notes());
    }

    #[test]
    fn a_side_tap_skips_to_a_break() {
        let front_level: Cell<bool> = Cell::new(false);
        let side_level: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let shared: SharedTimer = SharedTimer::new();
        let mut front: FakeButton = FakeButton {
            pressed: &front_level,
        };
        let mut side: FakeButton = FakeButton {
            pressed: &side_level,
        };
        let mut tone: RecordingTone = RecordingTone::default();
        let mut db: Debouncers = Debouncers::new();

        side_level.set(true);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 40);
        side_level.set(false);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 40);

        assert_eq!(shared.snapshot().phase(), Phase::ShortBreak);
    }

    #[test]
    fn a_running_phase_finishes_when_its_time_runs_out() {
        let front_level: Cell<bool> = Cell::new(false);
        let side_level: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let shared: SharedTimer = SharedTimer::new();
        let mut front: FakeButton = FakeButton {
            pressed: &front_level,
        };
        let mut side: FakeButton = FakeButton {
            pressed: &side_level,
        };
        let mut tone: RecordingTone = RecordingTone::default();
        let mut db: Debouncers = Debouncers::new();

        // Start the focus. Hold the release past the double-tap window so the tap is released.
        front_level.set(true);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 40);
        front_level.set(false);
        settle(
            &mut front, &mut side, &mut tone, &mut db, &shared, &now, 340,
        );
        assert_eq!(shared.snapshot().status(), Status::Running);

        // Jump the clock a full focus length past *now* (the tap started the countdown on its
        // release, not at t=0), so the running phase is certainly past zero; the next tick
        // finishes it.
        now.set(now.get() + D.focus_ms + 1);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 1);
        assert_eq!(shared.snapshot().status(), Status::Finished);
        assert!(
            tone.played.ends_with(Jingle::PhaseComplete.notes()),
            "the completion jingle must sound"
        );
    }

    #[test]
    fn a_front_double_tap_restarts_the_session() {
        let front_level: Cell<bool> = Cell::new(false);
        let side_level: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let shared: SharedTimer = SharedTimer::new();
        let mut front: FakeButton = FakeButton {
            pressed: &front_level,
        };
        let mut side: FakeButton = FakeButton {
            pressed: &side_level,
        };
        let mut tone: RecordingTone = RecordingTone::default();
        let mut db: Debouncers = Debouncers::new();

        // Start and run a focus: a single tap, released past the window so it flushes.
        front_level.set(true);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 40);
        front_level.set(false);
        settle(
            &mut front, &mut side, &mut tone, &mut db, &shared, &now, 340,
        );
        assert_eq!(shared.snapshot().status(), Status::Running);

        // Two quick front taps: each release is settled long enough to be accepted but not to
        // time the double-tap window out, so the second tap closes the first into a double-tap.
        front_level.set(true);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 40);
        front_level.set(false);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 25);
        front_level.set(true);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 40);
        front_level.set(false);
        settle(&mut front, &mut side, &mut tone, &mut db, &shared, &now, 25);

        // The double-tap restarted the whole session: fresh, idle, nothing completed.
        let timer: Timer = shared.snapshot();
        assert_eq!(timer.status(), Status::Idle);
        assert_eq!(timer.phase(), Phase::Focus);
        assert_eq!(timer.completed_focus(), 0);
        assert!(
            tone.played.ends_with(Jingle::SessionRestart.notes()),
            "the session-restart jingle must sound"
        );
    }

    /// The pure gesture mapping, pinned: tap, double-tap, and hold on each button.
    #[test]
    fn the_gesture_mapping_is_fixed() {
        assert_eq!(front_event(ButtonEvent::Tap), Event::StartPause);
        assert_eq!(front_event(ButtonEvent::DoubleTap), Event::RestartSession);
        assert_eq!(front_event(ButtonEvent::Hold), Event::Reset);
        assert_eq!(side_event(ButtonEvent::Tap), Some(Event::Skip));
        assert_eq!(side_event(ButtonEvent::Hold), None);
        assert_eq!(side_event(ButtonEvent::DoubleTap), None);
    }
}
