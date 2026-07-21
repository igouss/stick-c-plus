//! The input thread — poll the buttons, fold the gestures, step the timer, sound the jingles.
//!
//! The imperative shell's one background loop: every [`POLL_PERIOD`] it polls the board's
//! [`Buttons`], turns each [`ButtonEvent`] into a [`Command`], and also fires a
//! [`Tick`](Event::Tick) so the countdown finishes on time. Each `step` may return a
//! [`Jingle`](pomodoro_core::Jingle), which the loop plays on the injected
//! [`Tone`](platform_core::Tone). The controls:
//!
//! - **front click** → start / pause / resume (or begin the next phase when finished);
//! - **front double-click** → restart the whole session (back to a fresh, idle first focus);
//! - **front long hold** → reset the current phase;
//! - **side click** → skip to the next phase;
//! - **power click** → light the glass, or darken it.
//!
//! The front button is the only one that reports double-clicks, so a lone front click lands a
//! [`DOUBLE_CLICK_MS`](platform_input::DOUBLE_CLICK_MS) window later than the side button's
//! skip, which stays immediate. That trade is declared in [`INPUT_CONFIG`], where it is visible.
//!
//! ## Two kinds of command
//!
//! Most gestures are timer events and go through the pure FSM. The power button's is not: it
//! toggles the backlight, which is a fact about the board rather than about a pomodoro, and the
//! timer has no business knowing the glass went dark. So the mapping yields a [`Command`], and
//! the loop routes a timer event inward and a backlight toggle outward to the
//! [`Backlight`] port. The composition root injects a port that also publishes the flag the
//! display thread reads, so a dark screen stops being painted — but the shell never learns
//! that anybody is watching.
//!
//! Every gesture rule and the whole FSM stay pure inward, so the mapping and the wiring are
//! exercised on the host with fake peripherals.

use std::fmt::Display;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use platform_core::{Backlight, Clock, Tone};
use platform_input::{
    ButtonEvent, ButtonId, ButtonLevel, Buttons, Gesture, Gestures, InputConfig, LatchedGesture,
};
use pomodoro_core::{Durations, Event};

use crate::shared::SharedTimer;

/// How often the buttons are polled and a tick is fired.
///
/// Ten milliseconds is comfortably faster than the debounce window, so a real press settles
/// within a couple of polls, and firing a `Tick` this often costs only a mutex lock and a pure
/// `step` — the countdown then finishes within 10 ms of its true zero. It is also this board's
/// floor: ESP-IDF runs at `CONFIG_FREERTOS_HZ = 100`, and a shorter sleep cannot yield.
pub const POLL_PERIOD: Duration = Duration::from_millis(10);

/// The input thread's stack, in bytes. Sized explicitly (it becomes a FreeRTOS task stack on
/// device); the loop polls, steps, and blocks briefly playing a jingle, well within 8 KiB.
pub const INPUT_STACK_SIZE: usize = 8 * 1024;

/// Which gestures this app's buttons report.
///
/// Only the front button asks for double-clicks, and it is the only one that pays for them: a
/// lone front click is held back a window, while the side button's skip and the power button's
/// toggle stay immediate. Restarting the session is rare enough to be worth that; skipping a
/// phase is not.
pub const INPUT_CONFIG: InputConfig = InputConfig {
    front: Gestures::WithDoubleClick,
    side: Gestures::Prompt,
    timing: platform_input::GestureConfig {
        debounce_ms: platform_input::DEBOUNCE_MS,
        hold_ms: platform_input::HOLD_MS,
        double_click_ms: platform_input::DOUBLE_CLICK_MS,
    },
};

/// What a gesture asks the app to do.
///
/// Two kinds, because two of the board's controls belong to different worlds. A timer event
/// goes inward through the pure FSM; darkening the glass is a fact about the board that the
/// pomodoro domain neither knows nor should. Keeping them in one enum means the mapping stays a
/// single total function over every `(button, gesture)` pair, and stays pure.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// Step the timer with this event.
    Timer(Event),
    /// Light the glass, or darken it.
    ToggleBacklight,
    /// A gesture this app does not use.
    Ignore,
}

/// What each button's gestures mean to the pomodoro.
///
/// Total over every pair, so a gesture can never be silently dropped by an unwritten arm — and
/// pinned by a test, because this mapping *is* the app's control scheme.
fn command_for(event: ButtonEvent) -> Command {
    match (event.button, event.gesture) {
        (ButtonId::Front, Gesture::Click) => Command::Timer(Event::StartPause),
        (ButtonId::Front, Gesture::DoubleClick) => Command::Timer(Event::RestartSession),
        (ButtonId::Front, Gesture::LongHold) => Command::Timer(Event::Reset),
        (ButtonId::Side, Gesture::Click) => Command::Timer(Event::Skip),
        (ButtonId::Side, _) => Command::Ignore,
        (ButtonId::Power, Gesture::Click) => Command::ToggleBacklight,
        // The PMIC reports nothing else: a long press is its own power-off, and the port's type
        // says so. Named rather than caught by a wildcard, so a new gesture would fail to build.
        (ButtonId::Power, Gesture::DoubleClick | Gesture::LongHold) => Command::Ignore,
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

/// One poll cycle: drain the buttons, act on each gesture, then fire a tick — and sound whatever
/// jingles the transitions make. The whole shell↔domain seam, testable without a thread.
fn cycle<F, S, P, B, T>(
    buttons: &mut Buttons<F, S, P>,
    backlight: &mut B,
    tone: &mut T,
    shared: &SharedTimer,
    clock: &impl Clock,
    durations: Durations,
) where
    F: ButtonLevel,
    S: ButtonLevel,
    P: LatchedGesture,
    B: Backlight,
    B::Error: Display,
    T: Tone,
    T::Error: Display,
{
    let now: u64 = clock.now();

    buttons
        .poll(now)
        .for_each(|event: ButtonEvent| match command_for(event) {
            Command::Timer(event) => sound(shared.apply(event, now, durations), tone),
            Command::ToggleBacklight => switch(backlight),
            Command::Ignore => {}
        });

    // Fire a tick every cycle: a no-op until the running phase reaches zero, then it finishes.
    sound(shared.apply(Event::Tick, now, durations), tone);
}

/// Flip the backlight, logging (not propagating) a failure — a flaky PMIC read must not take the
/// timer down, and the switch leaves its flag honest so the display keeps painting a glass that
/// never actually went dark.
fn switch<B>(backlight: &mut B)
where
    B: Backlight,
    B::Error: Display,
{
    if let Err(err) = backlight.toggle() {
        warn!("pomodoro-input: backlight toggle failed: {err}");
    }
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

/// Spawn the input thread: poll `buttons` and the `clock`, drive the pure FSM in `shared`, sound
/// the jingles on `buzzer`, and switch `backlight` when the power button asks, every
/// [`POLL_PERIOD`].
///
/// Everything moves into the thread, so each must be [`Send`] + `'static`; the buzzer's and the
/// backlight's errors must be [`Display`] so a failure can be logged. Returns the [`InputTask`]
/// handle, or the [`io::Error`] from failing to spawn the thread.
pub fn spawn_input<F, S, P, B, T, C>(
    mut buttons: Buttons<F, S, P>,
    mut backlight: B,
    mut buzzer: T,
    shared: SharedTimer,
    clock: C,
    durations: Durations,
) -> io::Result<InputTask>
where
    F: ButtonLevel + Send + 'static,
    S: ButtonLevel + Send + 'static,
    P: LatchedGesture + Send + 'static,
    B: Backlight + Send + 'static,
    B::Error: Display,
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
            while !stop_in_thread.load(Ordering::Relaxed) {
                cycle(
                    &mut buttons,
                    &mut backlight,
                    &mut buzzer,
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
    use std::convert::Infallible;

    const D: Durations = Durations {
        focus_ms: 1_000,
        short_break_ms: 500,
        long_break_ms: 1_500,
        long_break_every: 4,
    };

    /// A levelled button whose raw level is scripted by a shared cell.
    struct FakeButton<'a> {
        pressed: &'a Cell<bool>,
    }

    impl ButtonLevel for FakeButton<'_> {
        fn pressed(&mut self) -> bool {
            self.pressed.get()
        }
    }

    /// A latch a test arms by hand, draining like the real PMIC register.
    struct FakeLatch<'a> {
        armed: &'a Cell<Option<Gesture>>,
    }

    impl LatchedGesture for FakeLatch<'_> {
        fn take(&mut self) -> Option<Gesture> {
            self.armed.take()
        }
    }

    /// A backlight that just remembers whether it is lit. The publishing decorator and its
    /// failure path are tested next door in `platform-runtime`; what matters here is only that
    /// the right gesture flips the right thing.
    struct RecordingBacklight {
        lit: bool,
    }

    impl Backlight for RecordingBacklight {
        type Error = Infallible;

        fn is_lit(&self) -> bool {
            self.lit
        }

        fn set(&mut self, lit: bool) -> Result<(), Infallible> {
            self.lit = lit;
            Ok(())
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
        type Error = Infallible;
        fn play(&mut self, notes: &[Note]) -> Result<(), Self::Error> {
            self.played.extend_from_slice(notes);
            Ok(())
        }
    }

    /// Everything one scenario drives: the three button inputs, the clock, and the app state.
    struct Rig<'a> {
        buttons: Buttons<FakeButton<'a>, FakeButton<'a>, FakeLatch<'a>>,
        backlight: RecordingBacklight,
        tone: RecordingTone,
        shared: SharedTimer,
        now: &'a Cell<u64>,
    }

    impl<'a> Rig<'a> {
        fn new(
            front: &'a Cell<bool>,
            side: &'a Cell<bool>,
            power: &'a Cell<Option<Gesture>>,
            now: &'a Cell<u64>,
        ) -> Self {
            Rig {
                buttons: Buttons::new(
                    FakeButton { pressed: front },
                    FakeButton { pressed: side },
                    FakeLatch { armed: power },
                    INPUT_CONFIG,
                ),
                backlight: RecordingBacklight { lit: true },
                tone: RecordingTone::default(),
                shared: SharedTimer::new(),
                now,
            }
        }

        /// Drive `n` poll cycles at 1 ms steps, holding every input where it is.
        fn settle(&mut self, n: u64) {
            let clock: ManualClock = ManualClock { now: self.now };
            (0..n).for_each(|_| {
                cycle(
                    &mut self.buttons,
                    &mut self.backlight,
                    &mut self.tone,
                    &self.shared,
                    &clock,
                    D,
                );
                self.now.set(self.now.get() + 1);
            });
        }
    }

    /// The three levels a scenario drives, fresh.
    fn inputs() -> (Cell<bool>, Cell<bool>, Cell<Option<Gesture>>, Cell<u64>) {
        (
            Cell::new(false),
            Cell::new(false),
            Cell::new(None),
            Cell::new(0),
        )
    }

    #[test]
    fn a_front_click_starts_the_timer_and_sounds_the_focus_jingle() {
        let (front, side, power, now) = inputs();
        let mut rig: Rig = Rig::new(&front, &side, &power, &now);

        // Press and release the front button: a click. The release is held well past the
        // double-click window so the cadence releases the lone click (no second one).
        front.set(true);
        rig.settle(40);
        front.set(false);
        rig.settle(340);

        assert_eq!(rig.shared.snapshot().status(), Status::Running);
        assert_eq!(rig.tone.played, Jingle::FocusStart.notes());
    }

    #[test]
    fn a_side_click_skips_to_a_break() {
        let (front, side, power, now) = inputs();
        let mut rig: Rig = Rig::new(&front, &side, &power, &now);

        side.set(true);
        rig.settle(40);
        side.set(false);
        rig.settle(40);

        assert_eq!(rig.shared.snapshot().phase(), Phase::ShortBreak);
    }

    #[test]
    fn a_running_phase_finishes_when_its_time_runs_out() {
        let (front, side, power, now) = inputs();
        let mut rig: Rig = Rig::new(&front, &side, &power, &now);

        front.set(true);
        rig.settle(40);
        front.set(false);
        rig.settle(340);
        assert_eq!(rig.shared.snapshot().status(), Status::Running);

        // Jump the clock a full focus length past *now* (the click started the countdown on its
        // release, not at t=0), so the running phase is certainly past zero; the next tick
        // finishes it.
        now.set(now.get() + D.focus_ms + 1);
        rig.settle(1);

        assert_eq!(rig.shared.snapshot().status(), Status::Finished);
        assert!(
            rig.tone.played.ends_with(Jingle::PhaseComplete.notes()),
            "the completion jingle must sound"
        );
    }

    #[test]
    fn a_front_double_click_restarts_the_session() {
        let (front, side, power, now) = inputs();
        let mut rig: Rig = Rig::new(&front, &side, &power, &now);

        // Start and run a focus: a single click, released past the window so it flushes.
        front.set(true);
        rig.settle(40);
        front.set(false);
        rig.settle(340);
        assert_eq!(rig.shared.snapshot().status(), Status::Running);

        // Two quick front clicks: each release settles long enough to be accepted but not to
        // time the window out, so the second closes the first into a double-click.
        front.set(true);
        rig.settle(40);
        front.set(false);
        rig.settle(25);
        front.set(true);
        rig.settle(40);
        front.set(false);
        rig.settle(25);

        let timer: Timer = rig.shared.snapshot();
        assert_eq!(timer.status(), Status::Idle);
        assert_eq!(timer.phase(), Phase::Focus);
        assert_eq!(timer.completed_focus(), 0);
        assert!(
            rig.tone.played.ends_with(Jingle::SessionRestart.notes()),
            "the session-restart jingle must sound"
        );
    }

    /// One: the power button darkens the glass, and the flag the display thread reads follows.
    #[test]
    fn a_power_click_darkens_the_glass() {
        let (front, side, power, now) = inputs();
        let mut rig: Rig = Rig::new(&front, &side, &power, &now);
        assert!(rig.backlight.is_lit(), "the board boots with the glass lit");

        power.set(Some(Gesture::Click));
        rig.settle(1);

        assert!(!rig.backlight.is_lit());
    }

    /// Many: a second power click lights it again.
    #[test]
    fn a_second_power_click_lights_it_again() {
        let (front, side, power, now) = inputs();
        let mut rig: Rig = Rig::new(&front, &side, &power, &now);
        power.set(Some(Gesture::Click));
        rig.settle(1);
        power.set(Some(Gesture::Click));
        rig.settle(1);

        assert!(rig.backlight.is_lit());
    }

    /// Darkening the glass must not touch the timer: the backlight is a fact about the board,
    /// and a pomodoro running behind a dark screen is still running.
    #[test]
    fn a_power_click_leaves_the_timer_alone() {
        let (front, side, power, now) = inputs();
        let mut rig: Rig = Rig::new(&front, &side, &power, &now);

        front.set(true);
        rig.settle(40);
        front.set(false);
        rig.settle(340);
        let before: Timer = rig.shared.snapshot();

        power.set(Some(Gesture::Click));
        rig.settle(1);
        let after: Timer = rig.shared.snapshot();

        assert_eq!(after.status(), before.status());
        assert_eq!(after.phase(), before.phase());
        assert_eq!(after.completed_focus(), before.completed_focus());
    }

    /// The control scheme, pinned. This mapping is the app's user interface; a change here is a
    /// change to what the buttons do, and should have to be written down twice.
    #[test]
    fn the_control_scheme_is_fixed() {
        let cmd = |button: ButtonId, gesture: Gesture| -> Command {
            command_for(ButtonEvent::new(button, gesture))
        };

        assert_eq!(
            cmd(ButtonId::Front, Gesture::Click),
            Command::Timer(Event::StartPause)
        );
        assert_eq!(
            cmd(ButtonId::Front, Gesture::DoubleClick),
            Command::Timer(Event::RestartSession)
        );
        assert_eq!(
            cmd(ButtonId::Front, Gesture::LongHold),
            Command::Timer(Event::Reset)
        );
        assert_eq!(
            cmd(ButtonId::Side, Gesture::Click),
            Command::Timer(Event::Skip)
        );
        assert_eq!(cmd(ButtonId::Side, Gesture::LongHold), Command::Ignore);
        assert_eq!(cmd(ButtonId::Side, Gesture::DoubleClick), Command::Ignore);
        assert_eq!(
            cmd(ButtonId::Power, Gesture::Click),
            Command::ToggleBacklight
        );
    }
}
