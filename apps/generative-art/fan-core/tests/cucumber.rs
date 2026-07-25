//! Gherkin plumbing test: proves the fan obeys its domain rules — a fixed grid of triangles, a
//! folding animation, and a phase that repeats every full turn. A few of these guard the boundary;
//! the numeric fidelity of the port to its source lives in the property tests next to the code.

use cucumber::{given, then, when, World};
use fan_core::{cells, phase, Cell, SinTable, PERIOD_MS};

/// One captured fan, as the bare apex coordinates the scenario compares — the table it was drawn
/// through is rebuilt per capture, so the world stays `Debug` (a 2048-entry table is not).
type Fan = Vec<(f32, f32)>;

/// The scenario's clock and whatever it has captured to compare.
#[derive(Debug, Default, World)]
struct FanWorld {
    /// Milliseconds on the animation clock.
    now_ms: u64,
    /// Fans captured this scenario, in order — compared for "different pictures".
    fans: Vec<Fan>,
    /// Phases captured this scenario, in order — compared for "the same phase".
    phases: Vec<f32>,
}

impl FanWorld {
    /// The fan at the current clock, as the folding apex of each triangle — the vertex that moves,
    /// so a fold shows as a change.
    fn fan(&self) -> Fan {
        let table: SinTable = SinTable::new();
        cells(phase(self.now_ms), &table)
            .map(|c: Cell| c.verts[1])
            .collect()
    }
}

#[given(regex = r"^the fan clock at (\d+) milliseconds$")]
fn the_fan_clock_at(world: &mut FanWorld, ms: u64) {
    world.now_ms = ms;
}

#[when("the fan is captured")]
fn the_fan_is_captured(world: &mut FanWorld) {
    let fan: Fan = world.fan();
    world.fans.push(fan);
}

#[when("the phase is captured")]
fn the_phase_is_captured(world: &mut FanWorld) {
    world.phases.push(phase(world.now_ms));
}

#[when("a quarter period passes")]
fn a_quarter_period_passes(world: &mut FanWorld) {
    world.now_ms += PERIOD_MS / 4;
}

#[when("a full period passes")]
fn a_full_period_passes(world: &mut FanWorld) {
    world.now_ms += PERIOD_MS;
}

#[then(regex = r"^the fan is made of (\d+) triangles$")]
fn the_fan_is_made_of_triangles(world: &mut FanWorld, count: usize) {
    assert_eq!(world.fan().len(), count);
}

#[then("the two captured fans are different pictures")]
fn the_two_fans_differ(world: &mut FanWorld) {
    assert_eq!(world.fans.len(), 2, "expected two captures");
    assert_ne!(
        world.fans[0], world.fans[1],
        "the fan did not fold as the phase advanced"
    );
}

#[then("the two captured phases are equal")]
fn the_two_phases_are_equal(world: &mut FanWorld) {
    assert_eq!(world.phases.len(), 2, "expected two captures");
    assert_eq!(
        world.phases[0], world.phases[1],
        "the phase did not repeat after a full period"
    );
}

#[tokio::main]
async fn main() {
    FanWorld::run("tests/features").await;
}
