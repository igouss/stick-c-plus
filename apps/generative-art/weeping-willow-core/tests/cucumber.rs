//! Gherkin plumbing test: proves the weeping willow obeys its domain rules — a frame of wood, a
//! whole canopy of fronds, a sway that moves the picture, and a full breath that returns it exactly.
//! These guard the boundary; the numeric fidelity of the sway to its reference lives in the property
//! tests next to the code.

use cucumber::{given, then, when, World};
use weeping_willow_core::{
    phase, Firefly, FrondPoint, SinTable, Swarm, Tree, FROND_COUNT, PERIOD_MS,
};

/// One captured picture, as the bare positions the scenario compares — the table it was swept
/// through is rebuilt per capture, so the world stays `Debug` (a large table is not).
type Shape = Vec<f32>;

/// The scenario's clock and whatever it has captured to compare.
#[derive(Debug, Default, World)]
struct WillowWorld {
    /// Milliseconds on the sway clock.
    now_ms: u64,
    /// Canopies captured this scenario, in order — compared for "different"/"same" pictures.
    canopies: Vec<Shape>,
    /// Swarms captured this scenario, in order — compared for "different"/"same" pictures.
    swarms: Vec<Shape>,
}

impl WillowWorld {
    /// The canopy's frond x-positions at the current clock.
    fn canopy(&self) -> Shape {
        let tree: Tree = Tree::new();
        let table: SinTable = SinTable::new();
        tree.sway(phase(self.now_ms), &table)
            .map(|point: FrondPoint| point.x)
            .collect()
    }

    /// The swarm's bug positions at the current clock, `x` then `y` per bug.
    fn swarm(&self) -> Shape {
        let swarm: Swarm = Swarm::new();
        let table: SinTable = SinTable::new();
        swarm
            .at(phase(self.now_ms), &table)
            .flat_map(|bug: Firefly| [bug.x, bug.y])
            .collect()
    }
}

#[given(regex = r"^the willow clock at (\d+) milliseconds$")]
fn the_willow_clock_at(world: &mut WillowWorld, ms: u64) {
    world.now_ms = ms;
}

#[when("the canopy is captured")]
fn the_canopy_is_captured(world: &mut WillowWorld) {
    let canopy: Shape = world.canopy();
    world.canopies.push(canopy);
}

#[when("the swarm is captured")]
fn the_swarm_is_captured(world: &mut WillowWorld) {
    let swarm: Shape = world.swarm();
    world.swarms.push(swarm);
}

#[when("the wind blows on")]
fn the_wind_blows_on(world: &mut WillowWorld) {
    world.now_ms += PERIOD_MS / 4; // a quarter breath on
}

#[when("a full breath passes")]
fn a_full_breath_passes(world: &mut WillowWorld) {
    world.now_ms += PERIOD_MS;
}

#[then("the tree stands on some wood")]
fn the_tree_stands_on_some_wood(_world: &mut WillowWorld) {
    let tree: Tree = Tree::new();
    assert!(!tree.wood().is_empty(), "the tree has no wood");
}

#[then(regex = r"^the canopy hangs (\d+) fronds$")]
fn the_canopy_hangs_fronds(_world: &mut WillowWorld, count: usize) {
    assert_eq!(FROND_COUNT, count);
}

#[then("the two captured canopies are different pictures")]
fn the_two_canopies_differ(world: &mut WillowWorld) {
    assert_eq!(world.canopies.len(), 2, "expected two captures");
    assert_ne!(
        world.canopies[0], world.canopies[1],
        "the canopy did not sway as the wind advanced"
    );
}

#[then("the two captured canopies are the same picture")]
fn the_two_canopies_match(world: &mut WillowWorld) {
    assert_eq!(world.canopies.len(), 2, "expected two captures");
    assert_eq!(
        world.canopies[0], world.canopies[1],
        "the canopy did not return to its start after a full breath"
    );
}

#[then("the two captured swarms are different pictures")]
fn the_two_swarms_differ(world: &mut WillowWorld) {
    assert_eq!(world.swarms.len(), 2, "expected two captures");
    assert_ne!(
        world.swarms[0], world.swarms[1],
        "the swarm did not drift as the wind advanced"
    );
}

#[then("the two captured swarms are the same picture")]
fn the_two_swarms_match(world: &mut WillowWorld) {
    assert_eq!(world.swarms.len(), 2, "expected two captures");
    assert_eq!(
        world.swarms[0], world.swarms[1],
        "the swarm did not return to its start after a full breath"
    );
}

#[tokio::main]
async fn main() {
    WillowWorld::run("tests/features").await;
}
