//! Gherkin plumbing test for the daemon-core coordinator — the permission loop end-to-end.
//!
//! These scenarios pin the load-bearing invariants at the daemon-core boundary; the fine grain
//! lives in the source's own `#[cfg(test)]` modules once implemented. What they pin:
//!
//! - **1 — a live prompt rides every emitted snapshot**: a keepalive or an unrelated lifecycle
//!   event while a prompt is outstanding never produces a snapshot line with an absent prompt.
//!   Concrete scenario + a PROPERTY over any keepalive/unrelated-event sequence, parsing the
//!   emitted `SendLink` JSON.
//! - **2 — an unbonded hook is answered exactly `[ReplyHook(hook, Unbonded)]`**, raises no prompt
//!   (none resurfaces on link-up), and sends nothing down the (absent) link.
//! - **3 — a device once/deny replies the matching `DaemonReply` to exactly the raising hook** and
//!   records `Resolved`, so the fresh snapshot clears the prompt.
//! - **4 — an unknown id / non-permission / unparseable / bad-decision line yields `[]`** — no
//!   reply, no prompt-clear — mirroring `buddy_wire`'s BadDecision-is-never-a-silent-Once rule.
//! - **5 — with concurrent prompts each answer routes to its own hook with no cross-talk**, and
//!   resolving one leaves the others on the glass. Concrete scenario + a PROPERTY over arbitrary
//!   answer orders.
//! - **6 — a link-down drains every waiting hook** with `Unbonded` (zero / one / many).
//! - **7 — the link-up handshake is time-sync, then owner, then snapshot, in order**, the
//!   time-sync being exactly `{"time":[epoch,tz]}` — a two-element array that can never fall
//!   through to the device's snapshot branch and clear a prompt.
//!
//! The coordinator methods are still `unimplemented!()`, so every scenario that drives one is RED
//! by construction — the correct starting state for the Specify stage.

use buddy_daemon_core::{Coordinator, Effect, HookId};
use buddy_permission::{DaemonReply, HookRequest, SessionEvent, SessionId};
use buddy_wire::{Decision, PermissionResponse};
use cucumber::{given, then, when, World};
use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use std::collections::HashMap;

/// The injected time-sync epoch — a fixed value so the handshake line is deterministic.
const EPOCH: i64 = 1_700_000_000;
/// The injected tz offset (seconds) — deterministic alongside [`EPOCH`].
const TZ: i32 = -14_400;
/// The injected owner label the handshake carries.
const OWNER: &str = "Iouri";

/// The scenario state: the coordinator under test, the effects its last fold produced, and the
/// bookkeeping that lets a step name a hook or a prompt id by session without peeking inside the
/// coordinator.
#[derive(Debug, Default, World)]
struct DaemonWorld {
    /// The coordinator being driven through the permission loop.
    coordinator: Coordinator,
    /// The effects the most recent fold produced — the sole observation surface.
    effects: Vec<Effect>,
    /// A monotonic source of opaque hook handles, one per arriving hook.
    hook_seq: u64,
    /// The hook handle assigned to each session, so a `Then` can assert the reply routed to it.
    hooks: HashMap<String, HookId>,
    /// The prompt id the coordinator raised for each session, read back from the emitted snapshot,
    /// so a device answer can echo the right id without assuming the counter's start value.
    prompts: HashMap<String, String>,
}

// ── Fixtures ──────────────────────────────────────────────────────────────────────────────────

/// A hook request for `session` invoking `tool` — the other fields are fixed, deterministic context.
fn a_request(session: &str, tool: &str) -> HookRequest {
    HookRequest {
        session_id: session.to_string(),
        tool_name: tool.to_string(),
        cwd: "/repo".to_string(),
        permission_mode: "default".to_string(),
        transcript_path: "/tmp/transcript.jsonl".to_string(),
    }
}

// ── Effect observation helpers (not step bodies — all branching lives here) ─────────────────────

/// The serialized line of a `SendLink` effect, or `None` for a `ReplyHook`.
fn send_line(effect: &Effect) -> Option<String> {
    match effect {
        Effect::SendLink(line) => Some(line.clone()),
        Effect::ReplyHook(_, _) => None,
    }
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

/// The `n`-th `SendLink` line among the effects, if present.
fn nth_send_line(effects: &[Effect], n: usize) -> Option<String> {
    effects.iter().filter_map(send_line).nth(n)
}

/// The last `SendLink` line among the effects — the fresh snapshot in the loop flows.
fn last_send_line(effects: &[Effect]) -> Option<String> {
    effects.iter().filter_map(send_line).next_back()
}

/// Whether the effects sent anything down the link at all.
fn sent_a_snapshot(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect: &Effect| send_line(effect).is_some())
}

/// The `prompt.id` a snapshot line carries, if it is a snapshot carrying a prompt.
fn snapshot_prompt_id(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value.get("prompt")?.get("id")?.as_str().map(str::to_string)
}

/// The prompt id the effects emitted — the id of the newest prompt on the last snapshot line.
fn emitted_prompt_id(effects: &[Effect]) -> Option<String> {
    effects
        .iter()
        .filter_map(send_line)
        .filter_map(|line: String| snapshot_prompt_id(&line))
        .next_back()
}

/// Whether the last snapshot line carries a `prompt` key.
fn last_snapshot_has_prompt(effects: &[Effect]) -> bool {
    last_send_line(effects)
        .and_then(|line: String| serde_json::from_str::<serde_json::Value>(&line).ok())
        .map(|value: serde_json::Value| value.get("prompt").is_some())
        .unwrap_or(false)
}

/// Whether EVERY snapshot line among the effects carries a prompt (vacuously true if none).
fn snapshots_all_carry_prompt(effects: &[Effect]) -> bool {
    effects
        .iter()
        .filter_map(send_line)
        .filter_map(|line: String| serde_json::from_str::<serde_json::Value>(&line).ok())
        .all(|value: serde_json::Value| value.get("prompt").is_some())
}

/// The reply the last fold routed to `session`'s hook must match `expected`.
fn assert_reply(world: &DaemonWorld, session: &str, expected: DaemonReply) {
    let hook: HookId = *world
        .hooks
        .get(session)
        .expect("a hook must have arrived for this session first");
    assert!(
        replies(&world.effects).contains(&(hook, expected)),
        "the reply must route to exactly the hook that raised this session's prompt"
    );
}

/// Answer `session`'s outstanding prompt with `decision`, echoing the id the coordinator emitted.
fn answer(world: &mut DaemonWorld, session: &str, decision: Decision) {
    let id: String = world
        .prompts
        .get(session)
        .cloned()
        .expect("a prompt must have been raised for this session first");
    let line: String = PermissionResponse::new(id, decision).to_json();
    world.effects = world.coordinator.on_device_line(line.as_bytes());
}

// ── Given: the coordinator's bonded state ───────────────────────────────────────────────────────

#[given("a bonded coordinator")]
fn a_bonded_coordinator(world: &mut DaemonWorld) {
    world.coordinator = Coordinator::new();
    // Bond by bringing the link up; the handshake effects are discarded — later steps overwrite.
    world.effects = world.coordinator.on_link_up(EPOCH, TZ, OWNER);
}

#[given("an unbonded coordinator")]
fn an_unbonded_coordinator(world: &mut DaemonWorld) {
    world.coordinator = Coordinator::new();
}

// ── When: the inputs the daemon feeds the coordinator ──────────────────────────────────────────

#[when("the link comes up")]
fn link_comes_up(world: &mut DaemonWorld) {
    world.effects = world.coordinator.on_link_up(EPOCH, TZ, OWNER);
}

#[when("the link goes down")]
fn link_goes_down(world: &mut DaemonWorld) {
    world.effects = world.coordinator.on_link_down();
}

#[when(regex = r#"^a hook arrives for session "([^"]*)" running tool "([^"]*)"$"#)]
fn hook_arrives(world: &mut DaemonWorld, session: String, tool: String) {
    let hook: HookId = HookId(world.hook_seq);
    world.hook_seq += 1;
    world.hooks.insert(session.clone(), hook);
    world.effects = world.coordinator.on_hook(hook, &a_request(&session, &tool));
    // Read the raised prompt id back from the emitted snapshot (Option iterates zero-or-one times,
    // so no branch): if unbonded, no snapshot, nothing recorded.
    let session_key: String = session.clone();
    emitted_prompt_id(&world.effects)
        .into_iter()
        .for_each(|id: String| {
            world.prompts.insert(session_key.clone(), id);
        });
}

#[when("a keepalive tick fires")]
fn keepalive_tick(world: &mut DaemonWorld) {
    world.effects = world.coordinator.on_keepalive();
}

#[when(regex = r#"^the device answers session "([^"]*)" with an allow$"#)]
fn device_allows(world: &mut DaemonWorld, session: String) {
    answer(world, &session, Decision::Once);
}

#[when(regex = r#"^the device answers session "([^"]*)" with a deny$"#)]
fn device_denies(world: &mut DaemonWorld, session: String) {
    answer(world, &session, Decision::Deny);
}

#[when("the device sends a line for an unknown prompt id")]
fn device_unknown_id(world: &mut DaemonWorld) {
    let line: String =
        PermissionResponse::new("req_not_a_real_id".to_string(), Decision::Once).to_json();
    world.effects = world.coordinator.on_device_line(line.as_bytes());
}

#[when("the device sends a non-permission line")]
fn device_non_permission(world: &mut DaemonWorld) {
    world.effects = world.coordinator.on_device_line(br#"{"cmd":"status"}"#);
}

#[when("the device sends an unparseable line")]
fn device_unparseable(world: &mut DaemonWorld) {
    world.effects = world.coordinator.on_device_line(b"not json at all");
}

#[when("the device sends a line with an unknown decision token")]
fn device_bad_decision(world: &mut DaemonWorld) {
    world.effects = world
        .coordinator
        .on_device_line(br#"{"cmd":"permission","id":"req_0","decision":"maybe"}"#);
}

// ── Then: the observable assertions ────────────────────────────────────────────────────────────

#[then("a gated snapshot carrying the prompt is sent down the link")]
fn snapshot_with_prompt_sent(world: &mut DaemonWorld) {
    assert!(
        last_snapshot_has_prompt(&world.effects),
        "the snapshot emitted for a hook must carry the freshly-raised prompt"
    );
}

#[then("the hook is not answered yet")]
fn hook_not_answered_yet(world: &mut DaemonWorld) {
    assert!(
        replies(&world.effects).is_empty(),
        "the hook waits on the device; raising a prompt never replies yet"
    );
}

#[then(regex = r#"^the waiting hook for session "([^"]*)" is answered allow$"#)]
fn hook_answered_allow(world: &mut DaemonWorld, session: String) {
    assert_reply(world, &session, DaemonReply::Allow);
}

#[then(regex = r#"^the waiting hook for session "([^"]*)" is answered deny$"#)]
fn hook_answered_deny(world: &mut DaemonWorld, session: String) {
    assert_reply(world, &session, DaemonReply::Deny);
}

#[then(regex = r#"^the waiting hook for session "([^"]*)" is answered Unbonded$"#)]
fn hook_answered_unbonded(world: &mut DaemonWorld, session: String) {
    assert_reply(world, &session, DaemonReply::Unbonded);
}

#[then(regex = r#"^the waiting hook for session "([^"]*)" is still waiting$"#)]
fn hook_still_waiting(world: &mut DaemonWorld, session: String) {
    let hook: HookId = *world.hooks.get(&session).expect("a hook for this session");
    let answered: bool = replies(&world.effects)
        .iter()
        .any(|(replied, _): &(HookId, DaemonReply)| *replied == hook);
    assert!(
        !answered,
        "resolving another session's prompt must not answer this hook"
    );
}

#[then("a fresh gated snapshot clearing the prompt is sent down the link")]
fn fresh_snapshot_clears_prompt(world: &mut DaemonWorld) {
    assert!(
        sent_a_snapshot(&world.effects),
        "a fresh snapshot must follow the reply"
    );
    assert!(
        !last_snapshot_has_prompt(&world.effects),
        "resolving the only prompt clears it off the glass"
    );
}

#[then("the fresh snapshot still carries a prompt")]
fn fresh_snapshot_still_carries_prompt(world: &mut DaemonWorld) {
    assert!(
        last_snapshot_has_prompt(&world.effects),
        "resolving one of several prompts leaves the others on the glass"
    );
}

#[then("the effects are exactly one Unbonded reply to the hook")]
fn exactly_unbonded_reply(world: &mut DaemonWorld) {
    let hook: HookId = *world
        .hooks
        .values()
        .next()
        .expect("a hook must have arrived");
    assert_eq!(
        world.effects,
        vec![Effect::ReplyHook(hook, DaemonReply::Unbonded)],
        "an unbonded hook is answered exactly [ReplyHook(hook, Unbonded)] — no snapshot, no prompt"
    );
}

#[then("the snapshot sent for the handshake carries no prompt")]
fn handshake_snapshot_no_prompt(world: &mut DaemonWorld) {
    assert!(
        !last_snapshot_has_prompt(&world.effects),
        "an unbonded hook raises no prompt, so none may resurface on the link-up snapshot"
    );
}

#[then("the snapshot sent for the keepalive still carries the prompt")]
fn keepalive_snapshot_carries_prompt(world: &mut DaemonWorld) {
    assert!(
        last_snapshot_has_prompt(&world.effects),
        "a keepalive must never clear a live prompt"
    );
}

#[then("no effects are produced")]
fn no_effects(world: &mut DaemonWorld) {
    assert!(
        world.effects.is_empty(),
        "an unknown or malformed device line produces no effect — never a guessed approval"
    );
}

#[then("no hook is answered")]
fn no_hook_answered(world: &mut DaemonWorld) {
    assert!(
        replies(&world.effects).is_empty(),
        "nothing was pending, so no hook is answered"
    );
}

#[then("no snapshot is sent down the link")]
fn no_snapshot_sent(world: &mut DaemonWorld) {
    assert!(
        !sent_a_snapshot(&world.effects),
        "nothing may be sent down an unbonded or dropped link"
    );
}

#[then("the first line sent is exactly the two-element time sync")]
fn first_line_is_time_sync(world: &mut DaemonWorld) {
    let line: String = nth_send_line(&world.effects, 0).expect("a first handshake line");
    // Exact string: the two-element array can never be the wrong arity that falls through to the
    // snapshot branch and clears a prompt.
    assert_eq!(
        line,
        format!(r#"{{"time":[{EPOCH},{TZ}]}}"#),
        "the handshake opens with an exact two-element time sync"
    );
}

#[then("the second line sent is the owner name")]
fn second_line_is_owner(world: &mut DaemonWorld) {
    let line: String = nth_send_line(&world.effects, 1).expect("a second handshake line");
    let value: serde_json::Value = serde_json::from_str(&line).expect("the owner line is JSON");
    assert_eq!(
        value.get("cmd").and_then(serde_json::Value::as_str),
        Some("owner"),
        "the owner command is the second handshake line"
    );
    assert_eq!(
        value.get("name").and_then(serde_json::Value::as_str),
        Some(OWNER),
        "the owner command carries the injected owner label"
    );
}

#[then("the third line sent is a snapshot")]
fn third_line_is_snapshot(world: &mut DaemonWorld) {
    let line: String = nth_send_line(&world.effects, 2).expect("a third handshake line");
    let value: serde_json::Value = serde_json::from_str(&line).expect("the snapshot line is JSON");
    // A snapshot is neither a time sync, a command, nor an event — the only shape that touches
    // prompt state on the device.
    assert!(
        value.get("time").is_none(),
        "a snapshot carries no time key"
    );
    assert!(value.get("cmd").is_none(), "a snapshot carries no cmd key");
    assert!(value.get("evt").is_none(), "a snapshot carries no evt key");
}

// ── Then: the totality properties ──────────────────────────────────────────────────────────────

#[then("any keepalive or unrelated event while a prompt is outstanding still carries the prompt")]
fn any_keepalive_keeps_the_prompt(_world: &mut DaemonWorld) {
    // Invariant 1, totality: raise ONE prompt on a bonded coordinator, then fold ANY sequence of
    // keepalives and unrelated lifecycle events (never a resolve, never an answer) and prove every
    // emitted snapshot line still carries the prompt — parsed straight out of the SendLink JSON.
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(&a_keepalive_op_sequence(), |ops: Vec<Op>| {
            let mut coordinator: Coordinator = Coordinator::new();
            coordinator.on_link_up(EPOCH, TZ, OWNER);
            coordinator.on_hook(HookId(0), &a_request("s1", "bash"));
            let all_carry: bool = ops
                .iter()
                .map(|op: &Op| apply_op(&mut coordinator, op))
                .all(|effects: Vec<Effect>| snapshots_all_carry_prompt(&effects));
            prop_assert!(
                all_carry,
                "a keepalive or unrelated event must never emit a prompt-less snapshot"
            );
            Ok(())
        })
        .expect("a live prompt survives every keepalive / unrelated-event sequence");
}

#[then("answering pending prompts in any order routes each reply to its own hook")]
fn answers_route_to_their_own_hook(_world: &mut DaemonWorld) {
    // Invariant 5, totality: raise a set of concurrent prompts, then answer them in an ARBITRARY
    // order, and prove each device answer replies exactly the hook that raised that prompt id —
    // no cross-talk, whatever the answer order.
    let mut runner: TestRunner = TestRunner::default();
    runner
        .run(
            &a_raise_answer_plan(),
            |(count, order): (usize, Vec<usize>)| {
                let mut coordinator: Coordinator = Coordinator::new();
                coordinator.on_link_up(EPOCH, TZ, OWNER);
                let raised: Vec<(HookId, String)> = (0..count)
                    .map(|index: usize| {
                        let hook: HookId = HookId(index as u64);
                        let effects: Vec<Effect> =
                            coordinator.on_hook(hook, &a_request(&format!("s{index}"), "bash"));
                        let id: String =
                            emitted_prompt_id(&effects).expect("a bonded hook raises a prompt");
                        (hook, id)
                    })
                    .collect();
                let all_routed: bool = order.iter().all(|&index: &usize| {
                    let (hook, id): &(HookId, String) = &raised[index];
                    let line: String =
                        PermissionResponse::new(id.clone(), Decision::Once).to_json();
                    let effects: Vec<Effect> = coordinator.on_device_line(line.as_bytes());
                    effects.contains(&Effect::ReplyHook(*hook, DaemonReply::Allow))
                });
                prop_assert!(
                    all_routed,
                    "each answer must route to exactly the hook that raised that prompt id"
                );
                Ok(())
            },
        )
        .expect("every answer routes to its own hook, in any order");
}

// ── Property strategies (data generation — not step bodies) ─────────────────────────────────────

/// One keepalive-class operation: a keepalive tick, or an unrelated non-approval lifecycle event
/// over some OTHER session. Never a resolve and never an answer — the events that must not clear a
/// live prompt.
#[derive(Clone, Debug)]
enum Op {
    /// A bare keepalive tick.
    Keepalive,
    /// A non-approval lifecycle event for an unrelated session.
    Lifecycle(String, SessionEvent),
}

/// Apply one op to the coordinator and return the effects it produced.
fn apply_op(coordinator: &mut Coordinator, op: &Op) -> Vec<Effect> {
    match op {
        Op::Keepalive => coordinator.on_keepalive(),
        Op::Lifecycle(session, event) => {
            coordinator.on_session_event(SessionId(session.clone()), event.clone())
        }
    }
}

/// Arbitrary keepalive-class op sequences: keepalives and unrelated Started / ToolStarted /
/// Completed / Stopped events over sessions distinct from the awaiting one.
fn a_keepalive_op_sequence() -> impl Strategy<Value = Vec<Op>> {
    let one: BoxedStrategy<Op> = (0u8..5, 0u8..3)
        .prop_map(|(kind, sid): (u8, u8)| match kind {
            0 => Op::Keepalive,
            1 => Op::Lifecycle(format!("o{sid}"), SessionEvent::Started),
            2 => Op::Lifecycle(format!("o{sid}"), SessionEvent::ToolStarted),
            3 => Op::Lifecycle(format!("o{sid}"), SessionEvent::Completed),
            _ => Op::Lifecycle(format!("o{sid}"), SessionEvent::Stopped),
        })
        .boxed();
    proptest::collection::vec(one, 0..10)
}

/// A plan for the routing property: a count of concurrent prompts and an arbitrary permutation of
/// `0..count` naming the order in which to answer them.
fn a_raise_answer_plan() -> impl Strategy<Value = (usize, Vec<usize>)> {
    (1usize..5).prop_flat_map(|count: usize| {
        proptest::collection::vec(any::<u32>(), count).prop_map(move |keys: Vec<u32>| {
            let mut order: Vec<usize> = (0..count).collect();
            // Sort the indices by random keys to obtain an arbitrary permutation of 0..count.
            order.sort_by_key(|index: &usize| keys[*index]);
            (count, order)
        })
    })
}

// The `harness = false` cucumber target needs its own `main`.
#[tokio::main]
async fn main() {
    DaemonWorld::run("tests/features").await;
}
