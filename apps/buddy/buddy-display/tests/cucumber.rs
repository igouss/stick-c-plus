//! Gherkin plumbing test for the buddy's glass.
//!
//! Drives [`buddy_display`] at its only boundary — `render` into a host framebuffer, and the
//! [`Animated`] contract the render loop reads — so what is proved here is what a caller can
//! actually observe: the compositing order, that a change reaches the picture, and that the
//! animation anchor moves on a persona change and not on a ticking counter.
//!
//! The exhaustive claims (every screen crossed with every overlay stays on the canvas, every
//! stat reaches the glass, no wrapped line is ever too wide) are pinned by the unit and property
//! tests beside each module: a handful of scenarios cannot stand for "for all". These prove the
//! plumbing made it through.

use buddy_core::{PersonaState, SpeciesIndex};
use buddy_display::{
    canvas_size, render, BuddyView, Hint, InfoPage, Overlay, PetPage, PromptView, Screen, Tool,
    Transcript,
};
use cucumber::{given, then, when, World};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_core::{Animated, ScreenRotation};
use platform_display::testing::Framebuffer;

/// One painting, kept as plain data so the world stays [`Debug`].
#[derive(Clone, PartialEq, Debug, Default)]
struct Painting {
    pixels: Vec<Rgb565>,
    size: Size,
    lit: usize,
    escaped: usize,
}

/// The view under test, the way up it is drawn, the two most recent paintings, and the
/// remembered animation anchor.
#[derive(Debug, World)]
struct GlassWorld {
    view: BuddyView,
    rotation: ScreenRotation,
    first: Option<Painting>,
    second: Option<Painting>,
    anchor: Option<(PersonaState, SpeciesIndex)>,
}

impl Default for GlassWorld {
    fn default() -> Self {
        GlassWorld {
            view: BuddyView::resting(SpeciesIndex::new(0)),
            rotation: ScreenRotation::Deg0,
            first: None,
            second: None,
            anchor: None,
        }
    }
}

impl GlassWorld {
    /// Paint the current view and return what landed on the glass.
    fn paint(&self) -> Painting {
        let mut fb: Framebuffer = Framebuffer::sized(canvas_size(self.rotation));
        render(&mut fb, &self.view, 0, self.rotation).expect("a framebuffer render cannot fail");
        Painting {
            pixels: fb.pixels().to_vec(),
            size: fb.size(),
            lit: fb.lit_pixels(),
            escaped: fb.escaped(),
        }
    }

    /// The most recent painting — every `Then` reads this one.
    fn latest(&self) -> &Painting {
        self.second
            .as_ref()
            .or(self.first.as_ref())
            .expect("the glass was never painted")
    }

    /// The pending prompt, which several steps amend in place.
    fn prompt(&mut self) -> &mut PromptView {
        self.view
            .prompt
            .as_mut()
            .expect("no prompt is pending in this scenario")
    }
}

#[given("a busy buddy at home")]
fn a_busy_buddy_at_home(world: &mut GlassWorld) {
    world.view = BuddyView::resting(SpeciesIndex::new(0));
    world.view.persona = PersonaState::Busy;
    world.view.device.linked = true;
    world.view.sessions_running = 3;
    world.view.transcript = Transcript::oldest_first(&["read the bead", "wrote the crate"]);
}

#[given(regex = r"^the screen is (\w+)$")]
#[when(regex = r"^the screen is (\w+)$")]
fn the_screen_is(world: &mut GlassWorld, name: String) {
    world.view.screen = match name.as_str() {
        "home" => Screen::Home,
        "pet" => Screen::Pet(PetPage::Stats),
        "info" => Screen::Info(InfoPage::About),
        "clock" => Screen::Clock,
        other => panic!("unknown screen {other:?}"),
    };
}

#[given(regex = r"^the passkey (\d+) is active$")]
#[when(regex = r"^the passkey (\d+) is active$")]
fn the_passkey_is_active(world: &mut GlassWorld, passkey: u32) {
    world.view.passkey = Some(passkey);
}

#[given("the overlay is reset")]
#[when("the overlay is reset")]
fn the_overlay_is_reset(world: &mut GlassWorld) {
    world.view.overlay = Overlay::Reset;
}

#[given("the board is turned a quarter")]
#[when("the board is turned a quarter")]
fn the_board_is_turned(world: &mut GlassWorld) {
    world.rotation = ScreenRotation::Deg90;
}

#[given(regex = r"^a prompt for (\w+) arrives$")]
#[when(regex = r"^a prompt for (\w+) arrives$")]
fn a_prompt_arrives(world: &mut GlassWorld, tool: String) {
    world.view.prompt = Some(PromptView {
        tool: Tool::new(&tool),
        hint: Hint::new("cargo test --workspace"),
        waiting_s: 1,
    });
    world.view.persona = PersonaState::Attention;
    world.view.sessions_waiting = 1;
}

#[given(regex = r"^the prompt has waited (\d+) seconds$")]
#[when(regex = r"^the prompt has waited (\d+) seconds$")]
fn the_prompt_has_waited(world: &mut GlassWorld, seconds: u32) {
    world.prompt().waiting_s = seconds;
}

#[given(regex = r"^the persona becomes (\w+)$")]
#[when(regex = r"^the persona becomes (\w+)$")]
fn the_persona_becomes(world: &mut GlassWorld, name: String) {
    world.view.persona = match name.as_str() {
        "Sleep" => PersonaState::Sleep,
        "Idle" => PersonaState::Idle,
        "Busy" => PersonaState::Busy,
        "Attention" => PersonaState::Attention,
        "Celebrate" => PersonaState::Celebrate,
        "Dizzy" => PersonaState::Dizzy,
        "Heart" => PersonaState::Heart,
        other => panic!("unknown persona {other:?}"),
    };
}

#[given("the transcript is empty")]
#[when("the transcript is empty")]
fn the_transcript_is_empty(world: &mut GlassWorld) {
    world.view.transcript = Transcript::oldest_first(&[]);
}

#[given(regex = r#"^the transcript holds "([^"]+)"$"#)]
#[when(regex = r#"^the transcript holds "([^"]+)"$"#)]
fn the_transcript_holds_one(world: &mut GlassWorld, entry: String) {
    world.view.transcript = Transcript::oldest_first(&[&entry]);
}

#[given(regex = r#"^the transcript holds "([^"]+)" and "([^"]+)"$"#)]
#[when(regex = r#"^the transcript holds "([^"]+)" and "([^"]+)"$"#)]
fn the_transcript_holds_two(world: &mut GlassWorld, older: String, newer: String) {
    world.view.transcript = Transcript::oldest_first(&[&older, &newer]);
}

#[given("the transcript holds a very long entry")]
#[when("the transcript holds a very long entry")]
fn the_transcript_holds_a_long_entry(world: &mut GlassWorld) {
    world.view.transcript = Transcript::oldest_first(&[
        "Bash: cargo test --workspace --all-features and then a good deal more besides",
    ]);
}

/// Nine older entries behind the two already on the glass: the visible text is unchanged and the
/// scroll indicator is not, which is the whole point of the indicator.
#[when("the transcript holds nine more entries behind them")]
fn the_transcript_holds_nine_more(world: &mut GlassWorld) {
    world.view.transcript = Transcript::oldest_first(&[
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "older", "newer",
    ]);
}

#[when("the glass is painted")]
fn the_glass_is_painted(world: &mut GlassWorld) {
    world.first = Some(world.paint());
}

#[when("the glass is painted again")]
fn the_glass_is_painted_again(world: &mut GlassWorld) {
    world.second = Some(world.paint());
}

#[when("the animation anchor is remembered")]
fn the_anchor_is_remembered(world: &mut GlassWorld) {
    world.anchor = Some(world.view.anchor());
}

#[then("the glass is not blank")]
fn the_glass_is_not_blank(world: &mut GlassWorld) {
    assert!(world.latest().lit > 0, "nothing was painted");
}

#[then("nothing escapes the canvas")]
fn nothing_escapes(world: &mut GlassWorld) {
    assert_eq!(world.latest().escaped, 0, "pixels were drawn off the glass");
}

#[then("the two paintings differ")]
fn the_two_paintings_differ(world: &mut GlassWorld) {
    let (first, second): (&Painting, &Painting) = both(world);
    assert_ne!(first.pixels, second.pixels, "the picture did not change");
}

#[then("both paintings are identical")]
fn both_paintings_are_identical(world: &mut GlassWorld) {
    let (first, second): (&Painting, &Painting) = both(world);
    assert_eq!(first.pixels, second.pixels, "the picture changed");
}

#[then("the two paintings have different shapes")]
fn the_two_paintings_have_different_shapes(world: &mut GlassWorld) {
    let (first, second): (&Painting, &Painting) = both(world);
    assert_ne!(first.size, second.size, "the canvas shape did not follow");
}

#[then("the animation anchor is unchanged")]
fn the_anchor_is_unchanged(world: &mut GlassWorld) {
    assert_eq!(
        world.anchor.expect("no anchor was remembered"),
        world.view.anchor(),
        "the creature's animation restarted"
    );
}

#[then("the animation anchor has changed")]
fn the_anchor_has_changed(world: &mut GlassWorld) {
    assert_ne!(
        world.anchor.expect("no anchor was remembered"),
        world.view.anchor(),
        "the creature's animation did not restart"
    );
}

/// The two paintings a comparison step needs, named once.
fn both(world: &GlassWorld) -> (&Painting, &Painting) {
    (
        world.first.as_ref().expect("the glass was painted once"),
        world.second.as_ref().expect("the glass was painted twice"),
    )
}

#[tokio::main]
async fn main() {
    GlassWorld::run("tests/features").await;
}
