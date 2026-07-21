//! The sampler thread — read the IMU, smooth it, publish the pose it implies.
//!
//! The orientation app's one writer. It runs fast on purpose: the readout's whole job is to
//! move when the board moves, so the loop polls at [`SAMPLE_PERIOD`] and does the least
//! possible work per poll — one I2C burst read, three integer averages, one arctangent pair,
//! and a copy under a briefly-held lock. Everything it decides is pure and lives inward in
//! `orientation-core`; this loop owns only the cadence, the thread, and the cache.

use std::fmt::Display;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use orientation_core::{Orientation, Smoother};
use platform_core::{Acceleration, Clock, Imu, Tick};

use crate::shared::SharedOrientation;

/// How often the IMU is polled — 100 Hz, which is **one FreeRTOS tick**.
///
/// The floor is a scheduling fact, not a taste: ESP-IDF here runs at
/// `CONFIG_FREERTOS_HZ = 100`, so a tick is 10 ms, and a sleep shorter than a tick cannot
/// yield — it falls through to a busy wait that burns the core while looking like a pause.
/// An earlier 5 ms period did exactly that and starved the idle task until the task watchdog
/// fired; it was found by flashing the board, because a host `cargo test` sleeps on Linux,
/// where any duration yields. So this is as fast as the IMU can honestly be polled.
///
/// It is comfortably fast enough. The smoother closes a turn in about seven samples, which
/// puts a full 90° rotation on the glass roughly 70 ms after the hand made it — inside the
/// window where a movement and its readout still feel like one event. The MPU6886 is
/// configured at 1 kHz, so every poll returns a genuinely fresh sample rather than re-reading
/// one, and a 6-byte burst read at 400 kHz costs well under a millisecond, so the loop spends
/// over 98% of its time asleep.
pub const SAMPLE_PERIOD: Duration = Duration::from_millis(10);

/// The sampler thread's stack, in bytes (it becomes a FreeRTOS task stack on device).
///
/// Sized like the other hardware-touching platform threads: each cycle runs an I2C
/// transaction and, on a failure, formats an error through `warn!` — `core::fmt`'s call tree
/// is deep and stack-hungry, and under FreeRTOS preemption an interrupt frame can land on top
/// of it. 8 KiB leaves margin for that worst case where 4 KiB would not.
pub const SAMPLER_STACK_SIZE: usize = 8 * 1024;

/// How the sampler is tuned: its poll cadence, its smoothing, and its stack.
#[derive(Clone, Copy)]
pub struct SamplerConfig {
    /// The interval between IMU polls.
    pub period: Duration,
    /// The smoother each reading is folded through before it becomes a pose.
    pub smoother: Smoother,
    /// The sampler thread's stack size, in bytes.
    pub stack_size: usize,
}

impl Default for SamplerConfig {
    fn default() -> Self {
        SamplerConfig {
            period: SAMPLE_PERIOD,
            smoother: Smoother::default(),
            stack_size: SAMPLER_STACK_SIZE,
        }
    }
}

/// A running sampler thread — a handle to stop and join it.
pub struct SamplerTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl SamplerTask {
    /// Ask the sampler to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the sampler thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// One sampler cycle: read the IMU, smooth it, publish the pose stamped at `now`.
///
/// A failed read is logged and skipped, never fatal, matching `spawn_display`'s fail-visible
/// renders and the power-watch's fail-visible polls: a flaky I2C transaction must not take the
/// readout down, and the next poll is ten milliseconds away. The previously published pose
/// stands until then, which is the right call for a transient glitch.
///
/// What a skip does *not* do is refresh the stamp. That is the whole mechanism: a glitch or
/// two costs a couple of missed refreshes and nothing more, while a sensor that stops
/// answering lets the published pose age past [`SIGNAL_TIMEOUT_MS`] until the reader marks it
/// lost. The sampler needs no dead-sensor logic of its own — it simply stops vouching.
///
/// Returns whether this cycle published, so a test can tell "read and published" from
/// "skipped a bad read" without inspecting the cache.
///
/// [`SIGNAL_TIMEOUT_MS`]: orientation_core::SIGNAL_TIMEOUT_MS
fn sample_once<I>(
    imu: &mut I,
    smoother: &mut Smoother,
    shared: &SharedOrientation,
    now: Tick,
) -> bool
where
    I: Imu,
    I::Error: Display,
{
    let raw: Acceleration = match imu.acceleration() {
        Ok(raw) => raw,
        Err(err) => {
            warn!("orientation-sampler: IMU read failed, skipping this cycle: {err}");
            return false;
        }
    };

    shared.publish(Orientation::of(smoother.update(raw)), now);
    true
}

/// Spawn the sampler thread: poll `imu` every `config.period`, smooth each reading, and
/// publish the pose it implies into `shared`, stamped from `clock`.
///
/// `imu` and `clock` move into the thread, so both must be [`Send`] + `'static`, and the IMU's
/// error must be [`Display`] so a failure can be logged. `clock` is the same monotonic clock
/// the render loop reads, which is what makes the two ends of the staleness measurement
/// comparable at all. Returns the [`SamplerTask`] handle, or the [`io::Error`] from failing to
/// spawn the OS/RTOS thread.
pub fn spawn_sampler<I, C>(
    imu: I,
    shared: SharedOrientation,
    clock: C,
    config: SamplerConfig,
) -> io::Result<SamplerTask>
where
    I: Imu + Send + 'static,
    I::Error: Display,
    C: Clock + Send + 'static,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("orientation-sampler".to_string())
        .stack_size(config.stack_size)
        .spawn(move || sample_loop(imu, shared, clock, config, stop_in_thread))?;
    Ok(SamplerTask { handle, stop })
}

/// The thread body: read → smooth → publish, every `config.period`, until stopped.
fn sample_loop<I, C>(
    mut imu: I,
    shared: SharedOrientation,
    clock: C,
    config: SamplerConfig,
    stop: Arc<AtomicBool>,
) where
    I: Imu,
    I::Error: Display,
    C: Clock,
{
    let mut smoother: Smoother = config.smoother;
    while !stop.load(Ordering::Relaxed) {
        // Read the clock before the IMU, so the stamp dates the reading rather than the
        // transaction that fetched it — a slow bus must not make a sample look fresher.
        let now: Tick = clock.now();
        sample_once(&mut imu, &mut smoother, &shared, now);
        thread::sleep(config.period);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::convert::Infallible;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;

    use orientation_core::{Facing, Reading, Signal, SIGNAL_TIMEOUT_MS};
    use platform_core::ONE_G_MG;

    /// A clock a test advances by hand, so a staleness rule is proven by choosing ticks
    /// rather than by sleeping.
    #[derive(Clone)]
    struct FakeClock {
        now: Arc<Mutex<Tick>>,
    }

    impl FakeClock {
        fn new() -> Self {
            FakeClock {
                now: Arc::new(Mutex::new(0)),
            }
        }

        fn advance(&self, by_ms: Tick) {
            *self.now.lock().expect("clock lock") += by_ms;
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Tick {
            *self.now.lock().expect("clock lock")
        }
    }

    /// An [`Imu`] a single-threaded test drives by hand through a shared cell.
    struct FakeImu<'a> {
        reading: &'a Cell<Acceleration>,
    }

    impl Imu for FakeImu<'_> {
        type Error = Infallible;
        fn acceleration(&mut self) -> Result<Acceleration, Infallible> {
            Ok(self.reading.get())
        }
    }

    /// An [`Imu`] whose every read fails — a dead or unplugged sensor.
    struct DeadImu;

    /// A read failure with a message, so the sampler's `Display` bound is exercised for real.
    #[derive(Debug)]
    struct BusFault;

    impl Display for BusFault {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the I2C bus did not answer")
        }
    }

    impl Imu for DeadImu {
        type Error = BusFault;
        fn acceleration(&mut self) -> Result<Acceleration, BusFault> {
            Err(BusFault)
        }
    }

    /// An [`Imu`] a spawned-thread test drives from outside the thread, pinging on each read
    /// so the test can await a cycle instead of sleep-polling.
    struct ThreadImu {
        reading: Arc<Mutex<Acceleration>>,
        ping: Sender<()>,
    }

    impl Imu for ThreadImu {
        type Error = Infallible;
        fn acceleration(&mut self) -> Result<Acceleration, Infallible> {
            let reading: Acceleration = *self.reading.lock().expect("reading lock");
            let _ = self.ping.send(());
            Ok(reading)
        }
    }

    /// Block until the sampler has started `count` cycles, or fail the test. Awaiting real
    /// pings rather than sleeping keeps this test deterministic on a loaded machine.
    fn await_cycles(rx: &std::sync::mpsc::Receiver<()>, count: usize) {
        (0..count).for_each(|_| {
            rx.recv_timeout(Duration::from_secs(2))
                .expect("the sampler must keep polling the IMU");
        });
    }

    /// Run `cycles` sampler cycles against a fixed reading, and return the published pose.
    fn sampled(reading: Acceleration, cycles: usize) -> Orientation {
        let cell: Cell<Acceleration> = Cell::new(reading);
        let mut imu: FakeImu = FakeImu { reading: &cell };
        let mut smoother: Smoother = Smoother::default();
        let shared: SharedOrientation = SharedOrientation::new();
        (0..cycles).for_each(|_| {
            sample_once(&mut imu, &mut smoother, &shared, 0);
        });
        shared.last_known()
    }

    /// Zero: before any cycle runs, the cache names no pose.
    #[test]
    fn nothing_is_published_before_the_first_cycle() {
        assert_eq!(
            sampled(Acceleration::new(0, 0, ONE_G_MG), 0).facing,
            Facing::Moving
        );
    }

    /// One: a single cycle publishes the pose the reading implies — no warm-up, because the
    /// smoother adopts its first sample whole.
    #[test]
    fn one_cycle_publishes_the_pose_at_once() {
        let published: Orientation = sampled(Acceleration::new(0, 0, ONE_G_MG), 1);
        assert_eq!(published.facing, Facing::ScreenUp);
        assert_eq!(published.acceleration.z_mg, ONE_G_MG);
    }

    /// Many: repeated cycles on a steady board converge on exactly that reading and stay.
    #[test]
    fn many_cycles_on_a_steady_board_converge_on_its_reading() {
        let steady: Acceleration = Acceleration::new(-342, 0, 940);
        let published: Orientation = sampled(steady, 40);
        assert_eq!(published.acceleration, steady);
        assert_eq!(published.facing, Facing::ScreenUp);
    }

    /// A turn mid-stream is followed: the cache tracks the board rather than latching onto
    /// whatever it saw first.
    #[test]
    fn the_cache_follows_the_board_when_it_turns() {
        let cell: Cell<Acceleration> = Cell::new(Acceleration::new(0, 0, ONE_G_MG));
        let mut imu: FakeImu = FakeImu { reading: &cell };
        let mut smoother: Smoother = Smoother::default();
        let shared: SharedOrientation = SharedOrientation::new();

        sample_once(&mut imu, &mut smoother, &shared, 0);
        assert_eq!(shared.last_known().facing, Facing::ScreenUp);

        cell.set(Acceleration::new(ONE_G_MG, 0, 0));
        (0..20).for_each(|_| {
            sample_once(&mut imu, &mut smoother, &shared, 0);
        });
        assert_eq!(shared.last_known().facing, Facing::Upright);
    }

    /// A failed read is survivable and reports itself as a skip, leaving the last good pose
    /// standing rather than blanking the readout on one flaky transaction.
    #[test]
    fn a_failed_read_is_skipped_and_leaves_the_last_pose_standing() {
        let shared: SharedOrientation = SharedOrientation::new();
        let mut smoother: Smoother = Smoother::default();

        let cell: Cell<Acceleration> = Cell::new(Acceleration::new(0, 0, ONE_G_MG));
        let mut good: FakeImu = FakeImu { reading: &cell };
        assert!(sample_once(&mut good, &mut smoother, &shared, 0));

        let mut dead: DeadImu = DeadImu;
        assert!(
            !sample_once(&mut dead, &mut smoother, &shared, 10),
            "a failed read must report that it did not publish"
        );
        assert_eq!(
            shared.reading(10).orientation.facing,
            Facing::ScreenUp,
            "a single failed read must not blank the readout"
        );
        assert_eq!(
            shared.reading(10).signal,
            Signal::Live,
            "one glitch must not be reported as a lost sensor"
        );
    }

    /// A sensor that keeps failing lets the last pose age out: the skip that survives a
    /// glitch is the same mechanism that surfaces a death, because a skip never restamps.
    #[test]
    fn a_persistently_failing_sensor_lets_the_readout_go_stale() {
        let shared: SharedOrientation = SharedOrientation::new();
        let mut smoother: Smoother = Smoother::default();

        let cell: Cell<Acceleration> = Cell::new(Acceleration::new(0, 0, ONE_G_MG));
        let mut good: FakeImu = FakeImu { reading: &cell };
        sample_once(&mut good, &mut smoother, &shared, 0);

        let mut dead: DeadImu = DeadImu;
        (0..50).for_each(|cycle: usize| {
            sample_once(&mut dead, &mut smoother, &shared, cycle as Tick * 10);
        });

        let stale: Reading = shared.reading(SIGNAL_TIMEOUT_MS);
        assert_eq!(stale.signal, Signal::Lost);
        assert_eq!(
            stale.orientation.facing,
            Facing::ScreenUp,
            "the last known pose must survive, it just stops being vouched for"
        );
    }

    /// A wholly dead sensor never publishes anything, so the cache keeps saying it knows
    /// nothing rather than inventing a pose.
    #[test]
    fn a_dead_sensor_never_publishes_a_pose() {
        let mut dead: DeadImu = DeadImu;
        let mut smoother: Smoother = Smoother::default();
        let shared: SharedOrientation = SharedOrientation::new();
        (0..10).for_each(|_| {
            sample_once(&mut dead, &mut smoother, &shared, 0);
        });
        assert_eq!(shared.last_known(), Orientation::default());
    }

    /// The one integration test: spawn the real thread, watch it track a turn through the
    /// real cache, then stop and join cleanly. The plumbing, proven end to end.
    #[test]
    fn the_spawned_sampler_tracks_the_board_and_stops_cleanly() {
        let reading: Arc<Mutex<Acceleration>> =
            Arc::new(Mutex::new(Acceleration::new(0, 0, ONE_G_MG)));
        let (tx, rx): (Sender<()>, _) = channel();
        let imu: ThreadImu = ThreadImu {
            reading: Arc::clone(&reading),
            ping: tx,
        };
        let shared: SharedOrientation = SharedOrientation::new();
        let clock: FakeClock = FakeClock::new();
        let config: SamplerConfig = SamplerConfig {
            period: Duration::from_millis(1),
            stack_size: 64 * 1024,
            ..SamplerConfig::default()
        };

        let task: SamplerTask = spawn_sampler(imu, shared.clone(), clock.clone(), config)
            .expect("spawn the sampler thread");

        // The ping fires *inside* the read, before the publish that follows it, so seeing one
        // ping only proves a cycle started. Awaiting the next one proves the previous cycle
        // ran to completion — which is what makes this assertion about the cache sound
        // rather than a race that happens to pass.
        await_cycles(&rx, 2);
        assert_eq!(shared.last_known().facing, Facing::ScreenUp);

        // Turn the board, then let the smoother settle over plenty of real cycles.
        *reading.lock().expect("reading lock") = Acceleration::new(ONE_G_MG, 0, 0);
        await_cycles(&rx, 40);
        assert_eq!(shared.last_known().facing, Facing::Upright);

        // The running thread stamps what it publishes, so a reader on the same clock sees a
        // live signal — the plumbing from clock through sampler to reader, proven end to end.
        assert_eq!(shared.reading(clock.now()).signal, Signal::Live);

        // Stop the sampler and let the clock run past the timeout: a readout nothing is
        // feeding any more reports itself lost, without a single sleep in this test.
        task.stop();
        task.join().expect("the sampler thread must not panic");
        clock.advance(SIGNAL_TIMEOUT_MS);
        assert_eq!(shared.reading(clock.now()).signal, Signal::Lost);
    }
}
