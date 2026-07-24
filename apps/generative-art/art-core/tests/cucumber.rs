//! Gherkin plumbing test: proves the gallery obeys its running-order rules — it opens on the
//! plume, a press steps to the next piece, and the order wraps after the last back to the first.
//! These guard the boundary; which pixels each sketch draws lives in the display crate's tests
//! and screenshots.

use art_core::{Selector, Sketch};
use cucumber::{given, then, when, World};

/// The scenario's gallery — just a [`Selector`] pressed and inspected.
#[derive(Debug, Default, World)]
struct GalleryWorld {
    gallery: Selector,
}

/// The sketch a scenario names in prose, mapped to its [`Sketch`]. Panics on an unknown name so
/// a typo in a feature file is a loud failure, not a silently skipped step.
fn sketch_named(name: &str) -> Sketch {
    match name {
        "plume" => Sketch::Plume,
        "squares" => Sketch::Squares,
        "fan" => Sketch::Fan,
        "orbits" => Sketch::Orbits,
        "willow" => Sketch::Willow,
        other => panic!("unknown sketch in a scenario: {other:?}"),
    }
}

#[given("a fresh gallery")]
fn a_fresh_gallery(world: &mut GalleryWorld) {
    world.gallery = Selector::new();
}

#[when(regex = r"^the button is pressed (\d+) times?$")]
fn the_button_is_pressed(world: &mut GalleryWorld, presses: usize) {
    for _ in 0..presses {
        world.gallery.advance();
    }
}

#[then(regex = r"^the sketch on the glass is the (\w+)$")]
fn the_sketch_on_the_glass_is(world: &mut GalleryWorld, name: String) {
    assert_eq!(world.gallery.current(), sketch_named(&name));
}

#[tokio::main]
async fn main() {
    GalleryWorld::run("tests/features").await;
}
