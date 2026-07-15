//! The display thread — read the shared cache, render the latest measurement.
//!
//! The imperative shell's second background loop, companion to the sampler: it reads the
//! freshest [`Observation`] from the [`SharedMoisture`] cache and, *only when the picture
//! changed*, hands it to a [`MoistureDisplay`] adapter to draw. It owns the *render
//! cadence* and the change-suppression; the pixel-pushing stays in the adapter (the ST7789
//! TFT on-device) and the freshness rule stays in the pure core, so this loop's body is a
//! straight line.
//!
//! ## The picture, not the reading
//!
//! The glass shows a creature beside the numbers, and for the states an operator must
//! notice that creature *animates* (`plant_display::scene`). So the loop suppresses on the
//! pair `(observation, frame)` rather than on the observation alone, and takes its cadence
//! from the same policy:
//!
//! - **healthy** → a motionless creature → a constant pair → painted once, then left
//!   alone, at [`RENDER_PERIOD`]. Nothing keeps the CPU awake between samples.
//! - **faulted / stale / never-sampled** → a moving creature → the pair advances on the
//!   sprite's own clock, so the panel repaints at [`ANIMATION_PERIOD`] — and only on the
//!   ticks where a new frame is genuinely due.
//!
//! That asymmetry is deliberate and load-bearing: motion costs power, so it is spent only
//! where it buys an operator information.
//!
//! ## Fail-visible, like the sampler
//!
//! - A **render error** is logged and skipped, never fatal: a flaky panel must not
//!   take a plant monitor down, and the next cycle repaints regardless.
//! - The display reads the *same* cache and the *same* staleness bound as the
//!   native-API server, so once the sensor dies the glass says so within a render
//!   cycle — never a frozen last value. Because it is handed an [`Observation`] and
//!   not an `Option`, it can name the *reason*: a faulted probe reads differently
//!   from a sampler that stopped.
//!
//! Nothing here is ESP-specific, so the whole loop is exercised on the host against
//! a fake display; the composition root only wires the real ST7789 adapter in.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::warn;
use plant_core::{MoistureDisplay, Observation, Tick};

use crate::clock::Monotonic;
use crate::shared::SharedMoisture;

/// How often the display is repainted.
///
/// The moisture value only changes each sample period (2 s), but a shorter render
/// tick lets the *unavailable* transition show promptly: the panel flips to its
/// placeholder within a second of the reading ageing out rather than waiting a full
/// sample period. Repainting a couple of text lines is cheap.
pub const RENDER_PERIOD: Duration = Duration::from_secs(1);

/// How often the display is repainted **while the creature is animating**.
///
/// An unhealthy observation draws a moving creature, whose shortest frame hold is 60 ms;
/// waking every 50 ms is enough to land on every frame. This cadence applies *only* to the
/// states an operator needs to notice — a healthy reading falls back to [`RENDER_PERIOD`],
/// so a working monitor is not kept awake by a creature that never moves. Waking is not
/// repainting: a tick that finds the same frame due touches no pixels and no SPI bus.
pub const ANIMATION_PERIOD: Duration = Duration::from_millis(50);

/// The shortest sleep the render loop will ever take.
///
/// A paint that outran its tick budget would otherwise sleep for zero and the display
/// thread would never yield — starving lower-priority FreeRTOS tasks on a single core.
/// That happened for real: before the sprite fill was made contiguous, a paint took 85 ms
/// against a 50 ms budget. The loop survived on luck, and luck is not a scheduling policy.
pub const MIN_YIELD: Duration = Duration::from_millis(1);

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
    /// The interval between ticks while the glass is still (a healthy reading).
    pub period: Duration,
    /// The interval between ticks while the creature animates (any unhealthy state).
    pub animation_period: Duration,
    /// The staleness bound (in [`Tick`] milliseconds) past which a reading is shown
    /// as unavailable — the same bound the sampler and native-API server use.
    pub max_age: Tick,
    /// The display thread's stack size, in bytes.
    pub stack_size: usize,
}

impl DisplayConfig {
    /// A config for the staleness bound `max_age`, with the cadences and stack size
    /// defaulted to the module constants.
    pub fn new(max_age: Tick) -> Self {
        Self {
            period: RENDER_PERIOD,
            animation_period: ANIMATION_PERIOD,
            max_age,
            stack_size: DISPLAY_STACK_SIZE,
        }
    }

    /// How long to sleep after painting `glass`.
    ///
    /// The whole cadence policy: a moving creature earns a fast tick, everything else
    /// (including the very first tick, before anything is drawn) gets the slow one.
    fn tick_period(&self, glass: Glass) -> Duration {
        match glass {
            Some(shown) if plant_display::is_animated(shown.observation) => self.animation_period,
            _ => self.period,
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

/// What is currently on the glass: `None` before anything is drawn.
///
/// Carries three things: the [`Observation`] painted, the *frame* of its creature that was
/// painted, and the [`Tick`] at which this observation first appeared. The option
/// distinguishes "nothing drawn yet" from "drew the never-sampled state", so the very
/// first tick always paints — even before a reading exists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shown {
    observation: Observation,
    frame: usize,
    since: Tick,
}

/// The glass, or nothing yet.
type Glass = Option<Shown>;

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
    let mut glass: Glass = None;
    while !stop.load(Ordering::Relaxed) {
        let before: Glass = glass;
        let started: Instant = Instant::now();
        glass = render_once(&mut display, &shared, clock.now(), config.max_age, glass);
        let budget: Duration = config.tick_period(glass);

        // A paint that outruns its own tick means the creature cannot keep its timing: the
        // panel silently drops frames and the animation drags. Nothing on the glass says
        // so — a slow creature still looks like a creature — so the loop says it here.
        // Only an actual repaint is measured; a suppressed tick costs nothing.
        let painted: bool = glass != before;
        let took: Duration = started.elapsed();
        if painted && took > budget {
            warn!("plant-display: paint took {took:?}, over the {budget:?} tick budget");
        }

        // The cadence follows what is on the glass: fast while a creature moves, slow
        // once the reading is healthy again. The paint's own cost is subtracted so the
        // frame rate does not drift, and the sleep never reaches zero — see [`MIN_YIELD`].
        thread::sleep(budget.saturating_sub(took).max(MIN_YIELD));
    }
}

/// One render cycle: read the freshest observation and draw it *only if the picture
/// changed*.
///
/// The whole shell↔adapter seam, in isolation and testable without a thread. The
/// freshness decision is the cache's ([`SharedMoisture::observe`]), so the adapter
/// receives a verdict — a measurement, a named probe fault, staleness, or
/// never-sampled — and renders whichever it is.
///
/// ## What "changed" means, now that the glass animates
///
/// The picture is the pair `(observation, frame)`, where the frame comes from
/// [`plant_display::frame_index`] — a pure function of the observation and how long it has
/// been current. Suppression compares the *pair*, not just the observation:
///
/// - A **healthy** reading maps to a motionless creature, so its frame is always 0 and the
///   pair is constant. The panel is painted once and then left alone, exactly as before —
///   no per-tick redraw, no flicker, and nothing to keep the CPU awake between samples.
/// - An **unhealthy** state animates, so its frame advances on its own clock and the pair
///   changes. The panel repaints only on the ticks where a new frame is actually due, not
///   on every tick.
///
/// `since` — when the current observation first appeared — is the animation's zero. It is
/// reset on any transition, so a creature always starts at its first frame; a fault giving
/// way to another fault therefore restarts the startle, which is what an operator watching
/// the glass should see.
///
/// A render error is logged, not propagated, and leaves the glass unchanged so the next
/// tick retries.
fn render_once<D>(
    display: &mut D,
    shared: &SharedMoisture,
    now: Tick,
    max_age: Tick,
    glass: Glass,
) -> Glass
where
    D: MoistureDisplay,
    D::Error: std::fmt::Display,
{
    let observation: Observation = shared.observe(now, max_age);

    // The animation's zero: the tick this observation became the current one. A changed
    // observation restarts its creature; an unchanged one keeps counting.
    let since: Tick = match glass {
        Some(shown) if shown.observation == observation => shown.since,
        _ => now,
    };
    let elapsed: Tick = now.saturating_sub(since);
    let frame: usize = plant_display::frame_index(observation, elapsed);

    let next: Shown = Shown {
        observation,
        frame,
        since,
    };
    if glass == Some(next) {
        return glass; // same picture — leave the glass untouched.
    }
    match display.show(observation, elapsed) {
        Ok(()) => Some(next),
        Err(err) => {
            warn!("plant-display: render failed, skipping this cycle: {err}");
            glass // keep the prior state; retry next tick.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;

    use plant_core::{Measurement, Moisture, ProbeFault};

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
        shown: Arc<Mutex<Vec<Observation>>>,
        /// Every `elapsed` the loop handed the adapter, in order — the animation clock as
        /// the panel sees it.
        elapsed: Arc<Mutex<Vec<Tick>>>,
        fail: bool,
        ping: Option<Sender<()>>,
    }

    impl FakeDisplay {
        /// A healthy display and a handle onto everything it is shown.
        fn new() -> (Self, Arc<Mutex<Vec<Observation>>>) {
            let shown: Arc<Mutex<Vec<Observation>>> = Arc::new(Mutex::new(Vec::new()));
            let display: FakeDisplay = FakeDisplay {
                shown: Arc::clone(&shown),
                elapsed: Arc::new(Mutex::new(Vec::new())),
                fail: false,
                ping: None,
            };
            (display, shown)
        }

        /// A display that errors on every `show` — a dead or disconnected panel.
        fn failing() -> Self {
            FakeDisplay {
                shown: Arc::new(Mutex::new(Vec::new())),
                elapsed: Arc::new(Mutex::new(Vec::new())),
                fail: true,
                ping: None,
            }
        }

        /// Ping `tx` after every render, so a test can await a real repaint.
        fn pinging(mut self, tx: Sender<()>) -> Self {
            self.ping = Some(tx);
            self
        }

        /// The `elapsed` values this display was handed, in order.
        fn elapsed_log(&self) -> Vec<Tick> {
            self.elapsed.lock().expect("elapsed log lock").clone()
        }
    }

    impl MoistureDisplay for FakeDisplay {
        type Error = TestError;

        fn show(&mut self, observation: Observation, elapsed: Tick) -> Result<(), TestError> {
            self.shown.lock().expect("shown log lock").push(observation);
            self.elapsed.lock().expect("elapsed log lock").push(elapsed);
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

    /// The observation a glass is showing, if anything.
    fn on_glass(glass: Glass) -> Option<Observation> {
        glass.map(|shown: Shown| shown.observation)
    }

    #[test]
    fn the_first_paint_shows_the_fresh_measurement_whole() {
        // The happy path: with nothing yet on the glass (`None`), a published
        // reading within the bound is painted — raw and percent both reach it.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let after: Glass = render_once(&mut display, &shared, 20, 50, None);

        let log: Vec<Observation> = shown.lock().unwrap().clone();
        assert_eq!(log, vec![Observation::Fresh(measurement(30))]);
        assert_eq!(
            log[0].measurement().unwrap().raw(),
            300,
            "the raw count reaches the display"
        );
        assert_eq!(
            on_glass(after),
            Some(Observation::Fresh(measurement(30))),
            "it is now on the glass"
        );
    }

    #[test]
    fn the_first_paint_of_an_empty_cache_shows_never_sampled() {
        // Nothing measured yet: the first paint still happens, so the adapter draws
        // a placeholder rather than leaving a blank screen — and it is told the
        // device has simply not sampled yet, not that something broke.
        let shared: SharedMoisture = SharedMoisture::new();
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let after: Glass = render_once(&mut display, &shared, 100, 50, None);

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![Observation::NeverSampled]
        );
        assert_eq!(on_glass(after), Some(Observation::NeverSampled));
    }

    #[test]
    fn a_stale_reading_shows_stale() {
        // A reading past the staleness bound reaches the display as `Stale` — the
        // dead-sampler case: never a frozen last value.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(42)), 0);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        // age = 51 > max_age 50.
        render_once(&mut display, &shared, 51, 50, None);

        assert_eq!(shown.lock().unwrap().clone(), vec![Observation::Stale]);
    }

    #[test]
    fn a_faulted_probe_reaches_the_glass_as_its_own_fault() {
        // The glass must be able to say WHY there is no number. A fault is not the
        // same picture as staleness, and the reason survives to the adapter.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Err(ProbeFault::OverRange), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        render_once(&mut display, &shared, 20, 50, None);

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![Observation::Faulted(ProbeFault::OverRange)]
        );
    }

    #[test]
    fn one_fault_giving_way_to_another_repaints() {
        // Fault -> fault is a real transition, not a steady state: an operator
        // watching the glass must see the reason change.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Err(ProbeFault::OverRange), 0);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let s0: Glass = render_once(&mut display, &shared, 1, 50, None);
        shared.publish(Err(ProbeFault::Unreadable), 2);
        render_once(&mut display, &shared, 3, 50, s0);

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![
                Observation::Faulted(ProbeFault::OverRange),
                Observation::Faulted(ProbeFault::Unreadable)
            ]
        );
    }

    #[test]
    fn a_steady_reading_is_drawn_once_then_suppressed() {
        // The flicker fix: three ticks over an unchanging reading paint exactly
        // once. A steady probe is drawn and then left alone — no per-tick redraw.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let s0: Glass = render_once(&mut display, &shared, 20, 50, None);
        let s1: Glass = render_once(&mut display, &shared, 21, 50, s0);
        let s2: Glass = render_once(&mut display, &shared, 22, 50, s1);

        assert_eq!(
            shown.lock().unwrap().len(),
            1,
            "a steady reading is painted once, not per tick"
        );
        assert_eq!(on_glass(s2), Some(Observation::Fresh(measurement(30))));
    }

    #[test]
    fn a_healthy_reading_never_repaints_and_keeps_the_slow_cadence() {
        // THE POWER RULE. A healthy reading maps to a motionless creature, so however many
        // ticks pass the panel is painted exactly once and the loop takes the slow
        // cadence. If this regresses the device repaints forever and the deep-sleep duty
        // cycle dies -- silently, because the picture still looks right.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 0);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();
        let config: DisplayConfig = DisplayConfig::new(60_000);

        let g0: Glass = render_once(&mut display, &shared, 0, 60_000, None);
        let g1: Glass = render_once(&mut display, &shared, 5_000, 60_000, g0);
        let g2: Glass = render_once(&mut display, &shared, 30_000, 60_000, g1);

        assert_eq!(shown.lock().unwrap().len(), 1, "a healthy glass repainted");
        assert_eq!(config.tick_period(g2), RENDER_PERIOD);
    }

    /// The first hold of the creature shown for `observation` — the exact tick at which it
    /// advances a frame. Anchored on the vendored art, so these tests cannot go vacuous if
    /// its timing changes.
    fn first_hold(observation: Observation) -> Tick {
        Tick::from(plant_display::scene(observation).sprite.frames()[0].hold_ms())
    }

    #[test]
    fn a_stale_reading_repaints_as_its_creature_animates() {
        // An unhealthy state animates: the same observation, at a later tick, is a
        // different creature frame and therefore a real repaint.
        //
        // A reading MUST be published for `observe` to return Stale — an empty cache is
        // NeverSampled, a different creature with a different frame timing.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(42)), 0);
        let hold: Tick = first_hold(Observation::Stale);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        // age = 100 > max_age 50, so every tick observes Stale.
        let g0: Glass = render_once(&mut display, &shared, 100, 50, None);
        let g1: Glass = render_once(&mut display, &shared, 100 + hold, 50, g0);

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![Observation::Stale, Observation::Stale],
            "the stale creature must repaint when its frame advances"
        );
        assert_eq!(display.elapsed_log(), vec![0, hold], "the animation clock");
        assert_eq!(on_glass(g1), Some(Observation::Stale));
    }

    #[test]
    fn an_animating_creature_is_not_repainted_between_frames() {
        // Between frames the creature is left alone: a tick landing inside the current
        // frame's hold repaints nothing. Waking is not repainting.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(42)), 0); // else the cache reads NeverSampled
        let hold: Tick = first_hold(Observation::Stale);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let g0: Glass = render_once(&mut display, &shared, 100, 50, None);
        let g1: Glass = render_once(&mut display, &shared, 100 + hold / 2, 50, g0);
        render_once(&mut display, &shared, 100 + hold - 1, 50, g1);

        assert_eq!(
            shown.lock().unwrap().len(),
            1,
            "ticks inside one frame's hold must not repaint"
        );
    }

    #[test]
    fn a_transition_restarts_the_animation_clock() {
        // A fault arriving after an hour of healthy readings must startle from its first
        // frame, not resume mid-startle.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 0);
        let (mut display, _shown): (FakeDisplay, _) = FakeDisplay::new();

        let g0: Glass = render_once(&mut display, &shared, 1_000, 60_000, None);
        shared.publish(Err(ProbeFault::OverRange), 9_000);
        render_once(&mut display, &shared, 9_000, 60_000, g0);

        assert_eq!(
            display.elapsed_log(),
            vec![0, 0],
            "each new observation starts its creature at elapsed 0"
        );
    }

    #[test]
    fn the_cadence_is_fast_only_while_a_creature_animates() {
        // The first tick (nothing drawn yet) takes the slow cadence rather than spinning.
        let config: DisplayConfig = DisplayConfig::new(60_000);
        let animating: Glass = Some(Shown {
            observation: Observation::Stale,
            frame: 0,
            since: 0,
        });
        let still: Glass = Some(Shown {
            observation: Observation::Fresh(measurement(30)),
            frame: 0,
            since: 0,
        });

        assert_eq!(config.tick_period(animating), ANIMATION_PERIOD);
        assert_eq!(config.tick_period(still), RENDER_PERIOD);
        assert_eq!(config.tick_period(None), RENDER_PERIOD);
    }

    #[test]
    fn a_changed_reading_is_redrawn() {
        // A genuinely new value does repaint: the two distinct readings both reach
        // the glass, in order, with no redundant frame between them.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 10);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let s0: Glass = render_once(&mut display, &shared, 11, 50, None);
        shared.publish(Ok(measurement(60)), 20);
        render_once(&mut display, &shared, 21, 50, s0);

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![
                Observation::Fresh(measurement(30)),
                Observation::Fresh(measurement(60))
            ]
        );
    }

    #[test]
    fn a_transition_to_stale_is_redrawn() {
        // Value → stale is a change, so it repaints: the glass follows a dying
        // sampler from its last value to the stale placeholder.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(42)), 0);
        let (mut display, shown): (FakeDisplay, _) = FakeDisplay::new();

        let s0: Glass = render_once(&mut display, &shared, 10, 50, None); // fresh
        render_once(&mut display, &shared, 60, 50, s0); // age 60 > 50 -> Stale

        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![Observation::Fresh(measurement(42)), Observation::Stale]
        );
    }

    #[test]
    fn a_render_error_keeps_the_prior_state_and_is_not_fatal() {
        // A panel that errors on every show must not panic the loop: render_once
        // logs the error, returns the *prior* state (so the next tick retries the
        // same reading), and does not advance `shown` past a failed paint.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(55)), 0);
        let mut display: FakeDisplay = FakeDisplay::failing();

        let after: Glass = render_once(&mut display, &shared, 0, 50, None);

        assert_eq!(
            on_glass(after),
            None,
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
        shared.publish(Ok(measurement(45)), clock.now());
        let (tx, rx): (Sender<()>, _) = channel();
        let (base, shown): (FakeDisplay, _) = FakeDisplay::new();
        let display: FakeDisplay = base.pinging(tx);
        let config: DisplayConfig = DisplayConfig {
            period: Duration::from_millis(1),
            animation_period: Duration::from_millis(1),
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
        shared.publish(Ok(measurement(50)), clock.now());
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a changed value must re-render");

        task.stop();
        task.join().expect("the display thread must not panic");

        // Exactly the two distinct values, deduped across the many 1 ms ticks.
        assert_eq!(
            shown.lock().unwrap().clone(),
            vec![
                Observation::Fresh(measurement(45)),
                Observation::Fresh(measurement(50))
            ]
        );
    }
}
