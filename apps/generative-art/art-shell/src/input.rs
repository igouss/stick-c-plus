//! The input thread — poll the front button, recognise its gesture, advance the gallery.
//!
//! The imperative shell's one background loop: every [`POLL_PERIOD`] it reads the board's front
//! button, folds its raw level through the single-button gesture [`Recognizer`], and on a click
//! advances the shared selector to the next sketch. That is the whole control scheme — one
//! button, one gesture that matters:
//!
//! - **front click** → the next sketch (wrapping after the last back to the first).
//!
//! A long hold does nothing: the gallery has one action, so every other gesture maps to
//! [`Command::Ignore`]. Keeping the mapping a single total function over [`Gesture`] means a new
//! gesture cannot be silently dropped by an unwritten arm, and the mapping is pinned by a test
//! because it *is* the gallery's controls.
//!
//! The gesture rules stay pure inward (in `platform-input`); this crate only drives them against
//! the injected Button and Clock ports, so the loop is exercised on the host with fake
//! peripherals — no thread, no board.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use platform_core::Clock;
use platform_input::{
    ButtonLevel, Gesture, GestureConfig, Gestures, Recognizer, DEBOUNCE_MS, DOUBLE_CLICK_MS,
    HOLD_MS,
};

use crate::shared::SharedSelector;

/// How often the front button is polled.
///
/// Ten milliseconds is comfortably faster than the debounce window, so a real press settles
/// within a couple of polls. It is also this board's floor: ESP-IDF runs at
/// `CONFIG_FREERTOS_HZ = 100`, and a shorter sleep cannot yield.
pub const POLL_PERIOD: Duration = Duration::from_millis(10);

/// The input thread's stack, in bytes. Sized explicitly (it becomes a FreeRTOS task stack on
/// device); the loop only polls one pin and steps a small state machine, well within 8 KiB.
pub const INPUT_STACK_SIZE: usize = 8 * 1024;

/// The gestures the gallery's one button reports.
///
/// [`Prompt`](Gestures::Prompt), not [`WithDoubleClick`](Gestures::WithDoubleClick): the gallery
/// has a single action, so it never needs a second click, and a lone click lands immediately on
/// release rather than being held back a window. The button is structurally incapable of a
/// double-click, which is why [`command_for`] never has to decide what one would mean.
pub const GESTURES: Gestures = Gestures::Prompt;

/// The gesture timing the recogniser runs on — the board-wide defaults.
pub const INPUT_CONFIG: GestureConfig = GestureConfig {
    debounce_ms: DEBOUNCE_MS,
    hold_ms: HOLD_MS,
    double_click_ms: DOUBLE_CLICK_MS,
};

/// What a gesture asks the gallery to do.
///
/// The gallery has exactly one action, so this is nearly all [`Ignore`](Command::Ignore) — but it
/// stays an explicit total function over every [`Gesture`], so a new gesture would fail to build
/// rather than be silently dropped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// Advance the gallery to the next sketch.
    Advance,
    /// A gesture the gallery does not use.
    Ignore,
}

/// What each gesture means to the gallery — the whole control scheme, in one pinned mapping.
fn command_for(gesture: Gesture) -> Command {
    match gesture {
        Gesture::Click => Command::Advance,
        // The gallery has no second action. A double-click cannot occur under `Prompt` anyway;
        // both are named rather than caught by a wildcard, so a new gesture fails to build.
        Gesture::DoubleClick | Gesture::LongHold => Command::Ignore,
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

/// One poll cycle: read the button, recognise its gesture, and advance the gallery on a click.
/// The whole shell↔domain seam, testable without a thread.
fn cycle<Bn, C>(
    button: &mut Bn,
    recognizer: &mut Recognizer,
    shared: &SharedSelector,
    clock: &C,
    config: GestureConfig,
) where
    Bn: ButtonLevel,
    C: Clock,
{
    let now: u64 = clock.now();
    if let Some(gesture) = recognizer.update(now, button.pressed(), config) {
        match command_for(gesture) {
            Command::Advance => shared.advance(),
            Command::Ignore => {}
        }
    }
}

/// Spawn the input thread: poll `button` against the `clock`, and advance `shared` on a click,
/// every [`POLL_PERIOD`].
///
/// Everything moves into the thread, so each must be [`Send`] + `'static`. Returns the
/// [`InputTask`] handle, or the [`io::Error`] from failing to spawn the thread.
pub fn spawn_input<Bn, C>(
    mut button: Bn,
    shared: SharedSelector,
    clock: C,
    config: GestureConfig,
) -> io::Result<InputTask>
where
    Bn: ButtonLevel + Send + 'static,
    C: Clock + Send + 'static,
{
    let mut recognizer: Recognizer = Recognizer::new(GESTURES);
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("art-input".to_string())
        .stack_size(INPUT_STACK_SIZE)
        .spawn(move || {
            while !stop_in_thread.load(Ordering::Relaxed) {
                cycle(&mut button, &mut recognizer, &shared, &clock, config);
                thread::sleep(POLL_PERIOD);
            }
        })?;
    Ok(InputTask { handle, stop })
}

#[cfg(test)]
mod tests {
    use super::*;
    use art_core::Sketch;
    use std::cell::Cell;

    /// A levelled button whose raw pressed level is scripted by a cell.
    struct FakeButton<'a> {
        level: &'a Cell<bool>,
    }

    impl ButtonLevel for FakeButton<'_> {
        fn pressed(&mut self) -> bool {
            self.level.get()
        }
    }

    /// A clock the test advances by hand, one poll at a time.
    struct FakeClock<'a> {
        now: &'a Cell<u64>,
    }

    impl Clock for FakeClock<'_> {
        fn now(&self) -> u64 {
            self.now.get()
        }
    }

    /// Drive `ms` poll cycles at 1 ms each with the button held at `raw`, folding each through the
    /// loop — enough resolution to cross the debounce and hold thresholds a real press does.
    fn hold(
        button: &Cell<bool>,
        now: &Cell<u64>,
        recognizer: &mut Recognizer,
        shared: &SharedSelector,
        raw: bool,
        ms: u64,
    ) {
        button.set(raw);
        let mut fake_button: FakeButton<'_> = FakeButton { level: button };
        let clock: FakeClock<'_> = FakeClock { now };
        for _ in 0..ms {
            cycle(&mut fake_button, recognizer, shared, &clock, INPUT_CONFIG);
            now.set(now.get() + 1);
        }
    }

    /// A click is a press then a release — that maps to advancing the gallery.
    #[test]
    fn a_click_advances_the_gallery() {
        assert_eq!(command_for(Gesture::Click), Command::Advance);
    }

    /// Nothing else the button can report is an action.
    #[test]
    fn a_hold_is_ignored() {
        assert_eq!(command_for(Gesture::LongHold), Command::Ignore);
        assert_eq!(command_for(Gesture::DoubleClick), Command::Ignore);
    }

    /// Zero presses: an idle loop leaves the gallery on its first piece.
    #[test]
    fn an_idle_loop_does_not_advance() {
        let button: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let mut recognizer: Recognizer = Recognizer::new(GESTURES);
        let shared: SharedSelector = SharedSelector::new();
        hold(&button, &now, &mut recognizer, &shared, false, 100);
        assert_eq!(shared.current(), Sketch::Plume);
    }

    /// One press-and-release advances the gallery exactly once — the whole loop, driven on the
    /// host with a fake button and clock.
    #[test]
    fn one_press_and_release_advances_once() {
        let button: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let mut recognizer: Recognizer = Recognizer::new(GESTURES);
        let shared: SharedSelector = SharedSelector::new();

        hold(&button, &now, &mut recognizer, &shared, true, 40); // press, past debounce
        hold(&button, &now, &mut recognizer, &shared, false, 40); // release → click

        assert_eq!(
            shared.current(),
            Sketch::Squares,
            "one click advances one step"
        );
    }

    /// A long hold does not advance: it is a `LongHold`, not a click, so the gallery stays put
    /// while the button is held down.
    #[test]
    fn a_long_hold_does_not_advance() {
        let button: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let mut recognizer: Recognizer = Recognizer::new(GESTURES);
        let shared: SharedSelector = SharedSelector::new();

        hold(&button, &now, &mut recognizer, &shared, true, HOLD_MS + 50); // held past the hold threshold
        hold(&button, &now, &mut recognizer, &shared, false, 40); // then released

        assert_eq!(shared.current(), Sketch::Plume, "a hold is not a click");
    }

    /// Many clicks: five press-releases cycle the whole order and return to the plume.
    #[test]
    fn five_clicks_cycle_back_to_the_plume() {
        let button: Cell<bool> = Cell::new(false);
        let now: Cell<u64> = Cell::new(0);
        let mut recognizer: Recognizer = Recognizer::new(GESTURES);
        let shared: SharedSelector = SharedSelector::new();

        for _ in 0..Sketch::ALL.len() {
            hold(&button, &now, &mut recognizer, &shared, true, 40);
            hold(&button, &now, &mut recognizer, &shared, false, 40);
        }
        assert_eq!(
            shared.current(),
            Sketch::Plume,
            "a full cycle returns to the start"
        );
    }
}
