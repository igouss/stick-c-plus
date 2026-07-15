//! Freshness — the staleness policy for the cached soil outcome.
//!
//! The plant monitor caches its latest sampling outcome in a slot the display and
//! the native-API server read (the imperative shell owns that slot; the policy here
//! stays pure). A cache alone is a hazard: a sensor that dies mid-run would keep
//! serving its last healthy value forever, and Home Assistant would graph a flat
//! line over a dead probe. This module decides *when a cached outcome has gone
//! stale* — a pure function of its age against a bound.
//!
//! What is cached is an **outcome**, not a measurement: `Ok(Measurement)` when the
//! probe was honest, `Err(ProbeFault)` when it was not. The sampler publishes on
//! every cycle, so a fault keeps the slot fresh. That is what lets [`observe`]
//! answer two questions at once — see [`Observation`] for why one `Option` could
//! not.
//!
//! Pure and `no_std`: the shell supplies the timestamps (a monotonic tick) and
//! calls [`observe`]; the whole staleness rule is exercised on the host with plain
//! integers, no clock and no thread.

use crate::moisture::Measurement;
use crate::observation::{Observation, ProbeFault};

/// A monotonic timestamp, in the caller's own unit.
///
/// Only *differences* between ticks carry meaning, so the unit and origin are the
/// shell's to choose — it uses milliseconds since boot. This policy compares ticks
/// and never interprets them, so any monotonic source works.
pub type Tick = u64;

/// What one sampling cycle produced: a measurement, or the fault that replaced it.
///
/// `Err` is not an error in the control-flow sense — it is a *published verdict*
/// about the probe, and it is as much evidence that the sampler ran as an `Ok` is.
pub type Outcome = Result<Measurement, ProbeFault>;

/// An [`Outcome`] stamped with the [`Tick`] it was produced at.
///
/// The unit stored in the shared slot: what the cycle concluded, plus *when*, so a
/// reader can decide whether it is still fresh (see [`observe`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reading {
    /// What the sampling cycle concluded — a measurement, or a probe fault.
    pub outcome: Outcome,
    /// The tick the cycle ran at.
    pub at: Tick,
}

impl Reading {
    /// A reading of `outcome` taken at tick `at`.
    pub const fn new(outcome: Outcome, at: Tick) -> Self {
        Self { outcome, at }
    }

    /// A successful measurement taken at tick `at` — the common case, spelled out.
    pub const fn measured(measurement: Measurement, at: Tick) -> Self {
        Self::new(Ok(measurement), at)
    }

    /// A published fault at tick `at`.
    pub const fn faulted(fault: ProbeFault, at: Tick) -> Self {
        Self::new(Err(fault), at)
    }
}

/// Decide what the cache can honestly report as of `now`.
///
/// A reading is fresh while its age — `now - at`, computed with a saturating
/// subtraction so a clock that steps backwards reads as age `0` rather than a huge
/// age — does not exceed `max_age`. Then:
///
/// - a never-written slot is [`Observation::NeverSampled`];
/// - an aged-out slot is [`Observation::Stale`], *whatever it holds* — once the
///   writer has stopped, its last verdict about the probe is no longer evidence
///   about the probe now, so a stale fault is simply stale;
/// - a fresh `Ok` is [`Observation::Fresh`];
/// - a fresh `Err` is [`Observation::Faulted`] — the sampler is alive and the probe
///   is lying, which is a different fact from either of the two above.
///
/// Pure and total: the same `(last, now, max_age)` always yields the same result,
/// and no input can panic.
pub fn observe(last: Option<Reading>, now: Tick, max_age: Tick) -> Observation {
    let Some(reading) = last else {
        return Observation::NeverSampled;
    };

    let age: Tick = now.saturating_sub(reading.at);
    if age > max_age {
        return Observation::Stale;
    }

    match reading.outcome {
        Ok(measurement) => Observation::Fresh(measurement),
        Err(fault) => Observation::Faulted(fault),
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
    fn a_never_written_slot_has_never_been_sampled() {
        // Zero readings: nothing has been measured, and that is its own state —
        // distinct from a sampler that ran and then died.
        assert_eq!(observe(None, 100, 50), Observation::NeverSampled);
    }

    #[test]
    fn a_recent_measurement_is_fresh() {
        // age = 20 - 10 = 10, within the 50-tick bound.
        let reading: Reading = Reading::measured(SOME_MEASUREMENT, 10);
        assert_eq!(
            observe(Some(reading), 20, 50),
            Observation::Fresh(SOME_MEASUREMENT)
        );
    }

    #[test]
    fn a_recent_fault_is_faulted_not_stale() {
        // The regression guard, at the policy level: a probe that just reported a
        // fault must not look like a sampler that stopped reporting.
        let reading: Reading = Reading::faulted(ProbeFault::OverRange, 10);
        assert_eq!(
            observe(Some(reading), 20, 50),
            Observation::Faulted(ProbeFault::OverRange)
        );
    }

    #[test]
    fn a_reading_at_exactly_the_bound_is_still_fresh() {
        // age = 50, max_age = 50: the boundary is inclusive — one tick of slack
        // decides whether a live-but-slow sensor flickers to unavailable.
        let reading: Reading = Reading::measured(SOME_MEASUREMENT, 0);
        assert_eq!(
            observe(Some(reading), 50, 50),
            Observation::Fresh(SOME_MEASUREMENT)
        );
    }

    #[test]
    fn a_measurement_past_the_bound_is_stale() {
        // age = 51, one tick past max_age = 50: the dead-sampler case.
        let reading: Reading = Reading::measured(SOME_MEASUREMENT, 0);
        assert_eq!(observe(Some(reading), 51, 50), Observation::Stale);
    }

    #[test]
    fn a_fault_past_the_bound_is_stale_not_faulted() {
        // A dead sampler that happened to publish a fault last is *stale*: its
        // verdict about the probe has expired along with its liveness.
        let reading: Reading = Reading::faulted(ProbeFault::Unreadable, 0);
        assert_eq!(observe(Some(reading), 51, 50), Observation::Stale);
    }

    #[test]
    fn a_backwards_clock_reads_as_fresh_not_ancient() {
        // now (50) is *before* the reading (100): a naive `now - at` would wrap to
        // a huge age and hide a perfectly recent reading. Saturating to age 0 keeps
        // it fresh.
        let reading: Reading = Reading::measured(SOME_MEASUREMENT, 100);
        assert_eq!(
            observe(Some(reading), 50, 10),
            Observation::Fresh(SOME_MEASUREMENT)
        );
    }

    /// Every `ProbeFault`, so a property test can range over the whole enum.
    fn any_fault() -> impl Strategy<Value = ProbeFault> {
        prop_oneof![
            Just(ProbeFault::OverRange),
            Just(ProbeFault::UnderRange),
            Just(ProbeFault::Unreadable),
        ]
    }

    fn any_measurement() -> impl Strategy<Value = Measurement> {
        (0u16..=RAW_MAX, 0u8..=100).prop_map(|(raw, pct): (u16, u8)| {
            Measurement::new(raw, Moisture::new(pct).expect("percent is 0..=100"))
        })
    }

    proptest! {
        /// The rule for a successful reading, stated directly: fresh exactly when
        /// its saturating age is within the bound, carrying the stored measurement
        /// unchanged; stale otherwise.
        #[test]
        fn a_measurement_is_fresh_iff_within_the_bound(
            measurement in any_measurement(),
            at in any::<Tick>(),
            now in any::<Tick>(),
            max_age in any::<Tick>(),
        ) {
            let age: Tick = now.saturating_sub(at);
            let got: Observation = observe(Some(Reading::measured(measurement, at)), now, max_age);
            if age <= max_age {
                prop_assert_eq!(got, Observation::Fresh(measurement));
            } else {
                prop_assert_eq!(got, Observation::Stale);
            }
        }

        /// The same rule for a fault, and the invariant that matters: within the
        /// bound a fault is *always* `Faulted`, never `Stale`.
        #[test]
        fn a_fault_is_faulted_iff_within_the_bound(
            fault in any_fault(),
            at in any::<Tick>(),
            now in any::<Tick>(),
            max_age in any::<Tick>(),
        ) {
            let age: Tick = now.saturating_sub(at);
            let got: Observation = observe(Some(Reading::faulted(fault, at)), now, max_age);
            if age <= max_age {
                prop_assert_eq!(got, Observation::Faulted(fault));
            } else {
                prop_assert_eq!(got, Observation::Stale);
            }
        }

        /// Liveness follows freshness, not success: any reading within the bound —
        /// measurement or fault — proves the writer ran, and any reading past it
        /// proves nothing. This is the property the old `Option` API could not state.
        #[test]
        fn freshness_alone_decides_writer_liveness(
            outcome in prop_oneof![any_measurement().prop_map(Ok), any_fault().prop_map(Err)],
            at in any::<Tick>(),
            now in any::<Tick>(),
            max_age in any::<Tick>(),
        ) {
            let age: Tick = now.saturating_sub(at);
            let got: Observation = observe(Some(Reading::new(outcome, at)), now, max_age);
            prop_assert_eq!(got.writer_is_alive(), age <= max_age);
        }

        /// A never-written slot is `NeverSampled` for every clock and bound — and
        /// never claims the writer is alive.
        #[test]
        fn a_missing_reading_is_never_sampled(now in any::<Tick>(), max_age in any::<Tick>()) {
            prop_assert_eq!(observe(None, now, max_age), Observation::NeverSampled);
            prop_assert!(!observe(None, now, max_age).writer_is_alive());
        }
    }
}
