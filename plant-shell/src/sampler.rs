//! The sampler thread — read the ADC, fold through the pure step, publish.
//!
//! The imperative shell's one background loop: every [`SAMPLE_PERIOD`] it takes a
//! reading from the [`SoilSensor`] adapter, hands it to the pure
//! [`step`](plant_core::sampler::step), and publishes the latest [`Moisture`]
//! into the [`SharedMoisture`] cache the display and native-API server read. It
//! owns the *timing* and the *cache*; every scrap of averaging and calibration
//! stays inward in `plant-core`, so this loop's body is a straight line.
//!
//! ## Two failure modes, both fail-visible
//!
//! - A **failed read** publishes nothing. The last reading is left to age out, so
//!   consumers see it go unavailable ([`fresh`](plant_core::fresh)) rather than
//!   frozen — a sensor that starts erroring must not keep serving its last
//!   healthy value.
//! - A **dead thread** (a panic, a hang) likewise stops refreshing the cache, so
//!   the same staleness rule retires the value within [`STALENESS_PERIODS`]
//!   periods. No supervisor is needed: staleness *is* the liveness check.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use plant_core::sampler::step;
use plant_core::{Calibration, Measurement, Sample, SoilSensor, Tick};

use crate::clock::Monotonic;
use crate::shared::SharedMoisture;

/// The interval between soil readings.
///
/// Soil moisture moves slowly, so a short period buys little accuracy and — once
/// probe power-gating lands (qhw.31) — costs an energize cycle each time. Two
/// seconds keeps the demo responsive (a probe wetting shows within a period)
/// while staying frugal; the composition root can lengthen it for production.
pub const SAMPLE_PERIOD: Duration = Duration::from_secs(2);

/// How many periods a reading may age before consumers treat it as unavailable.
///
/// Three periods tolerates a couple of missed or slow samples before declaring
/// the sensor stale — long enough not to flicker on one hiccup, short enough that
/// a genuinely dead probe surfaces quickly.
pub const STALENESS_PERIODS: u32 = 3;

/// The sampler thread's stack, in bytes.
///
/// On-device this sizes a FreeRTOS task stack, so it is set explicitly rather
/// than inherited: the loop reads the ADC and logs, well within 8 KiB, but SRAM
/// is scarce (520 KB, no PSRAM) so it is not made lavish. Validate the true
/// high-water mark on the metal (qhw.21 on-board acceptance) before trusting it.
pub const SAMPLER_STACK_SIZE: usize = 8 * 1024;

/// How the sampler is tuned: the calibration it folds through and its timing.
///
/// [`calibration`](Self::calibration) has no sensible default — it is the probe's
/// captured dry/wet endpoints (qhw.29) — so it must be supplied; the timing
/// fields default to the module constants. [`Copy`], so the composition root can
/// read [`max_age`](Self::max_age)/[`period`](Self::period) and still move it into
/// the thread.
#[derive(Clone, Copy)]
pub struct SamplerConfig {
    /// The dry/wet endpoints the pure step calibrates raw counts against.
    pub calibration: Calibration,
    /// The interval between readings.
    pub period: Duration,
    /// The staleness bound, in periods (see [`STALENESS_PERIODS`]).
    pub staleness_periods: u32,
    /// The sampler thread's stack size, in bytes.
    pub stack_size: usize,
}

impl SamplerConfig {
    /// A config for `calibration`, with the timing defaulted to the module
    /// constants.
    pub fn new(calibration: Calibration) -> Self {
        Self {
            calibration,
            period: SAMPLE_PERIOD,
            staleness_periods: STALENESS_PERIODS,
            stack_size: SAMPLER_STACK_SIZE,
        }
    }

    /// The maximum age (in [`Tick`] milliseconds) a reading may reach before
    /// consumers treat it as unavailable: `period * staleness_periods`.
    ///
    /// The reader passes this to [`SharedMoisture::latest`]; keeping the arithmetic
    /// here means the writer's period and the reader's bound can never drift apart.
    pub fn max_age(&self) -> Tick {
        let period_ms: Tick = self.period.as_millis().min(u128::from(Tick::MAX)) as Tick;
        period_ms.saturating_mul(Tick::from(self.staleness_periods))
    }
}

/// A running sampler thread — a handle to stop and join it.
///
/// Dropping the handle detaches the thread (it keeps sampling), which is what the
/// composition root wants: the monitor samples for the life of the program. Tests
/// and a future clean-shutdown path use [`stop`](Self::stop) + [`join`](Self::join).
pub struct Sampler {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl Sampler {
    /// Ask the sampler to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the sampler thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Spawn the sampler thread: `sensor` → [`step`] → `shared`, every `config.period`.
///
/// The thread is named and sized per `config`. `clock` is the shared time base —
/// the same [`Monotonic`] the consumers read — so a published reading's age is
/// measured on one clock. `sensor` moves into the thread, so it must be
/// [`Send`] + `'static`; its error must be [`Display`](std::fmt::Display) so a
/// failed read can be logged.
///
/// Returns the [`Sampler`] handle, or the [`io::Error`] from failing to spawn the
/// OS/RTOS thread.
pub fn spawn_sampler<S>(
    sensor: S,
    shared: SharedMoisture,
    clock: Monotonic,
    config: SamplerConfig,
) -> io::Result<Sampler>
where
    S: SoilSensor + Send + 'static,
    S::Error: std::fmt::Display,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("plant-sampler".to_string())
        .stack_size(config.stack_size)
        .spawn(move || sample_loop(sensor, shared, clock, config, stop_in_thread))?;
    Ok(Sampler { handle, stop })
}

/// The thread body: sample, publish, sleep — until asked to stop.
fn sample_loop<S>(
    mut sensor: S,
    shared: SharedMoisture,
    clock: Monotonic,
    config: SamplerConfig,
    stop: Arc<AtomicBool>,
) where
    S: SoilSensor,
    S::Error: std::fmt::Display,
{
    let mut prev: Option<Measurement> = None;
    while !stop.load(Ordering::Relaxed) {
        prev = sample_once(&mut sensor, &shared, config.calibration, clock.now(), prev);
        thread::sleep(config.period);
    }
}

/// One sampling cycle: read, fold through the pure [`step`], publish on success.
///
/// The whole shell↔core seam, in isolation and testable without a thread: on a
/// good read it calibrates through [`step`] and publishes the latest
/// [`Measurement`] (raw count and calibrated percent) at `now`; on a failed read
/// it publishes *nothing* — the last reading ages out — and carries `prev`
/// forward. Returns the measurement state to carry into the next cycle. Complexity
/// is one branch: read ok, or not.
fn sample_once<S>(
    sensor: &mut S,
    shared: &SharedMoisture,
    calibration: Calibration,
    now: Tick,
    prev: Option<Measurement>,
) -> Option<Measurement>
where
    S: SoilSensor,
    S::Error: std::fmt::Display,
{
    match sensor.read_raw() {
        Ok(raw) => {
            // One reading per cycle: the adapter already denoised it (electrical
            // oversampling), and the pure step calibrates + reports-on-change.
            let sample: Sample = step(prev, &[raw], calibration);
            if let Some(measurement) = sample.state {
                shared.publish(measurement, now);
            }
            sample.state
        }
        Err(err) => {
            warn!("plant-sampler: soil read failed, letting last reading age out: {err}");
            prev
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::mpsc::{channel, Sender};

    /// A calibration where raw `N` maps to `N %` (`N <= 100`), so a reading reads
    /// straight through: raw 30 → 30 %. Same convention as `plant-core`'s tests.
    const LINEAR: Calibration = Calibration::new(0, 100);

    /// The measurement the sensor yields at `raw` counts, through [`LINEAR`] (raw
    /// `N` → `N %`): `measured(30)` is raw 30 calibrated to 30 %.
    fn measured(raw: u16) -> Measurement {
        Measurement::measure(raw, LINEAR)
    }

    /// A `SoilSensor` fake: it yields a queue of readings, then a fixed `after`
    /// result forever (a healthy sensor is a `constant`; a `dead` one errors).
    /// Optionally pings a channel on every read, so a test can block until the
    /// thread has actually run rather than poll or sleep-and-hope.
    struct FakeSensor {
        queue: VecDeque<u16>,
        after: Result<u16, TestError>,
        ping: Option<Sender<()>>,
    }

    /// A read error whose message is [`Display`], as the sampler's log bound
    /// requires; [`Clone`] so the fake can hand it out on every exhausted read.
    #[derive(Clone, Debug)]
    struct TestError(&'static str);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl FakeSensor {
        /// Always returns `raw` — a healthy, steady probe.
        fn constant(raw: u16) -> Self {
            Self {
                queue: VecDeque::new(),
                after: Ok(raw),
                ping: None,
            }
        }

        /// Errors on every read — a probe that never came up.
        fn dead() -> Self {
            Self {
                queue: VecDeque::new(),
                after: Err(TestError("sensor offline")),
                ping: None,
            }
        }

        /// Yields `readings` once, then errors forever — a probe that dies.
        fn readings_then_dies(readings: &[u16]) -> Self {
            Self {
                queue: readings.iter().copied().collect(),
                after: Err(TestError("sensor died")),
                ping: None,
            }
        }

        /// Ping `tx` on every read, so a test can await a real sample.
        fn pinging(mut self, tx: Sender<()>) -> Self {
            self.ping = Some(tx);
            self
        }
    }

    impl SoilSensor for FakeSensor {
        type Error = TestError;

        fn read_raw(&mut self) -> Result<u16, TestError> {
            let result: Result<u16, TestError> = match self.queue.pop_front() {
                Some(raw) => Ok(raw),
                None => self.after.clone(),
            };
            if let Some(tx) = &self.ping {
                // Unbounded channel: the send never blocks the sampler thread.
                let _ = tx.send(());
            }
            result
        }
    }

    #[test]
    fn a_successful_sample_publishes_through_the_pure_step() {
        // The happy path: a raw read is calibrated by `step` and lands in the
        // cache. raw 30 through LINEAR is 30 %.
        let shared: SharedMoisture = SharedMoisture::new();
        let mut sensor: FakeSensor = FakeSensor::constant(30);

        let carried: Option<Measurement> = sample_once(&mut sensor, &shared, LINEAR, 5, None);

        assert_eq!(carried, Some(measured(30)), "state carries the new reading");
        assert_eq!(shared.latest(5, 1000), Some(measured(30)));
    }

    #[test]
    fn a_failed_read_publishes_nothing_and_keeps_state() {
        // An injected read error must not touch the cache: nothing is published,
        // and the prior state is carried forward untouched.
        let shared: SharedMoisture = SharedMoisture::new();
        let mut sensor: FakeSensor = FakeSensor::dead();

        let carried: Option<Measurement> =
            sample_once(&mut sensor, &shared, LINEAR, 5, Some(measured(40)));

        assert_eq!(
            carried,
            Some(measured(40)),
            "prior state survives a bad read"
        );
        assert_eq!(shared.latest(5, 1000), None, "a bad read publishes nothing");
    }

    #[test]
    fn a_live_but_steady_sensor_stays_fresh_by_resampling() {
        // A value that never changes must still keep its freshness alive: each
        // sample refreshes the timestamp even when the reading is identical, so a
        // healthy-but-steady probe never ages out.
        let shared: SharedMoisture = SharedMoisture::new();
        let mut sensor: FakeSensor = FakeSensor::constant(50);
        let max_age: Tick = 50;

        let prev: Option<Measurement> = sample_once(&mut sensor, &shared, LINEAR, 0, None);
        assert_eq!(shared.latest(0, max_age), Some(measured(50)));
        // Without a new sample, the reading ages out.
        assert_eq!(shared.latest(100, max_age), None);
        // A live sensor resamples, refreshing the stamp — fresh again at t=100.
        let _prev: Option<Measurement> = sample_once(&mut sensor, &shared, LINEAR, 100, prev);
        assert_eq!(shared.latest(100, max_age), Some(measured(50)));
    }

    #[test]
    fn a_dying_sensor_goes_unavailable_within_the_staleness_bound() {
        // The core safety property: once the sensor stops producing readings, the
        // cache flips to unavailable within STALENESS_PERIODS periods of the last
        // healthy reading — a dead sensor never keeps serving its last value.
        let shared: SharedMoisture = SharedMoisture::new();
        let mut sensor: FakeSensor = FakeSensor::readings_then_dies(&[60]);
        let config: SamplerConfig = SamplerConfig::new(LINEAR); // 2 s period, 3 periods
        let cal: Calibration = config.calibration;
        let max_age: Tick = config.max_age(); // 6000 ms

        // t=0: the one healthy reading is published.
        let mut prev: Option<Measurement> = sample_once(&mut sensor, &shared, cal, 0, None);
        assert_eq!(shared.latest(0, max_age), Some(measured(60)));

        // The sensor is dead now; the next cycles publish nothing.
        prev = sample_once(&mut sensor, &shared, cal, 2000, prev);
        prev = sample_once(&mut sensor, &shared, cal, 4000, prev);
        // At exactly the bound (age 6000 == max_age) it is still just available.
        assert_eq!(shared.latest(6000, max_age), Some(measured(60)));
        let _prev: Option<Measurement> = sample_once(&mut sensor, &shared, cal, 6000, prev);
        // One tick past the bound: unavailable — flipped within 3 periods.
        assert_eq!(shared.latest(6001, max_age), None);
    }

    #[test]
    fn the_spawned_thread_samples_publishes_and_stops_cleanly() {
        // The one integration test: prove the real thread wiring — spawn, sample
        // through the loop, publish into the shared cache, stop and join without a
        // panic. Blocking on two pings (rather than polling) makes it robust: the
        // second read starting proves the first publish completed.
        let clock: Monotonic = Monotonic::start();
        let shared: SharedMoisture = SharedMoisture::new();
        let (tx, rx): (Sender<()>, _) = channel();
        let sensor: FakeSensor = FakeSensor::constant(45).pinging(tx);
        let config: SamplerConfig = SamplerConfig {
            calibration: LINEAR,
            period: Duration::from_millis(1),
            staleness_periods: STALENESS_PERIODS,
            stack_size: 256 * 1024,
        };

        let sampler: Sampler =
            spawn_sampler(sensor, shared.clone(), clock, config).expect("spawn sampler thread");

        rx.recv_timeout(Duration::from_secs(2))
            .expect("the sampler must take a first reading");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a second reading proves the first publish landed");

        // A generous max_age so scheduler jitter can't masquerade as staleness.
        assert_eq!(shared.latest(clock.now(), 60_000), Some(measured(45)));

        sampler.stop();
        sampler.join().expect("the sampler thread must not panic");
    }
}
