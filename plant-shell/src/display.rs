//! The display thread — read the shared cache, render the latest measurement.
//!
//! The imperative shell's second background loop, companion to the sampler: every
//! [`RENDER_PERIOD`] it reads the freshest [`Measurement`] from the
//! [`SharedMoisture`] cache and hands it to a [`MoistureDisplay`] adapter to draw.
//! It owns the *render cadence*; the pixel-pushing stays in the adapter (the ST7789
//! TFT on-device) and the freshness rule stays in the pure core, so this loop's
//! body is a straight line.
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

/// The thread body: render, sleep — until asked to stop.
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
    while !stop.load(Ordering::Relaxed) {
        render_once(&mut display, &shared, clock.now(), config.max_age);
        thread::sleep(config.period);
    }
}

/// One render cycle: read the freshest measurement and draw it.
///
/// The whole shell↔adapter seam, in isolation and testable without a thread: the
/// freshness decision is the cache's ([`SharedMoisture::latest`]), so a stale or
/// absent reading arrives as `None` and the adapter shows its unavailable
/// placeholder. A render error is logged, not propagated — the panel is not allowed
/// to take the monitor down. Complexity is zero branches on the value.
fn render_once<D>(display: &mut D, shared: &SharedMoisture, now: Tick, max_age: Tick)
where
    D: MoistureDisplay,
    D::Error: std::fmt::Display,
{
    let reading: Option<Measurement> = shared.latest(now, max_age);
    if let Err(err) = display.show(reading) {
        warn!("plant-display: render failed, skipping this cycle: {err}");
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
    fn a_fresh_measurement_is_shown_with_its_raw_and_percent() {
        // The happy path: a published reading, within the bound, reaches the glass
        // whole — raw and percent.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(30), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        render_once(&mut display, &shared, 20, 50);

        let log: Vec<Option<Measurement>> = shown.lock().unwrap().clone();
        assert_eq!(log, vec![Some(measurement(30))]);
        assert_eq!(
            log[0].unwrap().raw(),
            300,
            "the raw count reaches the display"
        );
    }

    #[test]
    fn an_empty_cache_shows_unavailable() {
        // Nothing measured yet: the loop shows `None`, so the adapter draws its
        // unavailable placeholder rather than a blank value.
        let shared: SharedMoisture = SharedMoisture::new();
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        render_once(&mut display, &shared, 100, 50);

        assert_eq!(shown.lock().unwrap().clone(), vec![None]);
    }

    #[test]
    fn a_stale_reading_shows_unavailable() {
        // A reading past the staleness bound reaches the display as `None` — the
        // dead-sensor case: the glass shows unavailable, never a frozen last value.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(42), 0);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        // age = 51 > max_age 50.
        render_once(&mut display, &shared, 51, 50);

        assert_eq!(shown.lock().unwrap().clone(), vec![None]);
    }

    #[test]
    fn a_render_error_is_logged_not_fatal() {
        // A panel that errors on every show must not panic the loop: render_once
        // swallows the error (logs it) and returns.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(55), 0);
        let mut display: FakeDisplay = FakeDisplay::failing();

        // No panic, no propagation — the cycle simply completes.
        render_once(&mut display, &shared, 0, 50);
    }

    #[test]
    fn the_spawned_thread_renders_and_stops_cleanly() {
        // The one integration test: prove the real thread wiring — spawn, render
        // through the loop, then stop and join without a panic. Blocking on two
        // pings (rather than polling) makes it robust: a second render starting
        // proves the first completed.
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

        rx.recv_timeout(Duration::from_secs(2))
            .expect("the display must render once");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a second render proves the first completed");

        task.stop();
        task.join().expect("the display thread must not panic");

        // A generous max_age kept the value fresh, so every recorded frame is it.
        let log: Vec<Option<Measurement>> = shown.lock().unwrap().clone();
        assert!(!log.is_empty(), "the thread rendered at least once");
        assert!(
            log.iter()
                .all(|frame: &Option<Measurement>| *frame == Some(measurement(45))),
            "every frame shows the fresh value"
        );
    }
}
