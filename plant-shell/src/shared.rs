//! SharedMoisture — the latest reading, shared writer-to-readers.
//!
//! One slot, one writer (the sampler thread), many readers (the display, the
//! native-API server). The slot holds an [`Option<Reading>`]: the latest
//! measurement (raw count and calibrated percent) *with the tick it was taken at*,
//! so a reader can apply the pure staleness rule
//! ([`fresh`]) and treat an aged-out reading as unavailable.
//!
//! Every access recovers from a poisoned lock. If the writer — or any reader —
//! panics while holding the slot, a plain `lock().unwrap()` elsewhere would
//! propagate that panic and take the panicking thread's peers down with it. A
//! plant monitor must not let a sampler hiccup crash the server thread, so every
//! lock here steps over the poison and reads the value that was there: the cache
//! survives, and staleness ([`fresh`]) still retires a value the dead writer can
//! no longer refresh.

use std::sync::{Arc, Mutex, MutexGuard};

use plant_core::{fresh, Measurement, Reading, Tick};

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

    /// Store `measurement` as the latest reading, stamped at `now` (the writer).
    ///
    /// Overwrites any previous reading: the cache holds only the newest. Called on
    /// every successful sample — even when the value is unchanged — so the
    /// timestamp keeps advancing and a live-but-steady sensor stays fresh.
    pub fn publish(&self, measurement: Measurement, now: Tick) {
        *self.guard() = Some(Reading::new(measurement, now));
    }

    /// The latest measurement if it is still fresh as of `now`, else `None` (a
    /// reader: the display, the server).
    ///
    /// Delegates the freshness decision to the pure [`fresh`] policy: `None` means
    /// either nothing was ever measured or the last reading is older than
    /// `max_age` — to a consumer, both are the same "unavailable" state. The
    /// measurement carries both the raw ADC count and the calibrated percent, so a
    /// reader (the display) can show either; the native-API server takes the
    /// percent.
    pub fn latest(&self, now: Tick, max_age: Tick) -> Option<Measurement> {
        fresh(*self.guard(), now, max_age)
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
    use plant_core::Moisture;

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
    fn an_empty_cache_is_unavailable() {
        // Zero publishes: nothing measured, so nothing to serve.
        let shared: SharedMoisture = SharedMoisture::new();
        assert_eq!(shared.latest(100, 50), None);
    }

    #[test]
    fn a_fresh_publish_is_served_with_its_raw_and_percent() {
        // One publish, read within the bound: age = 20 - 10 = 10 <= 50. Both the
        // raw count and the percent survive the round-trip through the cache.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(30), 10);
        assert_eq!(shared.latest(20, 50), Some(measurement(30)));
        let served: Measurement = shared.latest(20, 50).expect("fresh");
        assert_eq!(served.raw(), 300, "the raw count rides through the cache");
        assert_eq!(served.percent(), 30);
    }

    #[test]
    fn a_stale_publish_is_hidden() {
        // One publish, read past the bound: age = 51 > 50, so unavailable — the
        // dead-writer case, decided by the timestamp, not the value.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(30), 0);
        assert_eq!(shared.latest(51, 50), None);
    }

    #[test]
    fn a_later_publish_supersedes_an_earlier_one() {
        // Many publishes: only the newest survives, with its own timestamp.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(20), 0);
        shared.publish(measurement(70), 10);
        assert_eq!(shared.latest(11, 50), Some(measurement(70)));
    }

    #[test]
    fn a_clone_shares_the_one_slot() {
        // The sampler and a reader hold clones; a write through one is visible
        // through the other.
        let writer: SharedMoisture = SharedMoisture::new();
        let reader: SharedMoisture = writer.clone();
        writer.publish(measurement(55), 5);
        assert_eq!(reader.latest(6, 50), Some(measurement(55)));
    }

    #[test]
    fn a_reader_survives_a_writer_that_poisoned_the_lock() {
        // The panic-isolation guarantee: a holder that panics *while holding the
        // slot* poisons the Mutex. A reader must step over that poison and still
        // read the value that was there, never inherit the panic.
        let shared: SharedMoisture = SharedMoisture::new();
        shared.publish(measurement(42), 0);

        let poisoner: SharedMoisture = shared.clone();
        let panicked: std::thread::Result<()> = std::thread::spawn(move || {
            let _held: MutexGuard<'_, Option<Reading>> = poisoner.slot.lock().unwrap();
            panic!("sampler thread died holding the slot");
        })
        .join();
        assert!(panicked.is_err(), "the helper thread must have panicked");

        // Lock is now poisoned; the reader recovers rather than propagating.
        assert_eq!(shared.latest(0, 50), Some(measurement(42)));
        // And the cache is still usable afterwards — a fresh write goes through.
        shared.publish(measurement(80), 10);
        assert_eq!(shared.latest(11, 50), Some(measurement(80)));
    }
}
