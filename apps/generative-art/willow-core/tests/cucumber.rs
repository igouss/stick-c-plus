//! Gherkin plumbing test: proves the willow curtain obeys its domain rules — a fixed set of
//! tendrils, a sway that moves the picture, and a full breath that returns it exactly. These guard
//! the boundary; the numeric fidelity of the sway to its reference lives in the property tests next
//! to the code.

use cucumber::{given, then, when, World};
use willow_core::{phase, Curtain, SinTable, StrandPoint, PERIOD_MS, STRAND_COUNT};

/// One captured curtain, as the bare x-positions the scenario compares — the table it was swept
/// through is rebuilt per capture, so the world stays `Debug` (a 2048-entry table is not).
type Shape = Vec<f32>;

/// The scenario's clock and whatever it has captured to compare.
#[derive(Debug, Default, World)]
struct WillowWorld {
    /// Milliseconds on the sway clock.
    now_ms: u64,
    /// Curtains captured this scenario, in order — compared for "different"/"same" pictures.
    curtains: Vec<Shape>,
}

impl WillowWorld {
    /// The curtain's x-positions at the current clock.
    fn curtain(&self) -> Shape {
        let table: SinTable = SinTable::new();
        Curtain::new()
            .sway(phase(self.now_ms), &table)
            .map(|point: StrandPoint| point.x)
            .collect()
    }
}

#[given(regex = r"^the willow clock at (\d+) milliseconds$")]
fn the_willow_clock_at(world: &mut WillowWorld, ms: u64) {
    world.now_ms = ms;
}

#[when("the curtain is captured")]
fn the_curtain_is_captured(world: &mut WillowWorld) {
    let curtain: Shape = world.curtain();
    world.curtains.push(curtain);
}

#[when("the wind blows on")]
fn the_wind_blows_on(world: &mut WillowWorld) {
    world.now_ms += PERIOD_MS / 4; // a quarter breath on
}

#[when("a full breath passes")]
fn a_full_breath_passes(world: &mut WillowWorld) {
    world.now_ms += PERIOD_MS;
}

#[then(regex = r"^the curtain hangs (\d+) tendrils$")]
fn the_curtain_hangs_tendrils(_world: &mut WillowWorld, count: usize) {
    assert_eq!(STRAND_COUNT, count);
}

#[then("the two captured curtains are different pictures")]
fn the_two_curtains_differ(world: &mut WillowWorld) {
    assert_eq!(world.curtains.len(), 2, "expected two captures");
    assert_ne!(
        world.curtains[0], world.curtains[1],
        "the curtain did not sway as the wind advanced"
    );
}

#[then("the two captured curtains are the same picture")]
fn the_two_curtains_match(world: &mut WillowWorld) {
    assert_eq!(world.curtains.len(), 2, "expected two captures");
    assert_eq!(
        world.curtains[0], world.curtains[1],
        "the curtain did not return to its start after a full breath"
    );
}

#[tokio::main]
async fn main() {
    WillowWorld::run("tests/features").await;
}
