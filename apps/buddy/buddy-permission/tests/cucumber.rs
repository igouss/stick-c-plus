//! Gherkin plumbing test for the bridge-side permission policy.
//!
//! These scenarios guard the policy seams at the domain boundary; the fine grain lives in the
//! source's own `#[cfg(test)]` modules once implemented. What they pin, invariant by invariant:
//!
//! - **1 — the fail-open guard**: [`decide`] maps EVERY non-decided outcome to
//!   [`HookOutput::Silent`], and only `Decided(Once)`/`Decided(Deny)` to an emit — proven by
//!   enumerating the whole non-decided failure space, not one example.
//! - **2 — no `ask`/`defer`, `Silent` renders empty**: a property over the whole output space.
//! - **3 — the exact PreToolUse JSON shape** for both decisions.
//! - **4 — ledger reconstruction** over zero / one / many sessions, plus a partition property.
//! - **5 — token latch reuse**: first credits 0, forward credits the delta, a backwards total
//!   (a restart) credits 0 and never double-counts.
//! - **6 — emission gating**: a live prompt rides every synthesis and clears only on resolve.
//! - **7 — IPC round-trip**: the DTOs serialize and deserialize back to the identical value.
//!
//! The source logic is still `unimplemented!()`, so the hook / ledger / gating scenarios are RED
//! by construction — the correct starting state. The IPC scenarios exercise the derived serde
//! codec, which is legitimately green.

use buddy_permission::{
    decide, to_stdout, AskOutcome, DaemonReply, GatedSnapshot, HookOutput, HookRequest, Ledger,
    Permission, PreToolUseDecision, SessionEvent, SessionId,
};
use buddy_wire::{Decision, Prompt, SnapshotPacket};
use cucumber::{given, then, when, World};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

/// The scenario state: the last hook decision, the folding ledger, the last synthesized packet,
/// and the last gated snapshot.
#[derive(Debug, Default, World)]
struct PermissionWorld {
    /// The last [`decide`] result, set by the outcome steps.
    output: Option<HookOutput>,
    /// The session-event fold under test.
    ledger: Ledger,
    /// The last snapshot the ledger synthesized.
    packet: Option<SnapshotPacket>,
    /// The last snapshot that passed the emission gate.
    gated: Option<GatedSnapshot>,
}

impl PermissionWorld {
    /// The last hook decision, or a panic — for the steps that assume an outcome was set.
    fn decision(&self) -> &HookOutput {
        self.output
            .as_ref()
            .expect("a scenario must set a device outcome first")
    }

    /// The last synthesized packet, or a panic — for the ledger / gating assertions.
    fn snapshot(&self) -> &SnapshotPacket {
        self.packet
            .as_ref()
            .expect("a scenario must synthesize a snapshot first")
    }
}

/// A fixed prompt used across the scenarios, so a fold is deterministic.
fn a_prompt(id: &str) -> Prompt {
    Prompt {
        id: id.to_string(),
        tool: "bash".to_string(),
        hint: "rm -rf".to_string(),
    }
}

/// A bare, prompt-less snapshot — a keepalive that would clear a live approval if it were emitted
/// while a prompt is outstanding.
fn a_bare_snapshot() -> SnapshotPacket {
    SnapshotPacket {
        running: Some(1),
        ..SnapshotPacket::default()
    }
}

// ---- Invariant 1 + 2 + 3: the fail-safe hook decision ----------------------------------------

#[when("the device outcome is a decision to allow")]
fn outcome_allow(world: &mut PermissionWorld) {
    world.output = Some(decide(AskOutcome::Decided(Decision::Once)));
}

#[when("the device outcome is a decision to deny")]
fn outcome_deny(world: &mut PermissionWorld) {
    world.output = Some(decide(AskOutcome::Decided(Decision::Deny)));
}

#[when("the device outcome is a timeout")]
fn outcome_timeout(world: &mut PermissionWorld) {
    world.output = Some(decide(AskOutcome::TimedOut));
}

#[when("the device outcome is a dead daemon")]
fn outcome_dead_daemon(world: &mut PermissionWorld) {
    world.output = Some(decide(AskOutcome::DaemonDown));
}

#[when("the device outcome is nothing bonded")]
fn outcome_unbonded(world: &mut PermissionWorld) {
    world.output = Some(decide(AskOutcome::Unbonded));
}

#[then("the hook emits an allow decision")]
fn emits_allow(world: &mut PermissionWorld) {
    match world.decision() {
        HookOutput::Emit(decision) => {
            assert_eq!(
                decision.permission,
                Permission::Allow,
                "an approval is an allow"
            )
        }
        HookOutput::Silent => panic!("a device approval must emit, not stay silent"),
    }
}

#[then("the hook emits a deny decision")]
fn emits_deny(world: &mut PermissionWorld) {
    match world.decision() {
        HookOutput::Emit(decision) => {
            assert_eq!(decision.permission, Permission::Deny, "a denial is a deny")
        }
        HookOutput::Silent => panic!("a device denial must emit, not stay silent"),
    }
}

#[then("the hook stays silent")]
fn stays_silent(world: &mut PermissionWorld) {
    // The load-bearing fail-open guard: a non-decided outcome NEVER emits.
    assert_eq!(
        world.decision(),
        &HookOutput::Silent,
        "a non-decided outcome must print nothing so the normal prompt takes over"
    );
}

#[then("the printed line is empty")]
fn printed_empty(world: &mut PermissionWorld) {
    // Silent must render to the EMPTY string, not "{}" and not "null".
    assert_eq!(
        to_stdout(world.decision()),
        "",
        "Silent renders to the empty string"
    );
}

#[then("the printed line is exactly the PreToolUse allow JSON")]
fn printed_allow_json(world: &mut PermissionWorld) {
    assert_pretooluse_json(&to_stdout(world.decision()), "allow");
}

#[then("the printed line is exactly the PreToolUse deny JSON")]
fn printed_deny_json(world: &mut PermissionWorld) {
    assert_pretooluse_json(&to_stdout(world.decision()), "deny");
}

/// Assert the rendered line is exactly the frozen PreToolUse shape with the expected decision.
fn assert_pretooluse_json(line: &str, expected_decision: &str) {
    let value: serde_json::Value =
        serde_json::from_str(line).expect("the emitted line is valid JSON");
    let object: &serde_json::Map<String, serde_json::Value> = value
        .as_object()
        .expect("the emitted line is a JSON object");
    // Exactly one top-level key: hookSpecificOutput.
    assert_eq!(object.len(), 1, "only hookSpecificOutput at the top level");
    let hso: &serde_json::Value = &value["hookSpecificOutput"];
    assert_eq!(
        hso["hookEventName"], "PreToolUse",
        "the event name is fixed"
    );
    assert_eq!(
        hso["permissionDecision"], expected_decision,
        "permissionDecision is only ever allow or deny"
    );
    assert!(
        hso["permissionDecisionReason"].is_string(),
        "a reason string is always present"
    );
    // The one thing it must NEVER say.
    assert!(!line.contains("\"ask\""), "never emits ask");
    assert!(!line.contains("\"defer\""), "never emits defer");
}

#[then("every non-decided outcome is silent")]
fn every_non_decided_is_silent(_world: &mut PermissionWorld) {
    // Invariant 1, totality: enumerate the WHOLE non-decided failure space and prove each is
    // Silent — the "break it" proof that there is no path from a failure to an emit.
    let space: BoxedStrategy<AskOutcome> = prop_oneof![
        Just(AskOutcome::TimedOut),
        Just(AskOutcome::DaemonDown),
        Just(AskOutcome::Unbonded),
    ]
    .boxed();
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(&space, |outcome: AskOutcome| {
            prop_assert_eq!(
                decide(outcome),
                HookOutput::Silent,
                "a non-decided outcome must never emit"
            );
            Ok(())
        })
        .expect("every non-decided outcome is Silent");
}

#[then("no hook output ever contains ask or defer")]
fn no_output_contains_ask_or_defer(_world: &mut PermissionWorld) {
    // Invariant 2, totality: over the whole HookOutput space (Silent, and Emit of either
    // permission with any reason), the rendered stdout never contains "ask" or "defer".
    let outputs: BoxedStrategy<HookOutput> = prop_oneof![
        Just(HookOutput::Silent),
        (any::<bool>(), ".*").prop_map(|(deny, reason): (bool, String)| {
            let permission: Permission = if deny {
                Permission::Deny
            } else {
                Permission::Allow
            };
            HookOutput::Emit(PreToolUseDecision { permission, reason })
        }),
    ]
    .boxed();
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(&outputs, |output: HookOutput| {
            let rendered: String = to_stdout(&output);
            prop_assert!(!rendered.contains("\"ask\""), "never renders ask");
            prop_assert!(!rendered.contains("\"defer\""), "never renders defer");
            Ok(())
        })
        .expect("no rendered output contains ask or defer");
}

// ---- Invariant 4 + 5: the ledger fold and the token latch ------------------------------------

#[given("a fresh ledger")]
fn a_fresh_ledger(world: &mut PermissionWorld) {
    world.ledger = Ledger::new();
    world.packet = None;
}

#[when(regex = r#"^session "([^"]*)" starts$"#)]
fn session_starts(world: &mut PermissionWorld, id: String) {
    world.ledger.record(SessionId(id), SessionEvent::Started);
}

#[when(regex = r#"^session "([^"]*)" stops$"#)]
fn session_stops(world: &mut PermissionWorld, id: String) {
    world.ledger.record(SessionId(id), SessionEvent::Stopped);
}

#[when(regex = r#"^session "([^"]*)" awaits approval "([^"]*)"$"#)]
fn session_awaits(world: &mut PermissionWorld, id: String, req: String) {
    world.ledger.record(
        SessionId(id),
        SessionEvent::AwaitingApproval(a_prompt(&req)),
    );
}

#[when(regex = r#"^session "([^"]*)" resolves$"#)]
fn session_resolves(world: &mut PermissionWorld, id: String) {
    world.ledger.record(SessionId(id), SessionEvent::Resolved);
}

#[when(regex = r"^the daemon observes a token total of (\d+)$")]
fn observe_tokens(world: &mut PermissionWorld, total: u32) {
    world.ledger.observe_tokens(total);
}

#[when("the ledger synthesizes a snapshot")]
fn synthesize(world: &mut PermissionWorld) {
    world.packet = Some(world.ledger.synthesize());
}

#[when("the ledger synthesizes a snapshot again")]
fn synthesize_again(world: &mut PermissionWorld) {
    world.packet = Some(world.ledger.synthesize());
}

#[then(regex = r"^the snapshot reports (\d+) total, (\d+) running, (\d+) waiting$")]
fn snapshot_reports(world: &mut PermissionWorld, total: u8, running: u8, waiting: u8) {
    let packet: &SnapshotPacket = world.snapshot();
    assert_eq!(packet.total, Some(total), "total = distinct sessions");
    assert_eq!(
        packet.running,
        Some(running),
        "running = started-not-stopped"
    );
    assert_eq!(
        packet.waiting,
        Some(waiting),
        "waiting = sessions with an outstanding prompt"
    );
}

#[then(regex = r"^the snapshot credits (\d+) tokens$")]
fn snapshot_credits(world: &mut PermissionWorld, credited: u32) {
    assert_eq!(
        world.snapshot().tokens,
        Some(credited),
        "the ledger credits tokens through the TokenLatch, never double-counting a restart"
    );
}

#[then("folding any session events keeps running and waiting within the total")]
fn partition_property(_world: &mut PermissionWorld) {
    // Invariant 4, totality: over ANY sequence of typed session events, running and waiting are
    // each bounded by the total — running-and-not-running partition the distinct sessions.
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(
            &an_event_sequence(),
            |events: Vec<(SessionId, SessionEvent)>| {
                let mut ledger: Ledger = Ledger::new();
                events
                    .iter()
                    .cloned()
                    .for_each(|(session, event): (SessionId, SessionEvent)| {
                        ledger.record(session, event);
                    });
                let packet: SnapshotPacket = ledger.synthesize();
                let total: u8 = packet.total.unwrap_or(0);
                let running: u8 = packet.running.unwrap_or(0);
                let waiting: u8 = packet.waiting.unwrap_or(0);
                prop_assert!(running <= total, "running never exceeds total");
                prop_assert!(waiting <= total, "waiting never exceeds total");
                Ok(())
            },
        )
        .expect("running and waiting stay within the total");
}

/// Arbitrary typed session events over a small session set — the space the partition property
/// quantifies over. A small id range keeps the distinct count within a `u8`.
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

// ---- Invariant 6: emission gating ------------------------------------------------------------

#[then("the snapshot carries a prompt")]
fn snapshot_carries_a_prompt(world: &mut PermissionWorld) {
    // The anti-hazard pin: while a prompt is outstanding, the emitted snapshot MUST carry it, or
    // an absent prompt field would clear the live approval on the device.
    assert!(
        world.snapshot().prompt.is_some(),
        "a live prompt must ride the heartbeat"
    );
}

#[then("the snapshot carries no prompt")]
fn snapshot_carries_no_prompt(world: &mut PermissionWorld) {
    // The prompt clears ONLY on an explicit resolve — a deliberate signal, not a keepalive side
    // effect.
    assert!(
        world.snapshot().prompt.is_none(),
        "an explicit resolve clears the prompt"
    );
}

#[then("any synthesis while a prompt is live carries a prompt")]
fn live_prompt_survives_any_keepalive(_world: &mut PermissionWorld) {
    // Invariant 6, totality: raise a prompt, then fold ANY sequence of keepalive events (never a
    // resolve, never a stop of the awaiting session) and prove every synthesis still carries it.
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(
            &a_keepalive_sequence(),
            |extra: Vec<(SessionId, SessionEvent)>| {
                let mut ledger: Ledger = Ledger::new();
                ledger.record(SessionId("s1".to_string()), SessionEvent::Started);
                ledger.record(
                    SessionId("s1".to_string()),
                    SessionEvent::AwaitingApproval(a_prompt("req_1")),
                );
                extra
                    .iter()
                    .cloned()
                    .for_each(|(session, event): (SessionId, SessionEvent)| {
                        ledger.record(session, event);
                    });
                let packet: SnapshotPacket = ledger.synthesize();
                prop_assert!(
                    packet.prompt.is_some(),
                    "a keepalive must never clear a live prompt"
                );
                Ok(())
            },
        )
        .expect("a live prompt survives every keepalive sequence");
}

/// Keepalive events that must NOT clear a live prompt: Started / ToolStarted / Completed over
/// other sessions — never Resolved (the only deliberate clear) and never a Stop of the awaiting
/// session.
fn a_keepalive_sequence() -> impl Strategy<Value = Vec<(SessionId, SessionEvent)>> {
    let one: BoxedStrategy<(SessionId, SessionEvent)> = (0u8..4, 0u8..3)
        .prop_map(|(sid, kind): (u8, u8)| {
            let event: SessionEvent = match kind {
                0 => SessionEvent::Started,
                1 => SessionEvent::ToolStarted,
                _ => SessionEvent::Completed,
            };
            (SessionId(format!("k{sid}")), event)
        })
        .boxed();
    proptest::collection::vec(one, 0..12)
}

#[then("any waiting session always rides a prompt")]
fn any_waiting_session_always_rides_a_prompt(_world: &mut PermissionWorld) {
    // Invariant 6, completeness: over ANY event sequence, a synthesized packet never carries an
    // absent prompt while a session still waits — the tie between `waiting` and the emitted prompt
    // that keeps a cross-session resolve from clearing a live approval.
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(
            &an_event_sequence(),
            |events: Vec<(SessionId, SessionEvent)>| {
                let mut ledger: Ledger = Ledger::new();
                events
                    .iter()
                    .cloned()
                    .for_each(|(session, event): (SessionId, SessionEvent)| {
                        ledger.record(session, event);
                    });
                let packet: SnapshotPacket = ledger.synthesize();
                let waiting: u8 = packet.waiting.unwrap_or(0);
                prop_assert!(
                    waiting == 0 || packet.prompt.is_some(),
                    "a waiting session must ride a prompt"
                );
                Ok(())
            },
        )
        .expect("a waiting session always rides a prompt");
}

#[when(regex = r#"^a bare snapshot is gated against the outstanding prompt "([^"]*)"$"#)]
fn gate_with_outstanding(world: &mut PermissionWorld, req: String) {
    world.gated = Some(GatedSnapshot::gate(a_bare_snapshot(), Some(a_prompt(&req))));
}

#[when("a bare snapshot is gated against no outstanding prompt")]
fn gate_without_outstanding(world: &mut PermissionWorld) {
    world.gated = Some(GatedSnapshot::gate(a_bare_snapshot(), None));
}

#[then("the gated snapshot carries a prompt")]
fn gated_carries_a_prompt(world: &mut PermissionWorld) {
    let gated: &GatedSnapshot = world
        .gated
        .as_ref()
        .expect("a scenario must gate a snapshot first");
    assert!(
        gated.packet().prompt.is_some(),
        "the gate restores the outstanding prompt onto a bare snapshot"
    );
}

#[then("the gated snapshot carries no prompt")]
fn gated_carries_no_prompt(world: &mut PermissionWorld) {
    let gated: &GatedSnapshot = world
        .gated
        .as_ref()
        .expect("a scenario must gate a snapshot first");
    assert!(
        gated.packet().prompt.is_none(),
        "with no outstanding prompt the bare snapshot passes through, a deliberate clear"
    );
}

// ---- Invariant 7: IPC DTO round-trip ---------------------------------------------------------

#[then("any hook request serializes and deserializes back to itself")]
fn hook_request_round_trips(_world: &mut PermissionWorld) {
    // Invariant 7, totality: for ANY field values, the hook request survives a serde round-trip
    // so the hook and daemon agree on the contract.
    let requests: BoxedStrategy<HookRequest> = (".*", ".*", ".*", ".*", ".*")
        .prop_map(
            |(session_id, tool_name, cwd, permission_mode, transcript_path): (
                String,
                String,
                String,
                String,
                String,
            )| HookRequest {
                session_id,
                tool_name,
                cwd,
                permission_mode,
                transcript_path,
            },
        )
        .boxed();
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(&requests, |request: HookRequest| {
            let json: String =
                serde_json::to_string(&request).expect("a hook request always serializes");
            let back: HookRequest =
                serde_json::from_str(&json).expect("a hook request always deserializes");
            prop_assert_eq!(back, request);
            Ok(())
        })
        .expect("any hook request round-trips");
}

#[then("every daemon reply serializes and deserializes back to itself")]
fn daemon_reply_round_trips(_world: &mut PermissionWorld) {
    // Invariant 7: the whole DaemonReply space (Allow / Deny / Unbonded) round-trips.
    let replies: BoxedStrategy<DaemonReply> = prop_oneof![
        Just(DaemonReply::Allow),
        Just(DaemonReply::Deny),
        Just(DaemonReply::Unbonded),
    ]
    .boxed();
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(&replies, |reply: DaemonReply| {
            let json: String =
                serde_json::to_string(&reply).expect("a daemon reply always serializes");
            let back: DaemonReply =
                serde_json::from_str(&json).expect("a daemon reply always deserializes");
            prop_assert_eq!(back, reply);
            Ok(())
        })
        .expect("every daemon reply round-trips");
}

// The `harness = false` cucumber target needs its own `main`.
#[tokio::main]
async fn main() {
    PermissionWorld::run("tests/features").await;
}
