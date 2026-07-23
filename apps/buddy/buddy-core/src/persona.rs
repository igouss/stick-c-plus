//! The persona base state: the seven moods, and the pure `derive` over a snapshot.
//!
//! [`PersonaState`] is the mood the buddy wears. [`derive`] is the base layer — a pure,
//! top-down, first-match-wins function of the current [`Snapshot`], with no time and no I/O.
//! The one-shot override layer ([`crate::oneshot`]), the wake window, and the charging clock
//! sit *above* this base; [`crate::step`] composes them in the one loop order that matters.
//!
//! ## Ordinals are load-bearing
//!
//! Upstream indexes a per-species state table by the persona ordinal, so the numeric order
//! below is part of the contract, not an accident of declaration. Keep it: `Sleep = 0`
//! through `Heart = 6`.

/// The buddy's mood — one of seven, rendered as one sprite per state.
///
/// `#[repr(u8)]` with explicit discriminants because the ordinals are **load-bearing**
/// upstream (a per-species state table is indexed by them). Do not reorder.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PersonaState {
    /// Asleep — the resting pose. Reached via the wake window, the nap, or the clock.
    Sleep = 0,
    /// Awake with nothing pressing — the default.
    Idle = 1,
    /// Three or more sessions running (`>= 3`, not one).
    Busy = 2,
    /// A session is waiting on the owner — outranks a running session.
    Attention = 3,
    /// A session just completed — outranks a running session.
    Celebrate = 4,
    /// Shaken, or the late-night clock flicker — a dizzy wobble.
    Dizzy = 5,
    /// An approval answered fast, or an affectionate clock slice.
    Heart = 6,
}

/// The heartbeat snapshot the base state is derived from — the merged session counts and
/// completion flag, with no time and no history.
///
/// A domain value, distinct from the wire crate's own snapshot DTO: the firmware maps the
/// parsed wire fields into this before calling [`derive`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// Whether the bridge link is live (fed by the 30 s liveness window on the wire side).
    pub connected: bool,
    /// Sessions waiting on an owner decision.
    pub sessions_waiting: u32,
    /// Sessions currently running.
    pub sessions_running: u32,
    /// Whether a session completed in this packet — **not sticky**, reset by an absent field.
    pub recently_completed: bool,
}

/// The base persona for a snapshot: pure, top-down, first match wins.
///
/// The order is the contract, so read it as a ladder:
/// 1. not connected → [`Idle`](PersonaState::Idle) (**not** sleep);
/// 2. any session waiting → [`Attention`](PersonaState::Attention);
/// 3. something recently completed → [`Celebrate`](PersonaState::Celebrate);
/// 4. three or more running → [`Busy`](PersonaState::Busy) (**three**, not one);
/// 5. otherwise → [`Idle`](PersonaState::Idle).
///
/// `recently_completed` outranks `sessions_running`, so completed-and-busy renders celebrate.
pub fn derive(snapshot: &Snapshot) -> PersonaState {
    if !snapshot.connected {
        PersonaState::Idle
    } else if snapshot.sessions_waiting > 0 {
        PersonaState::Attention
    } else if snapshot.recently_completed {
        PersonaState::Celebrate
    } else if snapshot.sessions_running >= 3 {
        PersonaState::Busy
    } else {
        PersonaState::Idle
    }
}

/// The wake-transition window, in milliseconds.
///
/// Armed **only** on a screen-off→on transition. While armed, [`crate::step`] rewrites a
/// base of [`Idle`](PersonaState::Idle) to [`Sleep`](PersonaState::Sleep) — and *only* Idle;
/// attention, celebrate and busy pass through untouched.
pub const WAKE_WINDOW_MS: platform_core::Tick = 12_000;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A helper snapshot: connected, no sessions, nothing completed.
    fn quiet() -> Snapshot {
        Snapshot {
            connected: true,
            sessions_waiting: 0,
            sessions_running: 0,
            recently_completed: false,
        }
    }

    /// The ordinals are the load-bearing contract the species table indexes by.
    #[test]
    fn the_ordinals_are_the_documented_contract() {
        assert_eq!(PersonaState::Sleep as u8, 0);
        assert_eq!(PersonaState::Idle as u8, 1);
        assert_eq!(PersonaState::Busy as u8, 2);
        assert_eq!(PersonaState::Attention as u8, 3);
        assert_eq!(PersonaState::Celebrate as u8, 4);
        assert_eq!(PersonaState::Dizzy as u8, 5);
        assert_eq!(PersonaState::Heart as u8, 6);
    }

    /// Disconnected derives idle, never sleep — the first README divergence the port fixes.
    #[test]
    fn disconnected_is_idle_not_sleep() {
        let snapshot: Snapshot = Snapshot {
            connected: false,
            sessions_waiting: 9,
            sessions_running: 9,
            recently_completed: true,
        };
        assert_eq!(derive(&snapshot), PersonaState::Idle);
    }

    /// A single waiting session outranks everything below it.
    #[test]
    fn one_waiting_session_is_attention() {
        let snapshot: Snapshot = Snapshot {
            sessions_waiting: 1,
            ..quiet()
        };
        assert_eq!(derive(&snapshot), PersonaState::Attention);
    }

    /// Completed outranks running: completed-and-busy renders celebrate.
    #[test]
    fn completed_outranks_running() {
        let snapshot: Snapshot = Snapshot {
            sessions_running: 5,
            recently_completed: true,
            ..quiet()
        };
        assert_eq!(derive(&snapshot), PersonaState::Celebrate);
    }

    /// Busy needs three running, not one — the second README divergence.
    #[test]
    fn two_running_is_not_yet_busy() {
        let two: Snapshot = Snapshot {
            sessions_running: 2,
            ..quiet()
        };
        let three: Snapshot = Snapshot {
            sessions_running: 3,
            ..quiet()
        };
        assert_eq!(derive(&two), PersonaState::Idle);
        assert_eq!(derive(&three), PersonaState::Busy);
    }

    /// A connected, idle snapshot falls through to idle.
    #[test]
    fn a_quiet_connected_snapshot_is_idle() {
        assert_eq!(derive(&quiet()), PersonaState::Idle);
    }

    proptest! {
        /// Waiting always wins over running/completed, at any counts, whenever connected.
        #[test]
        fn waiting_always_wins_when_connected(
            waiting in 1u32..1000,
            running in 0u32..1000,
            completed in proptest::bool::ANY,
        ) {
            let snapshot: Snapshot = Snapshot {
                connected: true,
                sessions_waiting: waiting,
                sessions_running: running,
                recently_completed: completed,
            };
            prop_assert_eq!(derive(&snapshot), PersonaState::Attention);
        }

        /// Disconnection dominates every count.
        #[test]
        fn disconnection_dominates_every_count(
            waiting in 0u32..1000,
            running in 0u32..1000,
            completed in proptest::bool::ANY,
        ) {
            let snapshot: Snapshot = Snapshot {
                connected: false,
                sessions_waiting: waiting,
                sessions_running: running,
                recently_completed: completed,
            };
            prop_assert_eq!(derive(&snapshot), PersonaState::Idle);
        }
    }
}
