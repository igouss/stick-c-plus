//! The pairing/connection decision — a Mealy machine the driving-adapter runs against a
//! `Central` port.
//!
//! The bridge's whole control flow is here as pure data: given the current [`State`] and the
//! [`Event`] the transport just reported, [`Fsm::on`] returns the single [`Action`] the loop
//! performs next. No async, no bluer, no clock — so every edge is proven on the host, and the
//! adapter is left with nothing but "call the method the action names".
//!
//! ## The stale-LTK recovery is a first-class transition
//!
//! The sharpest real-world edge (Handoff 1): when the *device* clears its bonds, BlueZ still
//! believes it is paired and offers a stale LTK, the device rejects encryption, and the link
//! drops. [`Event::EncryptionFailed`] and [`Event::PairRejectedAlreadyPaired`] both route to
//! [`State::Repairing`] with [`Action::RemoveDeviceThenReacquire`] — `remove_device` then a
//! fresh handle — rather than a mystery hang. A plain device *reboot* keeps the LTK valid, so
//! it surfaces as [`Event::Disconnected`] and reconnects with no re-pairing.
//!
//! ## Backoff is driven by an elapsed-timer event
//!
//! A failure yields [`Action::Backoff`]; the loop sleeps that long and then feeds
//! [`Event::BackoffElapsed`], which re-enters [`Action::Connect`]. Keeping the timer outside
//! the machine leaves the machine a pure function of `(state, event)`.

use std::time::Duration;

use crate::backoff::backoff;

/// Where the connection is in its lifecycle. Each state awaits exactly one transport
/// operation, so only that operation's outcome events are expected in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// No link. Waiting out a backoff before the next connect attempt.
    Disconnected,
    /// A connect is in flight (and, on success, the paired/not-paired check).
    Connecting,
    /// A pair is in flight — the agent is (or is about to be) prompting for the passkey.
    Pairing,
    /// Recovering from a stale bond: `remove_device` + re-acquire the handle.
    Repairing,
    /// The link is encrypted; a subscribe to TX notifications is in flight.
    Encrypted,
    /// Subscribed and pumping the notify/write loop — the one steady state.
    Subscribed,
}

/// What the transport just reported. These are the only inputs the machine reacts to; the
/// driving-adapter maps each `Central` outcome onto one of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// A backoff timer elapsed — time to (re)connect.
    BackoffElapsed,
    /// Connected, and the device is NOT yet paired — pairing is required.
    ConnectedFresh,
    /// Connected, and the device is ALREADY paired — go straight to subscribing.
    ConnectedPaired,
    /// A stale bond was removed and the handle re-acquired — connect afresh.
    Reacquired,
    /// Pairing succeeded; the link is encrypted.
    LinkEncrypted,
    /// The TX subscription is live.
    NotifySubscribed,
    /// Encryption failed after connect/subscribe — the stale-LTK trap.
    EncryptionFailed,
    /// `pair()` was rejected because BlueZ still believes it is paired (a no-op re-key).
    PairRejectedAlreadyPaired,
    /// The link dropped (device reboot, out of range, stream ended).
    Disconnected,
    /// No pairing agent is registered — a fatal misconfiguration (the dropped-`AgentHandle`
    /// trap), never a retry.
    AgentMissing,
}

/// The single next thing the driving-adapter does. `Backoff` and `FailFast` are the two that
/// do not name a `Central` method: the loop sleeps, or exits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// `Central::connect` (then the paired check).
    Connect,
    /// `Central::pair`.
    Pair,
    /// `Central::remove_and_reacquire` — stale-LTK recovery.
    RemoveDeviceThenReacquire,
    /// `Central::subscribe_tx`.
    Subscribe,
    /// Pump the notify stream and accept writes until the link ends.
    Run,
    /// Sleep this long, then feed [`Event::BackoffElapsed`].
    Backoff(Duration),
    /// Stop the daemon: an unrecoverable condition the operator must fix.
    FailFast(&'static str),
}

/// The connection decision machine. `attempt` counts consecutive failures for the backoff
/// schedule and resets the moment the steady [`State::Subscribed`] is reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fsm {
    state: State,
    attempt: u32,
}

impl Default for Fsm {
    fn default() -> Self {
        Fsm {
            state: State::Disconnected,
            attempt: 0,
        }
    }
}

impl Fsm {
    /// A fresh machine: disconnected, no failures yet.
    pub fn new() -> Self {
        Fsm::default()
    }

    /// The current state (for observability/tests).
    pub fn state(&self) -> State {
        self.state
    }

    /// The consecutive-failure count feeding the backoff schedule (for observability/tests).
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Kick the machine off: the first connect attempt.
    pub fn start(&mut self) -> Action {
        self.on(Event::BackoffElapsed)
    }

    /// Advance on one transport [`Event`] and return the next [`Action`].
    ///
    /// Total over every `(state, event)`: the reachable edges are explicit; a fatal
    /// [`Event::AgentMissing`] always fails fast, and any surprise pairing/link event drops to
    /// a backed-off reconnect rather than panicking (a live loop only ever feeds the expected
    /// events, so the catch-all is a safety net, not a path).
    pub fn on(&mut self, event: Event) -> Action {
        // A missing agent is fatal in every state — never retried, so a mis-wired daemon
        // fails loudly instead of hanging on "no agent".
        if event == Event::AgentMissing {
            self.state = State::Disconnected;
            return Action::FailFast("no pairing agent registered");
        }
        match (self.state, event) {
            // ---- Disconnected: wait out the backoff, then connect --------------------------
            (State::Disconnected, Event::BackoffElapsed) => {
                self.enter(State::Connecting, Action::Connect)
            }

            // ---- Connecting: the connect result + the paired check -------------------------
            (State::Connecting, Event::ConnectedFresh) => self.enter(State::Pairing, Action::Pair),
            (State::Connecting, Event::ConnectedPaired) => {
                self.enter(State::Encrypted, Action::Subscribe)
            }
            (State::Connecting, Event::EncryptionFailed) => {
                self.enter(State::Repairing, Action::RemoveDeviceThenReacquire)
            }

            // ---- Pairing: the pair result --------------------------------------------------
            (State::Pairing, Event::LinkEncrypted) => {
                self.enter(State::Encrypted, Action::Subscribe)
            }
            (State::Pairing, Event::PairRejectedAlreadyPaired)
            | (State::Pairing, Event::EncryptionFailed) => {
                self.enter(State::Repairing, Action::RemoveDeviceThenReacquire)
            }

            // ---- Repairing: the re-acquire result ------------------------------------------
            (State::Repairing, Event::Reacquired) => self.enter(State::Connecting, Action::Connect),

            // ---- Encrypted: the subscribe result -------------------------------------------
            (State::Encrypted, Event::NotifySubscribed) => {
                // The one success milestone: reset the failure counter, then pump.
                self.attempt = 0;
                self.enter(State::Subscribed, Action::Run)
            }
            (State::Encrypted, Event::EncryptionFailed) => {
                self.enter(State::Repairing, Action::RemoveDeviceThenReacquire)
            }

            // ---- Any drop, or an unexpected event: backed-off reconnect --------------------
            _ => self.fail_to_backoff(),
        }
    }

    /// Move to `state` and return `action` unchanged.
    fn enter(&mut self, state: State, action: Action) -> Action {
        self.state = state;
        action
    }

    /// Drop to [`State::Disconnected`] and schedule the next attempt, growing the backoff.
    fn fail_to_backoff(&mut self) -> Action {
        let delay: Duration = backoff(self.attempt);
        self.attempt = self.attempt.saturating_add(1);
        self.state = State::Disconnected;
        Action::Backoff(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ---- The happy path, one transition per test (cyclomatic complexity 1) ----------------

    #[test]
    fn start_connects() {
        let mut fsm: Fsm = Fsm::new();
        let action: Action = fsm.start();
        assert_eq!(action, Action::Connect);
        assert_eq!(fsm.state(), State::Connecting);
    }

    #[test]
    fn a_fresh_connection_pairs() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let action: Action = fsm.on(Event::ConnectedFresh);
        assert_eq!(action, Action::Pair);
        assert_eq!(fsm.state(), State::Pairing);
    }

    #[test]
    fn an_already_paired_connection_subscribes_without_pairing() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let action: Action = fsm.on(Event::ConnectedPaired);
        assert_eq!(action, Action::Subscribe);
        assert_eq!(fsm.state(), State::Encrypted);
    }

    #[test]
    fn a_successful_pair_subscribes() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::ConnectedFresh);
        let action: Action = fsm.on(Event::LinkEncrypted);
        assert_eq!(action, Action::Subscribe);
        assert_eq!(fsm.state(), State::Encrypted);
    }

    #[test]
    fn a_successful_subscribe_runs() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::ConnectedPaired);
        let action: Action = fsm.on(Event::NotifySubscribed);
        assert_eq!(action, Action::Run);
        assert_eq!(fsm.state(), State::Subscribed);
    }

    // ---- The stale-LTK recovery, from each place it can surface ----------------------------

    #[test]
    fn encryption_failure_on_connect_triggers_remove_and_reacquire() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let action: Action = fsm.on(Event::EncryptionFailed);
        assert_eq!(action, Action::RemoveDeviceThenReacquire);
        assert_eq!(fsm.state(), State::Repairing);
    }

    #[test]
    fn a_pair_rejected_as_already_paired_triggers_remove_and_reacquire() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::ConnectedFresh);
        let action: Action = fsm.on(Event::PairRejectedAlreadyPaired);
        assert_eq!(action, Action::RemoveDeviceThenReacquire);
        assert_eq!(fsm.state(), State::Repairing);
    }

    #[test]
    fn encryption_failure_on_subscribe_triggers_remove_and_reacquire() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::ConnectedPaired);
        let action: Action = fsm.on(Event::EncryptionFailed);
        assert_eq!(action, Action::RemoveDeviceThenReacquire);
        assert_eq!(fsm.state(), State::Repairing);
    }

    #[test]
    fn a_reacquired_handle_connects_afresh() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::EncryptionFailed);
        let action: Action = fsm.on(Event::Reacquired);
        assert_eq!(action, Action::Connect);
        assert_eq!(fsm.state(), State::Connecting);
    }

    // ---- Reboot / disconnect: backoff, and it grows ----------------------------------------

    #[test]
    fn a_drop_while_subscribed_backs_off() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::ConnectedPaired);
        fsm.on(Event::NotifySubscribed);
        let action: Action = fsm.on(Event::Disconnected);
        assert!(matches!(action, Action::Backoff(_)));
        assert_eq!(fsm.state(), State::Disconnected);
    }

    #[test]
    fn reaching_subscribed_resets_the_attempt_counter() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        // One failure bumps the counter above zero.
        fsm.on(Event::Disconnected);
        assert_eq!(fsm.attempt(), 1);
        // A full path back to Subscribed clears it.
        fsm.on(Event::BackoffElapsed);
        fsm.on(Event::ConnectedPaired);
        fsm.on(Event::NotifySubscribed);
        assert_eq!(fsm.attempt(), 0);
    }

    #[test]
    fn two_consecutive_failures_grow_the_backoff() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let Action::Backoff(first) = fsm.on(Event::Disconnected) else {
            panic!("a drop backs off");
        };
        fsm.on(Event::BackoffElapsed);
        let Action::Backoff(second) = fsm.on(Event::Disconnected) else {
            panic!("a second drop backs off");
        };
        assert!(
            second > first,
            "the backoff grows with consecutive failures"
        );
    }

    // ---- A missing agent is fatal, from anywhere -------------------------------------------

    #[test]
    fn a_missing_agent_fails_fast() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let action: Action = fsm.on(Event::AgentMissing);
        assert!(matches!(action, Action::FailFast(_)));
    }

    // ---- Properties ------------------------------------------------------------------------

    /// Every event maps to `KeyboardOnly`-style totality: the machine never panics, whatever
    /// the state and whatever the event.
    fn any_event() -> impl Strategy<Value = Event> {
        prop_oneof![
            Just(Event::BackoffElapsed),
            Just(Event::ConnectedFresh),
            Just(Event::ConnectedPaired),
            Just(Event::Reacquired),
            Just(Event::LinkEncrypted),
            Just(Event::NotifySubscribed),
            Just(Event::EncryptionFailed),
            Just(Event::PairRejectedAlreadyPaired),
            Just(Event::Disconnected),
            Just(Event::AgentMissing),
        ]
    }

    proptest! {
        /// Never proposes to pair while the link is already encrypted or subscribed — a
        /// redundant pair on a live link is exactly the no-op re-key that must never happen.
        #[test]
        fn never_pairs_while_encrypted_or_subscribed(events in proptest::collection::vec(any_event(), 0..40)) {
            let mut fsm: Fsm = Fsm::new();
            for event in events {
                let before: State = fsm.state();
                let action: Action = fsm.on(event);
                let live: bool = before == State::Encrypted || before == State::Subscribed;
                prop_assert!(!(live && action == Action::Pair));
            }
        }

        /// The machine is total: any event in any reachable state returns an action and never
        /// panics.
        #[test]
        fn never_panics_on_any_event_sequence(events in proptest::collection::vec(any_event(), 0..64)) {
            let mut fsm: Fsm = Fsm::new();
            for event in events {
                let _: Action = fsm.on(event);
            }
        }
    }
}
