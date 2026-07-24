//! Gherkin plumbing test: proves the plume field obeys its domain rules — a fixed point
//! budget, a breathing animation, and a phase that repeats every full turn. A few of these
//! guard the boundary; the numeric fidelity of the port to its source lives in the property
//! tests next to the code.

use cucumber::{given, then, when, World};
use plume_core::{phase, plume, point, FieldPoint, SinTable, FRAME_MS, PERIOD_FRAMES, POINT_COUNT};

/// One captured frond, as the bare coordinates the scenario compares — the table it was drawn
/// through is rebuilt per capture, so the world stays `Debug` (a 2048-entry table is not).
type Frond = Vec<(f32, f32)>;

/// The scenario's clock and whatever it has captured to compare.
#[derive(Debug, Default, World)]
struct PlumeWorld {
    /// Milliseconds on the animation clock.
    now_ms: u64,
    /// Fronds captured this scenario, in order — compared for "different pictures".
    fronds: Vec<Frond>,
    /// Phases captured this scenario, in order — compared for "the same phase".
    phases: Vec<f32>,
}

impl PlumeWorld {
    /// The frond at the current clock, as plain coordinates.
    fn frond(&self) -> Frond {
        let table: SinTable = SinTable::new();
        plume(phase(self.now_ms), &table)
            .map(|p: FieldPoint| (p.x, p.y))
            .collect()
    }
}

#[given(regex = r"^the plume clock at (\d+) milliseconds$")]
fn the_plume_clock_at(world: &mut PlumeWorld, ms: u64) {
    world.now_ms = ms;
}

#[when("the frond is captured")]
fn the_frond_is_captured(world: &mut PlumeWorld) {
    let frond: Frond = world.frond();
    world.fronds.push(frond);
}

#[when("the phase is captured")]
fn the_phase_is_captured(world: &mut PlumeWorld) {
    world.phases.push(phase(world.now_ms));
}

#[when("a frame passes")]
fn a_frame_passes(world: &mut PlumeWorld) {
    world.now_ms += FRAME_MS;
}

#[when("a full period passes")]
fn a_full_period_passes(world: &mut PlumeWorld) {
    world.now_ms += PERIOD_FRAMES * FRAME_MS;
}

#[then(regex = r"^the frond is made of (\d+) points$")]
fn the_frond_is_made_of_points(world: &mut PlumeWorld, count: u32) {
    assert_eq!(POINT_COUNT, count);
    assert_eq!(world.frond().len(), count as usize);
}

#[then(regex = r"^the point at index (\d+) is finite and within the (\d+) by (\d+) canvas$")]
fn the_point_is_on_canvas(world: &mut PlumeWorld, index: u32, width: f32, height: f32) {
    let table: SinTable = SinTable::new();
    let p: FieldPoint = point(index, phase(world.now_ms), &table);
    assert!(p.x.is_finite() && p.y.is_finite(), "point flew to infinity");
    assert!((0.0..width).contains(&p.x), "x = {} off canvas", p.x);
    assert!((0.0..height).contains(&p.y), "y = {} off canvas", p.y);
}

#[then("the two captured fronds are different pictures")]
fn the_two_fronds_differ(world: &mut PlumeWorld) {
    assert_eq!(world.fronds.len(), 2, "expected two captures");
    assert_ne!(
        world.fronds[0], world.fronds[1],
        "the frond did not move as the phase advanced"
    );
}

#[then("the two captured phases are equal")]
fn the_two_phases_are_equal(world: &mut PlumeWorld) {
    assert_eq!(world.phases.len(), 2, "expected two captures");
    assert_eq!(
        world.phases[0], world.phases[1],
        "the phase did not repeat after a full period"
    );
}

#[tokio::main]
async fn main() {
    PlumeWorld::run("tests/features").await;
}
