//! Status — what a consumer learns when it asks the cache about the host.
//!
//! As with the plant monitor's `Observation`, there are two independent questions and
//! a single `Option<Sample>` can only answer one:
//!
//! 1. **Is the poller alive?** Has the device scraped the host recently?
//! 2. **Did the host answer?** Did the last scrape carry metrics?
//!
//! Collapsing both into `None` throws away the fact an operator needs. A host that is
//! powered off and a poller thread that has panicked both present as "no fresh
//! sample" — yet one is a dead server on a healthy device, and the other is a broken
//! device. [`Status`] keeps them apart: a fresh [`Faulted`](Status::Faulted) says the
//! poller ran and the host did not answer, so [`Stale`](Status::Stale) recovers its
//! precise meaning — the poller stopped.
//!
//! Pure and `no_std`, like the rest of the core.

use core::fmt;

use crate::sample::Sample;

/// Why a scrape carried no usable sample.
///
/// The names describe *what the poller observed*, not a root cause it cannot know: an
/// unreachable host might be powered off, off the network, or simply not running
/// node_exporter — the failed connection cannot tell which, so the fault claims only
/// that the host did not answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum HostFault {
    /// The scrape request itself failed — the host is off, off the network, or the
    /// exporter is not listening. The device could not reach it.
    Unreachable,
    /// The host answered, but the body was not a usable node_exporter scrape — the
    /// wrong port, an error page, or a truncated read.
    Malformed,
}

impl fmt::Display for HostFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostFault::Unreachable => f.write_str(
                "the host did not answer — powered off, off the network, or no exporter",
            ),
            HostFault::Malformed => {
                f.write_str("the host answered but the scrape was not usable node_exporter output")
            }
        }
    }
}

/// What the shared cache reports about the host, as of some instant.
///
/// The four states are mutually exclusive and total. A *stale fault* is simply
/// [`Stale`](Self::Stale): once the poller has stopped, its last verdict about the
/// host is no longer evidence about the host now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// A recent, usable sample. The poller is alive and the host answered.
    Fresh(Sample),
    /// A recent *fault*. The poller is alive; the host did not give a usable scrape.
    /// Deliberately distinct from [`Stale`](Self::Stale).
    Faulted(HostFault),
    /// Nothing has been scraped recently enough to trust — the poller is dead, hung,
    /// or merely slower than its own staleness bound.
    Stale,
    /// Nothing usable has ever been sampled. The device has not completed a first CPU
    /// interval yet (a rate needs two scrapes — see [`step`](crate::step)).
    NeverSampled,
}

impl Status {
    /// The sample, if this status is a fresh one.
    ///
    /// The narrow accessor for a consumer that only needs the latest values; one that
    /// can explain *why* there is no sample should match on the status instead.
    pub const fn sample(self) -> Option<Sample> {
        match self {
            Status::Fresh(sample) => Some(sample),
            _ => None,
        }
    }

    /// The fault, if a fresh one was observed.
    pub const fn fault(self) -> Option<HostFault> {
        match self {
            Status::Faulted(fault) => Some(fault),
            _ => None,
        }
    }

    /// Whether the *poller* is demonstrably alive, regardless of the host's health.
    ///
    /// True for a fresh sample **and** for a fresh fault: publishing a fault is itself
    /// proof the poller ran. This is the liveness signal a bare `Option<Sample>`
    /// cannot express.
    pub const fn poller_is_alive(self) -> bool {
        matches!(self, Status::Fresh(_) | Status::Faulted(_))
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Fresh(sample) => {
                write!(
                    f,
                    "cpu {}% / mem {}%",
                    sample.cpu().value(),
                    sample.mem().value()
                )
            }
            Status::Faulted(fault) => write!(f, "host fault: {fault}"),
            Status::Stale => f.write_str("stale — no fresh scrape; the poller may be dead"),
            Status::NeverSampled => f.write_str("no sample taken yet"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::percent::Percent;

    /// A representative sample — the exact values are irrelevant here; these tests
    /// turn on which *variant* a status is, not on its numbers.
    const SOME_SAMPLE: Sample = Sample::new(Percent::FULL, Percent::ZERO);

    #[test]
    fn a_fresh_status_yields_its_sample() {
        let status: Status = Status::Fresh(SOME_SAMPLE);
        assert_eq!(status.sample(), Some(SOME_SAMPLE));
        assert_eq!(status.fault(), None);
    }

    #[test]
    fn a_faulted_status_yields_its_fault_and_no_sample() {
        let status: Status = Status::Faulted(HostFault::Unreachable);
        assert_eq!(status.sample(), None);
        assert_eq!(status.fault(), Some(HostFault::Unreachable));
    }

    #[test]
    fn a_stale_status_yields_neither() {
        assert_eq!(Status::Stale.sample(), None);
        assert_eq!(Status::Stale.fault(), None);
    }

    #[test]
    fn a_never_sampled_status_yields_neither() {
        assert_eq!(Status::NeverSampled.sample(), None);
        assert_eq!(Status::NeverSampled.fault(), None);
    }

    /// The whole point of the type: a fresh fault proves the poller ran; a stale slot
    /// proves nothing about the host.
    #[test]
    fn a_fresh_fault_proves_the_poller_is_alive() {
        assert!(Status::Faulted(HostFault::Unreachable).poller_is_alive());
    }

    #[test]
    fn a_fresh_sample_proves_the_poller_is_alive() {
        assert!(Status::Fresh(SOME_SAMPLE).poller_is_alive());
    }

    #[test]
    fn a_stale_slot_does_not_prove_the_poller_is_alive() {
        assert!(!Status::Stale.poller_is_alive());
    }

    #[test]
    fn a_never_sampled_slot_does_not_prove_the_poller_is_alive() {
        assert!(!Status::NeverSampled.poller_is_alive());
    }

    #[test]
    fn every_fault_renders_a_distinct_message() {
        let unreachable: String = HostFault::Unreachable.to_string();
        let malformed: String = HostFault::Malformed.to_string();
        assert_ne!(unreachable, malformed);
    }

    #[test]
    fn a_faulted_status_renders_the_fault_it_carries() {
        let rendered: String = Status::Faulted(HostFault::Malformed).to_string();
        assert!(
            rendered.contains(&HostFault::Malformed.to_string()),
            "the status must surface the reason, not just say 'fault': {rendered}"
        );
    }
}
