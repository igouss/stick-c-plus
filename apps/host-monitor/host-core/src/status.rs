//! Status — what the cache can honestly say about the *pulse endpoint* right now.
//!
//! One `GET /pulse` stands in for every host at once, so freshness is a property of the
//! frame, not of an individual host: either the last fetch succeeded, or it did not, or
//! the poller has stopped fetching. As with the plant monitor's `Observation`, a bare
//! `Option` cannot separate the two failures a human must tell apart —
//!
//! 1. **Is the poller alive?** Did the device fetch the endpoint recently?
//! 2. **Did the endpoint answer?** Did the last fetch return a usable frame?
//!
//! — because a poller thread that panicked and an endpoint that is unreachable both
//! present as "no fresh frame". [`Status`] keeps them apart: a fresh
//! [`Faulted`](Status::Faulted) says the poller ran and the endpoint did not answer, so
//! [`Stale`](Status::Stale) recovers its precise meaning — the poller stopped.
//!
//! The *data* is held separately (the last-good [`Pulse`](crate::Pulse) frame), so this
//! type carries only the verdict: the frame outlives the reading, exactly as the old
//! graph did. Pure and `no_std`.

use core::fmt;

/// Why a fetch carried no usable frame.
///
/// The names describe *what the poller observed*, not a root cause it cannot know.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum HostFault {
    /// The fetch itself failed, or the endpoint reported its own backend was down (a
    /// `502 prometheus_unavailable`): the device could not get a frame. The last good
    /// frame is kept and the status says so.
    Unreachable,
    /// The endpoint answered, but the body was not a usable pulse frame — the wrong
    /// service, an error page, or JSON that did not match the contract.
    Malformed,
}

impl fmt::Display for HostFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostFault::Unreachable => f.write_str(
                "the pulse endpoint did not answer — off the network, or its backend is down",
            ),
            HostFault::Malformed => {
                f.write_str("the pulse endpoint answered but the body was not a usable frame")
            }
        }
    }
}

/// What the shared cache reports about the endpoint, as of some instant.
///
/// The four states are mutually exclusive and total. [`Fresh`](Self::Fresh) is
/// deliberately payload-free — the frame it refers to lives in the cache, retained across
/// faults — so this is purely the endpoint's liveness verdict. A *stale fault* is simply
/// [`Stale`](Self::Stale): once the poller has stopped, its last verdict is no longer
/// evidence about the endpoint now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// The last fetch succeeded: the poller is alive and the endpoint answered with a
    /// usable frame.
    Fresh,
    /// The last fetch *failed* recently. The poller is alive; the endpoint did not give a
    /// usable frame. Deliberately distinct from [`Stale`](Self::Stale).
    Faulted(HostFault),
    /// Nothing has been fetched recently enough to trust — the poller is dead, hung, or
    /// merely slower than its own staleness bound.
    Stale,
    /// Nothing has ever been fetched. The device has not completed a first fetch yet.
    NeverSampled,
}

impl Status {
    /// The fault, if a fresh one was observed.
    pub const fn fault(self) -> Option<HostFault> {
        match self {
            Status::Faulted(fault) => Some(fault),
            _ => None,
        }
    }

    /// Whether the *poller* is demonstrably alive, regardless of the endpoint's health.
    ///
    /// True for a fresh fetch **and** for a fresh fault: publishing a fault is itself proof
    /// the poller ran. This is the liveness signal a bare `Option` cannot express.
    pub const fn poller_is_alive(self) -> bool {
        matches!(self, Status::Fresh | Status::Faulted(_))
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Fresh => f.write_str("fresh — the last fetch returned a frame"),
            Status::Faulted(fault) => write!(f, "endpoint fault: {fault}"),
            Status::Stale => f.write_str("stale — no fresh fetch; the poller may be dead"),
            Status::NeverSampled => f.write_str("no frame fetched yet"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_status_has_no_fault() {
        assert_eq!(Status::Fresh.fault(), None);
    }

    #[test]
    fn a_faulted_status_yields_its_fault() {
        assert_eq!(
            Status::Faulted(HostFault::Unreachable).fault(),
            Some(HostFault::Unreachable)
        );
    }

    #[test]
    fn stale_and_never_sampled_yield_no_fault() {
        assert_eq!(Status::Stale.fault(), None);
        assert_eq!(Status::NeverSampled.fault(), None);
    }

    /// The whole point of the type: a fresh fault proves the poller ran; a stale slot
    /// proves nothing about the endpoint.
    #[test]
    fn a_fresh_fetch_or_fault_proves_the_poller_is_alive() {
        assert!(Status::Fresh.poller_is_alive());
        assert!(Status::Faulted(HostFault::Unreachable).poller_is_alive());
    }

    #[test]
    fn a_stale_or_never_sampled_slot_does_not_prove_liveness() {
        assert!(!Status::Stale.poller_is_alive());
        assert!(!Status::NeverSampled.poller_is_alive());
    }

    #[test]
    fn every_fault_renders_a_distinct_message() {
        assert_ne!(
            HostFault::Unreachable.to_string(),
            HostFault::Malformed.to_string()
        );
    }

    #[test]
    fn a_faulted_status_renders_the_fault_it_carries() {
        let rendered: String = Status::Faulted(HostFault::Malformed).to_string();
        assert!(
            rendered.contains(&HostFault::Malformed.to_string()),
            "the status must surface the reason: {rendered}"
        );
    }
}
