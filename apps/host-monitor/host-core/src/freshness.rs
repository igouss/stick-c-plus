//! Freshness — the staleness policy for the cached host status.
//!
//! The host monitor caches its latest scrape outcome in a slot the display reads (the
//! imperative shell owns the slot; the policy here stays pure). A cache alone is a
//! hazard: a host that goes dark mid-run would keep showing its last percentages
//! forever, and the graph would flatline over a machine that is off. This module
//! decides *when a cached outcome has gone stale* — a pure function of its age against
//! a bound.
//!
//! What is cached is an **outcome**, not a bare sample: `Ok(Sample)` when the host
//! answered, `Err(HostFault)` when it did not. The poller publishes on every cycle, so
//! a fault keeps the slot fresh. That is what lets [`observe`] answer two questions at
//! once — see [`Status`] for why one `Option` could not.
//!
//! Pure and `no_std`: the shell supplies the timestamps (a monotonic tick) and calls
//! [`observe`]; the whole rule is exercised on the host with plain integers, no clock
//! and no thread.

use crate::sample::Sample;
use crate::status::{HostFault, Status};

/// A monotonic timestamp, in the caller's own unit.
///
/// Only *differences* between ticks carry meaning, so the unit and origin are the
/// shell's to choose — it uses milliseconds since boot. This policy compares ticks and
/// never interprets them, so any monotonic source works.
pub type Tick = u64;

/// What one poll cycle produced: a sample, or the fault that replaced it.
///
/// `Err` is not an error in the control-flow sense — it is a *published verdict* about
/// the host, and it is as much evidence that the poller ran as an `Ok` is.
pub type Outcome = Result<Sample, HostFault>;

/// An [`Outcome`] stamped with the [`Tick`] it was produced at.
///
/// The unit stored in the shared slot: what the cycle concluded, plus *when*, so a
/// reader can decide whether it is still fresh (see [`observe`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reading {
    /// What the poll cycle concluded — a sample, or a host fault.
    pub outcome: Outcome,
    /// The tick the cycle ran at.
    pub at: Tick,
}

impl Reading {
    /// A reading of `outcome` taken at tick `at`.
    pub const fn new(outcome: Outcome, at: Tick) -> Self {
        Self { outcome, at }
    }

    /// A successful sample taken at tick `at` — the common case, spelled out.
    pub const fn sampled(sample: Sample, at: Tick) -> Self {
        Self::new(Ok(sample), at)
    }

    /// A published fault at tick `at`.
    pub const fn faulted(fault: HostFault, at: Tick) -> Self {
        Self::new(Err(fault), at)
    }
}

/// Decide what the cache can honestly report as of `now`.
///
/// A reading is fresh while its age — `now - at`, computed with a saturating
/// subtraction so a clock that steps backwards reads as age `0` rather than a huge age
/// — does not exceed `max_age`. Then:
///
/// - a never-written slot is [`Status::NeverSampled`];
/// - an aged-out slot is [`Status::Stale`], *whatever it holds* — once the poller has
///   stopped, its last verdict about the host is no longer evidence about the host now;
/// - a fresh `Ok` is [`Status::Fresh`];
/// - a fresh `Err` is [`Status::Faulted`] — the poller is alive and the host did not
///   answer, which is a different fact from either of the two above.
///
/// Pure and total: the same `(last, now, max_age)` always yields the same result, and
/// no input can panic.
pub fn observe(last: Option<Reading>, now: Tick, max_age: Tick) -> Status {
    let Some(reading) = last else {
        return Status::NeverSampled;
    };

    let age: Tick = now.saturating_sub(reading.at);
    if age > max_age {
        return Status::Stale;
    }

    match reading.outcome {
        Ok(sample) => Status::Fresh(sample),
        Err(fault) => Status::Faulted(fault),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::percent::Percent;
    use proptest::prelude::*;

    /// A representative sample for the example tests — freshness turns on the
    /// *timestamp*, not the values.
    const SOME_SAMPLE: Sample = Sample::new(Percent::FULL, Percent::ZERO);

    #[test]
    fn a_never_written_slot_has_never_been_sampled() {
        assert_eq!(observe(None, 100, 50), Status::NeverSampled);
    }

    #[test]
    fn a_recent_sample_is_fresh() {
        // age = 20 - 10 = 10, within the 50-tick bound.
        let reading: Reading = Reading::sampled(SOME_SAMPLE, 10);
        assert_eq!(observe(Some(reading), 20, 50), Status::Fresh(SOME_SAMPLE));
    }

    #[test]
    fn a_recent_fault_is_faulted_not_stale() {
        // The regression guard, at the policy level: a host that just failed to answer
        // must not look like a poller that stopped.
        let reading: Reading = Reading::faulted(HostFault::Unreachable, 10);
        assert_eq!(
            observe(Some(reading), 20, 50),
            Status::Faulted(HostFault::Unreachable)
        );
    }

    #[test]
    fn a_reading_at_exactly_the_bound_is_still_fresh() {
        // age = 50, max_age = 50: the boundary is inclusive.
        let reading: Reading = Reading::sampled(SOME_SAMPLE, 0);
        assert_eq!(observe(Some(reading), 50, 50), Status::Fresh(SOME_SAMPLE));
    }

    #[test]
    fn a_sample_past_the_bound_is_stale() {
        // age = 51, one tick past max_age = 50: the dead-poller case.
        let reading: Reading = Reading::sampled(SOME_SAMPLE, 0);
        assert_eq!(observe(Some(reading), 51, 50), Status::Stale);
    }

    #[test]
    fn a_fault_past_the_bound_is_stale_not_faulted() {
        let reading: Reading = Reading::faulted(HostFault::Malformed, 0);
        assert_eq!(observe(Some(reading), 51, 50), Status::Stale);
    }

    #[test]
    fn a_backwards_clock_reads_as_fresh_not_ancient() {
        // now (50) is before the reading (100): saturating to age 0 keeps it fresh.
        let reading: Reading = Reading::sampled(SOME_SAMPLE, 100);
        assert_eq!(observe(Some(reading), 50, 10), Status::Fresh(SOME_SAMPLE));
    }

    fn any_fault() -> impl Strategy<Value = HostFault> {
        prop_oneof![Just(HostFault::Unreachable), Just(HostFault::Malformed)]
    }

    fn any_sample() -> impl Strategy<Value = Sample> {
        (0u8..=100, 0u8..=100).prop_map(|(cpu, mem): (u8, u8)| {
            Sample::new(
                Percent::new(cpu).expect("0..=100"),
                Percent::new(mem).expect("0..=100"),
            )
        })
    }

    proptest! {
        /// A sample is fresh exactly when its saturating age is within the bound,
        /// carrying the stored value unchanged; stale otherwise.
        #[test]
        fn a_sample_is_fresh_iff_within_the_bound(
            sample in any_sample(),
            at in any::<Tick>(),
            now in any::<Tick>(),
            max_age in any::<Tick>(),
        ) {
            let age: Tick = now.saturating_sub(at);
            let got: Status = observe(Some(Reading::sampled(sample, at)), now, max_age);
            if age <= max_age {
                prop_assert_eq!(got, Status::Fresh(sample));
            } else {
                prop_assert_eq!(got, Status::Stale);
            }
        }

        /// The same rule for a fault, and the invariant that matters: within the
        /// bound a fault is always `Faulted`, never `Stale`.
        #[test]
        fn a_fault_is_faulted_iff_within_the_bound(
            fault in any_fault(),
            at in any::<Tick>(),
            now in any::<Tick>(),
            max_age in any::<Tick>(),
        ) {
            let age: Tick = now.saturating_sub(at);
            let got: Status = observe(Some(Reading::faulted(fault, at)), now, max_age);
            if age <= max_age {
                prop_assert_eq!(got, Status::Faulted(fault));
            } else {
                prop_assert_eq!(got, Status::Stale);
            }
        }

        /// Liveness follows freshness, not success: any reading within the bound —
        /// sample or fault — proves the poller ran, and any reading past it proves
        /// nothing.
        #[test]
        fn freshness_alone_decides_poller_liveness(
            outcome in prop_oneof![any_sample().prop_map(Ok), any_fault().prop_map(Err)],
            at in any::<Tick>(),
            now in any::<Tick>(),
            max_age in any::<Tick>(),
        ) {
            let age: Tick = now.saturating_sub(at);
            let got: Status = observe(Some(Reading::new(outcome, at)), now, max_age);
            prop_assert_eq!(got.poller_is_alive(), age <= max_age);
        }

        /// A never-written slot is `NeverSampled` for every clock and bound.
        #[test]
        fn a_missing_reading_is_never_sampled(now in any::<Tick>(), max_age in any::<Tick>()) {
            prop_assert_eq!(observe(None, now, max_age), Status::NeverSampled);
            prop_assert!(!observe(None, now, max_age).poller_is_alive());
        }
    }
}
