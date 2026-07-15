//! SharedMoisture — the latest sampling outcome, shared writer-to-readers.
//!
//! One slot, one writer (the sampler thread), many readers (the display, the
//! native-API server). The slot holds an [`Option<Reading>`]: what the last cycle
//! concluded — a measurement, *or the fault that replaced it* — with the tick it
//! concluded at, so a reader can apply the pure staleness rule ([`observe`]) and
//! learn both whether the writer is alive and whether the probe is honest.
//!
//! The writer publishes on **every** cycle, faults included. That is what keeps the
//! two facts separable: a fresh fault proves the sampler ran, so
//! [`Observation::Stale`] can mean exactly one thing — the writer stopped.
//!
//! Every access recovers from a poisoned lock. If the writer — or any reader —
//! panics while holding the slot, a plain `lock().unwrap()` elsewhere would
//! propagate that panic and take the panicking thread's peers down with it. A
//! plant monitor must not let a sampler hiccup crash the server thread, so every
//! lock here steps over the poison and reads the value that was there: the cache
//! survives, and staleness ([`observe`]) still retires a value the dead writer can
//! no longer refresh.

use std::sync::{Arc, Mutex, MutexGuard};

use plant_core::{observe, Observation, Outcome, Reading, Tick};

/// The latest soil-moisture reading, shared between the sampler and its
/// consumers.
///
/// Cloning shares the *same* slot (an [`Arc`]); clones are how the sampler thread
/// and each reader hold the one cache. Reads and writes are non-blocking beyond
/// the brief lock, and poison-tolerant (see the module docs).
#[derive(Clone)]
pub struct SharedMoisture {
    slot: Arc<Mutex<Option<Reading>>>,
}

impl SharedMoisture {
    /// An empty cache — nothing measured yet, so every read is unavailable until
    /// the first [`publish`](Self::publish).
    pub fn new() -> Self {
        Self {
            slot: Arc::new(Mutex::new(None)),
        }
    }

    /// Store `outcome` as the latest reading, stamped at `now` (the writer).
    ///
    /// Overwrites any previous reading: the cache holds only the newest. Called on
    /// **every** sample — a successful one, an unchanged one, *and a faulted one* —
    /// so the timestamp keeps advancing and staleness stays a statement about the
    /// writer's liveness rather than about the probe's health.
    pub fn publish(&self, outcome: Outcome, now: Tick) {
        *self.guard() = Some(Reading::new(outcome, now));
    }

    /// What the cache can honestly report as of `now` (a reader: the display, the
    /// server).
    ///
    /// Delegates the decision to the pure [`observe`] policy. The four outcomes are
    /// distinct on purpose: a fresh [`Observation::Faulted`] means the sampler is
    /// alive and the probe is lying, whereas [`Observation::Stale`] means the
    /// sampler itself stopped. A consumer that can only render a number calls
    /// [`Observation::measurement`]; one that can explain itself — the display, a
    /// fault entity — matches on the observation.
    pub fn observe(&self, now: Tick, max_age: Tick) -> Observation {
        observe(*self.guard(), now, max_age)
    }

    /// Lock the slot, stepping over a poisoned lock left by a panicking holder.
    ///
    /// The recovered value is exactly the one the panicking thread left behind —
    /// the cache is a single overwrite-in-place slot, so there is no torn state to
    /// distrust — and readers must not inherit a writer's panic.
    fn guard(&self) -> MutexGuard<'_, Option<Reading>> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for SharedMoisture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_core::{Measurement, Moisture, ProbeFault};

    /// A representative measurement at `percent`, with a raw count derived from it
    /// (`raw = percent * 10`) so a test can confirm the raw rides through the cache.
    /// The exact values are otherwise irrelevant — these tests turn on presence,
    /// staleness, and lock recovery.
    fn measurement(percent: u8) -> Measurement {
        Measurement::new(
            u16::from(percent) * 10,
            Moisture::new(percent).expect("test percent is 0..=100"),
        )
    }

    #[test]
    fn an_empty_cache_has_never_been_sampled() {
        // Zero publishes: nothing measured, and the cache says exactly that rather
        // than pretending the writer died.
        let shared: SharedMoisture = SharedMoisture::new();
        assert_eq!(shared.observe(100, 50), Observation::NeverSampled);
    }

    #[test]
    fn a_fresh_publish_is_served_with_its_raw_and_percent() {
        // One publish, read within the bound: age = 20 - 10 = 10 <= 50. Both the
        // raw count and the percent survive the round-trip through the cache.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 10);
        assert_eq!(shared.observe(20, 50), Observation::Fresh(measurement(30)));
        let served: Measurement = shared.observe(20, 50).measurement().expect("fresh");
        assert_eq!(served.raw(), 300, "the raw count rides through the cache");
        assert_eq!(served.percent(), 30);
    }

    #[test]
    fn a_stale_publish_is_hidden() {
        // One publish, read past the bound: age = 51 > 50 — the dead-writer case,
        // decided by the timestamp, not the value.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 0);
        assert_eq!(shared.observe(51, 50), Observation::Stale);
    }

    /// The regression guard, at the cache level. A published fault keeps the slot
    /// fresh, so a lying probe reports as `Faulted` — provably *not* as the `Stale`
    /// a dead sampler thread would produce.
    #[test]
    fn a_published_fault_is_faulted_and_proves_the_writer_ran() {
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Err(ProbeFault::OverRange), 10);
        let observed: Observation = shared.observe(20, 50);
        assert_eq!(observed, Observation::Faulted(ProbeFault::OverRange));
        assert!(observed.writer_is_alive());
        assert_eq!(observed.measurement(), None, "a fault serves no number");
    }

    #[test]
    fn a_fault_that_ages_out_becomes_stale() {
        // The writer published a fault and then died: past the bound we can no
        // longer claim anything about the probe, only about the writer.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Err(ProbeFault::Unreadable), 0);
        let observed: Observation = shared.observe(51, 50);
        assert_eq!(observed, Observation::Stale);
        assert!(!observed.writer_is_alive());
    }

    #[test]
    fn a_fault_supersedes_an_earlier_measurement() {
        // A probe that was healthy and then failed: the cache reports the failure,
        // never the last healthy value.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(30)), 0);
        shared.publish(Err(ProbeFault::OverRange), 10);
        assert_eq!(
            shared.observe(11, 50),
            Observation::Faulted(ProbeFault::OverRange)
        );
    }

    #[test]
    fn a_measurement_supersedes_an_earlier_fault() {
        // And a probe that recovers is believed again, without a restart.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Err(ProbeFault::OverRange), 0);
        shared.publish(Ok(measurement(70)), 10);
        assert_eq!(shared.observe(11, 50), Observation::Fresh(measurement(70)));
    }

    #[test]
    fn a_later_publish_supersedes_an_earlier_one() {
        // Many publishes: only the newest survives, with its own timestamp.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(20)), 0);
        shared.publish(Ok(measurement(70)), 10);
        assert_eq!(shared.observe(11, 50), Observation::Fresh(measurement(70)));
    }

    #[test]
    fn a_clone_shares_the_one_slot() {
        // The sampler and a reader hold clones; a write through one is visible
        // through the other.
        let writer: SharedMoisture = SharedMoisture::new();
        let reader: SharedMoisture = writer.clone();
        writer.publish(Ok(measurement(55)), 5);
        assert_eq!(reader.observe(6, 50), Observation::Fresh(measurement(55)));
    }

    #[test]
    fn a_reader_survives_a_writer_that_poisoned_the_lock() {
        // The panic-isolation guarantee: a holder that panics *while holding the
        // slot* poisons the Mutex. A reader must step over that poison and still
        // read the value that was there, never inherit the panic.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(Ok(measurement(42)), 0);

        let poisoner: SharedMoisture = shared.clone();
        let panicked: std::thread::Result<()> = std::thread::spawn(move || {
            let _held: MutexGuard<'_, Option<Reading>> = poisoner.slot.lock().unwrap();
            panic!("sampler thread died holding the slot");
        })
        .join();
        assert!(panicked.is_err(), "the helper thread must have panicked");

        // Lock is now poisoned; the reader recovers rather than propagating.
        assert_eq!(shared.observe(0, 50), Observation::Fresh(measurement(42)));
        // And the cache is still usable afterwards — a fresh write goes through.
        shared.publish(Ok(measurement(80)), 10);
        assert_eq!(shared.observe(11, 50), Observation::Fresh(measurement(80)));
    }
}
