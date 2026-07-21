//! The rotation source — which way up the picture is drawn, shared between whoever reads the
//! IMU and the render loop that has to obey it.
//!
//! Two halves, because two apps need the same answer by different routes. [`SharedRotation`]
//! is the published rotation itself: it owns the [`RotationSettler`], takes accelerations in
//! and hands [`ScreenRotation`]s out. [`spawn_rotation`] is a thread that owns an [`Imu`] and
//! feeds it, for an app that has no other reason to read the sensor.
//!
//! The split is not decoration. An app that *already* runs an IMU thread — the orientation
//! readout does, at 100 Hz, for its pose — cannot spawn a second owner of a single I2C device,
//! because the sensor moves into whichever thread took it. Such an app holds a
//! [`SharedRotation`] and calls [`update`] from whatever already sees the readings, so the
//! settling rule stays in exactly one place either way. The alternative was a second settler
//! living in the app, which is the duplication this module exists to prevent.
//!
//! *Which* code in such an app does the calling is that app's question and is not settled here
//! — the orientation readout's sampler is `context = "orientation"` and cannot be reached from
//! this crate. All this module guarantees is that the rule and the published value stay
//! together whoever feeds them.
//!
//! Whichever route feeds it, [`SharedRotation::source`] hands back the
//! `FnMut(Tick) -> ScreenRotation` that [`spawn_display`](crate::spawn_display) wants, so
//! wiring a binary is one line.
//!
//! [`update`]: SharedRotation::update

use std::fmt::Display;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use platform_core::{Acceleration, Clock, Imu, RotationSettler, ScreenRotation, Tick};

/// How often the IMU is polled for the rotation.
///
/// Deliberately slower than the orientation readout's 100 Hz, because this asks a much lazier
/// question. A rotation only becomes real after
/// [`ROTATION_SETTLE_MS`](platform_core::ROTATION_SETTLE_MS) — a quarter second — so 50 ms
/// still puts five samples inside the settle window, which is more than enough to establish
/// that a quadrant is being held rather than passed through. Sampling faster would buy no
/// responsiveness at all: the settle time, not the poll rate, is what the eye sees.
///
/// What it *does* buy is bus quiet. Every one of these polls is an I2C transaction competing
/// with whatever else the app is doing — on the host monitor, with WiFi and an HTTP poller
/// alongside — so the cheapest cadence that still answers correctly is the right one. This
/// matches the power watch, the board's other lazy poller.
///
/// The floor here is 10 ms whatever the reasoning: `CONFIG_FREERTOS_HZ = 100` makes a tick
/// 10 ms, and a shorter sleep cannot yield — it falls through to a busy wait that burns the
/// core while looking in the source like a pause. A host `cargo test` cannot see that, because
/// `std::thread::sleep` on Linux yields at any duration.
pub const ROTATION_PERIOD: Duration = Duration::from_millis(50);

/// The rotation thread's stack, in bytes (it becomes a FreeRTOS task stack on device).
///
/// Sized like every other hardware-touching thread here: each cycle runs an I2C transaction
/// and, on a failure, formats an error through `warn!` — `core::fmt`'s call tree is deep and
/// stack-hungry, and under FreeRTOS preemption an interrupt frame can land on top of it.
/// 8 KiB leaves margin for that worst case where 4 KiB would not.
pub const ROTATION_STACK_SIZE: usize = 8 * 1024;

/// The rotation the glass should be drawn at, shared between whoever reads the IMU and the
/// render loop.
///
/// Holds the [`RotationSettler`] rather than a bare [`ScreenRotation`], so the settle-then-turn
/// rule sits behind the same lock as the value it governs. A caller cannot fold a reading in
/// without the rule applying, and two feeders cannot disagree about how long a candidate has
/// been held — there is one settler, and it is here.
///
/// Poison-tolerant, like `orientation-shell`'s `SharedOrientation`: a panic in any holder
/// recovers the inner value rather than propagating, so one wedged thread cannot take the
/// render loop down with it. The lock is held for exactly one settler update or one copy out —
/// a handful of words — so the render loop never waits on the feeder even at the feeder's full
/// rate.
///
/// Unlike that one, nothing here is stamped. A pose goes stale because a pose nobody is
/// confirming is a claim about the world that may have stopped being true; a rotation does not,
/// because the last legible way up is still the last legible way up. A dead IMU should leave
/// the picture where it was, not blank it.
#[derive(Clone)]
pub struct SharedRotation {
    inner: Arc<Mutex<RotationSettler>>,
}

impl SharedRotation {
    /// A rotation showing the panel's native landscape, with nothing pending.
    ///
    /// The same starting point [`RotationSettler::new`] takes, and the same one every app drew
    /// at before rotation existed — so a binary that holds one of these but never feeds it
    /// behaves exactly as it did before.
    pub fn new() -> Self {
        SharedRotation {
            inner: Arc::new(Mutex::new(RotationSettler::new())),
        }
    }

    /// Fold a reading taken at `now` into the settler, and report the rotation to draw at.
    ///
    /// The write side. Called either by [`spawn_rotation`]'s thread or, for an app that
    /// already owns the IMU, straight from its own sampler cycle.
    pub fn update(&self, acceleration: Acceleration, now: Tick) -> ScreenRotation {
        self.locked().update(acceleration, now)
    }

    /// The rotation the glass should be drawn at right now.
    ///
    /// The read side, and total: before anything has been published this is the native
    /// landscape rather than an absence, because there is always *some* way up to draw at.
    /// That is the difference between this and a pose — a pose can be unknown, a rotation
    /// cannot.
    pub fn current(&self) -> ScreenRotation {
        self.locked().showing()
    }

    /// A closure handing back the current rotation, for [`spawn_display`](crate::spawn_display).
    ///
    /// This is the whole wiring surface: a composition root replaces its
    /// `|_now: Tick| ScreenRotation::Deg0` placeholder with `rotation.source()` and the picture
    /// starts turning. The [`Tick`] is ignored — the settler is driven on the feeder's clock,
    /// at the moment each reading was actually taken, which is a better stamp than the moment
    /// the render loop happened to ask.
    pub fn source(&self) -> impl FnMut(Tick) -> ScreenRotation + Send + 'static {
        let shared: SharedRotation = self.clone();
        move |_now: Tick| shared.current()
    }

    /// The settler, recovering rather than propagating a poisoned lock.
    fn locked(&self) -> std::sync::MutexGuard<'_, RotationSettler> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for SharedRotation {
    fn default() -> Self {
        Self::new()
    }
}

/// How the rotation thread is tuned: its poll cadence and its stack.
#[derive(Clone, Copy)]
pub struct RotationConfig {
    /// The interval between IMU polls.
    pub period: Duration,
    /// The rotation thread's stack size, in bytes.
    pub stack_size: usize,
}

impl Default for RotationConfig {
    fn default() -> Self {
        RotationConfig {
            period: ROTATION_PERIOD,
            stack_size: ROTATION_STACK_SIZE,
        }
    }
}

/// A running rotation thread — a handle to stop and join it.
pub struct RotationTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl RotationTask {
    /// Ask the rotation loop to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the rotation thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// One rotation cycle: read `imu` and fold the reading into `shared`, stamped at `now`.
///
/// A failed read is logged and skipped, never fatal — the same fail-visible discipline as
/// `spawn_display`'s renders and the power watch's polls. A flaky I2C transaction must not take
/// the picture down, and the next poll is 50 ms away.
///
/// What a skip does is nothing at all, and that is the correct nothing: the settler keeps both
/// the rotation it is showing and the age of any candidate, so a glitch mid-turn neither turns
/// the picture early nor restarts the clock on a quadrant the board is genuinely holding. A
/// sensor that dies entirely leaves the last settled rotation on the glass, which is the right
/// answer — a picture that was legible a moment ago is still legible now.
///
/// Returns whether this cycle read successfully, so a test can tell "read and folded" from
/// "skipped a bad read" without inspecting the settler.
fn rotate_once<I>(imu: &mut I, shared: &SharedRotation, now: Tick) -> bool
where
    I: Imu,
    I::Error: Display,
{
    let raw: Acceleration = match imu.acceleration() {
        Ok(raw) => raw,
        Err(err) => {
            warn!("platform-rotation: IMU read failed, skipping this cycle: {err}");
            return false;
        }
    };

    shared.update(raw, now);
    true
}

/// Spawn the rotation thread: poll `imu` every `config.period` and fold each reading into
/// `shared`, stamped from `clock`.
///
/// For an app whose only reason to touch the IMU is which way up to draw. An app that already
/// runs a sampler should **not** call this — a single I2C device cannot have two owners — and
/// should instead call [`SharedRotation::update`] from the cycle it already runs.
///
/// `imu` and `clock` move into the thread, so both must be [`Send`] + `'static`, and the IMU's
/// error must be [`Display`] so a failure can be logged. `clock` should be the same monotonic
/// clock the render loop reads, so the settle window is measured on one time base. Returns the
/// [`RotationTask`] handle, or the [`io::Error`] from failing to spawn the OS/RTOS thread.
pub fn spawn_rotation<I, C>(
    imu: I,
    shared: SharedRotation,
    clock: C,
    config: RotationConfig,
) -> io::Result<RotationTask>
where
    I: Imu + Send + 'static,
    I::Error: Display,
    C: Clock + Send + 'static,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("platform-rotation".to_string())
        .stack_size(config.stack_size)
        .spawn(move || rotation_loop(imu, shared, clock, config, stop_in_thread))?;
    Ok(RotationTask { handle, stop })
}

/// The thread body: read → fold, every `config.period`, until stopped.
fn rotation_loop<I, C>(
    mut imu: I,
    shared: SharedRotation,
    clock: C,
    config: RotationConfig,
    stop: Arc<AtomicBool>,
) where
    I: Imu,
    I::Error: Display,
    C: Clock,
{
    while !stop.load(Ordering::Relaxed) {
        // Read the clock before the IMU, so the stamp dates the reading rather than the
        // transaction that fetched it — a slow bus must not make the settle window look
        // shorter than it was.
        let now: Tick = clock.now();
        rotate_once(&mut imu, &shared, now);
        thread::sleep(config.period);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::convert::Infallible;
    use std::sync::mpsc::{channel, Sender};

    use platform_core::{ONE_G_MG, ROTATION_SETTLE_MS};

    /// A board stood on its USB-C port: the stick's top at the sky.
    const UPRIGHT: Acceleration = Acceleration::new(ONE_G_MG, 0, 0);
    /// A board lying flat on its back — no in-plane direction to read.
    const FLAT: Acceleration = Acceleration::new(0, 0, ONE_G_MG);

    /// A clock a test advances by hand, so the settle rule is proven by choosing ticks rather
    /// than by sleeping.
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

        fn set(&self, to_ms: Tick) {
            *self.now.lock().expect("clock lock") = to_ms;
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

    /// A read failure with a message, so the loop's `Display` bound is exercised for real.
    #[derive(Debug)]
    struct BusFault;

    impl Display for BusFault {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the I2C bus did not answer")
        }
    }

    /// An [`Imu`] whose every read fails — a dead or unplugged sensor.
    struct DeadImu;

    impl Imu for DeadImu {
        type Error = BusFault;
        fn acceleration(&mut self) -> Result<Acceleration, BusFault> {
            Err(BusFault)
        }
    }

    /// An [`Imu`] a spawned-thread test drives from outside the thread, pinging on each read so
    /// the test can await a cycle instead of sleep-polling.
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

    /// Block until the loop has started `count` cycles, or fail the test. Awaiting real pings
    /// rather than sleeping keeps this deterministic on a loaded machine.
    fn await_cycles(rx: &std::sync::mpsc::Receiver<()>, count: usize) {
        (0..count).for_each(|_| {
            rx.recv_timeout(Duration::from_secs(2))
                .expect("the rotation loop must keep polling the IMU");
        });
    }

    /// Zero: a rotation nothing has fed shows the native landscape, so an app that holds one
    /// and never feeds it draws exactly as it did before rotation existed.
    #[test]
    fn an_unfed_rotation_shows_the_native_landscape() {
        assert_eq!(SharedRotation::new().current(), ScreenRotation::Deg0);
    }

    /// One: a single reading does not turn the picture — the settle rule applies through the
    /// shared cell exactly as it does to a bare settler.
    #[test]
    fn one_reading_does_not_turn_the_picture() {
        let shared: SharedRotation = SharedRotation::new();
        assert_eq!(shared.update(UPRIGHT, 0), ScreenRotation::Deg0);
        assert_eq!(shared.current(), ScreenRotation::Deg0);
    }

    /// Many: a reading held past the settle time turns the picture, and the reader sees it.
    #[test]
    fn a_reading_held_long_enough_turns_the_picture_for_the_reader() {
        let shared: SharedRotation = SharedRotation::new();
        shared.update(UPRIGHT, 0);
        shared.update(UPRIGHT, ROTATION_SETTLE_MS);
        assert_eq!(shared.current(), ScreenRotation::Deg270);
    }

    /// The writer and the reader hold clones of one rotation — the whole point of the type.
    #[test]
    fn a_clone_sees_the_same_rotation() {
        let writer: SharedRotation = SharedRotation::new();
        let reader: SharedRotation = writer.clone();
        writer.update(UPRIGHT, 0);
        writer.update(UPRIGHT, ROTATION_SETTLE_MS);
        assert_eq!(reader.current(), ScreenRotation::Deg270);
    }

    /// The source closure is what the render loop actually calls, and it tracks the feeder.
    /// This is the seam the whole capability is wired through, so it is proven as a closure
    /// rather than by reading `current` and hoping.
    #[test]
    fn the_source_closure_tracks_the_feeder() {
        let shared: SharedRotation = SharedRotation::new();
        // No annotation possible: `source` returns an unnameable `impl FnMut` closure type,
        // which is the point — the render loop takes it by bound, not by name.
        let mut source = shared.source();

        assert_eq!(source(0), ScreenRotation::Deg0);
        shared.update(UPRIGHT, 0);
        shared.update(UPRIGHT, ROTATION_SETTLE_MS);
        assert_eq!(source(ROTATION_SETTLE_MS), ScreenRotation::Deg270);
    }

    /// Two feeders share one settler rather than each keeping their own idea of how long a
    /// candidate has been held. This is why the settler lives behind the lock: split it and a
    /// board fed from two places would turn early, or never.
    #[test]
    fn two_feeders_share_one_settle_clock() {
        let first: SharedRotation = SharedRotation::new();
        let second: SharedRotation = first.clone();

        first.update(UPRIGHT, 0);
        // The second feeder's reading completes the settle the first one started.
        second.update(UPRIGHT, ROTATION_SETTLE_MS);
        assert_eq!(first.current(), ScreenRotation::Deg270);
    }

    /// A failed read is survivable and reports itself as a skip, leaving the picture where it
    /// was rather than snapping it back on one flaky transaction.
    #[test]
    fn a_failed_read_is_skipped_and_leaves_the_picture_where_it_was() {
        let shared: SharedRotation = SharedRotation::new();
        let cell: Cell<Acceleration> = Cell::new(UPRIGHT);
        let mut good: FakeImu = FakeImu { reading: &cell };

        assert!(rotate_once(&mut good, &shared, 0));
        assert!(rotate_once(&mut good, &shared, ROTATION_SETTLE_MS));
        assert_eq!(shared.current(), ScreenRotation::Deg270);

        let mut dead: DeadImu = DeadImu;
        assert!(
            !rotate_once(&mut dead, &shared, ROTATION_SETTLE_MS + 50),
            "a failed read must report that it did not fold"
        );
        assert_eq!(
            shared.current(),
            ScreenRotation::Deg270,
            "one glitch must not un-turn the picture"
        );
    }

    /// A sensor that dies entirely leaves the last settled rotation standing. A picture that
    /// was legible a moment ago is still legible now, so there is nothing to fall back to.
    #[test]
    fn a_dead_sensor_leaves_the_last_rotation_on_the_glass() {
        let shared: SharedRotation = SharedRotation::new();
        let cell: Cell<Acceleration> = Cell::new(UPRIGHT);
        let mut good: FakeImu = FakeImu { reading: &cell };
        rotate_once(&mut good, &shared, 0);
        rotate_once(&mut good, &shared, ROTATION_SETTLE_MS);

        let mut dead: DeadImu = DeadImu;
        (0..100).for_each(|cycle: usize| {
            rotate_once(&mut dead, &shared, ROTATION_SETTLE_MS + cycle as Tick * 50);
        });

        assert_eq!(shared.current(), ScreenRotation::Deg270);
    }

    /// A board that is never picked up never turns, however long the loop runs. The lazy
    /// cadence must not make a flat board drift into a rotation.
    #[test]
    fn a_flat_board_never_turns_however_long_it_is_polled() {
        let shared: SharedRotation = SharedRotation::new();
        let cell: Cell<Acceleration> = Cell::new(FLAT);
        let mut imu: FakeImu = FakeImu { reading: &cell };

        (0..200).for_each(|cycle: usize| {
            rotate_once(&mut imu, &shared, cycle as Tick * 50);
        });

        assert_eq!(shared.current(), ScreenRotation::Deg0);
    }

    /// The poll cadence fits the settle window with room to spare — the claim
    /// [`ROTATION_PERIOD`]'s doc makes, checked rather than asserted in prose. If either
    /// constant moves, this is what says whether the pair still works.
    #[test]
    fn the_poll_cadence_fits_several_samples_into_the_settle_window() {
        let period_ms: Tick = ROTATION_PERIOD.as_millis() as Tick;
        assert!(
            period_ms >= 10,
            "a period under one FreeRTOS tick busy-waits instead of yielding"
        );
        assert!(
            ROTATION_SETTLE_MS / period_ms >= 4,
            "the settle window must hold several polls, or a turn is decided on one reading"
        );
    }

    /// A panic while the lock was held must not wedge the picture: the next reader recovers the
    /// value rather than propagating the poison.
    #[test]
    fn a_poisoned_lock_still_reads() {
        let shared: SharedRotation = SharedRotation::new();
        shared.update(UPRIGHT, 0);
        shared.update(UPRIGHT, ROTATION_SETTLE_MS);

        let poisoner: SharedRotation = shared.clone();
        let panicked = std::thread::spawn(move || {
            let _held = poisoner.inner.lock().expect("lock");
            panic!("poison the lock while holding it");
        })
        .join();
        assert!(panicked.is_err(), "the helper thread must have panicked");

        assert_eq!(
            shared.current(),
            ScreenRotation::Deg270,
            "a poisoned lock wedged the picture"
        );
    }

    /// The one integration test: spawn the real thread, watch it turn the picture through the
    /// real shared cell on a clock the test controls, then stop and join cleanly. The plumbing,
    /// proven end to end.
    #[test]
    fn the_spawned_thread_turns_the_picture_and_stops_cleanly() {
        let reading: Arc<Mutex<Acceleration>> = Arc::new(Mutex::new(FLAT));
        let (tx, rx): (Sender<()>, _) = channel();
        let imu: ThreadImu = ThreadImu {
            reading: Arc::clone(&reading),
            ping: tx,
        };
        let shared: SharedRotation = SharedRotation::new();
        let clock: FakeClock = FakeClock::new();
        let config: RotationConfig = RotationConfig {
            period: Duration::from_millis(1),
            stack_size: 64 * 1024,
        };

        let task: RotationTask =
            spawn_rotation(imu, shared.clone(), clock.clone(), config).expect("spawn the thread");

        // A flat board, polled for real: nothing to turn to.
        await_cycles(&rx, 2);
        assert_eq!(shared.current(), ScreenRotation::Deg0);

        // Stand it on its USB-C port. The ping fires *inside* the read, before the fold that
        // follows it, so seeing one ping only proves a cycle started — awaiting the next
        // proves the previous one ran to completion, which is what makes this assertion sound
        // rather than a race that happens to pass.
        *reading.lock().expect("reading lock") = UPRIGHT;
        await_cycles(&rx, 2);
        assert_eq!(
            shared.current(),
            ScreenRotation::Deg0,
            "the picture must not turn before the candidate has been held"
        );

        // Let the settle window pass on the test's own clock, and it turns.
        clock.set(ROTATION_SETTLE_MS);
        await_cycles(&rx, 2);
        assert_eq!(shared.current(), ScreenRotation::Deg270);

        task.stop();
        task.join().expect("the rotation thread must not panic");
    }
}
