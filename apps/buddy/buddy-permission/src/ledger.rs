//! Session → snapshot synthesis — pure, and touching no clock.
//!
//! There is no status IPC surface to query the agent; the heartbeat is *reconstructed* from the
//! stream of already-typed session events the hook forwards (the raw Claude Code hook JSON is
//! parsed in the hook binary, an external schema verified by hand — never here). This ledger folds
//! those events, keyed by `session_id`, into the [`SnapshotPacket`] fields the device expects.
//!
//! ## The synthesized fields
//!
//! - `total` — distinct sessions seen;
//! - `running` — sessions started-and-not-stopped;
//! - `waiting` — sessions with an outstanding prompt;
//! - `completed` — a completion happened since the last [`synthesize`](Ledger::synthesize)
//!   (destructive-on-absent: it is a per-emission pulse, cleared each synth);
//! - `tokens` — a monotonic running total, credited through [`TokenLatch`](buddy_core::TokenLatch)
//!   from observed desktop totals so a daemon restart / device reboot never double-counts;
//! - `tokens_today` — may be `0`; hooks do not report usage directly. Zeros are honest here — the
//!   level/fed mechanics simply stay flat rather than being faked;
//! - `msg` / `entries` — as available.
//!
//! ## Time
//!
//! The ledger reads no clock. A [`SnapshotPacket`] carries no timestamp, and "this tick" means one
//! [`synthesize`](Ledger::synthesize) call (the daemon's emission cadence), not a wall-clock read.
//! The one-shot time-sync message is a *separate* wire shape the daemon sends via
//! [`buddy_wire::serialize_time`], not part of the heartbeat this ledger builds.
//!
//! ## The device-side hazard this ledger must not feed
//!
//! An absent `prompt` field CLEARS the approval on the device, so prompt clearing is a DELIBERATE
//! signal — [`SessionEvent::Resolved`] — never a side effect of a keepalive. The synthesized packet
//! carries the outstanding prompt whenever one is live; the final guard is
//! [`GatedSnapshot`](crate::gating::GatedSnapshot).

use buddy_core::TokenLatch;
use buddy_wire::{Prompt, SnapshotPacket};
use std::collections::HashMap;

/// A Claude Code session identity, taken verbatim from the hook payload's `session_id`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SessionId(pub String);

/// One already-typed session lifecycle event, keyed to a [`SessionId`] when recorded.
///
/// Modelled from the bead's "Feeding the pet" section. The raw hook JSON is parsed in the hook
/// binary; this crate only folds the typed result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SessionEvent {
    /// `SessionStart` — the session exists and is running.
    Started,
    /// A tool call began (no approval needed / already permitted).
    ToolStarted,
    /// A tool call is waiting on the owner's decision — raises the outstanding prompt.
    AwaitingApproval(Prompt),
    /// The owner's decision arrived — the DELIBERATE prompt-clear (never a keepalive side effect).
    Resolved,
    /// A tool call / turn completed — pulses `completed` for the next synth.
    Completed,
    /// `Stop` — the session is no longer running.
    Stopped,
}

/// The session → snapshot fold: the accumulated session state plus the token latch.
///
/// Pure state; no I/O, no clock. The daemon feeds it events and observed token totals, then calls
/// [`synthesize`](Ledger::synthesize) once per heartbeat.
///
/// ## One source of truth for the outstanding prompt
///
/// The live approval shown on the glass is **derived** from the per-session pending prompts, never
/// stored separately: [`outstanding_prompt`](Ledger::outstanding_prompt) picks the most recently
/// raised still-pending prompt. This makes the device-side hazard *unrepresentable* — because
/// `waiting` counts exactly the sessions with a pending prompt and the emitted prompt is drawn from
/// that same set, a [`Resolved`](SessionEvent::Resolved) on one session can never clear the glass
/// while another session still waits. A single stored `outstanding` field could diverge from the
/// per-session `waiting` count; a derived one cannot.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    /// The per-session lifecycle state, keyed by session id; its length is `total`.
    sessions: HashMap<SessionId, SessionState>,
    /// A monotonic counter stamped onto each raised prompt, so "the latest prompt is on the glass"
    /// is a deterministic, testable choice rather than a `HashMap` iteration accident.
    raise_seq: u64,
    /// A per-emission `completed` pulse: set by a [`Completed`](SessionEvent::Completed), cleared on
    /// the next [`synthesize`](Ledger::synthesize).
    completed_pulse: bool,
    /// Turns observed cumulative desktop totals into per-observation credits, RAM-only so a restart
    /// never double-counts.
    latch: TokenLatch,
    /// The monotonic sum of everything the latch has credited — the value emitted as `tokens`.
    credited_total: u32,
}

/// A pending prompt tagged with the raise order that stamped it, so the newest wins the glass.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Pending {
    /// The [`Ledger::raise_seq`] value at the moment this prompt was raised — higher is newer.
    seq: u64,
    /// The prompt itself, shown on the device until an explicit resolve.
    prompt: Prompt,
}

/// One session's folded lifecycle: whether it started, whether it stopped, and its pending prompt.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct SessionState {
    /// A [`Started`](SessionEvent::Started) was seen for this session.
    started: bool,
    /// A [`Stopped`](SessionEvent::Stopped) was seen for this session.
    stopped: bool,
    /// The session's pending prompt, set by [`AwaitingApproval`](SessionEvent::AwaitingApproval) and
    /// cleared by [`Resolved`](SessionEvent::Resolved) — this is what `waiting` counts and the sole
    /// source the emitted prompt is drawn from.
    pending: Option<Pending>,
}

impl SessionState {
    /// Running = started and not yet stopped.
    fn is_running(&self) -> bool {
        self.started && !self.stopped
    }

    /// Waiting = an outstanding pending prompt.
    fn is_waiting(&self) -> bool {
        self.pending.is_some()
    }
}

impl Ledger {
    /// A fresh ledger: no sessions, no outstanding prompt, an unsynced [`TokenLatch`].
    pub fn new() -> Self {
        Ledger::default()
    }

    /// Fold one typed session event for `session` into the accumulated state.
    ///
    /// A [`Resolved`](SessionEvent::Resolved) clears ONLY the resolving session's pending prompt;
    /// because the emitted prompt is derived from the surviving pending set, an unrelated session's
    /// resolve can never clear the glass while this session still waits.
    pub fn record(&mut self, session: SessionId, event: SessionEvent) {
        match event {
            // A completion pulses the emission flag and touches no per-session state.
            SessionEvent::Completed => self.completed_pulse = true,
            SessionEvent::AwaitingApproval(prompt) => {
                // Stamp the newest raise order so this prompt wins the glass, then record it on the
                // session — the sole place a prompt lives.
                self.raise_seq += 1;
                let pending: Pending = Pending {
                    seq: self.raise_seq,
                    prompt,
                };
                self.sessions.entry(session).or_default().pending = Some(pending);
            }
            SessionEvent::Started => self.sessions.entry(session).or_default().started = true,
            // A tool call that needs no approval — activity only; still marks the session seen.
            SessionEvent::ToolStarted => {
                self.sessions.entry(session).or_default();
            }
            // The DELIBERATE clear, scoped to THIS session only.
            SessionEvent::Resolved => self.sessions.entry(session).or_default().pending = None,
            SessionEvent::Stopped => self.sessions.entry(session).or_default().stopped = true,
        }
    }

    /// Observe a cumulative desktop token total, crediting the monotonic delta through the
    /// [`TokenLatch`](buddy_core::TokenLatch) so a backwards total (a restarted transcript)
    /// re-baselines instead of
    /// double-counting. A `0` or absent signal is acceptable — see the module docs.
    pub fn observe_tokens(&mut self, observed_total: u32) {
        // The latch credits 0 on boot and on a backwards total, and the delta otherwise; a restart
        // resending a cumulative total therefore never double-counts.
        self.credited_total = self
            .credited_total
            .saturating_add(self.latch.observe(observed_total));
    }

    /// Synthesize the heartbeat snapshot for the current state and clear the `completed` pulse.
    ///
    /// The packet carries the outstanding prompt whenever one is live; keep-prior fields the device
    /// merges asymmetrically are left `None` when unchanged. Pass the result through
    /// [`GatedSnapshot`](crate::gating::GatedSnapshot) before serializing.
    pub fn synthesize(&mut self) -> SnapshotPacket {
        let total: u8 = cap_u8(self.sessions.len());
        let running: u8 = cap_u8(
            self.sessions
                .values()
                .filter(|state: &&SessionState| state.is_running())
                .count(),
        );
        let waiting: u8 = cap_u8(
            self.sessions
                .values()
                .filter(|state: &&SessionState| state.is_waiting())
                .count(),
        );
        // The `completed` pulse is destructive-on-absent: emit Some(true) this once, then clear so
        // the next synthesis leaves it absent (which resets it to false on the device).
        let completed: Option<bool> = self.completed_pulse.then_some(true);
        self.completed_pulse = false;
        SnapshotPacket {
            // The ledger is authoritative on the counts, so it always transmits them (including a
            // drop to zero); an absent count would strand a stale value on the glass.
            total: Some(total),
            running: Some(running),
            waiting: Some(waiting),
            completed,
            // The accumulated credited total; zeros are honest — the device latches its first total.
            tokens: Some(self.credited_total),
            // Keep-prior fields the device merges asymmetrically stay absent when this crate has no
            // signal for them.
            tokens_today: None,
            msg: None,
            entries: None,
            // The live approval rides every heartbeat; absent only when NO session still pends.
            prompt: self.outstanding_prompt().cloned(),
        }
    }

    /// The live outstanding prompt, if any — the input the emission gate guards against.
    ///
    /// Derived, never stored: the most recently raised prompt among the sessions that still pend.
    /// This is the invariant's keystone — `waiting > 0` (any session pends) holds if and only if
    /// this returns `Some`, so a synthesized packet can never carry `prompt == None` while a
    /// session still waits, and a cross-session resolve cannot clear a live approval.
    pub fn outstanding_prompt(&self) -> Option<&Prompt> {
        self.sessions
            .values()
            .filter_map(|state: &SessionState| state.pending.as_ref())
            .max_by_key(|pending: &&Pending| pending.seq)
            .map(|pending: &Pending| &pending.prompt)
    }
}

/// Cap a session count into a `u8`, saturating at [`u8::MAX`] rather than wrapping.
fn cap_u8(count: usize) -> u8 {
    u8::try_from(count).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A prompt fixture — deterministic so a fold is reproducible.
    fn a_prompt(id: &str) -> Prompt {
        Prompt {
            id: id.to_string(),
            tool: "bash".to_string(),
            hint: "rm -rf".to_string(),
        }
    }

    /// No sessions synthesize to zero counts, all present as Some.
    #[test]
    fn no_sessions_are_zero_counts() {
        let mut ledger: Ledger = Ledger::new();
        let packet: SnapshotPacket = ledger.synthesize();
        assert_eq!(packet.total, Some(0));
        assert_eq!(packet.running, Some(0));
        assert_eq!(packet.waiting, Some(0));
    }

    /// One started session is one running session.
    #[test]
    fn one_started_session_runs() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(SessionId("s1".to_string()), SessionEvent::Started);
        let packet: SnapshotPacket = ledger.synthesize();
        assert_eq!(packet.total, Some(1));
        assert_eq!(packet.running, Some(1));
        assert_eq!(packet.waiting, Some(0));
    }

    /// A stopped session stays counted in the total but leaves the running count.
    #[test]
    fn a_stopped_session_leaves_running_but_stays_in_total() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(SessionId("s1".to_string()), SessionEvent::Started);
        ledger.record(SessionId("s2".to_string()), SessionEvent::Started);
        ledger.record(SessionId("s2".to_string()), SessionEvent::Stopped);
        let packet: SnapshotPacket = ledger.synthesize();
        assert_eq!(packet.total, Some(2));
        assert_eq!(packet.running, Some(1));
    }

    /// A session awaiting approval is a waiting session and raises the outstanding prompt.
    #[test]
    fn an_awaiting_session_is_waiting() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(SessionId("s1".to_string()), SessionEvent::Started);
        ledger.record(
            SessionId("s1".to_string()),
            SessionEvent::AwaitingApproval(a_prompt("req_1")),
        );
        let packet: SnapshotPacket = ledger.synthesize();
        assert_eq!(packet.waiting, Some(1));
        assert!(ledger.outstanding_prompt().is_some());
    }

    /// The first observed token total credits nothing (boot latch).
    #[test]
    fn the_first_token_total_credits_nothing() {
        let mut ledger: Ledger = Ledger::new();
        ledger.observe_tokens(1_000);
        assert_eq!(ledger.synthesize().tokens, Some(0));
    }

    /// A forward token total credits the delta.
    #[test]
    fn a_forward_total_credits_the_delta() {
        let mut ledger: Ledger = Ledger::new();
        ledger.observe_tokens(1_000);
        ledger.observe_tokens(1_500);
        assert_eq!(ledger.synthesize().tokens, Some(500));
    }

    /// A backwards total (a restart) credits nothing and never double-counts.
    #[test]
    fn a_backwards_total_never_double_counts() {
        let mut ledger: Ledger = Ledger::new();
        ledger.observe_tokens(1_000);
        ledger.observe_tokens(1_500);
        ledger.observe_tokens(300);
        assert_eq!(ledger.synthesize().tokens, Some(500));
    }

    /// An explicit resolve clears the outstanding prompt.
    #[test]
    fn a_resolve_clears_the_prompt() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(
            SessionId("s1".to_string()),
            SessionEvent::AwaitingApproval(a_prompt("req_1")),
        );
        ledger.record(SessionId("s1".to_string()), SessionEvent::Resolved);
        assert!(ledger.outstanding_prompt().is_none());
        assert_eq!(ledger.synthesize().prompt, None);
    }

    /// A repeated synthesis keeps a live prompt on the glass (no keepalive side effect).
    #[test]
    fn a_live_prompt_rides_repeated_synthesis() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(
            SessionId("s1".to_string()),
            SessionEvent::AwaitingApproval(a_prompt("req_1")),
        );
        assert!(ledger.synthesize().prompt.is_some());
        assert!(ledger.synthesize().prompt.is_some());
    }

    /// The device-side hazard, made unrepresentable: a resolve on an UNRELATED session must not
    /// clear the glass while another session still waits. (Regresses to `prompt == None, waiting
    /// == 1` under a single-global-outstanding model.)
    #[test]
    fn a_cross_session_resolve_keeps_a_still_waiting_prompt() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(
            SessionId("s1".to_string()),
            SessionEvent::AwaitingApproval(a_prompt("req_1")),
        );
        // An unrelated session that never awaited resolves — it must not touch s1's prompt.
        ledger.record(SessionId("s2".to_string()), SessionEvent::Resolved);
        let packet: SnapshotPacket = ledger.synthesize();
        assert_eq!(packet.waiting, Some(1), "s1 still waits");
        assert!(
            packet.prompt.is_some(),
            "a cross-session resolve must not clear a live approval"
        );
    }

    /// Two concurrent prompts: resolving the newer one restores the older to the glass rather than
    /// orphaning it. (Regresses to a lost prompt under a single-slot-overwrite model.)
    #[test]
    fn two_concurrent_prompts_survive_resolving_the_newer() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(
            SessionId("s1".to_string()),
            SessionEvent::AwaitingApproval(a_prompt("req_1")),
        );
        ledger.record(
            SessionId("s2".to_string()),
            SessionEvent::AwaitingApproval(a_prompt("req_2")),
        );
        // The newest raised prompt is on the glass.
        assert_eq!(
            ledger.outstanding_prompt().map(|p: &Prompt| p.id.clone()),
            Some("req_2".to_string())
        );
        ledger.record(SessionId("s2".to_string()), SessionEvent::Resolved);
        let packet: SnapshotPacket = ledger.synthesize();
        assert_eq!(packet.waiting, Some(1), "s1 still waits");
        assert_eq!(
            packet.prompt.map(|p: Prompt| p.id),
            Some("req_1".to_string()),
            "resolving the newer prompt restores the older to the glass"
        );
    }

    /// A session STOPPED while awaiting keeps its pending prompt — prompt-clear is deliberate-only
    /// (a `Resolved`), never a side effect of a stop.
    #[test]
    fn a_stopped_session_keeps_its_pending_prompt() {
        let mut ledger: Ledger = Ledger::new();
        ledger.record(
            SessionId("s1".to_string()),
            SessionEvent::AwaitingApproval(a_prompt("req_1")),
        );
        ledger.record(SessionId("s1".to_string()), SessionEvent::Stopped);
        let packet: SnapshotPacket = ledger.synthesize();
        assert_eq!(packet.waiting, Some(1), "a stop does not clear the prompt");
        assert!(packet.prompt.is_some(), "only a resolve clears the glass");
    }

    proptest! {
        /// The completeness invariant, over ANY event sequence: a synthesized packet NEVER carries
        /// `prompt == None` while `waiting > 0`. This ties the emitted prompt to the waiting count
        /// so the cross-session / concurrent-prompt hazard cannot re-enter.
        #[test]
        fn a_waiting_session_always_rides_a_prompt(events in an_event_sequence()) {
            let mut ledger: Ledger = Ledger::new();
            events
                .iter()
                .cloned()
                .for_each(|(session, event): (SessionId, SessionEvent)| {
                    ledger.record(session, event);
                });
            let packet: SnapshotPacket = ledger.synthesize();
            let waiting: u8 = packet.waiting.unwrap_or(0);
            // waiting > 0  ==>  prompt is Some.
            prop_assert!(waiting == 0 || packet.prompt.is_some());
        }
    }

    proptest! {
        /// Running and waiting each partition within the distinct-session total, over any sequence.
        #[test]
        fn running_and_waiting_stay_within_total(events in an_event_sequence()) {
            let mut ledger: Ledger = Ledger::new();
            events
                .iter()
                .cloned()
                .for_each(|(session, event): (SessionId, SessionEvent)| {
                    ledger.record(session, event);
                });
            let packet: SnapshotPacket = ledger.synthesize();
            let total: u8 = packet.total.unwrap_or(0);
            prop_assert!(packet.running.unwrap_or(0) <= total);
            prop_assert!(packet.waiting.unwrap_or(0) <= total);
        }
    }

    /// Arbitrary typed session events over a small session set.
    fn an_event_sequence() -> impl Strategy<Value = Vec<(SessionId, SessionEvent)>> {
        let one: BoxedStrategy<(SessionId, SessionEvent)> = (0u8..4, 0u8..6)
            .prop_map(|(sid, kind): (u8, u8)| {
                let event: SessionEvent = match kind {
                    0 => SessionEvent::Started,
                    1 => SessionEvent::ToolStarted,
                    2 => SessionEvent::AwaitingApproval(a_prompt(&format!("req_{sid}"))),
                    3 => SessionEvent::Resolved,
                    4 => SessionEvent::Completed,
                    _ => SessionEvent::Stopped,
                };
                (SessionId(format!("s{sid}")), event)
            })
            .boxed();
        proptest::collection::vec(one, 0..12)
    }
}
