//! Gherkin plumbing test: proves the orbits field obeys its domain rules — thirty orbits, a bloom
//! that peaks on a centre, and a comet that drifts as the frame advances. These guard the boundary;
//! the numeric fidelity of the port to its source (the triangle wave, the bloom) lives in the
//! property tests next to the code.

use cucumber::{given, then, when, World};
use orbits_core::{bloom, centres, for_each_cell, frame, Centre, COLS, ORBITS, SOURCE};

/// One captured field, as the bare grey levels the scenario compares.
type Field = Vec<f32>;

/// The scenario's clock (as a virtual frame) and whatever fields it has captured to compare.
#[derive(Debug, Default, World)]
struct OrbitsWorld {
    /// The virtual frame the field is drawn at.
    frame: f32,
    /// Fields captured this scenario, in order — compared for "different pictures".
    fields: Vec<Field>,
}

impl OrbitsWorld {
    /// The whole grid of grey levels at the current frame, in the source's walk order.
    fn field(&self) -> Field {
        let mut cells: Field = Vec::new();
        for_each_cell(self.frame, 0..COLS, |_col: u16, _row: u16, grey: f32| {
            cells.push(grey)
        });
        cells
    }
}

#[given(regex = r"^the orbits clock at (\d+) milliseconds$")]
fn the_orbits_clock_at(world: &mut OrbitsWorld, ms: u64) {
    world.frame = frame(ms);
}

#[when("the field is captured")]
fn the_field_is_captured(world: &mut OrbitsWorld) {
    let field: Field = world.field();
    world.fields.push(field);
}

#[when("the comet drifts on")]
fn the_comet_drifts_on(world: &mut OrbitsWorld) {
    world.frame += 30.0; // a moment: thirty virtual frames on
}

#[then(regex = r"^the field has (\d+) orbits$")]
fn the_field_has_orbits(world: &mut OrbitsWorld, count: usize) {
    assert_eq!(centres(world.frame).len(), count);
}

#[then("a bloom peaks on an orbit centre")]
fn a_bloom_peaks_on_an_orbit_centre(world: &mut OrbitsWorld) {
    let cs: [Centre; ORBITS] = centres(world.frame);
    let head: Centre = cs[0];
    // A cell exactly on an orbit's centre is at zero taxicab distance, so its bloom is the full
    // SOURCE — the brightest any cell can be.
    assert_eq!(bloom(head.x, head.y, &cs), SOURCE);
}

#[then("the two captured fields are different pictures")]
fn the_two_fields_differ(world: &mut OrbitsWorld) {
    assert_eq!(world.fields.len(), 2, "expected two captures");
    assert_ne!(
        world.fields[0], world.fields[1],
        "the comet did not drift as the frame advanced"
    );
}

#[tokio::main]
async fn main() {
    OrbitsWorld::run("tests/features").await;
}
