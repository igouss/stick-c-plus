//! Gherkin plumbing test for the buddy wire contract.
//!
//! Proves the boundary: [`parse_inbound`] classifies a line into the exhaustive [`Inbound`], the
//! asymmetric snapshot [`merge`](SnapshotState::merge) only runs for a snapshot, and [`dispatch`]
//! acks every command including the unknown one. The fine grain lives in the `#[cfg(test)]` unit
//! and property tests next to the code; these scenarios guard the wire seam end to end.

use buddy_wire::{
    dispatch, parse_inbound, Ack, Command, FrameError, Framer, Inbound, InboundError, Prompt, Ring,
    SnapshotState,
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

#[tokio::main]
async fn main() {
    WireWorld::run("tests/features").await;
}
