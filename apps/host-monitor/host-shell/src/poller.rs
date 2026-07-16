//! The poller thread — fetch the endpoint, publish the frame or the fault.
//!
//! The imperative shell's one background loop: every [`POLL_PERIOD`] it fetches one frame
//! from the [`PulseSource`](host_core::PulseSource) adapter and publishes the result into
//! the [`SharedMetrics`] cache the display reads. It owns the *timing* and the *cache*;
//! everything about *what a frame is* stays inward (the wire codec, the clamping/gap
//! transform), so this loop's body is a straight line.
//!
//! ## No accumulation
//!
//! Each fetch carries the whole window — the endpoint has already done the PromQL `rate()`
//! — so a success simply **replaces** the cached frame. There is no rolling history and no
//! carried state: unlike the old node_exporter poller, which threaded a CPU-counter baseline
//! through its cycles to compute a rate, this loop is stateless.
//!
//! ## Two failure modes, told apart
//!
//! - A **failed fetch** publishes a [`HostFault`](host_core::HostFault), stamped at the
//!   current tick. The last good frame is kept, but there is no fresh one — and because the
//!   publish is fresh, consumers see [`Faulted`](host_core::Status::Faulted): the poller is
//!   alive and the endpoint did not answer.
//! - A **dead thread** (a panic, a hang) stops refreshing the cache entirely, so
//!   [`observe`](host_core::observe) retires the reading within [`STALENESS_PERIODS`] periods
//!   and consumers see `Stale`. No supervisor is needed: staleness *is* the liveness check.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use host_core::{HostFault, PulseFault, PulseSource, Tick};
use log::warn;
use platform_core::Clock;

use crate::shared::SharedMetrics;

/// The interval between fetches.
///
/// The endpoint returns the whole window each time, so there is nothing to gain from a fast
/// poll — twenty seconds keeps the glass current (well inside the staleness bound) while
/// staying light on the LAN hop and the control node's Prometheus. The composition root can
/// change it.
pub const POLL_PERIOD: Duration = Duration::from_secs(20);

/// How many periods a reading may age before consumers treat it as unavailable.
///
/// Three periods tolerates a couple of missed or slow fetches before declaring the endpoint
/// stale — long enough not to flicker on one hiccup, short enough that a genuinely dead
/// poller surfaces quickly.
pub const STALENESS_PERIODS: u32 = 3;

/// The poller thread's stack, in bytes.
///
/// On-device this sizes a FreeRTOS task stack, so it is set explicitly. The loop drives the
/// HTTP adapter (whose transfer buffers are heap, not stack) and hands the small JSON body to
/// the codec, so the frame is shallow — but SRAM is scarce (520 KB, no PSRAM), so it is not
/// made lavish. Like every stack here, validate the true high-water mark on the metal before
/// trusting it.
pub const POLLER_STACK_SIZE: usize = 8 * 1024;

/// How the poller is tuned: its timing and its stack.
///
/// Every field has a sensible default (the module constants), so [`Default`] is enough for
/// every app so far; the fields are public so a composition root can override one. [`Copy`],
/// so it can be read for [`max_age`](Self::max_age) and still moved into the thread.
#[derive(Clone, Copy)]
pub struct PollerConfig {
    /// The interval between fetches.
    pub period: Duration,
    /// The staleness bound, in periods (see [`STALENESS_PERIODS`]).
    pub staleness_periods: u32,
    /// The poller thread's stack size, in bytes.
    pub stack_size: usize,
}

impl Default for PollerConfig {
    fn default() -> Self {
        Self {
            period: POLL_PERIOD,
            staleness_periods: STALENESS_PERIODS,
            stack_size: POLLER_STACK_SIZE,
        }
    }
}

impl PollerConfig {
    /// The default config — the module constants.
    pub fn new() -> Self {
        Self::default()
    }

    /// The maximum age (in [`Tick`] milliseconds) a reading may reach before consumers treat
    /// it as unavailable: `period * staleness_periods`.
    ///
    /// The reader passes this to [`SharedMetrics::snapshot`]; keeping the arithmetic here
    /// means the writer's period and the reader's bound can never drift apart.
    pub fn max_age(&self) -> Tick {
        let period_ms: Tick = self.period.as_millis().min(u128::from(Tick::MAX)) as Tick;
        period_ms.saturating_mul(Tick::from(self.staleness_periods))
    }
}

/// A running poller thread — a handle to stop and join it.
///
/// Dropping the handle detaches the thread (it keeps polling), which is what the composition
/// root wants: the monitor polls for the life of the program. Tests and a future clean-
/// shutdown path use [`stop`](Self::stop) + [`join`](Self::join).
pub struct Poller {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl Poller {
    /// Ask the poller to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the poller thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Spawn the poller thread: `source` → `shared`, every `config.period`.
///
/// The thread is named and sized per `config`. `clock` is the shared time base — an injected
/// [`Clock`] (the composition root's `Monotonic`), the same one the display reads — so a
/// published reading's age is measured on one clock. `source` and `clock` move into the
/// thread, so both must be [`Send`] + `'static`; the source's error must be
/// [`Display`](std::fmt::Display) so a failed fetch can be logged.
///
/// Returns the [`Poller`] handle, or the [`io::Error`] from failing to spawn the OS/RTOS
/// thread.
pub fn spawn_poller<S, C>(
    source: S,
    shared: SharedMetrics,
    clock: C,
    config: PollerConfig,
) -> io::Result<Poller>
where
    S: PulseSource + Send + 'static,
    S::Error: std::fmt::Display,
    C: Clock + Send + 'static,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("host-poller".to_string())
        .stack_size(config.stack_size)
        .spawn(move || poll_loop(source, shared, clock, config, stop_in_thread))?;
    Ok(Poller { handle, stop })
}

/// The thread body: fetch, publish, sleep — until asked to stop.
fn poll_loop<S, C>(
    mut source: S,
    shared: SharedMetrics,
    clock: C,
    config: PollerConfig,
    stop: Arc<AtomicBool>,
) where
    S: PulseSource,
    S::Error: std::fmt::Display,
    C: Clock,
{
    while !stop.load(Ordering::Relaxed) {
        poll_once(&mut source, &shared, clock.now());
        thread::sleep(config.period);
    }
}

/// One poll cycle: fetch a frame, publish it — or classify and publish the fault.
///
/// The whole shell↔core seam, in isolation and testable without a thread. On a good fetch it
/// replaces the cached frame at `now`; on a failed fetch it classifies the adapter's error
/// into a [`HostFault`] and publishes *that*, at `now`, keeping the last good frame.
/// Complexity is one branch: fetch ok, or not.
///
/// Publishing the fault is the point. A cycle that published nothing on failure would let the
/// reading age out, and an aged-out reading is what a **dead poller thread** looks like — so
/// an unreachable endpoint and a dead device would be indistinguishable. By stamping the
/// fault, the writer proves it ran; staleness then means one thing only. The adapter's own
/// error still reaches the log, where its detail is worth more than the verdict.
fn poll_once<S>(source: &mut S, shared: &SharedMetrics, now: Tick)
where
    S: PulseSource,
    S::Error: std::fmt::Display,
{
    match source.poll() {
        Ok(frame) => shared.publish_frame(frame, now),
        Err(err) => {
            let fault: HostFault = err.fault();
            warn!("host-poller: publishing endpoint fault ({fault}); adapter said: {err}");
            shared.publish_fault(fault, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{HostSeries, Percent, Pulse, PulseBuilder, Status};
    use std::collections::VecDeque;
    use std::sync::mpsc::{channel, Sender};
    use std::time::Instant;

    /// A monotonic test clock over [`Instant`] — host-shell cannot depend on the runtime's
    /// `Monotonic` (that is infra, wired by the composition root), so the one integration test
    /// that needs a real advancing clock brings its own `Clock` adapter.
    #[derive(Clone, Copy)]
    struct TestClock {
        origin: Instant,
    }

    impl TestClock {
        fn start() -> Self {
            Self {
                origin: Instant::now(),
            }
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> Tick {
            self.origin.elapsed().as_millis().min(u128::from(Tick::MAX)) as Tick
        }
    }

    /// A one-host frame whose fedora CPU ends at `cpu`, for identity in asserts.
    fn frame(cpu: i32) -> Pulse {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push("fedora", &[Some(cpu)], &[Some(50)]);
        b.build()
    }

    /// The latest CPU value of the first host in a snapshot's frame.
    fn latest_cpu(state: &host_core::HostState) -> Option<u8> {
        state.frame.and_then(|f: Pulse| {
            f.hosts()
                .first()
                .and_then(|h: &HostSeries| h.cpu().latest())
                .map(Percent::value)
        })
    }

    /// A fetch failure whose message is [`Display`], as the poller's log bound requires, and
    /// which classifies into a [`HostFault`], as the port requires. It carries its own fault
    /// rather than hardcoding one, so a test can prove the *adapter's* classification reaches
    /// the cache untouched.
    #[derive(Clone, Debug)]
    struct TestError(&'static str, HostFault);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl PulseFault for TestError {
        fn fault(&self) -> HostFault {
            self.1
        }
    }

    /// A `PulseSource` fake: it yields a queue of fetch results, then a fixed `after` result
    /// forever. Optionally pings a channel on every poll, so a test can block until the thread
    /// has actually run rather than poll or sleep-and-hope.
    struct FakeSource {
        queue: VecDeque<Result<Pulse, TestError>>,
        after: Result<Pulse, TestError>,
        ping: Option<Sender<()>>,
    }

    impl FakeSource {
        /// Yields each of `frames` once, then repeats the last forever — a healthy endpoint.
        fn frames(frames: &[Pulse]) -> Self {
            let last: Pulse = *frames.last().expect("at least one frame");
            Self {
                queue: frames.iter().copied().map(Ok).collect(),
                after: Ok(last),
                ping: None,
            }
        }

        /// Errors on every poll — an endpoint that never answered.
        fn unreachable() -> Self {
            Self {
                queue: VecDeque::new(),
                after: Err(TestError("connection refused", HostFault::Unreachable)),
                ping: None,
            }
        }

        /// Yields `frames` once, then reports `fault` forever — an endpoint that answered and
        /// then went dark.
        fn frames_then_fault(frames: &[Pulse], fault: HostFault) -> Self {
            Self {
                queue: frames.iter().copied().map(Ok).collect(),
                after: Err(TestError("endpoint went dark", fault)),
                ping: None,
            }
        }

        /// Ping `tx` on every poll, so a test can await a real fetch.
        fn pinging(mut self, tx: Sender<()>) -> Self {
            self.ping = Some(tx);
            self
        }
    }

    impl PulseSource for FakeSource {
        type Error = TestError;

        fn poll(&mut self) -> Result<Pulse, TestError> {
            let result: Result<Pulse, TestError> =
                self.queue.pop_front().unwrap_or_else(|| self.after.clone());
            if let Some(tx) = &self.ping {
                let _ = tx.send(());
            }
            result
        }
    }

    #[test]
    fn a_fetch_publishes_a_fresh_frame() {
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::frames(&[frame(30)]);

        poll_once(&mut source, &shared, 5);

        let snap: host_core::HostState = shared.snapshot(5, 1000);
        assert_eq!(snap.status, Status::Fresh);
        assert_eq!(latest_cpu(&snap), Some(30));
    }

    #[test]
    fn a_failed_fetch_publishes_the_fault_and_keeps_the_frame() {
        // Publish a frame, then the endpoint goes dark: the reading becomes a fresh fault and
        // the frame stays.
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource =
            FakeSource::frames_then_fault(&[frame(60)], HostFault::Unreachable);

        poll_once(&mut source, &shared, 2); // publishes a frame
        assert_eq!(latest_cpu(&shared.snapshot(2, 1000)), Some(60));

        poll_once(&mut source, &shared, 4); // fault
        let snap: host_core::HostState = shared.snapshot(4, 1000);
        assert_eq!(snap.status, Status::Faulted(HostFault::Unreachable));
        assert!(snap.status.poller_is_alive());
        assert_eq!(
            latest_cpu(&snap),
            Some(60),
            "the fault must not erase the frame"
        );
    }

    #[test]
    fn the_adapters_own_classification_reaches_the_cache() {
        // The poller must not invent a fault: whatever the adapter classified is what a
        // consumer sees. A malformed body is Malformed, not Unreachable.
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::frames_then_fault(&[], HostFault::Malformed);

        poll_once(&mut source, &shared, 5);

        assert_eq!(
            shared.snapshot(5, 1000).status,
            Status::Faulted(HostFault::Malformed)
        );
    }

    /// An unreachable endpoint reports as `Faulted` on the very first cycle — never as the
    /// `Stale` a dead poller thread produces — and keeps doing so, so it never masquerades as
    /// a dead device.
    #[test]
    fn an_unreachable_endpoint_reports_faulted_at_once_and_never_looks_stale() {
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::unreachable();
        let config: PollerConfig = PollerConfig::new();
        let max_age: Tick = config.max_age();

        poll_once(&mut source, &shared, 0);
        assert_eq!(
            shared.snapshot(0, max_age).status,
            Status::Faulted(HostFault::Unreachable)
        );

        // The poller keeps running and republishing, so the reading never ages out.
        poll_once(&mut source, &shared, max_age / 2);
        poll_once(&mut source, &shared, max_age + 100);
        let snap: host_core::HostState = shared.snapshot(max_age + 100, max_age);
        assert_eq!(snap.status, Status::Faulted(HostFault::Unreachable));
        assert!(snap.status.poller_is_alive());
    }

    /// The other half of the distinction: when the poller *stops running*, nothing
    /// republishes, and the reading goes `Stale` within the bound.
    #[test]
    fn a_poller_that_stops_running_goes_stale_within_the_bound() {
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::frames(&[frame(50)]);
        let config: PollerConfig = PollerConfig::new();
        let max_age: Tick = config.max_age();

        poll_once(&mut source, &shared, 0); // a frame at t=0

        // No further cycles: the thread died. At exactly the bound it is still fresh.
        assert_eq!(shared.snapshot(max_age, max_age).status, Status::Fresh);
        // One tick past it, stale — and stale does not claim the poller is alive.
        let snap: host_core::HostState = shared.snapshot(max_age + 1, max_age);
        assert_eq!(snap.status, Status::Stale);
        assert!(!snap.status.poller_is_alive());
        assert_eq!(
            latest_cpu(&snap),
            Some(50),
            "the stale frame is still drawn"
        );
    }

    #[test]
    fn the_spawned_thread_polls_publishes_and_stops_cleanly() {
        // The one integration test: spawn the real thread, fetch and publish through the loop
        // into the shared cache, stop and join without a panic. Blocking on two pings (not
        // polling) makes it robust: the first proves a fetch ran, the second proves the loop
        // kept cycling.
        let clock: TestClock = TestClock::start();
        let shared: SharedMetrics = SharedMetrics::new();
        let (tx, rx): (Sender<()>, _) = channel();
        let source: FakeSource = FakeSource::frames(&[frame(10), frame(20), frame(30)]).pinging(tx);
        let config: PollerConfig = PollerConfig {
            period: Duration::from_millis(1),
            staleness_periods: STALENESS_PERIODS,
            stack_size: 256 * 1024,
        };

        let poller: Poller =
            spawn_poller(source, shared.clone(), clock, config).expect("spawn poller thread");

        rx.recv_timeout(Duration::from_secs(2))
            .expect("the poller must fetch");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("the poller must keep cycling");

        // A generous max_age so scheduler jitter can't masquerade as staleness.
        assert_eq!(
            shared.snapshot(clock.now(), 60_000).status,
            Status::Fresh,
            "the cache must serve a fresh frame"
        );

        poller.stop();
        poller.join().expect("the poller thread must not panic");
    }
}
