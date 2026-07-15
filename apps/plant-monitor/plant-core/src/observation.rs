//! Observation — what a consumer learns when it asks the cache for the soil.
//!
//! There are two independent questions to answer about a plant monitor, and a
//! single `Option<Measurement>` can only answer one of them:
//!
//! 1. **Is the writer alive?** Has anything been sampled recently?
//! 2. **Is the probe honest?** Did the last sample carry information?
//!
//! Collapsing both into `None` throws away exactly the fact an operator needs. A
//! probe whose electrodes have corroded open and a sampler thread that has panicked
//! both present as "no fresh reading" — yet one is a dead plant sensor on a healthy
//! device, and the other is a dead device. Distinguishing them cost a USB cable, a
//! board reset and a serial capture on 2026-07-08; it should cost one field.
//!
//! [`Observation`] carries both axes. A *fresh fault* says the sampler is running
//! and the probe is lying; [`Observation::Stale`] then recovers its precise
//! meaning — the writer stopped. Every consumer (the TFT, the native-API sensor,
//! the serial heartbeat) renders the same value instead of each re-deriving a worse
//! answer from an `Option`.
//!
//! Pure and `no_std`, like the rest of the core.

use core::fmt;

use crate::moisture::Measurement;

/// Why a soil reading carried no information.
///
/// The names describe *what was observed*, not what caused it, because the cause
/// is not knowable from the reading alone. A count pinned to the top of the ADC's
/// range means the probe's divider is not conducting — which is an open (corroded)
/// electrode **or** soil drier than the converter can express. Claiming
/// `OpenCircuit` would assert more than the evidence supports.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ProbeFault {
    /// The reading pinned the top of the readable range. The probe's soil leg is
    /// not conducting: electrodes corroded open, or soil beyond the range.
    OverRange,
    /// The reading pinned the bottom of the readable range. The divider is not
    /// being pulled up — most likely the probe's power rail never came up.
    UnderRange,
    /// The sensor could not be read at all: the converter, or the probe's power
    /// mechanism, failed outright.
    Unreadable,
}

impl fmt::Display for ProbeFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeFault::OverRange => f.write_str(
                "reading above the readable range — probe open, or soil drier than the ADC can express",
            ),
            ProbeFault::UnderRange => {
                f.write_str("reading below the readable range — the probe rail is likely down")
            }
            ProbeFault::Unreadable => f.write_str("the probe could not be read"),
        }
    }
}

/// What the shared cache reports about the soil, as of some instant.
///
/// The four states are mutually exclusive and total. Note that a *stale fault* is
/// simply [`Stale`](Self::Stale): once the writer has stopped, its last verdict
/// about the probe is no longer evidence about the probe *now*.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Observation {
    /// A recent, trustworthy measurement. The sampler is alive and the probe is
    /// honest.
    Fresh(Measurement),
    /// A recent *fault*. The sampler is alive; the probe is not telling the truth.
    /// This is the state a corroded probe should produce, and it is deliberately
    /// distinct from [`Stale`](Self::Stale).
    Faulted(ProbeFault),
    /// Nothing has been sampled recently enough to trust — the writer is dead,
    /// hung, or merely slower than its own staleness bound.
    Stale,
    /// Nothing has ever been sampled. The device has not completed its first cycle.
    NeverSampled,
}

impl Observation {
    /// The measurement, if this observation is a trustworthy one.
    ///
    /// The narrow accessor for consumers that can only render a number — the
    /// native-API sensor maps `None` to `missing_state`. A consumer that can say
    /// *why* should match on the observation instead of reaching for this.
    pub const fn measurement(self) -> Option<Measurement> {
        match self {
            Observation::Fresh(measurement) => Some(measurement),
            _ => None,
        }
    }

    /// The fault, if a fresh one was observed.
    pub const fn fault(self) -> Option<ProbeFault> {
        match self {
            Observation::Faulted(fault) => Some(fault),
            _ => None,
        }
    }

    /// Whether the *sampler* is demonstrably alive, regardless of probe health.
    ///
    /// True for a fresh measurement **and** for a fresh fault: publishing a fault
    /// is itself proof the writer ran. This is the liveness signal that a bare
    /// `Option<Measurement>` cannot express.
    pub const fn writer_is_alive(self) -> bool {
        matches!(self, Observation::Fresh(_) | Observation::Faulted(_))
    }
}

impl fmt::Display for Observation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Observation::Fresh(m) => write!(f, "{}% (raw {})", m.percent(), m.raw()),
            Observation::Faulted(fault) => write!(f, "probe fault: {fault}"),
            Observation::Stale => f.write_str("stale — no fresh sample; the sampler may be dead"),
            Observation::NeverSampled => f.write_str("no sample taken yet"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moisture::Moisture;

    /// A representative measurement — the exact raw/percent are irrelevant here;
    /// these tests turn on which *variant* an observation is, not on its value.
    const SOME_MOISTURE: Moisture = match Moisture::new(42) {
        Some(m) => m,
        None => unreachable!(),
    };
    const SOME_MEASUREMENT: Measurement = Measurement::new(1234, SOME_MOISTURE);

    #[test]
    fn a_fresh_observation_yields_its_measurement() {
        let observation: Observation = Observation::Fresh(SOME_MEASUREMENT);
        assert_eq!(observation.measurement(), Some(SOME_MEASUREMENT));
        assert_eq!(observation.fault(), None);
    }

    #[test]
    fn a_faulted_observation_yields_its_fault_and_no_measurement() {
        let observation: Observation = Observation::Faulted(ProbeFault::OverRange);
        assert_eq!(observation.measurement(), None);
        assert_eq!(observation.fault(), Some(ProbeFault::OverRange));
    }

    #[test]
    fn a_stale_observation_yields_neither() {
        assert_eq!(Observation::Stale.measurement(), None);
        assert_eq!(Observation::Stale.fault(), None);
    }

    #[test]
    fn a_never_sampled_observation_yields_neither() {
        assert_eq!(Observation::NeverSampled.measurement(), None);
        assert_eq!(Observation::NeverSampled.fault(), None);
    }

    /// The whole point of the type: a fresh fault proves the writer ran; a stale
    /// slot proves nothing about the probe. If these ever agree, the regression
    /// this module exists to fix has come back.
    #[test]
    fn a_fresh_fault_proves_the_writer_is_alive() {
        assert!(Observation::Faulted(ProbeFault::OverRange).writer_is_alive());
    }

    #[test]
    fn a_fresh_measurement_proves_the_writer_is_alive() {
        assert!(Observation::Fresh(SOME_MEASUREMENT).writer_is_alive());
    }

    #[test]
    fn a_stale_slot_does_not_prove_the_writer_is_alive() {
        assert!(!Observation::Stale.writer_is_alive());
    }

    #[test]
    fn a_never_sampled_slot_does_not_prove_the_writer_is_alive() {
        assert!(!Observation::NeverSampled.writer_is_alive());
    }

    /// A log line or a fault text-sensor must never conflate "probe open" with
    /// "rail down". Asserted by rendering, not by trusting the variant names.
    #[test]
    fn every_fault_renders_a_distinct_message() {
        let over: String = ProbeFault::OverRange.to_string();
        let under: String = ProbeFault::UnderRange.to_string();
        let unreadable: String = ProbeFault::Unreadable.to_string();
        assert_ne!(over, under);
        assert_ne!(under, unreadable);
        assert_ne!(over, unreadable);
    }

    #[test]
    fn a_faulted_observation_renders_the_fault_it_carries() {
        let rendered: String = Observation::Faulted(ProbeFault::UnderRange).to_string();
        assert!(
            rendered.contains(&ProbeFault::UnderRange.to_string()),
            "the observation must surface the reason, not just say 'fault': {rendered}"
        );
    }
}
