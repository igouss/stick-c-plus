//! Gherkin plumbing test: proves the settled screen rotation is what the feature file says,
//! driven the way the board drives it — a stream of readings against a clock.
//!
//! A few of these guard the kernel's boundary; the fine grain lives in the unit tests next to
//! `rotation.rs`.

use cucumber::{then, when, World};
use platform_core::{Acceleration, RotationSettler, ScreenRotation, Tick};

/// The rotation currently showing, the clock it is settled against, and the reading it is
/// being held at.
#[derive(Debug, Default, World)]
struct GlassWorld {
    /// Which way up the picture is being drawn, and what is waiting to replace it.
    settler: RotationSettler,
    /// The wall clock the settler is driven from.
    now_ms: Tick,
    /// The last reading taken — what "time passes" keeps feeding, since a board nobody
    /// touches goes on reporting the same vector.
    held: Acceleration,
}

#[when(regex = r"^the accelerometer reads (-?\d+), (-?\d+), (-?\d+) milli-g$")]
fn the_accelerometer_reads(world: &mut GlassWorld, x_mg: i32, y_mg: i32, z_mg: i32) {
    world.held = Acceleration::new(x_mg, y_mg, z_mg);
    let (held, now): (Acceleration, Tick) = (world.held, world.now_ms);
    world.settler.update(held, now);
}

#[when(regex = r"^(\d+) milliseconds pass$")]
fn milliseconds_pass(world: &mut GlassWorld, elapsed_ms: Tick) {
    world.now_ms += elapsed_ms;
    // Time passing is not a new reading: the settler sees the pose it already had, which is
    // exactly what lets a held rotation come good without the board being touched.
    let (held, now): (Acceleration, Tick) = (world.held, world.now_ms);
    world.settler.update(held, now);
}

#[then(regex = r"^the picture is drawn at (\d+) degrees$")]
fn the_picture_is_drawn_at(world: &mut GlassWorld, degrees: u32) {
    let expected: ScreenRotation = match degrees {
        0 => ScreenRotation::Deg0,
        90 => ScreenRotation::Deg90,
        180 => ScreenRotation::Deg180,
        270 => ScreenRotation::Deg270,
        other => panic!("{other} is not a quarter turn"),
    };
    assert_eq!(world.settler.showing(), expected);
}

#[tokio::main]
async fn main() {
    GlassWorld::run("tests/features").await;
}
