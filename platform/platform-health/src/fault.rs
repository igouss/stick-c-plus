//! The fault vocabulary: what can be broken, and how one fault outranks another.

use platform_core::Tick;

use crate::{boot::CrashCause, Component};

/// Which of the four epic kinds a [`Fault`] is, stripped of its payload — the part of a
/// fault's identity that survives its duration growing every tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum FaultKind {
    /// The board itself restarted unexpectedly.
    Crashed,
    /// The board is low on a resource it needs to keep running.
    Starved,
    /// A component concluded its own fault and said so.
    Reported,
    /// A component has gone silent past its own deadline.
    Stalled,
}

/// Something wrong, naming what and — where it makes sense — for how long.
///
/// `Starved` and `Crashed` are board-scoped, not component-scoped: a starving board has been
/// starving since some unobserved moment, so it carries a level against a floor rather than a
/// fabricated duration; a crash names the board rather than whichever component happened to be
/// running when it fell over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// The board restarted; `since_ms` is time since that boot, i.e. `now` itself.
    Crashed { cause: CrashCause, since_ms: Tick },
    /// The board is under its configured heap floor.
    Starved {
        free_heap_bytes: u32,
        floor_bytes: u32,
    },
    /// `component` concluded its own fault `for_ms` milliseconds ago.
    Reported { component: Component, for_ms: Tick },
    /// `component` has been silent for `silent_for_ms` — the total time since its
    /// last beat, not the excess past its deadline. The deadline decides *whether*
    /// the component is stalled; it does not deform the age that is then reported.
    Stalled {
        component: Component,
        silent_for_ms: Tick,
    },
}

/// A fault's identity, with its ever-growing duration stripped out.
///
/// The reason an acknowledgement can hold: if the alarm compared whole [`Fault`] values, the
/// same still-present stall would compare unequal a millisecond later as its duration grew,
/// and the acknowledgement would evaporate. Comparing on `(kind, component)` instead makes the
/// same fault the same key for as long as it is the same fault.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FaultKey {
    pub kind: FaultKind,
    /// `None` for the two board-scoped kinds (`Crashed`, `Starved`); `Some` otherwise.
    pub component: Option<Component>,
}

/// The total rank order faults are compared by. Ordered so the derived [`Ord`] puts
/// [`Severity::Crashed`] at the maximum (R12): the board already fell over, and that explains
/// everything downstream of it. [`Severity::Starved`] outranks [`Severity::Reported`] because
/// it is a cause rather than a symptom; [`Severity::Reported`] outranks [`Severity::Stalled`]
/// because a component that knows it gave up is direct evidence, where a stall is only
/// inferred from outside.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Stalled,
    Reported,
    Starved,
    Crashed,
}

impl Fault {
    /// Which kind this fault is, stripped of its payload.
    pub fn kind(&self) -> FaultKind {
        match self {
            Fault::Crashed { .. } => FaultKind::Crashed,
            Fault::Starved { .. } => FaultKind::Starved,
            Fault::Reported { .. } => FaultKind::Reported,
            Fault::Stalled { .. } => FaultKind::Stalled,
        }
    }

    /// This fault's identity — kind and, where it applies, which component — with the
    /// duration that grows every tick left out. See [`FaultKey`].
    pub fn key(&self) -> FaultKey {
        let component: Option<Component> = match self {
            Fault::Crashed { .. } | Fault::Starved { .. } => None,
            Fault::Reported { component, .. } | Fault::Stalled { component, .. } => {
                Some(*component)
            }
        };
        FaultKey {
            kind: self.kind(),
            component,
        }
    }

    /// This fault's rank. Higher outranks lower; `assess` reports the highest-ranked fault
    /// present, breaking ties by which has stood longest, then by position in the evidence
    /// slice.
    pub fn severity(&self) -> Severity {
        match self.kind() {
            FaultKind::Crashed => Severity::Crashed,
            FaultKind::Starved => Severity::Starved,
            FaultKind::Reported => Severity::Reported,
            FaultKind::Stalled => Severity::Stalled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::CrashCause;

    // --- R12 — the total order, crash at the top. ---

    /// R12 — a crash outranks a starved board.
    #[test]
    fn crashed_outranks_starved() {
        assert!(Severity::Crashed > Severity::Starved);
    }

    /// R12 (unit companion) — starvation, a cause, outranks a component that reported giving
    /// up, a symptom.
    #[test]
    fn starved_outranks_reported() {
        assert!(Severity::Starved > Severity::Reported);
    }

    /// R12 (unit companion) — a component that reported giving up outranks an inferred stall.
    #[test]
    fn reported_outranks_stalled() {
        assert!(Severity::Reported > Severity::Stalled);
    }

    /// `severity()` reads the fault's own rank straight from the total order above.
    #[test]
    fn a_crashed_fault_has_crashed_severity() {
        let fault: Fault = Fault::Crashed {
            cause: CrashCause::Panic,
            since_ms: 4_000,
        };
        assert_eq!(fault.severity(), Severity::Crashed);
    }

    // --- The re-fault trap: identity survives the payload changing. ---

    /// The whole reason `FaultKey` exists: two stalls on the same component, at different
    /// durations, are the *same* fault as far as an acknowledgement is concerned.
    #[test]
    fn key_strips_the_growing_duration() {
        let early: Fault = Fault::Stalled {
            component: Component("display"),
            silent_for_ms: 500,
        };
        let late: Fault = Fault::Stalled {
            component: Component("display"),
            silent_for_ms: 12_000,
        };
        assert_eq!(early.key(), late.key());
    }

    /// A stall on one component and a stall on another are different faults, so their keys
    /// must differ even though the kind is the same.
    #[test]
    fn key_distinguishes_which_component() {
        let display: Fault = Fault::Stalled {
            component: Component("display"),
            silent_for_ms: 500,
        };
        let sensors: Fault = Fault::Stalled {
            component: Component("sensors"),
            silent_for_ms: 500,
        };
        assert_ne!(display.key(), sensors.key());
    }

    /// The two board-scoped kinds carry no component in their key.
    #[test]
    fn a_crashed_key_names_no_component() {
        let fault: Fault = Fault::Crashed {
            cause: CrashCause::Brownout,
            since_ms: 10,
        };
        assert_eq!(fault.key().component, None);
    }

    proptest::proptest! {
        /// P — `key()` never depends on the duration payload, for any duration at all: this is
        /// the property behind `key_strips_the_growing_duration`, generalised over every
        /// possible tick.
        #[test]
        fn key_is_independent_of_duration(early in 0u64..1_000_000, late in 0u64..1_000_000) {
            let a: Fault = Fault::Reported { component: Component("display"), for_ms: early };
            let b: Fault = Fault::Reported { component: Component("display"), for_ms: late };
            proptest::prop_assert_eq!(a.key(), b.key());
        }
    }
}
