//! Gherkin plumbing test for the buddy wire contract.
//!
//! Proves the boundary: [`parse_inbound`] classifies a line into the exhaustive [`Inbound`], the
//! asymmetric snapshot [`merge`](SnapshotState::merge) only runs for a snapshot, and [`dispatch`]
//! acks every command including the unknown one. The fine grain lives in the `#[cfg(test)]` unit
//! and property tests next to the code; these scenarios guard the wire seam end to end.

use buddy_wire::{
    dispatch, parse_inbound, serialize_name, serialize_owner, serialize_snapshot, serialize_time,
    Ack, Command, Decision, FrameError, Framer, Inbound, InboundError, PermissionParseError,
    PermissionResponse, Prompt, Ring, SnapshotPacket, SnapshotState,
};
use cucumber::gherkin::Step;
use cucumber::{given, then, when, World};

#[derive(Debug, Default, World)]
struct WireWorld {
    /// The most recent classification: `Some(Ok)` on a typed message, `Some(Err)` on a non-message.
    result: Option<Result<Inbound, InboundError>>,
    /// The accumulated snapshot state a merge folds into.
    state: SnapshotState,
    /// The line framer for the truncation scenarios (defect fix c).
    framer: Framer,
    /// The BLE receive ring for the overflow scenarios (defect fix c).
    ring: Ring,
    /// The last framing outcome: `Ok(lines)` or a typed [`FrameError`].
    frame_result: Option<Result<Vec<Vec<u8>>, FrameError>>,
    /// The last ring-push outcome: `Ok(())` or a typed [`FrameError`].
    ring_result: Option<Result<(), FrameError>>,
    /// The last central-side serialization (snapshot / time / connect / permission line).
    serialized: Option<String>,
    /// The snapshot the daemon serialized, kept to assert a faithful device-side round-trip.
    sent_packet: Option<SnapshotPacket>,
    /// The central-side read of a permission line: `Ok(response)` or a typed parse error.
    perm_parse: Option<Result<PermissionResponse, PermissionParseError>>,
}

impl WireWorld {
    /// The classified inbound, or a panic — for steps that assume a successful parse.
    fn inbound(&self) -> &Inbound {
        self.result
            .as_ref()
            .expect("a scenario must parse a line first")
            .as_ref()
            .expect("this scenario expects a valid message")
    }

    /// The last central-side serialization, or a panic — for the mirror steps.
    fn line(&self) -> &str {
        self.serialized
            .as_deref()
            .expect("a scenario must serialize a line first")
    }
}

/// A fixed prompt used across the mirror scenarios, so a round-trip is deterministic.
fn a_wire_prompt(id: String) -> Prompt {
    Prompt {
        id,
        tool: "bash".to_string(),
        hint: "rm -rf".to_string(),
    }
}

#[given(regex = r#"^a pending prompt "([^"]*)" is on the glass$"#)]
fn a_pending_prompt_is_on_the_glass(world: &mut WireWorld, id: String) {
    world.state.prompt = Some(Prompt {
        id,
        tool: "bash".to_string(),
        hint: "rm -rf".to_string(),
    });
}

#[when("the line is parsed:")]
fn the_line_is_parsed(world: &mut WireWorld, step: &Step) {
    let line: &str = step
        .docstring
        .as_deref()
        .expect("the step must carry a docstring line");
    world.result = Some(parse_inbound(line.trim().as_bytes()));
}

#[then("it is a snapshot")]
fn it_is_a_snapshot(world: &mut WireWorld) {
    assert!(matches!(world.inbound(), Inbound::Snapshot(_)));
}

#[then("it is a command")]
fn it_is_a_command(world: &mut WireWorld) {
    assert!(matches!(world.inbound(), Inbound::Command(_)));
}

#[then("it is an event")]
fn it_is_an_event(world: &mut WireWorld) {
    assert!(matches!(world.inbound(), Inbound::Event(_)));
}

#[then(regex = r"^it is a time sync with epoch (-?\d+) and offset (-?\d+)$")]
fn it_is_a_time_sync(world: &mut WireWorld, epoch: i64, offset: i32) {
    assert_eq!(
        world.inbound(),
        &Inbound::Time {
            epoch,
            tz_offset_s: offset,
        }
    );
}

#[then(regex = r"^merging it leaves the running count at (\d+)$")]
fn merging_leaves_running_at(world: &mut WireWorld, running: u8) {
    let Inbound::Snapshot(packet) = world.inbound().clone() else {
        panic!("this scenario expects a snapshot");
    };
    world.state.merge(&packet);
    assert_eq!(world.state.running, running);
}

#[then(regex = r#"^the pending prompt "([^"]*)" is still on the glass$"#)]
fn the_pending_prompt_is_still_on_the_glass(world: &mut WireWorld, id: String) {
    // Structural D1 proof: an event carries no packet, so merge is never called.
    assert!(matches!(world.inbound(), Inbound::Event(_)));
    assert_eq!(
        world
            .state
            .prompt
            .as_ref()
            .map(|prompt: &Prompt| prompt.id.clone()),
        Some(id)
    );
}

#[then("after merging, no prompt is on the glass")]
fn after_merging_no_prompt(world: &mut WireWorld) {
    let Inbound::Snapshot(packet) = world.inbound().clone() else {
        panic!("this scenario expects a snapshot");
    };
    world.state.merge(&packet);
    assert_eq!(world.state.prompt, None);
}

#[then(regex = r#"^dispatching it acks "([^"]*)" ok$"#)]
fn dispatching_acks_ok(world: &mut WireWorld, what: String) {
    let Inbound::Command(command) = world.inbound().clone() else {
        panic!("this scenario expects a command");
    };
    let ack: Ack = dispatch(&command);
    assert_eq!(ack.what, what);
    assert!(ack.ok, "a known command acks ok");
}

#[then(regex = r#"^dispatching it nacks "([^"]*)"$"#)]
fn dispatching_nacks(world: &mut WireWorld, what: String) {
    let Inbound::Command(command) = world.inbound().clone() else {
        panic!("this scenario expects a command");
    };
    assert_eq!(command, Command::Unknown(what.clone()));
    let ack: Ack = dispatch(&command);
    assert_eq!(ack.what, what);
    assert!(!ack.ok, "an unknown command is nacked, never swallowed");
}

#[then("parsing fails")]
fn parsing_fails(world: &mut WireWorld) {
    let result: &Result<Inbound, InboundError> = world
        .result
        .as_ref()
        .expect("a scenario must parse a line first");
    assert!(
        result.is_err(),
        "a wrong-arity time is an error, not a snapshot"
    );
}

// ---- D7: entries are stored oldest-first ----------------------------------------------

#[then(regex = r#"^merging keeps "([^"]*)" oldest and "([^"]*)" newest$"#)]
fn merging_keeps_oldest_newest(world: &mut WireWorld, oldest: String, newest: String) {
    let Inbound::Snapshot(packet) = world.inbound().clone() else {
        panic!("this scenario expects a snapshot");
    };
    world.state.merge(&packet);
    // Index 0 is the oldest; the wire order is preserved verbatim (D7).
    assert_eq!(
        world.state.entries.first().map(String::as_str),
        Some(oldest.as_str()),
        "the first entry is the oldest"
    );
    assert_eq!(
        world.state.entries.last().map(String::as_str),
        Some(newest.as_str()),
        "the last entry is the newest"
    );
}

// ---- Defect fix (c): truncation is a TYPED error at the framing seam -------------------

#[when(regex = r"^a chunk of (\d+) object bytes with no terminator is pushed to the line framer$")]
fn a_chunk_is_pushed_to_the_framer(world: &mut WireWorld, count: usize) {
    // A leading `{` then filler: `count` non-terminator bytes, no `\n`/`\r`.
    let mut chunk: Vec<u8> = vec![b'{'];
    chunk.extend(std::iter::repeat_n(b'x', count.saturating_sub(1)));
    world.frame_result = Some(world.framer.push(&chunk));
}

#[when("a full-capacity object line terminated by newline is pushed to the line framer")]
fn a_full_capacity_line_is_pushed(world: &mut WireWorld) {
    // 1023 usable bytes (`{` + 1022 filler) plus a terminator: the boundary that completes.
    let mut chunk: Vec<u8> = vec![b'{'];
    chunk.extend(std::iter::repeat_n(b'x', buddy_wire::LINE_CAPACITY - 1));
    chunk.push(b'\n');
    world.frame_result = Some(world.framer.push(&chunk));
}

#[then("the framer completes exactly one line")]
fn the_framer_completes_one_line(world: &mut WireWorld) {
    let lines: &Vec<Vec<u8>> = world
        .frame_result
        .as_ref()
        .expect("a scenario must push to the framer first")
        .as_ref()
        .expect("this scenario expects a completed line, not an overflow");
    assert_eq!(
        lines.len(),
        1,
        "a within-capacity line frames to exactly one line"
    );
}

#[then("the framer reports the line was too long, not a silent truncation")]
fn the_framer_reports_too_long(world: &mut WireWorld) {
    // The pin (fix c): upstream dropped the tail with zero diagnostics; here it is typed.
    let result: &Result<Vec<Vec<u8>>, FrameError> = world
        .frame_result
        .as_ref()
        .expect("a scenario must push to the framer first");
    assert_eq!(result.as_ref().err(), Some(&FrameError::LineTooLong));
}

#[when(regex = r"^(\d+) bytes are pushed to the receive ring$")]
fn bytes_are_pushed_to_the_ring(world: &mut WireWorld, count: usize) {
    let bytes: Vec<u8> = vec![b'z'; count];
    world.ring_result = Some(world.ring.push(&bytes));
}

#[then("the ring accepts them")]
fn the_ring_accepts_them(world: &mut WireWorld) {
    let result: &Result<(), FrameError> = world
        .ring_result
        .as_ref()
        .expect("a scenario must push to the ring first");
    assert!(result.is_ok(), "a within-capacity push is accepted");
}

#[then("the ring reports an overflow, not a silent mid-line drop")]
fn the_ring_reports_an_overflow(world: &mut WireWorld) {
    // The pin (fix c): upstream did a bare return on a full ring; here it is typed.
    let result: &Result<(), FrameError> = world
        .ring_result
        .as_ref()
        .expect("a scenario must push to the ring first");
    assert_eq!(result.as_ref().err(), Some(&FrameError::RxOverflow));
}

// ---- The central->device mirror (outbound serializers + central-side permission parse) ----

#[when(regex = r#"^the daemon serializes a snapshot carrying the prompt "([^"]*)"$"#)]
fn serialize_snapshot_with_prompt(world: &mut WireWorld, id: String) {
    let packet: SnapshotPacket = SnapshotPacket {
        running: Some(2),
        prompt: Some(a_wire_prompt(id)),
        ..SnapshotPacket::default()
    };
    world.serialized = Some(serialize_snapshot(&packet));
    world.sent_packet = Some(packet);
}

#[when("the daemon serializes a prompt-less snapshot")]
fn serialize_prompt_less_snapshot(world: &mut WireWorld) {
    let packet: SnapshotPacket = SnapshotPacket {
        running: Some(1),
        ..SnapshotPacket::default()
    };
    world.serialized = Some(serialize_snapshot(&packet));
    world.sent_packet = Some(packet);
}

#[when(regex = r"^the daemon serializes a time sync with epoch (-?\d+) and offset (-?\d+)$")]
fn serialize_time_sync(world: &mut WireWorld, epoch: i64, offset: i32) {
    world.serialized = Some(serialize_time(epoch, offset));
}

#[when(regex = r#"^the daemon serializes the owner name "([^"]*)"$"#)]
fn serialize_owner_name(world: &mut WireWorld, name: String) {
    world.serialized = Some(serialize_owner(&name));
}

#[when(regex = r#"^the daemon serializes the buddy name "([^"]*)"$"#)]
fn serialize_buddy_name(world: &mut WireWorld, name: String) {
    world.serialized = Some(serialize_name(&name));
}

#[then("the serialized line has a prompt key")]
fn the_line_has_a_prompt_key(world: &mut WireWorld) {
    assert!(
        world.line().contains("\"prompt\""),
        "a prompt-bearing snapshot must carry the prompt key"
    );
}

#[then("the serialized line has no prompt key")]
fn the_line_has_no_prompt_key(world: &mut WireWorld) {
    // The anti-hazard pin: an absent prompt is CLEARED on the device — the key must be omitted,
    // never emitted as null, so a bare snapshot is distinct from a prompt-bearing one after parse.
    assert!(
        !world.line().contains("\"prompt\""),
        "a prompt-less snapshot must omit the prompt key entirely"
    );
}

#[then("the serialized line has no null")]
fn the_line_has_no_null(world: &mut WireWorld) {
    assert!(
        !world.line().contains("null"),
        "an absent field is omitted, never emitted as null"
    );
}

#[then("the device parses it back as the same snapshot")]
fn the_device_parses_the_same_snapshot(world: &mut WireWorld) {
    let packet: SnapshotPacket = world
        .sent_packet
        .clone()
        .expect("a scenario must serialize a snapshot first");
    let parsed: Result<Inbound, InboundError> = parse_inbound(world.line().as_bytes());
    assert_eq!(parsed.ok(), Some(Inbound::Snapshot(packet)));
}

#[then(regex = r"^the serialized time array has exactly two elements$")]
fn the_time_array_has_two_elements(world: &mut WireWorld) {
    // The strict-arity pin: a wrong arity would fall through the device parse and clear a prompt.
    let value: serde_json::Value =
        serde_json::from_str(world.line()).expect("a time sync is valid JSON");
    let len: Option<usize> = value
        .get("time")
        .and_then(|time: &serde_json::Value| time.as_array())
        .map(Vec::len);
    assert_eq!(len, Some(2), "the time array must be exactly two elements");
}

#[then(regex = r"^the device parses it as a time sync with epoch (-?\d+) and offset (-?\d+)$")]
fn the_device_parses_a_time_sync(world: &mut WireWorld, epoch: i64, offset: i32) {
    let parsed: Result<Inbound, InboundError> = parse_inbound(world.line().as_bytes());
    assert_eq!(
        parsed.ok(),
        Some(Inbound::Time {
            epoch,
            tz_offset_s: offset,
        })
    );
}

#[then(regex = r#"^the device parses it as an owner command "([^"]*)"$"#)]
fn the_device_parses_an_owner_command(world: &mut WireWorld, name: String) {
    let parsed: Result<Inbound, InboundError> = parse_inbound(world.line().as_bytes());
    assert_eq!(parsed.ok(), Some(Inbound::Command(Command::Owner(name))));
}

#[then(regex = r#"^the device parses it as a name command "([^"]*)"$"#)]
fn the_device_parses_a_name_command(world: &mut WireWorld, name: String) {
    let parsed: Result<Inbound, InboundError> = parse_inbound(world.line().as_bytes());
    assert_eq!(parsed.ok(), Some(Inbound::Command(Command::Name(name))));
}

#[given(regex = r#"^a device permission response for "([^"]*)" decided "([^"]*)"$"#)]
fn a_device_permission_response(world: &mut WireWorld, id: String, decision: String) {
    let decision: Decision = match decision.as_str() {
        "once" => Decision::Once,
        "deny" => Decision::Deny,
        other => panic!("unexpected decision literal in the scenario: {other:?}"),
    };
    let response: PermissionResponse = PermissionResponse::new(id, decision);
    world.serialized = Some(response.to_json());
}

#[when("the central parses that permission line")]
fn the_central_parses_that_permission_line(world: &mut WireWorld) {
    world.perm_parse = Some(PermissionResponse::parse(world.line().as_bytes()));
}

#[when("the central parses the permission line:")]
fn the_central_parses_a_raw_permission_line(world: &mut WireWorld, step: &Step) {
    let line: &str = step
        .docstring
        .as_deref()
        .expect("the step must carry a docstring line");
    world.perm_parse = Some(PermissionResponse::parse(line.trim().as_bytes()));
}

#[then(regex = r#"^the central reads back decision "([^"]*)" for "([^"]*)"$"#)]
fn the_central_reads_back(world: &mut WireWorld, decision: String, id: String) {
    let expected: Decision = match decision.as_str() {
        "once" => Decision::Once,
        "deny" => Decision::Deny,
        other => panic!("unexpected decision literal in the scenario: {other:?}"),
    };
    let parsed: &Result<PermissionResponse, PermissionParseError> = world
        .perm_parse
        .as_ref()
        .expect("a scenario must parse a permission line first");
    let response: &PermissionResponse = parsed
        .as_ref()
        .expect("this scenario expects a valid permission response");
    assert_eq!(response, &PermissionResponse::new(id, expected));
}

#[then("the central parse is a typed error, not a silent approval")]
fn the_central_parse_is_a_typed_error(world: &mut WireWorld) {
    // The load-bearing pin: a bad or missing decision must NEVER read as Ok(Decision::Once),
    // which would approve a tool call the owner denied.
    let parsed: &Result<PermissionResponse, PermissionParseError> = world
        .perm_parse
        .as_ref()
        .expect("a scenario must parse a permission line first");
    assert!(
        parsed.is_err(),
        "a malformed permission line must be a typed error, never a silent approval"
    );
}

#[then("the central parse reports a bad decision")]
fn the_central_parse_reports_a_bad_decision(world: &mut WireWorld) {
    let parsed: &Result<PermissionResponse, PermissionParseError> = world
        .perm_parse
        .as_ref()
        .expect("a scenario must parse a permission line first");
    assert!(
        matches!(parsed, Err(PermissionParseError::BadDecision(_))),
        "an unknown or absent decision must be a BadDecision error"
    );
}

#[then("the central parse reports a missing id")]
fn the_central_parse_reports_a_missing_id(world: &mut WireWorld) {
    let parsed: &Result<PermissionResponse, PermissionParseError> = world
        .perm_parse
        .as_ref()
        .expect("a scenario must parse a permission line first");
    assert!(
        matches!(parsed, Err(PermissionParseError::MissingId)),
        "an absent id must be a MissingId error"
    );
}

#[tokio::main]
async fn main() {
    WireWorld::run("tests/features").await;
}
