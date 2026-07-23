//! Gherkin plumbing test for the buddy domain.
//!
//! Drives [`buddy_core::step`] at the domain boundary: the persona ladder, the wake window,
//! the one-shot overrides, the charging clock, and the button-driven menu, with the clock
//! hand-cranked by every step so the scenarios stay deterministic. The fine grain (EMA,
//! saturation, median/mood/energy math, the token latch) lives in the inline unit and property
//! tests next to each module; these prove the plumbing made it through.

use buddy_core::Approval;
use buddy_core::{
    charging_mood, step, Buddy, MenuOutcome, Outcome, PersonaState, Snapshot, SpeciesIndex,
    StepInput,
};
use cucumber::{given, then, when, World};
use platform_core::{Acceleration, Tick};
use platform_input::{ButtonEvent, ButtonId, Gesture};

/// A resting acceleration under gravity — no shake, not face-down.
const REST: Acceleration = Acceleration::new(0, 0, 1_000);
/// Straight into the desk: the face-down nap predicate holds.
const FACE_DOWN: Acceleration = Acceleration::new(0, 0, -1_000);
/// Face-up under gravity: not face-down, decays the nap counter.
const FACE_UP: Acceleration = Acceleration::new(0, 0, 1_000);
/// A steady 1.82 g tilt. Against the seed baseline of 1.0 g the delta is 0.82 (> 0.8, a shake);
/// against a baseline that has taken one 1.82 g sample (→ 1.041 g) the delta is 0.779 (no shake).
/// That gap is what makes the stale-baseline quirk observable as a persona flip.
const TILT: Acceleration = Acceleration::new(0, 0, 1_820);

/// The scenario's buddy, the snapshot it derives from, and the last outcome a step produced.
#[derive(Debug, World)]
struct BuddyWorld {
    buddy: Buddy,
    snapshot: Snapshot,
    persona: PersonaState,
    menu: MenuOutcome,
    napping: bool,
}

impl Default for BuddyWorld {
    fn default() -> Self {
        BuddyWorld {
            buddy: Buddy::new(SpeciesIndex::new(0)),
            snapshot: Snapshot {
                connected: true,
                sessions_waiting: 0,
                sessions_running: 0,
                recently_completed: false,
            },
            persona: PersonaState::Idle,
            menu: MenuOutcome::None,
            napping: false,
        }
    }
}

/// A fully defaulted input carrying the world's current snapshot at time `now`.
fn input_at(world: &BuddyWorld, now: Tick) -> StepInput {
    StepInput {
        snapshot: world.snapshot,
        now,
        accel: REST,
        screen_on: true,
        menu_open: false,
        prompt_unanswered: false,
        level_up: false,
        approval: None,
        button: None,
        charging_mood: None,
    }
}

/// Fold one input into the world's buddy, recording the persona, menu outcome, and nap state.
fn apply(world: &mut BuddyWorld, input: StepInput) {
    let (next, outcome): (Buddy, Outcome) = step(world.buddy, &input);
    world.buddy = next;
    world.persona = outcome.persona;
    world.menu = outcome.menu;
    world.napping = world.buddy.nap.is_napping();
}

fn parse_persona(name: &str) -> PersonaState {
    match name {
        "Sleep" => PersonaState::Sleep,
        "Idle" => PersonaState::Idle,
        "Busy" => PersonaState::Busy,
        "Attention" => PersonaState::Attention,
        "Celebrate" => PersonaState::Celebrate,
        "Dizzy" => PersonaState::Dizzy,
        "Heart" => PersonaState::Heart,
        other => panic!("unknown persona {other:?}"),
    }
}

#[given("a fresh buddy")]
fn a_fresh_buddy(world: &mut BuddyWorld) {
    *world = BuddyWorld::default();
}

#[given(regex = r"^the bridge is (connected|disconnected)$")]
fn the_bridge_is(world: &mut BuddyWorld, link: String) {
    world.snapshot.connected = link == "connected";
}

#[given(regex = r"^(\d+) sessions are waiting$")]
fn sessions_waiting(world: &mut BuddyWorld, count: u32) {
    world.snapshot.sessions_waiting = count;
}

#[given(regex = r"^(\d+) sessions are running$")]
fn sessions_running(world: &mut BuddyWorld, count: u32) {
    world.snapshot.sessions_running = count;
}

#[given("a session just completed")]
fn a_session_completed(world: &mut BuddyWorld) {
    world.snapshot.recently_completed = true;
}

#[when(regex = r"^the buddy steps at (\d+) ms$")]
fn steps_at(world: &mut BuddyWorld, now: Tick) {
    let input: StepInput = input_at(world, now);
    apply(world, input);
}

#[when(regex = r"^a level-up occurs at (\d+) ms$")]
fn level_up_at(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.level_up = true;
    apply(world, input);
}

#[when(regex = r"^the buddy is shaken at (\d+) ms$")]
fn shaken_at(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    // 2.0 g against the 1.0 g seed baseline is a delta of 1.0 > 0.8.
    input.accel = Acceleration::new(0, 0, 2_000);
    apply(world, input);
}

#[when(regex = r"^an approval is answered in (\d+) s at (\d+) ms$")]
fn approval_answered_at(world: &mut BuddyWorld, took: u32, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.approval = Some(Approval {
        approved: true,
        took_s: took,
    });
    apply(world, input);
}

#[when(regex = r"^the front button is long-held at (\d+) ms$")]
fn long_hold_at(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.button = Some(ButtonEvent::new(ButtonId::Front, Gesture::LongHold));
    apply(world, input);
}

#[when(regex = r"^the charging clock says (\w+) at (\d+) ms$")]
fn charging_says_at(world: &mut BuddyWorld, name: String, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.charging_mood = Some(parse_persona(&name));
    apply(world, input);
}

// --- The charging clock, read directly (defect (d): the midnight-Dizzy hoist). ---

#[when(regex = r"^the charging clock is read at hour (\d+) on day (\d+) at (\d+) ms$")]
fn charging_clock_read(world: &mut BuddyWorld, hour: u8, dow: u8, now: Tick) {
    world.persona = charging_mood(hour, dow, now);
}

// --- The shake detector's stale-baseline quirk. ---

#[when(regex = r"^the menu is held open under a steady tilt at (\d+) ms$")]
fn menu_held_open_under_tilt(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.menu_open = true;
    input.accel = TILT;
    apply(world, input);
}

#[when(regex = r"^the buddy is tilted the same amount with the menu closed at (\d+) ms$")]
fn tilted_menu_closed(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.accel = TILT;
    apply(world, input);
}

// --- The face-down nap: counter frozen under a prompt, transition still live. ---

#[when(regex = r"^the buddy lies face-down at (\d+) ms$")]
fn lies_face_down(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.accel = FACE_DOWN;
    apply(world, input);
}

#[when(regex = r"^the buddy lies face-down under an unanswered prompt at (\d+) ms$")]
fn lies_face_down_under_prompt(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.accel = FACE_DOWN;
    input.prompt_unanswered = true;
    apply(world, input);
}

#[when(regex = r"^the buddy lies face-up under an unanswered prompt at (\d+) ms$")]
fn lies_face_up_under_prompt(world: &mut BuddyWorld, now: Tick) {
    let mut input: StepInput = input_at(world, now);
    input.accel = FACE_UP;
    input.prompt_unanswered = true;
    apply(world, input);
}

#[then(regex = r"^the persona is (\w+)$")]
fn persona_is(world: &mut BuddyWorld, name: String) {
    assert_eq!(world.persona, parse_persona(&name));
}

#[then(regex = r"^the buddy is (napping|not napping|still napping)$")]
fn napping_is(world: &mut BuddyWorld, state: String) {
    let expected: bool = state != "not napping";
    assert_eq!(world.napping, expected);
}

#[then("the menu is open")]
fn the_menu_is_open(world: &mut BuddyWorld) {
    assert!(world.buddy.menu.is_open());
    assert_eq!(world.menu, MenuOutcome::Opened);
}

#[tokio::main]
async fn main() {
    BuddyWorld::run("tests/features").await;
}
