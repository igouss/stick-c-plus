//! Gherkin plumbing test: proves an [`Effect`] renders into a frame the way the
//! feature file describes. A few of these guard the domain boundary; the fine
//! grain lives in the unit and property tests next to the code.

use core::time::Duration;
use cucumber::{given, then, when, World};
use led_core::{Effect, Rainbow, Rgb, SolidColor};

const RED: Rgb = Rgb::new(255, 0, 0);

/// Which effect the scenario selected. An enum (not a `Box<dyn Effect>`) keeps
/// the [`World`] `Debug` and lets us build the concrete effect at render time.
#[derive(Debug, Clone, Copy)]
enum Chosen {
    SolidRed,
    Rainbow,
}

#[derive(Debug, Default, World)]
struct StripWorld {
    frame: Vec<Rgb>,
    chosen: Option<Chosen>,
}

#[given(regex = r"^a strip of (\d+) pixels$")]
fn a_strip(world: &mut StripWorld, count: usize) {
    world.frame = vec![Rgb::BLACK; count];
}

#[given("a solid red effect")]
fn a_solid_red_effect(world: &mut StripWorld) {
    world.chosen = Some(Chosen::SolidRed);
}

#[given("a rainbow effect")]
fn a_rainbow_effect(world: &mut StripWorld) {
    world.chosen = Some(Chosen::Rainbow);
}

#[when(regex = r"^the strip is rendered at (\d+) ms$")]
fn render(world: &mut StripWorld, ms: u64) {
    let t = Duration::from_millis(ms);
    match world.chosen.expect("a scenario must choose an effect") {
        Chosen::SolidRed => SolidColor::new(RED).render(t, &mut world.frame),
        Chosen::Rainbow => {
            Rainbow { spatial: 85, speed: 0, sat: 255, val: 255 }.render(t, &mut world.frame)
        }
    }
}

#[then("every pixel is red")]
fn every_pixel_is_red(world: &mut StripWorld) {
    assert!(world.frame.iter().all(|px| *px == RED));
}

#[then("the strip stays empty")]
fn the_strip_stays_empty(world: &mut StripWorld) {
    assert!(world.frame.is_empty());
}

#[then("pixel 0 differs from pixel 1")]
fn pixel_0_differs_from_pixel_1(world: &mut StripWorld) {
    assert_ne!(world.frame[0], world.frame[1]);
}

#[tokio::main]
async fn main() {
    StripWorld::run("tests/features").await;
}
