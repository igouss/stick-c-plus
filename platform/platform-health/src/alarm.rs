//! Responsibility two: given the verdict, should the buzzer be making a noise right now?

use platform_core::Tick;

use crate::{ack::AckSet, fault::Fault, fault::FaultKey, siren::Siren, verdict::Verdict};

/// The alarm's state, as far as the spec describes it: silent, sounding, or acknowledged.
///
/// A fourth piece of memory — which identities have been acknowledged — rides alongside inside
/// [`Alarm`] rather than being folded into this enum, because it answers a different question:
/// this enum is the relationship to the fault *currently announced*; the ack set is what
/// survives a higher-ranked fault masking it and then clearing. See [`Alarm`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlarmState {
    /// Nothing to report, or the last fault was acknowledged and nothing has taken its place.
    Silent,
    /// `key` is the currently-announced, un-acknowledged fault; the next chirp is due at
    /// `next_chirp_at`.
    Sounding { key: FaultKey, next_chirp_at: Tick },
    /// `key` is the currently-announced fault, silenced by the operator. The fault itself is
    /// unaffected — `assess` still reports it — only the sound is gone.
    Acknowledged { key: FaultKey },
}

/// The buzzer alarm: a small FSM over [`AlarmState`], plus the memory ([`AckSet`]) that makes
/// an acknowledgement survive a higher-ranked fault masking it in between.
///
/// `Copy`, value-in/value-out: `step` and `acknowledge` take `self` by value and return the
/// next `Alarm`, so this crate owns no cell and reads no clock of its own — the caller injects
/// `now` and holds the value between calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Alarm {
    pub state: AlarmState,
    acked: AckSet,
}

impl Alarm {
    /// A freshly-booted alarm: silent, nothing acknowledged.
    pub fn new() -> Self {
        Alarm {
            state: AlarmState::Silent,
            acked: AckSet::empty(),
        }
    }

    /// Advance the alarm by one evaluation of `verdict`, at `now`, chirping on cadence
    /// `cadence_ms`. Returns the next `Alarm` and, when a chirp is due this step, which
    /// [`Siren`] to sound.
    ///
    /// - `Healthy` always resets to `Silent` and clears every acknowledgement (R17b): the
    ///   board going healthy is the one event that lets a since-acknowledged fault sound again
    ///   if it later returns.
    /// - A fault whose key is already acknowledged stays `Acknowledged` and never chirps,
    ///   unconditionally — including when it was masked by a higher-ranked fault that has
    ///   since cleared (R17, R17a).
    /// - A newly-seen, un-acknowledged fault chirps immediately (R16): the first chirp is not
    ///   delayed a cadence period for no benefit.
    /// - The same still-sounding, un-acknowledged fault chirps again only once its cadence has
    ///   elapsed.
    pub fn step(self, verdict: &Verdict, cadence_ms: Tick, now: Tick) -> (Alarm, Option<Siren>) {
        let fault: Fault = match verdict {
            Verdict::Healthy => return (self.recovered(), None),
            Verdict::Faulted(fault) => *fault,
        };
        let key: FaultKey = fault.key();
        if self.acked.contains(key) {
            return (self.announcing(AlarmState::Acknowledged { key }), None);
        }
        match self.state {
            AlarmState::Sounding {
                key: announced,
                next_chirp_at,
            } if announced == key && now < next_chirp_at => (self, None),
            _ => (
                self.announcing(AlarmState::Sounding {
                    key,
                    next_chirp_at: now.saturating_add(cadence_ms),
                }),
                Some(Siren::for_kind(key.kind)),
            ),
        }
    }

    /// The board is healthy: silent, and every acknowledgement forgotten. Recovery is the one
    /// event that lets an acknowledged fault sound again if it later returns (R17b) — without
    /// it, one press would mute that fault for the life of the boot.
    fn recovered(self) -> Alarm {
        Alarm {
            state: AlarmState::Silent,
            acked: self.acked.clear(),
        }
    }

    /// The same alarm, now announcing `state`. The ack set rides across unchanged: what is
    /// announced and what has been silenced are separate memories, which is exactly why an
    /// acknowledgement survives a higher-ranked fault masking it.
    fn announcing(self, state: AlarmState) -> Alarm {
        Alarm {
            state,
            acked: self.acked,
        }
    }

    /// Silence the currently-sounding fault, and remember its identity so it stays silent even
    /// if a higher-ranked fault masks it and then clears (R15, R17). Has no effect from
    /// `Silent` — there is nothing to silence, and latching a keyless silence would swallow the
    /// next fault to arrive.
    pub fn acknowledge(self) -> Alarm {
        match self.state {
            AlarmState::Silent => self,
            AlarmState::Sounding { key, .. } | AlarmState::Acknowledged { key } => Alarm {
                state: AlarmState::Acknowledged { key },
                acked: self.acked.insert(key),
            },
        }
    }
}

impl Default for Alarm {
    fn default() -> Self {
        Alarm::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fault::Fault, Component};
    use proptest::prelude::*;

    /// `matches!(.., AlarmState::Sounding { .. })` inline inside `prop_assert!` trips proptest's
    /// own message-formatting on the struct-pattern braces; a named predicate sidesteps it.
    fn is_sounding(state: &AlarmState) -> bool {
        matches!(state, AlarmState::Sounding { .. })
    }

    fn a_stall() -> Fault {
        Fault::Stalled {
            component: Component("display"),
            silent_for_ms: 3_000,
        }
    }

    // --- R16 — a new fault sounds immediately. ---

    /// A brand-new fault chirps on the very first step, not one cadence later.
    #[test]
    fn a_new_fault_chirps_immediately() {
        let (_, chirp): (Alarm, Option<Siren>) =
            Alarm::new().step(&Verdict::Faulted(a_stall()), 5_000, 0);
        assert!(chirp.is_some());
    }

    /// A healthy verdict never chirps, from a freshly-booted alarm.
    #[test]
    fn a_healthy_verdict_never_chirps() {
        let (_, chirp): (Alarm, Option<Siren>) = Alarm::new().step(&Verdict::Healthy, 5_000, 0);
        assert_eq!(chirp, None);
    }

    // --- R15 — acknowledging silences the sound, and nothing else. ---

    /// Acknowledging a sounding alarm moves it to `Acknowledged`.
    #[test]
    fn acknowledging_a_sounding_alarm_silences_it() {
        let (sounding, _): (Alarm, Option<Siren>) =
            Alarm::new().step(&Verdict::Faulted(a_stall()), 5_000, 0);
        let acked: Alarm = sounding.acknowledge();
        assert!(matches!(acked.state, AlarmState::Acknowledged { .. }));
    }

    /// Acknowledging a silent alarm changes nothing — there is nothing to silence.
    #[test]
    fn acknowledging_a_silent_alarm_is_a_no_op() {
        let acked: Alarm = Alarm::new().acknowledge();
        assert_eq!(acked.state, AlarmState::Silent);
    }

    // --- Property tests: the general laws. ---

    proptest! {
        /// R25 — a healthy board never sounds, from any prior alarm state at all: silent,
        /// sounding, or acknowledged, with any cadence and any tick.
        #[test]
        fn a_healthy_board_never_sounds(cadence in 1u64..60_000, now in 0u64..1_000_000, prior_chirp_at in 0u64..1_000_000) {
            let sounding: Alarm = Alarm {
                state: AlarmState::Sounding { key: a_stall().key(), next_chirp_at: prior_chirp_at },
                acked: AckSet::empty(),
            };
            let (next, chirp): (Alarm, Option<Siren>) = sounding.step(&Verdict::Healthy, cadence, now);
            prop_assert_eq!(chirp, None);
            prop_assert_eq!(next.state, AlarmState::Silent);
        }

        /// R27 — once acknowledged, the same still-present fault never chirps again, at any
        /// later tick and any cadence: the silence is unconditional, not merely "eventual".
        #[test]
        fn acknowledgement_silences_at_any_later_tick(cadence in 1u64..60_000, now in 0u64..1_000_000) {
            let (sounding, _): (Alarm, Option<Siren>) = Alarm::new().step(&Verdict::Faulted(a_stall()), cadence, 0);
            let acked: Alarm = sounding.acknowledge();
            let (_, chirp): (Alarm, Option<Siren>) = acked.step(&Verdict::Faulted(a_stall()), cadence, now);
            prop_assert_eq!(chirp, None);
        }

        /// R28 — from every state, a brand-new (never-acknowledged) fault reaches `Sounding`
        /// with an immediate chirp: there is no state the FSM can get stuck in that can never
        /// sound again. Three starting states, unrolled rather than looped, so a failing
        /// `prop_assert!` can still return out of the property body.
        #[test]
        fn every_state_can_still_reach_sounding(now in 0u64..1_000_000) {
            let new_fault: Fault = Fault::Crashed { cause: crate::boot::CrashCause::Panic, since_ms: now };

            let silent: Alarm = Alarm::new();
            let (from_silent, silent_chirp): (Alarm, Option<Siren>) = silent.step(&Verdict::Faulted(new_fault), 5_000, now);
            prop_assert!(is_sounding(&from_silent.state));
            prop_assert!(silent_chirp.is_some());

            let sounding: Alarm = Alarm { state: AlarmState::Sounding { key: a_stall().key(), next_chirp_at: now }, acked: AckSet::empty() };
            let (from_sounding, sounding_chirp): (Alarm, Option<Siren>) = sounding.step(&Verdict::Faulted(new_fault), 5_000, now);
            prop_assert!(is_sounding(&from_sounding.state));
            prop_assert!(sounding_chirp.is_some());

            let acknowledged: Alarm = Alarm::new().acknowledge();
            let (from_acked, acked_chirp): (Alarm, Option<Siren>) = acknowledged.step(&Verdict::Faulted(new_fault), 5_000, now);
            prop_assert!(is_sounding(&from_acked.state));
            prop_assert!(acked_chirp.is_some());
        }
    }
}
