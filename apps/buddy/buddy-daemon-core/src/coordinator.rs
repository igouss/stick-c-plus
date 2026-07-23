//! The [`Coordinator`] — the ECB Control that folds each input into an ordered [`Vec<Effect>`].
//!
//! It wraps a [`Ledger`](buddy_permission::Ledger) (the session -> snapshot fold), a monotonic
//! prompt-id counter, a pending map from prompt id to the waiting hook and its session, and a
//! `bonded` flag. Every method returns the effects the async daemon must perform, in order.
//!
//! ## The single emission chokepoint
//!
//! Every snapshot line is produced by exactly one private helper, [`Coordinator::emit_snapshot`],
//! which synthesizes the ledger and passes the packet through
//! [`GatedSnapshot::gate`](buddy_permission::GatedSnapshot) with the ledger's live outstanding
//! prompt BEFORE [`serialize_snapshot`](buddy_wire::serialize_snapshot). Because that is the sole
//! place a snapshot line is built, a bare keepalive that clears a live prompt is unrepresentable.

use crate::effect::{Effect, HookId};
use buddy_permission::{DaemonReply, GatedSnapshot, HookRequest, Ledger, SessionEvent, SessionId};
use buddy_wire::{
    serialize_owner, serialize_snapshot, serialize_time, Decision, PermissionResponse, Prompt,
};
use std::collections::HashMap;

/// The pure coordinator sequencing the permission loop. Holds only state, owns no effects.
#[derive(Debug, Default)]
pub struct Coordinator {
    /// The session -> snapshot fold: counts, tokens, and the derived outstanding prompt.
    ledger: Ledger,
    /// A monotonic counter stamped into each fresh prompt id (`req_{n}`).
    prompt_seq: u64,
    /// The prompts awaiting a device decision, keyed by prompt id, each naming the waiting hook and
    /// the session that raised it, so a device answer can reply the right hook and resolve the
    /// right session.
    pending: HashMap<String, (HookId, SessionId)>,
    /// Whether a link is currently bonded — the gate on whether a hook can be asked at all.
    bonded: bool,
}

impl Coordinator {
    /// A fresh coordinator: no sessions, no pending prompts, nothing bonded.
    pub fn new() -> Self {
        Coordinator::default()
    }

    /// The link came up: mark bonded and emit the handshake IN ORDER — the time sync, the owner
    /// name, then the current gated snapshot. Time is INJECTED (`epoch` / `tz_offset_s`); this crate
    /// reads no clock.
    pub fn on_link_up(&mut self, epoch: i64, tz_offset_s: i32, owner: &str) -> Vec<Effect> {
        self.bonded = true;
        // The handshake, IN ORDER: time sync, owner name, then the current gated snapshot. The
        // snapshot is produced last so it can never be confused with the time-sync line the device
        // parses as its own shape.
        vec![
            Effect::SendLink(serialize_time(epoch, tz_offset_s)),
            Effect::SendLink(serialize_owner(owner)),
            Effect::SendLink(self.emit_snapshot()),
        ]
    }

    /// The link went down: mark unbonded and DRAIN every pending approval — each becomes a
    /// [`DaemonReply::Unbonded`] reply to its hook plus a [`SessionEvent::Resolved`] recorded for its
    /// session so the prompt clears. No snapshot is emitted (the link is gone).
    pub fn on_link_down(&mut self) -> Vec<Effect> {
        self.bonded = false;
        // Drain every pending approval: each waiting hook gets an Unbonded reply, and its session's
        // prompt is deliberately Resolved so no stale approval lingers. No snapshot — the link is
        // gone, and nothing may be sent down it.
        let drained: Vec<(String, (HookId, SessionId))> = self.pending.drain().collect();
        drained
            .into_iter()
            .map(|(_id, (hook, session)): (String, (HookId, SessionId))| {
                self.ledger.record(session, SessionEvent::Resolved);
                Effect::ReplyHook(hook, DaemonReply::Unbonded)
            })
            .collect()
    }

    /// A hook arrived. If NOT bonded, answer exactly `[ReplyHook(hook, Unbonded)]` and raise no
    /// prompt (the HOST fail-safe: the hook degrades to the normal terminal prompt). If bonded,
    /// allocate a fresh prompt id (`req_{n}`), record an
    /// [`AwaitingApproval`](SessionEvent::AwaitingApproval) for the session, remember the pending
    /// (hook, session), and emit `[SendLink(gated snapshot carrying the new prompt)]`. Do NOT reply
    /// yet — the reply waits on the device.
    pub fn on_hook(&mut self, hook: HookId, req: &HookRequest) -> Vec<Effect> {
        // The HOST fail-safe: nothing bonded means the device cannot be asked, so answer exactly
        // Unbonded and raise NO prompt — the hook degrades to Claude Code's normal terminal prompt.
        if !self.bonded {
            return vec![Effect::ReplyHook(hook, DaemonReply::Unbonded)];
        }
        // Bonded: allocate a fresh prompt id, record the awaiting approval, remember the pending
        // (hook, session), and emit the gated snapshot carrying the new prompt. Do NOT reply yet —
        // the reply waits on the device's button press.
        let id: String = format!("req_{n}", n = self.prompt_seq);
        self.prompt_seq += 1;
        let session: SessionId = SessionId(req.session_id.clone());
        self.ledger.record(
            session.clone(),
            SessionEvent::AwaitingApproval(Prompt {
                id: id.clone(),
                tool: req.tool_name.clone(),
                hint: req.cwd.clone(),
            }),
        );
        self.pending.insert(id, (hook, session));
        vec![Effect::SendLink(self.emit_snapshot())]
    }

    /// A line arrived from the device. Parse it via
    /// [`PermissionResponse::parse`](buddy_wire::PermissionResponse::parse). On a well-formed
    /// response whose id is a KNOWN pending prompt: remove it, record
    /// [`SessionEvent::Resolved`] for its session, and return
    /// `[ReplyHook(hook, reply), SendLink(fresh gated snapshot)]` (`Once` -> `Allow`, `Deny` ->
    /// `Deny`). On an unknown id, or any parse error (non-permission, bad JSON,
    /// `BadDecision`), return `[]` — NEVER guess an approval.
    pub fn on_device_line(&mut self, line: &[u8]) -> Vec<Effect> {
        // A parse error (non-permission cmd, bad JSON, or a BadDecision token that is neither once
        // nor deny) is never a guessed approval — it yields no effect at all.
        let response: PermissionResponse = match PermissionResponse::parse(line) {
            Ok(response) => response,
            Err(_) => return Vec::new(),
        };
        // A well-formed response for an UNKNOWN pending id is likewise ignored — no reply, no clear.
        let (hook, session): (HookId, SessionId) = match self.pending.remove(&response.id) {
            Some(pending) => pending,
            None => return Vec::new(),
        };
        // A known id: record the deliberate Resolved so the fresh snapshot clears the glass, and
        // reply the matching decision to exactly the hook that raised this prompt.
        self.ledger.record(session, SessionEvent::Resolved);
        let reply: DaemonReply = match response.decision {
            Decision::Once => DaemonReply::Allow,
            Decision::Deny => DaemonReply::Deny,
        };
        vec![
            Effect::ReplyHook(hook, reply),
            Effect::SendLink(self.emit_snapshot()),
        ]
    }

    /// Fold a non-approval lifecycle event ([`Started`](SessionEvent::Started) /
    /// [`ToolStarted`](SessionEvent::ToolStarted) / [`Completed`](SessionEvent::Completed) /
    /// [`Stopped`](SessionEvent::Stopped)) for `session`; if bonded return
    /// `[SendLink(gated snapshot)]`, else `[]`. It must NOT accept
    /// [`AwaitingApproval`](SessionEvent::AwaitingApproval) or [`Resolved`](SessionEvent::Resolved)
    /// — those are owned by [`on_hook`](Coordinator::on_hook) /
    /// [`on_device_line`](Coordinator::on_device_line) / [`on_link_down`](Coordinator::on_link_down).
    pub fn on_session_event(&mut self, session: SessionId, event: SessionEvent) -> Vec<Effect> {
        // Only a non-approval lifecycle event is folded here. The approval variants are owned by
        // on_hook / on_device_line / on_link_down — which also keep the `pending` map in step with
        // the ledger — so an AwaitingApproval / Resolved arriving through this door is ignored rather
        // than allowed to mutate approval state behind the pending map's back.
        if is_lifecycle(&event) {
            self.ledger.record(session, event);
        }
        self.emit_if_bonded()
    }

    /// A keepalive tick: if bonded return `[SendLink(gated snapshot)]`, else `[]`. The snapshot is
    /// built by the single gated helper, so a keepalive can never clear a live prompt.
    pub fn on_keepalive(&mut self) -> Vec<Effect> {
        self.emit_if_bonded()
    }

    /// Emit the current gated snapshot iff bonded, else nothing. The single place `bonded` decides
    /// whether a heartbeat reaches the link, shared by the keepalive and the lifecycle fold.
    fn emit_if_bonded(&mut self) -> Vec<Effect> {
        if self.bonded {
            vec![Effect::SendLink(self.emit_snapshot())]
        } else {
            Vec::new()
        }
    }

    /// The SOLE place a snapshot line is produced: synthesize the ledger, gate the packet against
    /// the ledger's live outstanding prompt, then serialize. Because every emitted snapshot flows
    /// through here, a bare keepalive that clears a live prompt is unrepresentable.
    fn emit_snapshot(&mut self) -> String {
        let packet: buddy_wire::SnapshotPacket = self.ledger.synthesize();
        let gated: GatedSnapshot =
            GatedSnapshot::gate(packet, self.ledger.outstanding_prompt().cloned());
        serialize_snapshot(gated.packet())
    }
}

/// Whether a session event is a non-approval lifecycle event — the only kind
/// [`Coordinator::on_session_event`] folds. The two approval variants
/// ([`AwaitingApproval`](SessionEvent::AwaitingApproval) / [`Resolved`](SessionEvent::Resolved))
/// are owned by the hook / device / link-down folds and are rejected here.
fn is_lifecycle(event: &SessionEvent) -> bool {
    match event {
        SessionEvent::Started
        | SessionEvent::ToolStarted
        | SessionEvent::Completed
        | SessionEvent::Stopped => true,
        SessionEvent::AwaitingApproval(_) | SessionEvent::Resolved => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The injected handshake context — fixed so every emitted line is deterministic.
    const EPOCH: i64 = 1_700_000_000;
    /// The injected tz offset, in seconds.
    const TZ: i32 = -14_400;
    /// The injected owner label.
    const OWNER: &str = "Iouri";

    // ── Fixtures & observation helpers (all branching lives here, never in a test body) ──────────

    /// A hook request for `session` invoking `tool`; the other fields are fixed context.
    fn a_request(session: &str, tool: &str) -> HookRequest {
        HookRequest {
            session_id: session.to_string(),
            tool_name: tool.to_string(),
            cwd: "/repo".to_string(),
            permission_mode: "default".to_string(),
            transcript_path: "/tmp/t.jsonl".to_string(),
        }
    }

    /// The serialized line of a `SendLink` effect, or `None` for a `ReplyHook`.
    fn send_line(effect: &Effect) -> Option<String> {
        match effect {
            Effect::SendLink(line) => Some(line.clone()),
            Effect::ReplyHook(_, _) => None,
        }
    }

    /// Every `SendLink` line among the effects, in order.
    fn send_lines(effects: &[Effect]) -> Vec<String> {
        effects.iter().filter_map(send_line).collect()
    }

    /// The `(hook, reply)` pairs among the effects, in order.
    fn replies(effects: &[Effect]) -> Vec<(HookId, DaemonReply)> {
        effects
            .iter()
            .filter_map(|effect: &Effect| match effect {
                Effect::ReplyHook(hook, reply) => Some((*hook, *reply)),
                Effect::SendLink(_) => None,
            })
            .collect()
    }

    /// Whether a serialized snapshot line carries a `prompt` key.
    fn line_has_prompt(line: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .map(|value: serde_json::Value| value.get("prompt").is_some())
            .unwrap_or(false)
    }

    /// The `prompt.id` of the last snapshot line among the effects, if any.
    fn last_prompt_id(effects: &[Effect]) -> Option<String> {
        send_lines(effects)
            .iter()
            .filter_map(|line: &String| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()?
                    .get("prompt")?
                    .get("id")?
                    .as_str()
                    .map(str::to_string)
            })
            .next_back()
    }

    /// Raise a prompt for `session` on a bonded `coordinator` and return the emitted prompt id.
    fn raise(coordinator: &mut Coordinator, hook: HookId, session: &str) -> String {
        let effects: Vec<Effect> = coordinator.on_hook(hook, &a_request(session, "bash"));
        last_prompt_id(&effects).expect("a bonded hook raises a prompt")
    }

    /// A bonded coordinator (link brought up, handshake discarded).
    fn bonded() -> Coordinator {
        let mut coordinator: Coordinator = Coordinator::new();
        coordinator.on_link_up(EPOCH, TZ, OWNER);
        coordinator
    }

    // ── new() and the handshake ─────────────────────────────────────────────────────────────────

    /// A fresh coordinator is unbonded: a hook is answered exactly Unbonded, nothing else.
    #[test]
    fn a_fresh_coordinator_answers_a_hook_unbonded() {
        let mut coordinator: Coordinator = Coordinator::new();
        let effects: Vec<Effect> = coordinator.on_hook(HookId(0), &a_request("s1", "bash"));
        assert_eq!(
            effects,
            vec![Effect::ReplyHook(HookId(0), DaemonReply::Unbonded)]
        );
    }

    /// The link-up handshake is exactly time, owner, snapshot — three SendLink lines in order.
    #[test]
    fn link_up_emits_time_owner_snapshot_in_order() {
        let mut coordinator: Coordinator = Coordinator::new();
        let lines: Vec<String> = send_lines(&coordinator.on_link_up(EPOCH, TZ, OWNER));
        let mut fresh: Coordinator = Coordinator::new();
        let empty_snapshot: String = serialize_snapshot(&fresh.ledger.synthesize());
        assert_eq!(
            lines,
            vec![
                format!(r#"{{"time":[{EPOCH},{TZ}]}}"#),
                serialize_owner(OWNER),
                empty_snapshot,
            ]
        );
    }

    /// The first handshake line is the exact two-element time sync — never a wrong arity that would
    /// fall through to the device's snapshot branch and clear a prompt.
    #[test]
    fn the_time_sync_line_is_exactly_the_two_element_array() {
        let mut coordinator: Coordinator = Coordinator::new();
        let lines: Vec<String> = send_lines(&coordinator.on_link_up(EPOCH, TZ, OWNER));
        assert_eq!(
            lines.first(),
            Some(&format!(r#"{{"time":[{EPOCH},{TZ}]}}"#))
        );
    }

    // ── on_hook: raise a prompt, reply nothing yet ──────────────────────────────────────────────

    /// A bonded hook emits exactly one snapshot carrying the fresh prompt, and no reply yet.
    #[test]
    fn a_bonded_hook_raises_a_prompt_without_replying() {
        let mut coordinator: Coordinator = bonded();
        let effects: Vec<Effect> = coordinator.on_hook(HookId(1), &a_request("s1", "bash"));
        assert!(replies(&effects).is_empty(), "the hook waits on the device");
        assert!(
            last_prompt_id(&effects).is_some(),
            "the snapshot carries the prompt"
        );
    }

    /// An unbonded hook raises NO prompt: none resurfaces on the subsequent link-up handshake.
    #[test]
    fn an_unbonded_hook_raises_no_prompt() {
        let mut coordinator: Coordinator = Coordinator::new();
        coordinator.on_hook(HookId(0), &a_request("s1", "bash"));
        let handshake: Vec<Effect> = coordinator.on_link_up(EPOCH, TZ, OWNER);
        assert!(
            !line_has_prompt(send_lines(&handshake).last().expect("a handshake snapshot")),
            "an unbonded hook must raise no prompt to resurface"
        );
    }

    // ── on_device_line: the fail-safe routing ───────────────────────────────────────────────────

    /// A device `once` answering the raising hook replies Allow and clears the prompt off the glass.
    #[test]
    fn a_device_once_replies_allow_and_clears_the_prompt() {
        let mut coordinator: Coordinator = bonded();
        let id: String = raise(&mut coordinator, HookId(7), "s1");
        let line: String = PermissionResponse::new(id, Decision::Once).to_json();
        let effects: Vec<Effect> = coordinator.on_device_line(line.as_bytes());
        assert!(replies(&effects).contains(&(HookId(7), DaemonReply::Allow)));
        assert!(!line_has_prompt(
            send_lines(&effects).last().expect("a fresh snapshot")
        ));
    }

    /// A device `deny` answering the raising hook replies Deny and clears the prompt.
    #[test]
    fn a_device_deny_replies_deny_and_clears_the_prompt() {
        let mut coordinator: Coordinator = bonded();
        let id: String = raise(&mut coordinator, HookId(7), "s1");
        let line: String = PermissionResponse::new(id, Decision::Deny).to_json();
        let effects: Vec<Effect> = coordinator.on_device_line(line.as_bytes());
        assert!(replies(&effects).contains(&(HookId(7), DaemonReply::Deny)));
        assert!(!line_has_prompt(
            send_lines(&effects).last().expect("a fresh snapshot")
        ));
    }

    /// A device line for an UNKNOWN pending id yields no effect — never a guessed approval.
    #[test]
    fn an_unknown_id_yields_no_effect() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        let line: String =
            PermissionResponse::new("req_nope".to_string(), Decision::Once).to_json();
        assert!(coordinator.on_device_line(line.as_bytes()).is_empty());
    }

    /// A bad-decision token yields no effect — mirrors buddy_wire's BadDecision-is-never-a-silent-Once.
    #[test]
    fn a_bad_decision_token_yields_no_effect() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        let line: &[u8] = br#"{"cmd":"permission","id":"req_0","decision":"maybe"}"#;
        assert!(coordinator.on_device_line(line).is_empty());
    }

    /// Unparseable bytes yield no effect and never panic.
    #[test]
    fn unparseable_bytes_yield_no_effect() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        assert!(coordinator.on_device_line(b"not json").is_empty());
    }

    /// An ignored device line leaves the live prompt on the glass for the next keepalive.
    #[test]
    fn an_ignored_line_leaves_the_prompt_on_the_next_keepalive() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        coordinator.on_device_line(b"not json");
        let keepalive: Vec<Effect> = coordinator.on_keepalive();
        assert!(line_has_prompt(
            send_lines(&keepalive).last().expect("a keepalive snapshot")
        ));
    }

    // ── Concurrent prompts route with no cross-talk ─────────────────────────────────────────────

    /// Two pending prompts: resolving s2 replies s2's hook, leaves s1's hook unanswered, and keeps a
    /// prompt on the glass.
    #[test]
    fn resolving_one_of_two_prompts_leaves_the_other_on_the_glass() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        let id2: String = raise(&mut coordinator, HookId(2), "s2");
        let line: String = PermissionResponse::new(id2, Decision::Once).to_json();
        let effects: Vec<Effect> = coordinator.on_device_line(line.as_bytes());
        assert!(replies(&effects).contains(&(HookId(2), DaemonReply::Allow)));
        assert!(
            !replies(&effects)
                .iter()
                .any(|(hook, _): &(HookId, DaemonReply)| *hook == HookId(1)),
            "resolving s2 must not answer s1's hook"
        );
        assert!(line_has_prompt(
            send_lines(&effects).last().expect("a fresh snapshot")
        ));
    }

    // ── Keepalive & lifecycle never clear a live prompt ─────────────────────────────────────────

    /// A keepalive while a prompt is outstanding still carries the prompt.
    #[test]
    fn a_keepalive_keeps_a_live_prompt() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        let keepalive: Vec<Effect> = coordinator.on_keepalive();
        assert!(line_has_prompt(
            send_lines(&keepalive).last().expect("a keepalive snapshot")
        ));
    }

    /// An unrelated lifecycle event while a prompt is outstanding still carries the prompt.
    #[test]
    fn an_unrelated_lifecycle_event_keeps_a_live_prompt() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        let effects: Vec<Effect> =
            coordinator.on_session_event(SessionId("other".to_string()), SessionEvent::Started);
        assert!(line_has_prompt(
            send_lines(&effects).last().expect("a lifecycle snapshot")
        ));
    }

    /// on_session_event ignores the approval variants: a Resolved arriving through this door does
    /// NOT clear a live prompt raised via on_hook (the pending map stays authoritative).
    #[test]
    fn on_session_event_ignores_a_resolved_and_keeps_the_prompt() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        let effects: Vec<Effect> =
            coordinator.on_session_event(SessionId("s1".to_string()), SessionEvent::Resolved);
        assert!(line_has_prompt(
            send_lines(&effects).last().expect("a snapshot")
        ));
    }

    /// on_session_event ignores the OTHER approval variant symmetrically: an AwaitingApproval
    /// arriving through this door does NOT raise a prompt behind the pending map's back — on a
    /// bonded coordinator with nothing pending, the emitted snapshot stays prompt-less. (If
    /// `is_lifecycle` widened to accept AwaitingApproval, the ledger would record it and this
    /// snapshot would carry a prompt — the mutation this test catches.)
    #[test]
    fn on_session_event_ignores_an_awaiting_approval_and_raises_no_prompt() {
        let mut coordinator: Coordinator = bonded();
        let effects: Vec<Effect> = coordinator.on_session_event(
            SessionId("s1".to_string()),
            SessionEvent::AwaitingApproval(Prompt {
                id: "req_smuggled".to_string(),
                tool: "bash".to_string(),
                hint: "/repo".to_string(),
            }),
        );
        assert!(
            !line_has_prompt(send_lines(&effects).last().expect("a snapshot")),
            "an AwaitingApproval through on_session_event must raise no prompt"
        );
    }

    /// A keepalive on an unbonded coordinator sends nothing.
    #[test]
    fn a_keepalive_while_unbonded_sends_nothing() {
        let mut coordinator: Coordinator = Coordinator::new();
        assert!(coordinator.on_keepalive().is_empty());
    }

    // ── link-down drains: zero / one / many ─────────────────────────────────────────────────────

    /// A link-down with nothing pending answers no hook and sends no snapshot.
    #[test]
    fn a_link_down_with_nothing_pending_answers_no_hook() {
        let mut coordinator: Coordinator = bonded();
        let effects: Vec<Effect> = coordinator.on_link_down();
        assert!(effects.is_empty());
    }

    /// A link-down with one pending hook drains it with Unbonded and sends no snapshot.
    #[test]
    fn a_link_down_drains_one_pending_hook() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        let effects: Vec<Effect> = coordinator.on_link_down();
        assert_eq!(
            effects,
            vec![Effect::ReplyHook(HookId(1), DaemonReply::Unbonded)]
        );
    }

    /// A link-down with two pending hooks drains BOTH with Unbonded and sends no snapshot.
    #[test]
    fn a_link_down_drains_every_pending_hook() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        raise(&mut coordinator, HookId(2), "s2");
        let drained: Vec<(HookId, DaemonReply)> = replies(&coordinator.on_link_down());
        assert!(drained.contains(&(HookId(1), DaemonReply::Unbonded)));
        assert!(drained.contains(&(HookId(2), DaemonReply::Unbonded)));
        assert_eq!(
            drained.len(),
            2,
            "exactly the two waiting hooks, no snapshot"
        );
    }

    /// After a link-down clears the prompts, a fresh link-up handshake carries no stale prompt.
    #[test]
    fn a_link_down_leaves_no_stale_prompt_for_the_next_link_up() {
        let mut coordinator: Coordinator = bonded();
        raise(&mut coordinator, HookId(1), "s1");
        coordinator.on_link_down();
        let handshake: Vec<Effect> = coordinator.on_link_up(EPOCH, TZ, OWNER);
        assert!(
            !line_has_prompt(send_lines(&handshake).last().expect("a handshake snapshot")),
            "a drained link leaves no stale prompt on the glass"
        );
    }

    // ── Interleaved raise/answer routing (the fine grain the Gherkin does not reach) ────────────

    /// One step of an interleaved raise/answer plan: raise a fresh prompt, or answer one of the
    /// currently-pending prompts. Answer carries an opaque selector taken modulo the live pending
    /// count at apply time, so an answer against an empty set is a well-formed no-op.
    #[derive(Clone, Debug)]
    enum RaiseAnswer {
        /// Raise a new prompt for a fresh hook/session.
        Raise,
        /// Answer one currently-pending prompt, chosen by `selector % pending_len`.
        Answer(usize),
    }

    /// The running state of an interleaving fold: the coordinator, a monotonic hook/session source,
    /// the prompts still on the glass (hook + id), and whether every answer so far routed cleanly.
    struct Interleave {
        /// The coordinator being driven through interleaved raises and answers.
        coordinator: Coordinator,
        /// A monotonic source of hook handles and session names.
        next: u64,
        /// The prompts raised but not yet answered — each (hook, prompt id).
        pending: Vec<(HookId, String)>,
        /// Whether every answer routed to exactly its own hook and nothing else, so far.
        ok: bool,
    }

    impl Interleave {
        /// A bonded, empty interleaving state.
        fn new() -> Self {
            Interleave {
                coordinator: bonded(),
                next: 0,
                pending: Vec::new(),
                ok: true,
            }
        }
    }

    /// Answer the pending prompt at `selector % pending_len` (a no-op when nothing is pending):
    /// exactly one reply, routed to that prompt's own hook, or `ok` goes false. Removing the
    /// answered entry leaves the rest of `pending` untouched — the no-cross-talk claim.
    fn answer_one(mut state: Interleave, selector: usize) -> Interleave {
        // checked_rem yields None when pending is empty, so into_iter folds zero-or-one times — no
        // branch in the caller, and an answer against nothing is a clean no-op.
        let index: Option<usize> = selector.checked_rem(state.pending.len());
        index.into_iter().for_each(|index: usize| {
            let (hook, id): (HookId, String) = state.pending.remove(index);
            let line: String = PermissionResponse::new(id, Decision::Once).to_json();
            let effects: Vec<Effect> = state.coordinator.on_device_line(line.as_bytes());
            let routed: bool = replies(&effects) == vec![(hook, DaemonReply::Allow)];
            state.ok = state.ok && routed;
        });
        state
    }

    /// Fold one interleaving op into the state: a raise pushes a fresh pending prompt, an answer
    /// resolves one of the live prompts.
    fn step(mut state: Interleave, op: &RaiseAnswer) -> Interleave {
        match op {
            RaiseAnswer::Raise => {
                let hook: HookId = HookId(state.next);
                let session: String = format!("s{n}", n = state.next);
                let id: String = raise(&mut state.coordinator, hook, &session);
                state.next += 1;
                state.pending.push((hook, id));
                state
            }
            RaiseAnswer::Answer(selector) => answer_one(state, *selector),
        }
    }

    /// Arbitrary interleaved raise/answer plans — raises weighted higher so answers have live
    /// targets, over sequences of length zero..twelve (zero / one / many).
    fn a_raise_answer_ops() -> impl Strategy<Value = Vec<RaiseAnswer>> {
        let one: BoxedStrategy<RaiseAnswer> = prop_oneof![
            2 => Just(RaiseAnswer::Raise),
            1 => (0usize..64).prop_map(RaiseAnswer::Answer),
        ]
        .boxed();
        proptest::collection::vec(one, 0..12)
    }

    // ── Properties (the fine grain the Gherkin does not reach) ──────────────────────────────────

    proptest! {
        /// GENUINELY interleaved raise-while-others-pending and answer: over any plan that raises
        /// and answers in any interleaving, every device answer routes to exactly the hook that
        /// raised that prompt id and to no other — the no-cross-talk invariant under interleaving,
        /// beyond the cucumber property's raise-all-then-permute-answers order.
        #[test]
        fn interleaved_raises_and_answers_route_each_to_its_own_hook(ops in a_raise_answer_ops()) {
            let folded: Interleave = ops
                .iter()
                .fold(Interleave::new(), |state: Interleave, op: &RaiseAnswer| step(state, op));
            prop_assert!(
                folded.ok,
                "each answer routes to exactly its own hook, whatever the raise/answer interleaving"
            );
        }

        /// Fresh prompt ids are distinct across many hooks — no two waiting hooks ever share an id
        /// (which would cross-route a device answer). Over 1..=5 concurrent raises.
        #[test]
        fn fresh_prompt_ids_are_distinct(count in 1usize..6) {
            let mut coordinator: Coordinator = bonded();
            let ids: std::collections::BTreeSet<String> = (0..count)
                .map(|index: usize| raise(&mut coordinator, HookId(index as u64), &format!("s{index}")))
                .collect();
            prop_assert_eq!(ids.len(), count, "every raised prompt id is distinct");
        }

        /// on_device_line never fabricates a reply from arbitrary bytes when nothing is pending —
        /// the daemon never guesses an approval for a device it never asked.
        #[test]
        fn arbitrary_bytes_never_fabricate_a_reply(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            let mut coordinator: Coordinator = bonded();
            let effects: Vec<Effect> = coordinator.on_device_line(&bytes);
            prop_assert!(replies(&effects).is_empty());
        }
    }
}
