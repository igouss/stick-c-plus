//! The display thread — read the shared cache, render the latest measurement.
//!
//! The imperative shell's second background loop, companion to the sampler: every
//! [`RENDER_PERIOD`] it reads the freshest [`Measurement`] from the
//! [`SharedMoisture`] cache and, *only when it changed*, hands it to a
//! [`MoistureDisplay`] adapter to draw. It owns the *render cadence* and the
//! change-suppression; the pixel-pushing stays in the adapter (the ST7789 TFT
//! on-device) and the freshness rule stays in the pure core, so this loop's body is
//! a straight line. Redrawing only on change keeps a steady panel from flickering —
//! it is painted once, then left alone until the value or its availability moves.
//!
//! ## Fail-visible, like the sampler
//!
//! - A **render error** is logged and skipped, never fatal: a flaky panel must not
//!   take a plant monitor down, and the next cycle repaints regardless.
//! - The display reads the *same* cache and the *same* staleness bound as the
//!   native-API server, so once the sensor dies the glass shows *unavailable* within
//!   a render cycle of the reading ageing out — never a frozen last value.
//!
//! Nothing here is ESP-specific, so the whole loop is exercised on the host against
//! a fake display; the composition root only wires the real ST7789 adapter in.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use plant_core::{Measurement, MoistureDisplay, Tick};

use crate::clock::Monotonic;
use crate::shared::SharedMoisture;

/// How often the display is repainted.
///
/// The moisture value only changes each sample period (2 s), but a shorter render
/// tick lets the *unavailable* transition show promptly: the panel flips to its
/// placeholder within a second of the reading ageing out rather than waiting a full
/// sample period. Repainting a couple of text lines is cheap.
pub const RENDER_PERIOD: Duration = Duration::from_secs(1);

/// The display thread's stack, in bytes.
///
/// On-device this sizes a FreeRTOS task stack, so it is set explicitly rather than
/// inherited. embedded-graphics text + a screen clear stream through the adapter's
/// SPI buffer at constant stack depth, so 8 KiB holds it with headroom on the
/// ESP32's scarce SRAM — but, like the sampler's, it is validated against the true
/// high-water mark on the metal before it is trusted.
pub const DISPLAY_STACK_SIZE: usize = 8 * 1024;

/// How the display loop is tuned: its cadence, its staleness bound, and its stack.
///
/// [`max_age`](Self::max_age) has no sensible default — it is the sampler's
/// staleness bound, so the display's *unavailable* agrees with the server's — and
/// must be supplied; the cadence and stack default to the module constants.
/// [`Copy`], so the composition root can build it and still move it into the thread.
#[derive(Clone, Copy)]
pub struct DisplayConfig {
    /// The interval between repaints.
    pub period: Duration,
    /// The staleness bound (in [`Tick`] milliseconds) past which a reading is shown
    /// as unavailable — the same bound the sampler and native-API server use.
    pub max_age: Tick,
    /// The display thread's stack size, in bytes.
    pub stack_size: usize,
}

impl DisplayConfig {
    /// A config for the staleness bound `max_age`, with the cadence and stack size
    /// defaulted to the module constants.
    pub fn new(max_age: Tick) -> Self {
        Self {
            period: RENDER_PERIOD,
            max_age,
            stack_size: DISPLAY_STACK_SIZE,
        }
    }
}

/// A running display thread — a handle to stop and join it.
///
/// Dropping the handle detaches the thread (it keeps repainting), which is what the
/// composition root wants: the monitor renders for the life of the program. Tests
/// and a future clean-shutdown path use [`stop`](Self::stop) + [`join`](Self::join).
pub struct DisplayTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl DisplayTask {
    /// Ask the display loop to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the display thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Spawn the display thread: `shared` → [`MoistureDisplay::show`], every
/// `config.period`.
///
/// The thread is named and sized per `config`. `clock` is the shared time base —
/// the same [`Monotonic`] the sampler writes against — so the reading's age is
/// measured on one clock. `display` moves into the thread, so it must be [`Send`] +
/// `'static`; its error must be [`Display`](std::fmt::Display) so a render failure
/// can be logged.
///
/// Returns the [`DisplayTask`] handle, or the [`io::Error`] from failing to spawn
/// the OS/RTOS thread.
pub fn spawn_display<D>(
    display: D,
    shared: SharedMoisture,
    clock: Monotonic,
    config: DisplayConfig,
) -> io::Result<DisplayTask>
where
    D: MoistureDisplay + Send + 'static,
    D::Error: std::fmt::Display,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("plant-display".to_string())
        .stack_size(config.stack_size)
        .spawn(move || render_loop(display, shared, clock, config, stop_in_thread))?;
    Ok(DisplayTask { handle, stop })
}

/// What is currently on the glass: `None` before anything is drawn, else the last
/// reading handed to the display (itself `Some`/`None` for a value/unavailable).
///
/// The outer option distinguishes "nothing drawn yet" from "drew the unavailable
/// state", so the very first tick always paints — even when there is no reading.
type Shown = Option<Option<Measurement>>;

/// The thread body: on each tick, redraw only if the reading changed — until asked
/// to stop.
fn render_loop<D>(
    mut display: D,
    shared: SharedMoisture,
    clock: Monotonic,
    config: DisplayConfig,
    stop: Arc<AtomicBool>,
) where
    D: MoistureDisplay,
    D::Error: std::fmt::Display,
{
    let mut shown: Shown = None;
    while !stop.load(Ordering::Relaxed) {
        shown = render_once(&mut display, &shared, clock.now(), config.max_age, shown);
        thread::sleep(config.period);
    }
}

/// One render cycle: read the freshest measurement and draw it *only if it changed*.
///
/// The whole shell↔adapter seam, in isolation and testable without a thread. The
/// freshness decision is the cache's ([`SharedMoisture::latest`]), so a stale or
/// absent reading arrives as `None` and the adapter shows its unavailable
/// placeholder. Change-suppression keeps the panel steady: a reading equal to what
/// is already on the glass is *not* redrawn, so a steady probe is painted once and
/// then left alone — no per-tick redraw, no flicker. A transition (value → value,
/// value → unavailable, or the first paint) does redraw. A render error is logged,
/// not propagated, and leaves `shown` unchanged so the next tick retries. Returns
/// the value now on the glass.
fn render_once<D>(
    display: &mut D,
    shared: &SharedMoisture,
    now: Tick,
    max_age: Tick,
    shown: Shown,
) -> Shown
where
    D: MoistureDisplay,
    D::Error: std::fmt::Display,
{
    let reading: Option<Measurement> = shared.latest(now, max_age);
    if shown == Some(reading) {
        return shown; // unchanged — leave the glass untouched.
    }
    match display.show(reading) {
        Ok(()) => Some(reading),
        Err(err) => {
            warn!("plant-display: render failed, skipping this cycle: {err}");
            shown // keep the prior state; retry next tick.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;

    use plant_core::Moisture;

    /// A measurement at `percent`, with a raw count derived from it so a test can
    /// confirm the exact value reached the glass.
    fn measurement(percent: u8) -> Measurement {
        Measurement::new(
            u16::from(percent) * 10,
            Moisture::new(percent).expect("test percent is 0..=100"),
        )
    }

    /// A render error whose message is [`Display`], as the loop's log bound
    /// requires.
    #[derive(Clone, Debug)]
    struct TestError(&'static str);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    /// A [`MoistureDisplay`] fake that records every reading it is shown, so a test
    /// can assert the exact sequence the loop painted. The `shown` log is an [`Arc`]
    /// so a test keeps a handle after the display moves into the thread; `fail`
    /// makes every `show` error (still recording first); `ping` signals each render
    /// so a test can await a real repaint rather than poll.
    struct FakeDisplay {
        shown: Arc<Mutex<Vec<Option<Measurement>>>>,
        fail: bool,
        ping: Option<Sender<()>>,
    }

    impl FakeDisplay {
        /// A healthy display and a handle onto everything it is shown.
        fn new() -> (Self, Arc<Mutex<Vec<Option<Measurement>>>>) {
            let shown: Arc<Mutex<Vec<Option<Measurement>>>> = Arc::new(Mutex::new(Vec::new()));
            let display: FakeDisplay = FakeDisplay {
                shown: Arc::clone(&shown),
                fail: false,
                ping: None,
            };
            (display, shown)
        }

        /// A display that errors on every `show` — a dead or disconnected panel.
        fn failing() -> Self {
            FakeDisplay {
                shown: Arc::new(Mutex::new(Vec::new())),
                fail: true,
                ping: None,
            }
        }

        /// Ping `tx` after every render, so a test can await a real repaint.
        fn pinging(mut self, tx: Sender<()>) -> Self {
            self.ping = Some(tx);
            self
        }
    }

    impl MoistureDisplay for FakeDisplay {
        type Error = TestError;

        fn show(&mut self, reading: Option<Measurement>) -> Result<(), TestError> {
            self.shown.lock().expect("shown log lock").push(reading);
            if let Some(tx) = &self.ping {
                // Unbounded channel: the send never blocks the display thread.
                let _ = tx.send(());
            }
            if self.fail {
                Err(TestError("panel offline"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn the_first_paint_shows_the_fresh_measurement_whole() {
        // The happy path: with nothing yet on the glass (`None`), a published
        // reading within the bound is painted — raw and percent both reach it.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(30), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let after: Shown = render_once(&mut display, &shared, 20, 50, None);

        let log: Vec<Option<Measurement>> = shown.lock().unwrap().clone();
        assert_eq!(log, vec![Some(measurement(30))]);
        assert_eq!(
            log[0].unwrap().raw(),
            300,
            "the raw count reaches the display"
        );
        assert_eq!(after, Some(Some(measurement(30))), "it is now on the glass");
    }

    #[test]
    fn the_first_paint_of_an_empty_cache_shows_unavailable() {
        // Nothing measured yet: the first paint still happens, showing `None`, so
        // the adapter draws its unavailable placeholder rather than a blank screen.
        let shared: SharedMoisture = SharedMoisture::new();
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let after: Shown = render_once(&mut display, &shared, 100, 50, None);

        assert_eq!(shown.lock().unwrap().clone(), vec![None]);
        assert_eq!(after, Some(None));
    }

    #[test]
    fn a_stale_reading_shows_unavailable() {
        // A reading past the staleness bound reaches the display as `None` — the
        // dead-sensor case: the glass shows unavailable, never a frozen last value.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(42), 0);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        // age = 51 > max_age 50.
        render_once(&mut display, &shared, 51, 50, None);

        assert_eq!(shown.lock().unwrap().clone(), vec![None]);
    }

    #[test]
    fn a_steady_reading_is_drawn_once_then_suppressed() {
        // The flicker fix: three ticks over an unchanging reading paint exactly
        // once. A steady probe is drawn and then left alone — no per-tick redraw.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(30), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let s0: Shown = render_once(&mut display, &shared, 20, 50, None);
        let s1: Shown = render_once(&mut display, &shared, 21, 50, s0);
        let s2: Shown = render_once(&mut display, &shared, 22, 50, s1);

        assert_eq!(
            shown.lock().unwrap().len(),
            1,
            "a steady reading is painted once, not per tick"
        );
        assert_eq!(s2, Some(Some(measurement(30))));
    }

    #[test]
    fn a_changed_reading_is_redrawn() {
        // A genuinely new value does repaint: the two distinct readings both reach
        // the glass, in order, with no redundant frame between them.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(30), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let s0: Shown = render_once(&mut display, &shared, 11, 50, None);
        shared.publish(measurement(60), 20);
        render_once(&mut display, &shared, 21, 50, s0);

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![Some(measurement(30)), Some(measurement(60))]
        );
    }

    #[test]
    fn a_transition_to_unavailable_is_redrawn() {
        // Value → unavailable is a change, so it repaints: the glass follows a dying
        // probe from its last value to the unavailable placeholder.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(42), 0);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let s0: Shown = render_once(&mut display, &shared, 10, 50, None); // fresh -> Some(42)
        render_once(&mut display, &shared, 60, 50, s0); // age 60 > 50 -> None

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![Some(measurement(42)), None]
        );
    }

    #[test]
    fn a_render_error_keeps_the_prior_state_and_is_not_fatal() {
        // A panel that errors on every show must not panic the loop: render_once
        // logs the error, returns the *prior* state (so the next tick retries the
        // same reading), and does not advance `shown` past a failed paint.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(55), 0);
        let mut display: FakeDisplay = FakeDisplay::failing();

        let after: Shown = render_once(&mut display, &shared, 0, 50, None);

        assert_eq!(
            after, None,
            "a failed paint does not advance the shown state"
        );
    }

    #[test]
    fn the_spawned_thread_redraws_on_change_and_stops_cleanly() {
        // The one integration test: prove the real thread wiring — spawn, paint the
        // first value, suppress the steady ticks, repaint on a change, then stop and
        // join without a panic. Blocking on pings (not polling) keeps it robust.
        let clock: Monotonic = Monotonic::start();
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(45), clock.now());
        let (tx, rx): (Sender<()>, _) = channel();
        let (base, shown): (FakeDisplay, _) = FakeDisplay::new();
        let display: FakeDisplay = base.pinging(tx);
        let config: DisplayConfig = DisplayConfig {
            period: Duration::from_millis(1),
            max_age: 60_000,
            stack_size: 256 * 1024,
        };

        let task: DisplayTask =
            spawn_display(display, shared.clone(), clock, config).expect("spawn display thread");

        // First paint fires once; the steady value is then suppressed (no second
        // ping arrives on its own).
        rx.recv_timeout(Duration::from_secs(2))
            .expect("the display must paint the first value");
        // A genuine change re-arms a paint.
        shared.publish(measurement(50), clock.now());
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a changed value must re-render");

        task.stop();
        task.join().expect("the display thread must not panic");

        // Exactly the two distinct values, deduped across the many 1 ms ticks.
        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![Some(measurement(45)), Some(measurement(50))]
        );
    }
}
