//! Freshness — the staleness policy for a cached moisture reading.
//!
//! The plant monitor caches its latest [`Measurement`] in a slot the display and
//! the native-API server read (the imperative shell owns that slot; the policy
//! here stays pure). A cache alone is a hazard: a sensor that dies mid-run would
//! keep serving its last healthy value forever, and Home Assistant would graph a
//! flat line over a dead probe. This module defines *when a cached reading has
//! gone stale* — a pure function of the reading's age against a bound — so a dead
//! sensor becomes visibly unavailable rather than silently frozen.
//!
//! Pure and `no_std`: the shell supplies the timestamps (a monotonic tick) and
//! calls [`fresh`]; the whole staleness rule is exercised on the host with plain
//! integers, no clock and no thread.

use crate::moisture::Measurement;

/// A monotonic timestamp, in the caller's own unit.
///
/// Only *differences* between ticks carry meaning, so the unit and origin are the
/// shell's to choose — it uses milliseconds since boot. This policy compares
/// ticks and never interprets them, so any monotonic source works.
pub type Tick = u64;

/// A [`Measurement`] stamped with the [`Tick`] it was taken at.
///
/// The unit stored in the shared slot: the measurement (raw count and calibrated
/// percent) plus *when* it was taken, so a reader can decide whether it is still
/// fresh (see [`fresh`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reading {
    /// The measurement taken — raw ADC count and calibrated percent.
    pub measurement: Measurement,
    /// The tick the reading was taken at.
    pub at: Tick,
}

impl Reading {
    /// A reading of `measurement` taken at tick `at`.
    pub const fn new(measurement: Measurement, at: Tick) -> Self {
        Self { measurement, at }
    }
}

/// The cached measurement if it is still fresh as of `now`, else `None`.
///
/// A reading is fresh while its age — `now - at`, computed with a saturating
/// subtraction so a clock that steps backwards reads as age `0` rather than a
/// huge age — does not exceed `max_age`. Both a never-written slot (`last` is
/// `None`) and a stale one surface as `None`: to a consumer, "never measured" and
/// "not measured recently enough" are the same unavailable state, and a dead
/// sensor must never keep reporting its last healthy value.
///
/// Pure and total: the same `(last, now, max_age)` always yields the same result,
/// and no input can panic.
pub fn fresh(last: Option<Reading>, now: Tick, max_age: Tick) -> Option<Measurement> {
    let reading: Reading = last?;
    let age: Tick = now.saturating_sub(reading.at);
    if age <= max_age {
        Some(reading.measurement)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moisture::{Moisture, RAW_MAX};
    use proptest::prelude::*;

    /// A representative measurement for the example tests — the exact raw/percent
    /// are irrelevant here; freshness turns on the *timestamp*, not the value.
    const SOME_MOISTURE: Moisture = match Moisture::new(42) {
        Some(m) => m,
        None => unreachable!(),
    };
    const SOME_MEASUREMENT: Measurement = Measurement::new(1234, SOME_MOISTURE);

    #[test]
    fn a_never_written_slot_is_unavailable() {
        // Zero readings: nothing has been measured, so nothing is fresh.
        assert_eq!(fresh(None, 100, 50), None);
    }

    #[test]
    fn a_recent_reading_is_available() {
        // age = 20 - 10 = 10, within the 50-tick bound.
        let reading: Reading = Reading::new(SOME_MEASUREMENT, 10);
        assert_eq!(fresh(Some(reading), 20, 50), Some(SOME_MEASUREMENT));
    }

    #[test]
    fn a_reading_at_exactly_the_bound_is_still_available() {
        // age = 50, max_age = 50: the boundary is inclusive — one tick of slack
        // decides whether a live-but-slow sensor flickers to unavailable.
        let reading: Reading = Reading::new(SOME_MEASUREMENT, 0);
        assert_eq!(fresh(Some(reading), 50, 50), Some(SOME_MEASUREMENT));
    }

    #[test]
    fn a_reading_past_the_bound_is_unavailable() {
        // age = 51, one tick past max_age = 50: the dead-sensor case.
        let reading: Reading = Reading::new(SOME_MEASUREMENT, 0);
        assert_eq!(fresh(Some(reading), 51, 50), None);
    }

    #[test]
    fn a_backwards_clock_reads_as_fresh_not_ancient() {
        // now (50) is *before* the reading (100): a naive `now - at` would wrap
        // to a huge age and hide a perfectly recent reading. Saturating to age 0
        // keeps it available.
        let reading: Reading = Reading::new(SOME_MEASUREMENT, 100);
        assert_eq!(fresh(Some(reading), 50, 10), Some(SOME_MEASUREMENT));
    }

    proptest! {
        /// The rule, stated directly: a present reading is available exactly when
        /// its saturating age is within the bound, and when available it is the
        /// stored measurement (raw and all) unchanged.
        #[test]
        fn available_iff_age_within_bound(
            raw in 0u16..=RAW_MAX,
            moisture_pct in 0u8..=100,
            at in any::<Tick>(),
            now in any::<Tick>(),
            max_age in any::<Tick>(),
        ) {
            let measurement: Measurement =
                Measurement::new(raw, Moisture::new(moisture_pct).unwrap());
            let reading: Reading = Reading::new(measurement, at);
            let age: Tick = now.saturating_sub(at);
            let got: Option<Measurement> = fresh(Some(reading), now, max_age);
            if age <= max_age {
                prop_assert_eq!(got, Some(measurement));
            } else {
                prop_assert_eq!(got, None);
            }
        }

        /// A never-written slot is unavailable for every clock and bound.
        #[test]
        fn a_missing_reading_is_never_available(now in any::<Tick>(), max_age in any::<Tick>()) {
            prop_assert_eq!(fresh(None, now, max_age), None);
        }
    }
}
