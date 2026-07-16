//! SharedMetrics — the last good frame and latest status, shared writer-to-readers.
//!
//! One cache, one writer (the poller thread), one reader (the display). It holds two
//! things the display needs together each tick:
//!
//! - the last good [`Pulse`] frame the endpoint returned — the rows the display draws — and
//! - the latest [`Reading`] — what the last fetch concluded (a success, *or the fault that
//!   replaced it*), stamped with when — so a reader can apply the pure staleness rule
//!   ([`observe`]) and learn both whether the poller is alive and whether the endpoint
//!   answered.
//!
//! Each fetch carries the *whole* window, so a success simply **replaces** the frame —
//! there is no on-device accumulation. The writer publishes on **every** cycle, faults
//! included. A successful fetch replaces the frame *and* refreshes the reading; a failed
//! fetch refreshes only the reading (with the fault) and leaves the last good frame intact —
//! so the glass keeps showing the recent window while the marker reports the trouble. That
//! is the deliberate divergence from the plant monitor's scalar, which must blank when
//! stale.
//!
//! Every access recovers from a poisoned lock. If the writer — or the reader — panics while
//! holding the cache, a plain `lock().unwrap()` elsewhere would propagate that panic and
//! take the panicking thread's peers down with it. A desk monitor must not let a poller
//! hiccup crash the display thread, so every lock here steps over the poison and reads the
//! value that was there: the cache survives, and staleness ([`observe`]) still retires a
//! value the dead writer can no longer refresh.

use std::sync::{Arc, Mutex, MutexGuard};

use host_core::{observe, HostFault, HostState, Pulse, Reading, Tick};

/// The cache's contents behind the lock: the last good frame, and the latest reading.
#[derive(Clone, Copy)]
struct Inner {
    /// The last frame the endpoint returned — retained across faults; `None` until the
    /// first success.
    frame: Option<Pulse>,
    /// What the last fetch concluded, and when — `None` until the first fetch.
    last: Option<Reading>,
}

/// The latest pulse frame and status, shared between the poller and the display.
///
/// Cloning shares the *same* cache (an [`Arc`]); clones are how the poller thread and the
/// display hold the one cache. Reads and writes are non-blocking beyond the brief lock, and
/// poison-tolerant (see the module docs).
#[derive(Clone)]
pub struct SharedMetrics {
    slot: Arc<Mutex<Inner>>,
}

impl SharedMetrics {
    /// An empty cache — no frame fetched yet, so every read is
    /// [`NeverSampled`](host_core::Status::NeverSampled) until the first
    /// [`publish_frame`](Self::publish_frame).
    pub fn new() -> Self {
        Self {
            slot: Arc::new(Mutex::new(Inner {
                frame: None,
                last: None,
            })),
        }
    }

    /// Replace the frame with a freshly fetched `frame` and record success, stamped at `now`.
    ///
    /// The whole window arrives at once, so this overwrites rather than accumulates. The
    /// freshness clock advances. Called on every fetch that yields a usable frame.
    pub fn publish_frame(&self, frame: Pulse, now: Tick) {
        let mut inner: MutexGuard<'_, Inner> = self.guard();
        inner.frame = Some(frame);
        inner.last = Some(Reading::fetched(now));
    }

    /// Record a `fault` as the latest reading, stamped at `now`, leaving the last good frame
    /// intact.
    ///
    /// The freshness clock advances — proving the poller ran — so a consumer sees
    /// [`Faulted`](host_core::Status::Faulted), not the [`Stale`](host_core::Status::Stale) a
    /// *dead* poller would produce. The frame stays: an endpoint that stopped answering has a
    /// recent past worth showing.
    pub fn publish_fault(&self, fault: HostFault, now: Tick) {
        self.guard().last = Some(Reading::faulted(fault, now));
    }

    /// What the display should draw as of `now`: the retained frame, and the status the pure
    /// [`observe`] policy derives from the latest reading and `max_age`.
    ///
    /// A stale or faulted status still carries the last good frame — the frame outlives the
    /// reading. A consumer wraps this in `host_display::Glass` for the render loop.
    pub fn snapshot(&self, now: Tick, max_age: Tick) -> HostState {
        let inner: MutexGuard<'_, Inner> = self.guard();
        HostState::new(inner.frame, observe(inner.last, now, max_age))
    }

    /// Lock the cache, stepping over a poisoned lock left by a panicking holder.
    ///
    /// The recovered value is exactly the one the panicking thread left behind — the cache is
    /// two overwrite-in-place slots — and readers must not inherit a writer's panic.
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
    use host_core::{HostSeries, Percent, PulseBuilder, Status};

    /// A one-host frame whose fedora CPU/mem end at `cpu`/`mem`, for identity in asserts.
    fn frame(cpu: i32, mem: i32) -> Pulse {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push("fedora", &[Some(cpu)], &[Some(mem)]);
        b.build()
    }

    /// The latest CPU value of the first host in a snapshot's frame.
    fn latest_cpu(state: &HostState) -> Option<u8> {
        state.frame.and_then(|f: Pulse| {
            f.hosts()
                .first()
                .and_then(|h: &HostSeries| h.cpu().latest())
                .map(Percent::value)
        })
    }

    #[test]
    fn an_empty_cache_has_never_been_sampled() {
        let shared: SharedMetrics = SharedMetrics::new();
        let snap: HostState = shared.snapshot(100, 50);
        assert_eq!(snap.status, Status::NeverSampled);
        assert!(snap.frame.is_none());
    }

    #[test]
    fn a_fresh_frame_is_served() {
        // One publish, read within the bound: age = 20 - 10 = 10 <= 50.
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_frame(frame(30, 40), 10);
        let snap: HostState = shared.snapshot(20, 50);
        assert_eq!(snap.status, Status::Fresh);
        assert_eq!(latest_cpu(&snap), Some(30));
    }

    #[test]
    fn a_stale_frame_is_marked_but_its_data_survives() {
        // The key divergence from the plant scalar: past the bound the status is Stale, yet
        // the frame keeps its data.
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_frame(frame(30, 40), 0);
        let snap: HostState = shared.snapshot(51, 50);
        assert_eq!(snap.status, Status::Stale);
        assert_eq!(latest_cpu(&snap), Some(30), "a stale frame is still drawn");
    }

    /// A published fault keeps the poller-liveness signal fresh and does *not* touch the
    /// frame — the recent window stays while the marker reports the fault.
    #[test]
    fn a_published_fault_is_faulted_and_leaves_the_frame_intact() {
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_frame(frame(60, 55), 0);
        shared.publish_fault(HostFault::Unreachable, 10);

        let snap: HostState = shared.snapshot(20, 50);
        assert_eq!(snap.status, Status::Faulted(HostFault::Unreachable));
        assert!(snap.status.poller_is_alive());
        assert_eq!(
            latest_cpu(&snap),
            Some(60),
            "the fault must not erase the frame"
        );
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
    fn a_later_frame_replaces_the_earlier_one() {
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_frame(frame(10, 10), 0);
        shared.publish_frame(frame(80, 80), 1);
        let snap: HostState = shared.snapshot(2, 50);
        assert_eq!(
            latest_cpu(&snap),
            Some(80),
            "each fetch replaces the window"
        );
    }

    #[test]
    fn a_clone_shares_the_one_cache() {
        let writer: SharedMetrics = SharedMetrics::new();
        let reader: SharedMetrics = writer.clone();
        writer.publish_frame(frame(55, 50), 5);
        let snap: HostState = reader.snapshot(6, 50);
        assert_eq!(snap.status, Status::Fresh);
        assert_eq!(latest_cpu(&snap), Some(55));
    }

    #[test]
    fn a_reader_survives_a_writer_that_poisoned_the_lock() {
        // The panic-isolation guarantee: a holder that panics while holding the cache poisons
        // the Mutex. A reader must step over that poison and still read the value that was
        // there, never inherit the panic.
        let shared: SharedMetrics = SharedMetrics::new();
        shared.publish_frame(frame(42, 42), 0);

        let poisoner: SharedMetrics = shared.clone();
        let panicked: std::thread::Result<()> = std::thread::spawn(move || {
            let _held: MutexGuard<'_, Inner> = poisoner.slot.lock().unwrap();
            panic!("poller thread died holding the cache");
        })
        .join();
        assert!(panicked.is_err(), "the helper thread must have panicked");

        // Lock is now poisoned; the reader recovers rather than propagating.
        assert_eq!(latest_cpu(&shared.snapshot(0, 50)), Some(42));
        // And the cache is still usable afterwards — a fresh write goes through.
        shared.publish_frame(frame(80, 80), 10);
        assert_eq!(latest_cpu(&shared.snapshot(11, 50)), Some(80));
    }
}
