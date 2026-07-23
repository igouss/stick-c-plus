//! Gherkin plumbing test for the stick's shell.
//!
//! Drives [`buddy_shell`] end to end on the host: a line goes into `DeviceState::apply`, a press
//! goes into `DeviceState::press`, and the `Effect` it hands back is carried out through
//! [`perform`] — the very function the input thread calls — against a fake [`Notifier`] that
//! records the bytes. So what these scenarios assert is the actual JSON that would leave the
//! device, read back through the central's own parser, not a stand-in for it.
//!
//! Two features, because the bead has two obligations that must not be got wrong: **the answer**
//! (A allows, B denies, once each, naming the prompt) and **the fail-safe** (only a real snapshot
//! may clear a pending prompt). The exhaustive claims live in the unit tests beside each module;
//! these prove the plumbing made it through.

use std::sync::{Arc, Mutex};

use buddy_core::SpeciesIndex;
use buddy_display::BuddyView;
use buddy_shell::{
    perform, Bond, DeviceState, Effect, Identity, Notifier, NotifyError, SpeciesStore,
    LINK_WINDOW_MS,
};
use buddy_wire::{BuddyEvent, Command, Inbound, PermissionResponse, Prompt, SnapshotPacket};
use cucumber::{given, then, when, World};
use platform_core::{Acceleration, Tick};
use platform_input::{ButtonEvent, ButtonId, Gesture};

/// A resting acceleration — the sensors are not what these scenarios are about.
const REST: Acceleration = Acceleration::new(0, 0, 1_000);

/// Every line the device tried to send, in order.
#[derive(Clone, Default)]
struct RecordingNotifier {
    sent: Arc<Mutex<Vec<String>>>,
}

impl Notifier for RecordingNotifier {
    fn notify(&self, line: &str) -> Result<(), NotifyError> {
        self.sent
            .lock()
            .expect("the recorder is never held across a panic")
            .push(line.to_string());
        Ok(())
    }
}

impl core::fmt::Debug for RecordingNotifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RecordingNotifier")
    }
}

/// A bond that only remembers being forgotten.
#[derive(Clone, Default)]
struct RecordingBond {
    forgotten: Arc<Mutex<bool>>,
}

impl Bond for RecordingBond {
    fn forget(&self) {
        *self
            .forgotten
            .lock()
            .expect("the recorder is never held across a panic") = true;
    }
}

impl core::fmt::Debug for RecordingBond {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RecordingBond")
    }
}

/// A species store that discards — no scenario here asks about persistence.
#[derive(Clone, Copy, Default)]
struct NullSpecies;

impl SpeciesStore for NullSpecies {
    fn store(&self, _index: u8) {}
}

/// The stick under test, the clock it is driven at, and what it sent.
#[derive(Debug, World)]
struct StickWorld {
    state: DeviceState,
    now: Tick,
    notifier: RecordingNotifier,
    bond: RecordingBond,
}

impl Default for StickWorld {
    fn default() -> Self {
        StickWorld {
            state: DeviceState::new(
                Identity::new("Claude-4F2A", "0.1.0", "A0:B7:65:4F:2A:11"),
                SpeciesIndex::new(0),
            ),
            now: 1_000,
            notifier: RecordingNotifier::default(),
            bond: RecordingBond::default(),
        }
    }
}

impl StickWorld {
    /// Press a button and carry out whatever it asked for, exactly as the input thread would.
    fn press(&mut self, button: ButtonId) {
        self.now += 1_000;
        let effect: Effect = self
            .state
            .press(ButtonEvent::new(button, Gesture::Click), self.now);
        perform(effect, &self.notifier, &self.bond, &NullSpecies);
    }

    /// Every line sent so far.
    fn sent(&self) -> Vec<String> {
        self.notifier
            .sent
            .lock()
            .expect("the recorder is never held across a panic")
            .clone()
    }

    /// The picture, at the current clock.
    fn view(&mut self) -> BuddyView {
        self.state.tick(self.now, REST, true);
        self.state.view()
    }
}

#[given("a booted stick")]
fn a_booted_stick(world: &mut StickWorld) {
    *world = StickWorld::default();
}

#[given(regex = r#"^a snapshot arrives with prompt "([^"]+)" for (\w+)$"#)]
#[when(regex = r#"^a snapshot arrives with prompt "([^"]+)" for (\w+)$"#)]
fn a_snapshot_with_a_prompt(world: &mut StickWorld, id: String, tool: String) {
    world.now += 1_000;
    world.state.apply(
        Inbound::Snapshot(SnapshotPacket {
            running: Some(1),
            waiting: Some(1),
            prompt: Some(Prompt {
                id,
                tool,
                hint: "cargo test --workspace".to_string(),
            }),
            ..SnapshotPacket::default()
        }),
        world.now,
    );
}

#[when("an empty snapshot arrives")]
fn an_empty_snapshot(world: &mut StickWorld) {
    world.now += 1_000;
    world
        .state
        .apply(Inbound::Snapshot(SnapshotPacket::default()), world.now);
}

#[when("a transcript event arrives")]
fn a_transcript_event(world: &mut StickWorld) {
    world.now += 1_000;
    world
        .state
        .apply(Inbound::Event(BuddyEvent::Turn), world.now);
}

#[when("a time sync arrives")]
fn a_time_sync(world: &mut StickWorld) {
    world.now += 1_000;
    world.state.apply(
        Inbound::Time {
            epoch: 1_700_000_000,
            tz_offset_s: 0,
        },
        world.now,
    );
}

#[when("a status command arrives")]
fn a_status_command(world: &mut StickWorld) {
    world.now += 1_000;
    world
        .state
        .apply(Inbound::Command(Command::Status), world.now);
}

#[when(regex = r"^(\d+) seconds pass$")]
fn seconds_pass(world: &mut StickWorld, seconds: u64) {
    world.now += seconds * 1_000;
    world.state.tick(world.now, REST, true);
}

#[when("the front button is clicked")]
#[then("the front button is clicked")]
fn the_front_button_is_clicked(world: &mut StickWorld) {
    world.press(ButtonId::Front);
}

#[when("the side button is clicked")]
fn the_side_button_is_clicked(world: &mut StickWorld) {
    world.press(ButtonId::Side);
}

#[then(regex = r#"^the answer sent is "(\w+)" for prompt "([^"]+)"$"#)]
fn the_answer_sent_is(world: &mut StickWorld, decision: String, id: String) {
    let sent: Vec<String> = world.sent();
    let last: &String = sent.last().expect("no answer was ever sent");
    let parsed: PermissionResponse = PermissionResponse::parse(last.as_bytes())
        .expect("the device sent a line the central cannot read");
    assert_eq!(parsed.id, id);
    assert_eq!(parsed.decision.as_wire(), decision);
}

#[then(regex = r"^exactly (\d+) answers? (?:was|were) sent$")]
fn exactly_n_answers(world: &mut StickWorld, count: usize) {
    assert_eq!(world.sent().len(), count);
}

#[then(regex = r#"^the prompt for "([^"]+)" is still on the glass$"#)]
fn the_prompt_is_still_on_the_glass(world: &mut StickWorld, id: String) {
    let view: BuddyView = world.view();
    assert!(
        view.prompt.is_some(),
        "keepalive traffic wiped the pending decision on {id}"
    );
}

#[then("no prompt is on the glass")]
fn no_prompt_is_on_the_glass(world: &mut StickWorld) {
    assert!(world.view().prompt.is_none());
}

#[then("the link is up")]
fn the_link_is_up(world: &mut StickWorld) {
    world.state.tick(world.now, REST, true);
    assert!(world.state.is_linked());
}

#[then("the link is down")]
fn the_link_is_down(world: &mut StickWorld) {
    world.state.tick(world.now, REST, true);
    assert!(
        !world.state.is_linked(),
        "the link should lapse after {LINK_WINDOW_MS} ms without a line"
    );
}

#[tokio::main]
async fn main() {
    StickWorld::run("tests/features").await;
}
