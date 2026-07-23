//! Gherkin plumbing test for the creature seam.
//!
//! Drives [`buddy_creature`] at the domain boundary: the registry resolves a persisted index
//! totally (in-range → Some, out-of-range and the GIF sentinel → None), the claudepix creature
//! binds each [`PersonaState`] onto its exact vendored preset set, and [`current`] selects one
//! sprite over injected elapsed time — deterministically, and delegating frame timing to the
//! sprite's own clock.
//!
//! The TOTALITY claims (every species answers every state; resolve over the whole `u8` index
//! space; the chosen sprite is always a member of the state's set for any elapsed) are pinned by
//! the inline property tests next to each module — a single example cannot stand for "for all".
//! These scenarios prove the plumbing made it through.

use buddy_core::GIF_SENTINEL;
use buddy_creature::{current, resolve, PersonaState, Selected, Species, SpeciesIndex, REGISTRY};
use cucumber::{given, then, when, World};
use platform_display::sprite::Sprite;

/// The scenario's resolved creature, the creature under selection, the last queried sprite set,
/// and the last selection(s) `current` produced.
#[derive(Debug, World)]
struct CreatureWorld {
    resolved: Option<&'static Species>,
    species: &'static Species,
    sprites: &'static [&'static Sprite],
    elapsed_ms: u64,
    first: Option<Selected>,
    second: Option<Selected>,
}

impl Default for CreatureWorld {
    fn default() -> Self {
        CreatureWorld {
            resolved: None,
            species: &buddy_creature::CLAUDEPIX,
            sprites: &[],
            elapsed_ms: 0,
            first: None,
            second: None,
        }
    }
}

fn parse_persona(name: &str) -> PersonaState {
    match name {
        "Sleep" => PersonaState::Sleep,
        "Idle" => PersonaState::Idle,
        "Busy" => PersonaState::Busy,
        "Attention" => PersonaState::Attention,
        "Celebrate" => PersonaState::Celebrate,
        "Dizzy" => PersonaState::Dizzy,
        "Heart" => PersonaState::Heart,
        other => panic!("unknown persona {other:?}"),
    }
}

#[given("the claudepix creature")]
fn the_claudepix_creature(world: &mut CreatureWorld) {
    world.species = &buddy_creature::CLAUDEPIX;
}

#[when(regex = r"^species index (\d+) is resolved$")]
fn index_is_resolved(world: &mut CreatureWorld, index: u8) {
    world.resolved = resolve(SpeciesIndex::new(index));
}

#[when("the GIF sentinel index is resolved")]
fn the_sentinel_is_resolved(world: &mut CreatureWorld) {
    world.resolved = resolve(SpeciesIndex::new(GIF_SENTINEL));
}

#[when(regex = r"^the sprite set for (\w+) is queried$")]
fn the_set_is_queried(world: &mut CreatureWorld, state: String) {
    world.sprites = world.species.sprites(parse_persona(&state));
}

#[when(regex = r"^current is selected for (\w+) at (\d+) ms$")]
fn current_is_selected(world: &mut CreatureWorld, state: String, elapsed: u64) {
    world.elapsed_ms = elapsed;
    world.first = Some(current(world.species, parse_persona(&state), elapsed));
}

#[when(regex = r"^current is selected twice for (\w+) at (\d+) ms$")]
fn current_is_selected_twice(world: &mut CreatureWorld, state: String, elapsed: u64) {
    world.elapsed_ms = elapsed;
    let persona: PersonaState = parse_persona(&state);
    world.first = Some(current(world.species, persona, elapsed));
    world.second = Some(current(world.species, persona, elapsed));
}

#[then("resolution yields the claudepix creature")]
fn resolution_yields_claudepix(world: &mut CreatureWorld) {
    assert_eq!(
        world.resolved.map(|s: &Species| s.name()),
        Some("claudepix")
    );
}

#[then("resolution yields nothing")]
fn resolution_yields_nothing(world: &mut CreatureWorld) {
    assert!(world.resolved.is_none());
}

#[then(regex = r"^the registry holds (\d+) creatures?$")]
fn the_registry_holds(world: &mut CreatureWorld, count: usize) {
    let _ = world;
    assert_eq!(REGISTRY.len(), count);
}

#[then("registry entry 0 is claudepix")]
fn registry_entry_zero(world: &mut CreatureWorld) {
    let _ = world;
    assert_eq!(REGISTRY[0].name(), "claudepix");
}

#[then(regex = r"^the set is exactly (.+)$")]
fn the_set_is_exactly(world: &mut CreatureWorld, expected: String) {
    let got: String = world
        .sprites
        .iter()
        .map(|s: &&Sprite| s.slug())
        .collect::<Vec<&str>>()
        .join(", ");
    assert_eq!(got, expected);
}

#[then("the set is not empty")]
fn the_set_is_not_empty(world: &mut CreatureWorld) {
    assert!(!world.sprites.is_empty());
}

#[then(regex = r"^the chosen sprite is (\w+)$")]
fn the_chosen_sprite_is(world: &mut CreatureWorld, slug: String) {
    let selected: Selected = world.first.expect("no selection recorded");
    assert_eq!(selected.sprite.slug(), slug);
}

#[then("both selections are identical")]
fn both_selections_are_identical(world: &mut CreatureWorld) {
    let first: Selected = world.first.expect("no first selection");
    let second: Selected = world.second.expect("no second selection");
    assert!(core::ptr::eq(first.sprite, second.sprite));
    assert_eq!(first.frame_index, second.frame_index);
}

#[then("the reported frame index equals the chosen sprite's own frame_at")]
fn reported_frame_is_delegated(world: &mut CreatureWorld) {
    let selected: Selected = world.first.expect("no selection recorded");
    assert_eq!(
        selected.frame_index,
        selected.sprite.frame_index_at(world.elapsed_ms)
    );
}

#[tokio::main]
async fn main() {
    CreatureWorld::run("tests/features").await;
}
