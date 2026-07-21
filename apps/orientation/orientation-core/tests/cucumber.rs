//! Gherkin plumbing test: proves the orientation transform turns a gravity vector into the
//! tilt and the pose the feature file describes, raw and through the smoother alike. A few of
//! these guard the domain boundary; the fine grain lives in the unit and property tests next
//! to the code.

use cucumber::{given, then, when, World};
use orientation_core::{
    Facing, Orientation, Reading, RotationSettler, ScreenRotation, Signal, Smoother,
};
use platform_core::{Acceleration, Tick};

/// The scenario's latest orientation, the smoother it was read through (if any), and how long
/// ago the sensor last answered.
#[derive(Debug, Default, World)]
struct BoardWorld {
    /// `Some` once a scenario asks for a smoothed readout; `None` reads samples raw.
    smoother: Option<Smoother>,
    /// The orientation the last reading produced.
    orientation: Orientation,
    /// Milliseconds since the last successful read — what the staleness rule judges.
    age_ms: Tick,
    /// The wall clock the rotation settler is driven from.
    now_ms: Tick,
    /// Which way up the picture is being drawn, and what is waiting to replace it.
    settler: RotationSettler,
}

impl BoardWorld {
    /// Take one accelerometer reading, through the smoother if this scenario has one. A
    /// successful read is what resets the age, exactly as a publication does on the board.
    fn read(&mut self, sample: Acceleration) {
        let effective: Acceleration = match &mut self.smoother {
            Some(smoother) => smoother.update(sample),
            None => sample,
        };
        self.orientation = Orientation::of(effective);
        self.age_ms = 0;
        self.settler.update(effective, self.now_ms);
    }

    /// What the glass is handed: the pose, and whether it is still being confirmed.
    fn reading(&self) -> Reading {
        Reading::aged(self.orientation, self.age_ms)
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

#[when(regex = r"^the accelerometer does not answer for (\d+) milliseconds$")]
fn the_accelerometer_does_not_answer(world: &mut BoardWorld, silent_ms: Tick) {
    // A failed read publishes nothing, so the last pose simply ages — the same mechanism the
    // sampler relies on, with no dead-sensor branch anywhere.
    world.age_ms += silent_ms;
}

#[when(regex = r"^(\d+) milliseconds pass$")]
fn milliseconds_pass(world: &mut BoardWorld, elapsed_ms: Tick) {
    world.now_ms += elapsed_ms;
    // Time passing is not a new reading: the settler sees the pose it already had, which is
    // exactly what lets a held rotation come good without the board being touched.
    let held: Acceleration = world.orientation.acceleration;
    world.settler.update(held, world.now_ms);
}

#[then(regex = r"^the picture is drawn at (\d+) degrees$")]
fn the_picture_is_drawn_at(world: &mut BoardWorld, degrees: u32) {
    let expected: ScreenRotation = match degrees {
        0 => ScreenRotation::Deg0,
        90 => ScreenRotation::Deg90,
        180 => ScreenRotation::Deg180,
        270 => ScreenRotation::Deg270,
        other => panic!("{other} is not a quarter turn"),
    };
    assert_eq!(world.settler.showing(), expected);
}

#[then(regex = r"^the facing is (\w+)$")]
fn the_facing_is(world: &mut BoardWorld, name: String) {
    assert_eq!(world.orientation.facing, parse_facing(&name));
}

#[then("the readout is live")]
fn the_readout_is_live(world: &mut BoardWorld) {
    assert_eq!(world.reading().signal, Signal::Live);
}

#[then("the readout reports no signal")]
fn the_readout_reports_no_signal(world: &mut BoardWorld) {
    assert_eq!(world.reading().signal, Signal::Lost);
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
