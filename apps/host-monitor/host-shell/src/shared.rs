//! SharedMetrics — the rolling history and latest status, shared writer-to-readers.
//!
//! One cache, one writer (the poller thread), one reader (the display). It holds two
//! things the display needs together each tick:
//!
//! - the rolling [`History`] of samples the two graphs plot, and
//! - the latest [`Reading`] — what the last poll concluded (a [`Sample`], *or the fault
//!   that replaced it*), stamped with when — so a reader can apply the pure staleness
//!   rule ([`observe`]) and learn both whether the poller is alive and whether the host
//!   answered.
//!
//! The writer publishes on **every** cycle, faults included. A successful scrape pushes a
//! sample into the history *and* refreshes the latest reading; a failed scrape refreshes
//! only the reading (with the fault) and leaves the history intact — so the graph keeps
//! the trailing window of what the host was doing while the label and creature report the
//! trouble. That is the deliberate divergence from the plant monitor's scalar, which must
//! blank when stale.
//!
//! Every access recovers from a poisoned lock. If the writer — or the reader — panics
//! while holding the cache, a plain `lock().unwrap()` elsewhere would propagate that
//! panic and take the panicking thread's peers down with it. A desk monitor must not let
//! a poller hiccup crash the display thread, so every lock here steps over the poison and
//! reads the value that was there: the cache survives, and staleness ([`observe`]) still
//! retires a value the dead writer can no longer refresh.

use std::sync::{Arc, Mutex, MutexGuard};

use host_core::{observe, History, HostFault, HostState, Reading, Sample, Tick};

/// The cache's contents behind the lock: the rolling history, and the latest reading.
#[derive(Clone, Copy)]
struct Inner {
    /// The rolling window of recent samples — the graphs' data, retained across faults.
    history: History,
    /// What the last poll concluded, and when — `None` until the first sample is published.
    last: Option<Reading>,
}

/// The latest host metrics, shared between the poller and the display.
///
/// Cloning shares the *same* cache (an [`Arc`]); clones are how the poller thread and the
/// display hold the one cache. Reads and writes are non-blocking beyond the brief lock,
/// and poison-tolerant (see the module docs).
#[derive(Clone)]
pub struct SharedMetrics {
    slot: Arc<Mutex<Inner>>,
}

impl SharedMetrics {
    /// An empty cache — no samples yet, so every read is [`NeverSampled`](host_core::Status::NeverSampled)
    /// until the first [`publish_sample`](Self::publish_sample).
    pub fn new() -> Self {
        Self {
            slot: Arc::new(Mutex::new(Inner {
                history: History::new(),
                last: None,
            })),
        }
    }

    /// Push a successful `sample` into the history and record it as the latest reading,
    /// stamped at `now`.
    ///
    /// The graph gains a column and the freshness clock advances. Called on every scrape
    /// that yields a usable sample.
    pub fn publish_sample(&self, sample: Sample, now: Tick) {
        let mut inner: MutexGuard<'_, Inner> = self.guard();
        inner.history.push(sample);
        inner.last = Some(Reading::sampled(sample, now));
    }

    /// Record a `fault` as the latest reading, stamped at `now`, leaving the history intact.
    ///
    /// The freshness clock advances — proving the poller ran — so a consumer sees
    /// [`Faulted`](host_core::Status::Faulted), not the [`Stale`](host_core::Status::Stale)
    /// a *dead* poller would produce. The graph keeps its trailing window: a host that
    /// stopped answering has a recent past worth showing.
    pub fn publish_fault(&self, fault: HostFault, now: Tick) {
        self.guard().last = Some(Reading::faulted(fault, now));
    }

    /// What the display should draw as of `now`: the retained history, and the status the
    /// pure [`observe`] policy derives from the latest reading and `max_age`.
    ///
    /// A stale or faulted status still carries the full history — the graph outlives the
    /// reading. A consumer wraps this in `host_display::Glass` for the render loop.
    pub fn snapshot(&self, now: Tick, max_age: Tick) -> HostState {
        let inner: MutexGuard<'_, Inner> = self.guard();
        HostState::new(inner.history, observe(inner.last, now, max_age))
    }

    /// Lock the cache, stepping over a poisoned lock left by a panicking holder.
    ///
    /// The recovered value is exactly the one the panicking thread left behind — the cache
    /// is an overwrite-in-place slot plus an append-only window — and readers must not
    /// inherit a writer's panic.
    fn guard(&self) -> MutexGuard<'_, Inner> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for SharedMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{Percent, Status};

    /// A sample with the given CPU/memory percents.
    fn sample(cpu: u8, mem: u8) -> Sample {
        Sample::new(
            Percent::new(cpu).expect("0..=100"),
            Percent::new(mem).expect("0..=100"),
        )
    }

    #[test]
    fn an_empty_cache_has_never_been_sampled() {
        let shared: SharedMetrics = SharedMetrics::new();
        assert_eq!(shared.snapshot(100, 50).status, Status::NeverSampled);
        assert!(shared.snapshot(100, 50).history.is_empty());
    }

    #[test]
    fn a_fresh_sample_is_served_and_lands_in_the_history() {
        // One publish, read within the bound: age = 20 - 10 = 10 <= 50.
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_sample(sample(30, 40), 10);
        let snap: HostState = shared.snapshot(20, 50);
        assert_eq!(snap.status, Status::Fresh(sample(30, 40)));
        assert_eq!(snap.history.len(), 1, "the sample joined the graph");
        assert_eq!(snap.history.latest(), Some(sample(30, 40)));
    }

    #[test]
    fn a_stale_sample_is_hidden_but_its_history_survives() {
        // The key divergence from the plant scalar: past the bound the status is Stale, yet
        // the graph keeps the samples.
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_sample(sample(30, 40), 0);
        let snap: HostState = shared.snapshot(51, 50);
        assert_eq!(snap.status, Status::Stale);
        assert_eq!(snap.history.len(), 1, "a stale graph is still drawn");
    }

    /// A published fault keeps the poller-liveness signal fresh and does *not* touch the
    /// history — the trailing window stays while the label reports the fault.
    #[test]
    fn a_published_fault_is_faulted_and_leaves_the_history_intact() {
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_sample(sample(60, 55), 0);
        shared.publish_fault(HostFault::Unreachable, 10);

        let snap: HostState = shared.snapshot(20, 50);
        assert_eq!(snap.status, Status::Faulted(HostFault::Unreachable));
        assert!(snap.status.poller_is_alive());
        assert_eq!(snap.history.len(), 1, "the fault must not erase the graph");
        assert_eq!(snap.history.latest(), Some(sample(60, 55)));
    }

    #[test]
    fn a_fault_that_ages_out_becomes_stale() {
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_fault(HostFault::Unreachable, 0);
        let snap: HostState = shared.snapshot(51, 50);
        assert_eq!(snap.status, Status::Stale);
        assert!(!snap.status.poller_is_alive());
    }

    #[test]
    fn samples_accumulate_in_order() {
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_sample(sample(10, 10), 0);
        shared.publish_sample(sample(20, 20), 1);
        shared.publish_sample(sample(30, 30), 2);
        let snap: HostState = shared.snapshot(2, 50);
        assert_eq!(snap.history.len(), 3);
        assert_eq!(snap.history.latest(), Some(sample(30, 30)));
    }

    #[test]
    fn a_clone_shares_the_one_cache() {
        let writer: SharedMetrics = SharedMetrics::new();
        let reader: SharedMetrics = writer.clone();
        writer.publish_sample(sample(55, 50), 5);
        assert_eq!(reader.snapshot(6, 50).status, Status::Fresh(sample(55, 50)));
    }

    #[test]
    fn a_reader_survives_a_writer_that_poisoned_the_lock() {
        // The panic-isolation guarantee: a holder that panics while holding the cache
        // poisons the Mutex. A reader must step over that poison and still read the value
        // that was there, never inherit the panic.
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_sample(sample(42, 42), 0);

        let poisoner: SharedMetrics = shared.clone();
        let panicked: std::thread::Result<()> = std::thread::spawn(move || {
            let _held: MutexGuard<'_, Inner> = poisoner.slot.lock().unwrap();
            panic!("poller thread died holding the cache");
        })
        .join();
        assert!(panicked.is_err(), "the helper thread must have panicked");

        // Lock is now poisoned; the reader recovers rather than propagating.
        assert_eq!(shared.snapshot(0, 50).status, Status::Fresh(sample(42, 42)));
        // And the cache is still usable afterwards — a fresh write goes through.
        shared.publish_sample(sample(80, 80), 10);
        assert_eq!(
            shared.snapshot(11, 50).status,
            Status::Fresh(sample(80, 80))
        );
    }
}
