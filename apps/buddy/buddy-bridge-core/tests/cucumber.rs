//! Gherkin plumbing test for the bridge policy.
//!
//! Proves the decision seam: the [`Fsm`] turns a transport outcome into the next [`Action`]
//! (pair / subscribe / recover a stale bond / fail fast), and [`chunk`] splits a framed line to
//! fit the negotiated MTU. The fine grain lives in the `#[cfg(test)]` unit and property tests
//! next to the code; these scenarios guard the boundary.

use buddy_bridge_core::{chunk, Action, Event, Fsm};
use cucumber::{given, then, when, World};

#[derive(Debug, Default, World)]
struct BridgeWorld {
    /// The decision machine under test.
    fsm: Fsm,
    /// The most recent action the FSM returned.
    action: Option<Action>,
    /// The number of pieces the chunker produced.
    pieces: Option<usize>,
}

impl BridgeWorld {
    /// The last action, or a panic — for steps that assume the FSM has been driven.
    fn action(&self) -> &Action {
        self.action
            .as_ref()
            .expect("a scenario must drive the FSM first")
    }
}

#[given("a bridge that has started connecting")]
fn a_bridge_that_has_started_connecting(world: &mut BridgeWorld) {
    world.action = Some(world.fsm.start());
}

#[when("the transport reports a fresh (unpaired) connection")]
fn a_fresh_connection(world: &mut BridgeWorld) {
    world.action = Some(world.fsm.on(Event::ConnectedFresh));
}

#[when("the transport reports an already-paired connection")]
fn an_already_paired_connection(world: &mut BridgeWorld) {
    world.action = Some(world.fsm.on(Event::ConnectedPaired));
}

#[when("the transport reports an encryption failure")]
fn an_encryption_failure(world: &mut BridgeWorld) {
    world.action = Some(world.fsm.on(Event::EncryptionFailed));
}

#[when("the transport reports a missing agent")]
fn a_missing_agent(world: &mut BridgeWorld) {
    world.action = Some(world.fsm.on(Event::AgentMissing));
}

#[then("the bridge decides to pair")]
fn decides_to_pair(world: &mut BridgeWorld) {
    assert_eq!(world.action(), &Action::Pair);
}

#[then("the bridge decides to subscribe")]
fn decides_to_subscribe(world: &mut BridgeWorld) {
    assert_eq!(world.action(), &Action::Subscribe);
}

#[then("the bridge decides to remove the device and re-acquire")]
fn decides_to_remove_and_reacquire(world: &mut BridgeWorld) {
    assert_eq!(world.action(), &Action::RemoveDeviceThenReacquire);
}

#[then("the bridge decides to fail fast")]
fn decides_to_fail_fast(world: &mut BridgeWorld) {
    assert!(matches!(world.action(), Action::FailFast(_)));
}

#[when(regex = r"^a payload of (\d+) bytes is chunked for an MTU of (\d+)$")]
fn a_payload_is_chunked(world: &mut BridgeWorld, bytes: usize, mtu: u16) {
    let payload: Vec<u8> = vec![b'x'; bytes];
    world.pieces = Some(chunk(&payload, mtu).len());
}

#[then(regex = r"^it splits into (\d+) pieces?$")]
fn it_splits_into(world: &mut BridgeWorld, count: usize) {
    let pieces: usize = world.pieces.expect("a scenario must chunk a payload first");
    assert_eq!(pieces, count);
}

#[tokio::main]
async fn main() {
    BridgeWorld::run("tests/features").await;
}
