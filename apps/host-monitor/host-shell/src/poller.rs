//! The poller thread — scrape the host, fold through the pure step, publish.
//!
//! The imperative shell's one background loop: every [`POLL_PERIOD`] it takes a scrape
//! from the [`MetricsSource`](host_core::MetricsSource) adapter, hands it to the pure
//! stateful [`step`](host_core::step), and publishes the result into the
//! [`SharedMetrics`] cache the display reads. It owns the *timing* and the *cache*; every
//! scrap of parsing and rate arithmetic stays inward in `host-core`, so this loop's body
//! is a straight line.
//!
//! ## Two failure modes, told apart
//!
//! - A **failed scrape** publishes a [`HostFault`](host_core::HostFault), stamped at the
//!   current tick. The graph's trailing history is kept, but there is no fresh sample —
//!   and because the publish is fresh, consumers see
//!   [`Faulted`](host_core::Status::Faulted): the poller is alive and the host did not
//!   answer.
//! - A **dead thread** (a panic, a hang) stops refreshing the cache entirely, so
//!   [`observe`](host_core::observe) retires the reading within [`STALENESS_PERIODS`]
//!   periods and consumers see `Stale`. No supervisor is needed: staleness *is* the
//!   liveness check.
//!
//! ## CPU is a rate, so the loop carries state
//!
//! Unlike a stateless sampler, the poller threads a [`PollState`](host_core::PollState)
//! through its cycles — the previous scrape's CPU counters, which the busy-fraction
//! arithmetic needs. The first scrape only primes that state (publishing nothing); the
//! second onward publishes a sample. Across a failed scrape the state is *kept*, so the
//! next success measures the average load over the gap rather than restarting.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use host_core::{step, HostFault, MetricsFault, MetricsSource, PollState, Sample, Tick};
use log::warn;
use platform_core::Clock;

use crate::shared::SharedMetrics;

/// The interval between scrapes.
///
/// Two seconds keeps the graph responsive (a load spike shows within a period) while
/// staying frugal on the network and the host's exporter; with a [`CAPACITY`]-wide graph
/// that is a few minutes of visible history. The composition root can change it.
///
/// [`CAPACITY`]: host_core::history::CAPACITY
pub const POLL_PERIOD: Duration = Duration::from_secs(2);

/// How many periods a reading may age before consumers treat it as unavailable.
///
/// Three periods tolerates a couple of missed or slow scrapes before declaring the host
/// stale — long enough not to flicker on one hiccup, short enough that a genuinely dead
/// poller surfaces quickly.
pub const STALENESS_PERIODS: u32 = 3;

/// The poller thread's stack, in bytes.
///
/// On-device this sizes a FreeRTOS task stack, so it is set explicitly. The loop drives
/// the HTTP adapter (whose transfer buffers are heap, not stack) and folds one scrape
/// line at a time, so the frame is shallow — but SRAM is scarce (520 KB, no PSRAM), so it
/// is not made lavish. Like every stack here, validate the true high-water mark on the
/// metal before trusting it.
pub const POLLER_STACK_SIZE: usize = 8 * 1024;

/// How the poller is tuned: its timing and its stack.
///
/// Every field has a sensible default (the module constants), so [`Default`] is enough for
/// every app so far; the fields are public so a composition root can override one.
/// [`Copy`], so it can be read for [`max_age`](Self::max_age) and still moved into the
/// thread.
#[derive(Clone, Copy)]
pub struct PollerConfig {
    /// The interval between scrapes.
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

    /// The maximum age (in [`Tick`] milliseconds) a reading may reach before consumers
    /// treat it as unavailable: `period * staleness_periods`.
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
/// Dropping the handle detaches the thread (it keeps polling), which is what the
/// composition root wants: the monitor polls for the life of the program. Tests and a
/// future clean-shutdown path use [`stop`](Self::stop) + [`join`](Self::join).
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

/// Spawn the poller thread: `source` → [`step`] → `shared`, every `config.period`.
///
/// The thread is named and sized per `config`. `clock` is the shared time base — an
/// injected [`Clock`] (the composition root's `Monotonic`), the same one the display
/// reads — so a published reading's age is measured on one clock. `source` and `clock`
/// move into the thread, so both must be [`Send`] + `'static`; the source's error must be
/// [`Display`](std::fmt::Display) so a failed scrape can be logged.
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
    S: MetricsSource + Send + 'static,
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

/// The thread body: poll, publish, sleep — until asked to stop, carrying the CPU baseline.
fn poll_loop<S, C>(
    mut source: S,
    shared: SharedMetrics,
    clock: C,
    config: PollerConfig,
    stop: Arc<AtomicBool>,
) where
    S: MetricsSource,
    S::Error: std::fmt::Display,
    C: Clock,
{
    let mut state: PollState = PollState::new();
    while !stop.load(Ordering::Relaxed) {
        state = poll_once(&mut source, &shared, clock.now(), state);
        thread::sleep(config.period);
    }
}

/// One poll cycle: scrape, fold through the pure [`step`], publish the outcome.
///
/// The whole shell↔core seam, in isolation and testable without a thread. On a good
/// scrape it folds through [`step`], and — once a CPU rate exists (the second scrape
/// onward) — publishes the [`Sample`] at `now`; a priming first scrape publishes nothing
/// but advances the carried state. On a failed scrape it classifies the adapter's error
/// into a [`HostFault`] and publishes *that*, at `now`, keeping the CPU baseline so the
/// next success spans the gap. Complexity is one branch: scrape ok, or not.
///
/// Publishing the fault is the point. A cycle that published nothing on failure would let
/// the reading age out, and an aged-out reading is what a **dead poller thread** looks
/// like — so an unreachable host and a dead device would be indistinguishable. By stamping
/// the fault, the writer proves it ran; staleness then means one thing only. The adapter's
/// own error still reaches the log, where its detail is worth more than the verdict.
fn poll_once<S>(source: &mut S, shared: &SharedMetrics, now: Tick, state: PollState) -> PollState
where
    S: MetricsSource,
    S::Error: std::fmt::Display,
{
    match source.poll() {
        Ok(scrape) => {
            let (next, sample): (PollState, Option<Sample>) = step(state, scrape);
            if let Some(sample) = sample {
                shared.publish_sample(sample, now);
            }
            next
        }
        Err(err) => {
            let fault: HostFault = err.fault();
            warn!("host-poller: publishing host fault ({fault}); adapter said: {err}");
            shared.publish_fault(fault, now);
            state
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{Percent, RawScrape, Status};
    use std::collections::VecDeque;
    use std::sync::mpsc::{channel, Sender};
    use std::time::Instant;

    /// A monotonic test clock over [`Instant`] — host-shell cannot depend on the runtime's
    /// `Monotonic` (that is infra, wired by the composition root), so the one integration
    /// test that needs a real advancing clock brings its own `Clock` adapter.
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

    /// A scrape carrying the given CPU counters, with a fixed 16 GB / 8 GB memory (50 %).
    fn scrape(idle: f64, total: f64) -> RawScrape {
        RawScrape {
            cpu_idle_secs: idle,
            cpu_total_secs: total,
            mem_total: 16.0e9,
            mem_avail: 8.0e9,
        }
    }

    /// The sample a two-scrape sequence with the fixed memory yields: `cpu` % busy, 50 % mem.
    fn sample(cpu: u8) -> Sample {
        Sample::new(
            Percent::new(cpu).expect("0..=100"),
            Percent::new(50).unwrap(),
        )
    }

    /// A scrape read failure whose message is [`Display`], as the poller's log bound
    /// requires, and which classifies into a [`HostFault`], as the port requires. It carries
    /// its own fault rather than hardcoding one, so a test can prove the *adapter's*
    /// classification reaches the cache untouched.
    #[derive(Clone, Debug)]
    struct TestError(&'static str, HostFault);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl MetricsFault for TestError {
        fn fault(&self) -> HostFault {
            self.1
        }
    }

    /// A `MetricsSource` fake: it yields a queue of scrape results, then a fixed `after`
    /// result forever. Optionally pings a channel on every poll, so a test can block until
    /// the thread has actually run rather than poll or sleep-and-hope.
    struct FakeSource {
        queue: VecDeque<Result<RawScrape, TestError>>,
        after: Result<RawScrape, TestError>,
        ping: Option<Sender<()>>,
    }

    impl FakeSource {
        /// Yields each of `scrapes` once, then repeats the last forever — a healthy host.
        fn scrapes(scrapes: &[RawScrape]) -> Self {
            let last: RawScrape = *scrapes.last().expect("at least one scrape");
            Self {
                queue: scrapes.iter().copied().map(Ok).collect(),
                after: Ok(last),
                ping: None,
            }
        }

        /// Errors on every poll — a host that never answered.
        fn unreachable() -> Self {
            Self {
                queue: VecDeque::new(),
                after: Err(TestError("connection refused", HostFault::Unreachable)),
                ping: None,
            }
        }

        /// Yields `scrapes` once, then reports `fault` forever — a host that answered and
        /// then went dark.
        fn scrapes_then_fault(scrapes: &[RawScrape], fault: HostFault) -> Self {
            Self {
                queue: scrapes.iter().copied().map(Ok).collect(),
                after: Err(TestError("host went dark", fault)),
                ping: None,
            }
        }

        /// Ping `tx` on every poll, so a test can await a real scrape.
        fn pinging(mut self, tx: Sender<()>) -> Self {
            self.ping = Some(tx);
            self
        }
    }

    impl MetricsSource for FakeSource {
        type Error = TestError;

        fn poll(&mut self) -> Result<RawScrape, TestError> {
            let result: Result<RawScrape, TestError> =
                self.queue.pop_front().unwrap_or_else(|| self.after.clone());
            if let Some(tx) = &self.ping {
                let _ = tx.send(());
            }
            result
        }
    }

    #[test]
    fn the_first_scrape_primes_and_publishes_nothing() {
        // A single scrape has no CPU rate, so the cache is still NeverSampled after it.
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::scrapes(&[scrape(1000.0, 2000.0)]);

        let _state: PollState = poll_once(&mut source, &shared, 5, PollState::new());

        assert_eq!(shared.snapshot(5, 1000).status, Status::NeverSampled);
        assert!(shared.snapshot(5, 1000).history.is_empty());
    }

    #[test]
    fn a_second_scrape_publishes_a_sample_through_the_pure_step() {
        // idle +25 of total +100 over the interval → 75 % busy; memory the fixed 50 %.
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource =
            FakeSource::scrapes(&[scrape(1000.0, 2000.0), scrape(1025.0, 2100.0)]);

        let state: PollState = poll_once(&mut source, &shared, 0, PollState::new());
        let _state: PollState = poll_once(&mut source, &shared, 2, state);

        let snap: host_core::HostState = shared.snapshot(2, 1000);
        assert_eq!(snap.status, Status::Fresh(sample(75)));
        assert_eq!(snap.history.len(), 1, "the sample joined the graph");
    }

    #[test]
    fn a_failed_scrape_publishes_the_fault_and_keeps_the_history() {
        // Build up a sample, then the host goes dark: the reading becomes a fresh fault and
        // the graph keeps its trailing window.
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::scrapes_then_fault(
            &[scrape(1000.0, 2000.0), scrape(1050.0, 2100.0)],
            HostFault::Unreachable,
        );

        let mut state: PollState = poll_once(&mut source, &shared, 0, PollState::new());
        state = poll_once(&mut source, &shared, 2, state); // publishes a sample
        assert_eq!(shared.snapshot(2, 1000).history.len(), 1);

        let _state: PollState = poll_once(&mut source, &shared, 4, state); // fault
        let snap: host_core::HostState = shared.snapshot(4, 1000);
        assert_eq!(snap.status, Status::Faulted(HostFault::Unreachable));
        assert!(snap.status.poller_is_alive());
        assert_eq!(snap.history.len(), 1, "the fault must not erase the graph");
    }

    #[test]
    fn the_adapters_own_classification_reaches_the_cache() {
        // The poller must not invent a fault: whatever the adapter classified is what a
        // consumer sees. A malformed body is Malformed, not Unreachable.
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::scrapes_then_fault(&[], HostFault::Malformed);

        let _state: PollState = poll_once(&mut source, &shared, 5, PollState::new());

        assert_eq!(
            shared.snapshot(5, 1000).status,
            Status::Faulted(HostFault::Malformed)
        );
    }

    /// An unreachable host reports as `Faulted` on the very first cycle — never as the
    /// `Stale` a dead poller thread produces — and keeps doing so, so it never masquerades
    /// as a dead device.
    #[test]
    fn an_unreachable_host_reports_faulted_at_once_and_never_looks_stale() {
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource = FakeSource::unreachable();
        let config: PollerConfig = PollerConfig::new();
        let max_age: Tick = config.max_age(); // 6000 ms

        let mut state: PollState = poll_once(&mut source, &shared, 0, PollState::new());
        assert_eq!(
            shared.snapshot(0, max_age).status,
            Status::Faulted(HostFault::Unreachable)
        );

        // The poller keeps running and republishing, so the reading never ages out.
        state = poll_once(&mut source, &shared, 4000, state);
        let _state: PollState = poll_once(&mut source, &shared, 8000, state);
        let snap: host_core::HostState = shared.snapshot(8000, max_age);
        assert_eq!(snap.status, Status::Faulted(HostFault::Unreachable));
        assert!(snap.status.poller_is_alive());
    }

    /// The other half of the distinction: when the poller *stops running*, nothing
    /// republishes, and the reading goes `Stale` within the bound.
    #[test]
    fn a_poller_that_stops_running_goes_stale_within_the_bound() {
        let shared: SharedMetrics = SharedMetrics::new();
        let mut source: FakeSource =
            FakeSource::scrapes(&[scrape(1000.0, 2000.0), scrape(1050.0, 2100.0)]);
        let config: PollerConfig = PollerConfig::new();
        let max_age: Tick = config.max_age(); // 6000 ms

        let state: PollState = poll_once(&mut source, &shared, 0, PollState::new());
        let _state: PollState = poll_once(&mut source, &shared, 0, state); // a sample at t=0

        // No further cycles: the thread died. At exactly the bound it is still fresh.
        assert_eq!(
            shared.snapshot(6000, max_age).status,
            Status::Fresh(sample(50))
        );
        // One tick past it, stale — and stale does not claim the poller is alive.
        let snap: host_core::HostState = shared.snapshot(6001, max_age);
        assert_eq!(snap.status, Status::Stale);
        assert!(!snap.status.poller_is_alive());
        assert_eq!(snap.history.len(), 1, "the stale graph is still drawn");
    }

    #[test]
    fn the_spawned_thread_polls_publishes_and_stops_cleanly() {
        // The one integration test: spawn the real thread, prime and publish through the
        // loop into the shared cache, stop and join without a panic. Blocking on three
        // pings (not polling) makes it robust: the first primes, the second publishes the
        // first sample, the third proves that publish landed.
        let clock: TestClock = TestClock::start();
        let shared: SharedMetrics = SharedMetrics::new();
        let (tx, rx): (Sender<()>, _) = channel();
        let source: FakeSource = FakeSource::scrapes(&[
            scrape(1000.0, 2000.0),
            scrape(1010.0, 2100.0),
            scrape(1020.0, 2200.0),
        ])
        .pinging(tx);
        let config: PollerConfig = PollerConfig {
            period: Duration::from_millis(1),
            staleness_periods: STALENESS_PERIODS,
            stack_size: 256 * 1024,
        };

        let poller: Poller =
            spawn_poller(source, shared.clone(), clock, config).expect("spawn poller thread");

        rx.recv_timeout(Duration::from_secs(2))
            .expect("the poller must prime");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a second scrape must publish");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a third scrape proves the publish landed");

        // A generous max_age so scheduler jitter can't masquerade as staleness.
        assert!(
            matches!(
                shared.snapshot(clock.now(), 60_000).status,
                Status::Fresh(_)
            ),
            "the cache must serve a fresh sample"
        );

        poller.stop();
        poller.join().expect("the poller thread must not panic");
    }
}
