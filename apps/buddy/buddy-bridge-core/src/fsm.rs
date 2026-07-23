//! The pairing/connection decision — a Mealy machine the driving-adapter runs against a
//! `Central` port.
//!
//! The bridge's whole control flow is here as pure data: given the current [`State`] and the
//! [`Event`] the transport just reported, [`Fsm::on`] returns the single [`Action`] the loop
//! performs next. No async, no bluer, no clock — so every edge is proven on the host, and the
//! adapter is left with nothing but "call the method the action names".
//!
//! ## Every attempt begins by locating the device
//!
//! [`State::Disconnected`] does not connect — it *locates*. BlueZ can only connect to a device
//! it currently knows, and the two ways it forgets are routine: a cold daemon whose adapter has
//! never seen the stick, and `remove_device` (which the stale-bond recovery below performs by
//! definition). Routing every attempt — first, reconnect, and post-recovery — through
//! [`Action::Locate`] means there is exactly one way the address is acquired and no path that
//! can connect to a handle BlueZ has evicted.
//!
//! ## The stale-LTK recovery is a first-class transition, and a LAST resort
//!
//! The sharpest real-world edge (Handoff 1): when the *device* clears its bonds, BlueZ still
//! believes it is paired and offers a stale LTK, the device rejects encryption, and the link
//! drops. That routes to [`State::Repairing`] with [`Action::RemoveDeviceThenReacquire`] —
//! `remove_device` then a fresh handle — rather than a mystery hang. A plain device *reboot*
//! keeps the LTK valid, so it surfaces as [`Event::Disconnected`] and reconnects with no
//! re-pairing.
//!
//! But `remove_device` **destroys the bond**, and re-bonding is the one act that needs a human
//! at the glass inside BlueZ's 30-second window. The adapter cannot tell a stale LTK from a
//! stick that is merely switched off — both surface as "already paired, yet the link failed" —
//! so a hair-trigger recovery throws away a perfectly good bond every time the device is out of
//! range. The machine therefore *counts*: [`Event::EncryptionFailed`] backs off and retries, and
//! only after [`REBOND_AFTER_ENCRYPTION_FAILURES`] consecutive failures is the bond given up.
//! A genuine stale LTK fails deterministically and reaches the threshold in seconds; an absent
//! device reconnects the moment it returns, with its bond intact. Bond once, and forever.
//!
//! [`Event::PairRejectedAlreadyPaired`] is exempt: "connect said unpaired, then pair said
//! already paired" is a contradiction no absent device can produce, so it recovers at once.
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
    /// No link. Waiting out a backoff before the next attempt.
    Disconnected,
    /// A discovery is in flight, resolving the advertising stick to an address BlueZ knows.
    Locating,
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
    /// A backoff timer elapsed — time to look for the device again.
    BackoffElapsed,
    /// The device was found advertising (or is already known to BlueZ) at a usable address.
    Located,
    /// The scan window closed without the device appearing — it is off, asleep, or out of range.
    NotFound,
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
    /// `Central::locate` — discover the advertising stick, so BlueZ has a device to connect to.
    Locate,
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

/// How many consecutive encryption failures are tolerated before the bond is given up and
/// re-pairing (which needs a human at the glass) is forced.
///
/// The discriminator this threshold buys is *time*: a genuine stale LTK is rejected immediately
/// and deterministically, so it burns through the count in seconds, while a stick that is off or
/// out of range spends a growing backoff between each attempt and gets its bond back intact the
/// moment it returns. Three is enough to be sure and short enough to recover quickly.
pub const REBOND_AFTER_ENCRYPTION_FAILURES: u32 = 3;

/// The connection decision machine. `attempt` counts consecutive failures for the backoff
/// schedule and resets the moment the steady [`State::Subscribed`] is reached;
/// `encryption_failures` separately counts the consecutive failures that *look* like a stale
/// bond, so destroying one takes evidence rather than a single bad attempt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fsm {
    state: State,
    attempt: u32,
    encryption_failures: u32,
}

impl Default for Fsm {
    fn default() -> Self {
        Fsm {
            state: State::Disconnected,
            attempt: 0,
            encryption_failures: 0,
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

    /// The consecutive stale-bond-looking failures counted so far (for observability/tests).
    pub fn encryption_failures(&self) -> u32 {
        self.encryption_failures
    }

    /// Kick the machine off: the first attempt to locate the device.
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
            // ---- Disconnected: wait out the backoff, then go find the device ---------------
            (State::Disconnected, Event::BackoffElapsed) => {
                self.enter(State::Locating, Action::Locate)
            }

            // ---- Locating: the discovery result --------------------------------------------
            (State::Locating, Event::Located) => self.enter(State::Connecting, Action::Connect),

            // ---- Connecting: the connect result + the paired check -------------------------
            (State::Connecting, Event::ConnectedFresh) => self.enter(State::Pairing, Action::Pair),
            (State::Connecting, Event::ConnectedPaired) => {
                self.enter(State::Encrypted, Action::Subscribe)
            }
            (State::Connecting, Event::EncryptionFailed) => self.on_encryption_failure(),

            // ---- Pairing: the pair result --------------------------------------------------
            (State::Pairing, Event::LinkEncrypted) => {
                self.enter(State::Encrypted, Action::Subscribe)
            }
            // A contradiction, not a transient: connect reported unpaired and pair reported
            // paired. No absent device produces it, so recover the bond immediately.
            (State::Pairing, Event::PairRejectedAlreadyPaired) => {
                self.enter(State::Repairing, Action::RemoveDeviceThenReacquire)
            }
            (State::Pairing, Event::EncryptionFailed) => self.on_encryption_failure(),

            // ---- Repairing: the re-acquire result ------------------------------------------
            // `remove_device` evicted the device from BlueZ, so the address must be discovered
            // again before anything can connect to it.
            (State::Repairing, Event::Reacquired) => self.enter(State::Locating, Action::Locate),

            // ---- Encrypted: the subscribe result -------------------------------------------
            (State::Encrypted, Event::NotifySubscribed) => {
                // The one success milestone: both failure counters reset, then pump.
                self.attempt = 0;
                self.encryption_failures = 0;
                self.enter(State::Subscribed, Action::Run)
            }
            (State::Encrypted, Event::EncryptionFailed) => self.on_encryption_failure(),

            // ---- A failed scan, any drop, or an unexpected event: backed-off retry ---------
            _ => self.fail_to_backoff(),
        }
    }

    /// Absorb a failure that *looks* like a stale bond. Below the threshold this is just another
    /// backed-off retry — the device may simply be away, and its bond must survive that. At the
    /// threshold the evidence is conclusive, so the bond is given up and the counter cleared.
    fn on_encryption_failure(&mut self) -> Action {
        self.encryption_failures = self.encryption_failures.saturating_add(1);
        if self.encryption_failures < REBOND_AFTER_ENCRYPTION_FAILURES {
            return self.fail_to_backoff();
        }
        self.encryption_failures = 0;
        self.enter(State::Repairing, Action::RemoveDeviceThenReacquire)
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

    /// Drive a fresh machine to the steady [`State::Subscribed`] over an already-bonded link —
    /// the routine path, and the precondition for every "once we are running" test below.
    fn subscribed() -> Fsm {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::ConnectedPaired);
        fsm.on(Event::NotifySubscribed);
        fsm
    }

    // ---- The happy path, one transition per test (cyclomatic complexity 1) ----------------

    #[test]
    fn start_locates() {
        let mut fsm: Fsm = Fsm::new();
        let action: Action = fsm.start();
        assert_eq!(action, Action::Locate);
        assert_eq!(fsm.state(), State::Locating);
    }

    #[test]
    fn a_located_device_is_connected_to() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let action: Action = fsm.on(Event::Located);
        assert_eq!(action, Action::Connect);
        assert_eq!(fsm.state(), State::Connecting);
    }

    #[test]
    fn a_scan_that_finds_nothing_backs_off_and_scans_again() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let action: Action = fsm.on(Event::NotFound);
        assert!(matches!(action, Action::Backoff(_)));
        assert_eq!(fsm.on(Event::BackoffElapsed), Action::Locate);
    }

    #[test]
    fn a_fresh_connection_pairs() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        let action: Action = fsm.on(Event::ConnectedFresh);
        assert_eq!(action, Action::Pair);
        assert_eq!(fsm.state(), State::Pairing);
    }

    #[test]
    fn an_already_paired_connection_subscribes_without_pairing() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        let action: Action = fsm.on(Event::ConnectedPaired);
        assert_eq!(action, Action::Subscribe);
        assert_eq!(fsm.state(), State::Encrypted);
    }

    #[test]
    fn a_successful_pair_subscribes() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::ConnectedFresh);
        let action: Action = fsm.on(Event::LinkEncrypted);
        assert_eq!(action, Action::Subscribe);
        assert_eq!(fsm.state(), State::Encrypted);
    }

    #[test]
    fn a_successful_subscribe_runs() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::ConnectedPaired);
        let action: Action = fsm.on(Event::NotifySubscribed);
        assert_eq!(action, Action::Run);
        assert_eq!(fsm.state(), State::Subscribed);
    }

    // ---- The bond survives a device that is merely away ------------------------------------

    #[test]
    fn a_single_encryption_failure_keeps_the_bond() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        let action: Action = fsm.on(Event::EncryptionFailed);
        assert!(
            matches!(action, Action::Backoff(_)),
            "one failure is not evidence of a stale bond — the stick may simply be off"
        );
        assert_eq!(fsm.state(), State::Disconnected);
    }

    #[test]
    fn a_failure_short_of_the_threshold_still_keeps_the_bond() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::EncryptionFailed);
        fsm.on(Event::BackoffElapsed);
        fsm.on(Event::Located);
        let action: Action = fsm.on(Event::EncryptionFailed);
        assert!(matches!(action, Action::Backoff(_)));
        assert_eq!(fsm.encryption_failures(), 2);
    }

    #[test]
    fn the_threshold_of_encryption_failures_gives_the_bond_up() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::EncryptionFailed);
        fsm.on(Event::BackoffElapsed);
        fsm.on(Event::Located);
        fsm.on(Event::EncryptionFailed);
        fsm.on(Event::BackoffElapsed);
        fsm.on(Event::Located);
        let action: Action = fsm.on(Event::EncryptionFailed);
        assert_eq!(action, Action::RemoveDeviceThenReacquire);
        assert_eq!(fsm.state(), State::Repairing);
    }

    #[test]
    fn a_successful_link_forgets_earlier_encryption_failures() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::EncryptionFailed);
        assert_eq!(fsm.encryption_failures(), 1);
        fsm.on(Event::BackoffElapsed);
        fsm.on(Event::Located);
        fsm.on(Event::ConnectedPaired);
        fsm.on(Event::NotifySubscribed);
        assert_eq!(
            fsm.encryption_failures(),
            0,
            "a working link proves the bond is good; the count must not carry over"
        );
    }

    // ---- The stale-LTK recovery ------------------------------------------------------------

    #[test]
    fn a_pair_rejected_as_already_paired_recovers_at_once() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::ConnectedFresh);
        let action: Action = fsm.on(Event::PairRejectedAlreadyPaired);
        assert_eq!(action, Action::RemoveDeviceThenReacquire);
        assert_eq!(fsm.state(), State::Repairing);
    }

    #[test]
    fn a_reacquired_handle_locates_afresh_because_remove_evicted_it() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        fsm.on(Event::Located);
        fsm.on(Event::ConnectedFresh);
        fsm.on(Event::PairRejectedAlreadyPaired);
        let action: Action = fsm.on(Event::Reacquired);
        assert_eq!(
            action,
            Action::Locate,
            "remove_device evicted the device from BlueZ; connecting to it now cannot work"
        );
        assert_eq!(fsm.state(), State::Locating);
    }

    // ---- Reboot / disconnect: backoff, and it grows ----------------------------------------

    #[test]
    fn a_drop_while_subscribed_backs_off() {
        let mut fsm: Fsm = subscribed();
        let action: Action = fsm.on(Event::Disconnected);
        assert!(matches!(action, Action::Backoff(_)));
        assert_eq!(fsm.state(), State::Disconnected);
    }

    #[test]
    fn a_drop_while_subscribed_reconnects_by_locating_again() {
        let mut fsm: Fsm = subscribed();
        fsm.on(Event::Disconnected);
        assert_eq!(fsm.on(Event::BackoffElapsed), Action::Locate);
    }

    #[test]
    fn reaching_subscribed_resets_the_attempt_counter() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        // One failure bumps the counter above zero.
        fsm.on(Event::NotFound);
        assert_eq!(fsm.attempt(), 1);
        // A full path back to Subscribed clears it.
        fsm.on(Event::BackoffElapsed);
        fsm.on(Event::Located);
        fsm.on(Event::ConnectedPaired);
        fsm.on(Event::NotifySubscribed);
        assert_eq!(fsm.attempt(), 0);
    }

    #[test]
    fn two_consecutive_failures_grow_the_backoff() {
        let mut fsm: Fsm = Fsm::new();
        fsm.start();
        let Action::Backoff(first) = fsm.on(Event::NotFound) else {
            panic!("a failed scan backs off");
        };
        fsm.on(Event::BackoffElapsed);
        fsm.on(Event::Located);
        let Action::Backoff(second) = fsm.on(Event::Disconnected) else {
            panic!("a second failure backs off");
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
            Just(Event::Located),
            Just(Event::NotFound),
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

    /// Every event *except* the fatal `AgentMissing` — used to drive the machine into an
    /// arbitrary reachable state before the fatal event is injected, so the FailFast property
    /// is tested from every place the machine can actually be.
    fn any_non_fatal_event() -> impl Strategy<Value = Event> {
        prop_oneof![
            Just(Event::BackoffElapsed),
            Just(Event::Located),
            Just(Event::NotFound),
            Just(Event::ConnectedFresh),
            Just(Event::ConnectedPaired),
            Just(Event::Reacquired),
            Just(Event::LinkEncrypted),
            Just(Event::NotifySubscribed),
            Just(Event::EncryptionFailed),
            Just(Event::PairRejectedAlreadyPaired),
            Just(Event::Disconnected),
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

        /// The bond is never given up on thin evidence. `RemoveDeviceThenReacquire` destroys the
        /// bond, and re-bonding is the one act needing a human at the glass inside a 30-second
        /// window — so across any event sequence, the machine only ever chooses it after
        /// [`REBOND_AFTER_ENCRYPTION_FAILURES`] consecutive encryption failures, or on the
        /// self-contradictory `PairRejectedAlreadyPaired`. A regression to the old hair trigger
        /// (where a single failure sufficed) turns this red — and that trigger fired every time
        /// the stick was merely switched off.
        #[test]
        fn a_bond_is_only_destroyed_on_conclusive_evidence(
            events in proptest::collection::vec(any_event(), 0..64),
        ) {
            let mut fsm: Fsm = Fsm::new();
            for event in events {
                let failures_before: u32 = fsm.encryption_failures();
                let action: Action = fsm.on(event);
                let destroys: bool = action == Action::RemoveDeviceThenReacquire;
                let conclusive: bool = event == Event::PairRejectedAlreadyPaired
                    || failures_before + 1 >= REBOND_AFTER_ENCRYPTION_FAILURES;
                prop_assert!(!destroys || conclusive);
            }
        }

        /// A device that is away never costs its bond, however long it stays away: an unbounded
        /// run of failed scans and dropped links only ever backs off. This is "bond once, and
        /// forever" stated as a property — the counter that gates re-bonding must not be
        /// advanced by absence, only by a link that came up and then refused to encrypt.
        #[test]
        fn an_absent_device_never_costs_its_bond(
            events in proptest::collection::vec(
                prop_oneof![Just(Event::NotFound), Just(Event::Disconnected), Just(Event::BackoffElapsed)],
                0..64,
            ),
        ) {
            let mut fsm: Fsm = Fsm::new();
            for event in events {
                let action: Action = fsm.on(event);
                prop_assert!(action != Action::RemoveDeviceThenReacquire);
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

        /// `AgentMissing` fails fast from *every* reachable state, not just `Connecting`: drive
        /// the machine through an arbitrary non-fatal event sequence to land it somewhere, then
        /// inject the fatal event and require `FailFast`. A regression that narrowed the check to
        /// a single match arm would turn this red from whatever state it missed.
        #[test]
        fn a_missing_agent_fails_fast_from_every_reachable_state(
            events in proptest::collection::vec(any_non_fatal_event(), 0..40),
        ) {
            let mut fsm: Fsm = Fsm::new();
            events.iter().for_each(|event: &Event| {
                let _: Action = fsm.on(*event);
            });
            let action: Action = fsm.on(Event::AgentMissing);
            prop_assert!(matches!(action, Action::FailFast(_)));
        }
    }
}
