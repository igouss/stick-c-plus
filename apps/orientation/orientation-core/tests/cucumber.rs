//! Gherkin plumbing test: proves the orientation transform turns a gravity vector into the
//! tilt and the pose the feature file describes, raw and through the smoother alike. A few of
//! these guard the domain boundary; the fine grain lives in the unit and property tests next
//! to the code.

use cucumber::{given, then, when, World};
use orientation_core::{Facing, Orientation, Smoother};
use platform_core::Acceleration;

/// The scenario's latest orientation, and the smoother it was read through (if any).
#[derive(Debug, Default, World)]
struct BoardWorld {
    /// `Some` once a scenario asks for a smoothed readout; `None` reads samples raw.
    smoother: Option<Smoother>,
    /// The orientation the last reading produced.
    orientation: Orientation,
}

impl BoardWorld {
    /// Take one accelerometer reading, through the smoother if this scenario has one.
    fn read(&mut self, sample: Acceleration) {
        let effective: Acceleration = match &mut self.smoother {
            Some(smoother) => smoother.update(sample),
            None => sample,
        };
        self.orientation = Orientation::of(effective);
    }
}

fn parse_facing(name: &str) -> Facing {
    match name {
        "ScreenUp" => Facing::ScreenUp,
        "ScreenDown" => Facing::ScreenDown,
        "Upright" => Facing::Upright,
        "Inverted" => Facing::Inverted,
        "LeftEdge" => Facing::LeftEdge,
        "RightEdge" => Facing::RightEdge,
        "Tilted" => Facing::Tilted,
        "Moving" => Facing::Moving,
        other => panic!("unknown facing {other:?}"),
    }
}

#[given("a smoothed readout")]
fn a_smoothed_readout(world: &mut BoardWorld) {
    world.smoother = Some(Smoother::default());
}

#[when(regex = r"^the accelerometer reads (-?\d+), (-?\d+), (-?\d+) milli-g$")]
fn the_accelerometer_reads(world: &mut BoardWorld, x_mg: i32, y_mg: i32, z_mg: i32) {
    world.read(Acceleration::new(x_mg, y_mg, z_mg));
}

#[when(regex = r"^the accelerometer reads (-?\d+), (-?\d+), (-?\d+) milli-g (\d+) times$")]
fn the_accelerometer_reads_repeatedly(
    world: &mut BoardWorld,
    x_mg: i32,
    y_mg: i32,
    z_mg: i32,
    times: usize,
) {
    let sample: Acceleration = Acceleration::new(x_mg, y_mg, z_mg);
    (0..times).for_each(|_| world.read(sample));
}

#[then(regex = r"^the facing is (\w+)$")]
fn the_facing_is(world: &mut BoardWorld, name: String) {
    assert_eq!(world.orientation.facing, parse_facing(&name));
}

#[then(regex = r"^the pitch is (-?\d+) degrees$")]
fn the_pitch_is(world: &mut BoardWorld, expected: i32) {
    assert_eq!(world.orientation.attitude.pitch_deg, expected);
}

#[then(regex = r"^the roll is (-?\d+) degrees$")]
fn the_roll_is(world: &mut BoardWorld, expected: i32) {
    assert_eq!(world.orientation.attitude.roll_deg, expected);
}

#[then(regex = r"^the reading is still (-?\d+), (-?\d+), (-?\d+) milli-g$")]
fn the_reading_is_still(world: &mut BoardWorld, x_mg: i32, y_mg: i32, z_mg: i32) {
    assert_eq!(
        world.orientation.acceleration,
        Acceleration::new(x_mg, y_mg, z_mg)
    );
}

#[tokio::main]
async fn main() {
    BoardWorld::run("tests/features").await;
}
