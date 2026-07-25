//! Gherkin plumbing test: proves the squares grid obeys its domain rules — a fixed set of cells,
//! a breathing animation, and a phase that repeats every full turn. A few of these guard the
//! boundary; the numeric fidelity of the port to its source lives in the property tests next to
//! the code.

use cucumber::{given, then, when, World};
use squares_core::{cells, phase, Cell, SinTable, PERIOD_MS};

/// One captured grid, as the bare breaths the scenario compares — the table it was drawn through
/// is rebuilt per capture, so the world stays `Debug` (a 2048-entry table is not).
type Grid = Vec<f32>;

/// The scenario's clock and whatever it has captured to compare.
#[derive(Debug, Default, World)]
struct SquaresWorld {
    /// Milliseconds on the animation clock.
    now_ms: u64,
    /// Grids captured this scenario, in order — compared for "different pictures".
    grids: Vec<Grid>,
    /// Phases captured this scenario, in order — compared for "the same phase".
    phases: Vec<f32>,
}

impl SquaresWorld {
    /// The grid at the current clock, as plain breaths.
    fn grid(&self) -> Grid {
        let table: SinTable = SinTable::new();
        cells(phase(self.now_ms), &table)
            .map(|c: Cell| c.breath)
            .collect()
    }
}

#[given(regex = r"^the squares clock at (\d+) milliseconds$")]
fn the_squares_clock_at(world: &mut SquaresWorld, ms: u64) {
    world.now_ms = ms;
}

#[when("the grid is captured")]
fn the_grid_is_captured(world: &mut SquaresWorld) {
    let grid: Grid = world.grid();
    world.grids.push(grid);
}

#[when("the phase is captured")]
fn the_phase_is_captured(world: &mut SquaresWorld) {
    world.phases.push(phase(world.now_ms));
}

#[when("a quarter period passes")]
fn a_quarter_period_passes(world: &mut SquaresWorld) {
    world.now_ms += PERIOD_MS / 4;
}

#[when("a full period passes")]
fn a_full_period_passes(world: &mut SquaresWorld) {
    world.now_ms += PERIOD_MS;
}

#[then(regex = r"^the grid is made of (\d+) cells$")]
fn the_grid_is_made_of_cells(world: &mut SquaresWorld, count: usize) {
    assert_eq!(world.grid().len(), count);
}

#[then("the two captured grids are different pictures")]
fn the_two_grids_differ(world: &mut SquaresWorld) {
    assert_eq!(world.grids.len(), 2, "expected two captures");
    assert_ne!(
        world.grids[0], world.grids[1],
        "the grid did not breathe as the phase advanced"
    );
}

#[then("the two captured phases are equal")]
fn the_two_phases_are_equal(world: &mut SquaresWorld) {
    assert_eq!(world.phases.len(), 2, "expected two captures");
    assert_eq!(
        world.phases[0], world.phases[1],
        "the phase did not repeat after a full period"
    );
}

#[tokio::main]
async fn main() {
    SquaresWorld::run("tests/features").await;
}
