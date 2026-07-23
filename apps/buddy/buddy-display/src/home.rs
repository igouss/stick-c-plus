//! The home screen: the creature, its status, and the band underneath.
//!
//! The resting picture, and the one the buddy spends nearly all its life showing. It is three
//! things composed: the creature, the two status rows beside it, and the band — which is the
//! transcript HUD normally and the approval screen while a permission prompt is pending.
//!
//! The swap is *only* the band. The creature and the status stay exactly where they are, so the
//! screen does not rearrange itself at the moment the owner most needs to read it fast.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::RenderError;

use crate::layout::Layout;
use crate::view::{BuddyView, PromptView};
use crate::{approval, creature, hud, status};

/// Draw the home screen for `view`, with the creature's animation clock at `elapsed_ms`.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    view: &BuddyView,
    elapsed_ms: u64,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    creature::draw(
        target,
        view.species,
        view.persona,
        elapsed_ms,
        layout.creature_origin,
    )?;
    status::render(target, layout, view)?;
    let prompt: Option<&PromptView> = view.prompt.as_ref();
    match prompt {
        Some(pending) => approval::render(target, layout, pending),
        None => hud::render(target, layout, &view.transcript),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use crate::view::{Hint, Tool, Transcript};
    use buddy_core::{PersonaState, SpeciesIndex};
    use platform_display::testing::Framebuffer;

    fn busy() -> BuddyView {
        let mut view: BuddyView = BuddyView::resting(SpeciesIndex::new(0));
        view.persona = PersonaState::Busy;
        view.device.linked = true;
        view.sessions_running = 3;
        view.transcript = Transcript::oldest_first(&["read the bead", "wrote the crate"]);
        view
    }

    fn asking() -> BuddyView {
        let mut view: BuddyView = busy();
        view.persona = PersonaState::Attention;
        view.sessions_waiting = 1;
        view.prompt = Some(PromptView {
            tool: Tool::new("Bash"),
            hint: Hint::new("cargo test --workspace"),
            waiting_s: 3,
        });
        view
    }

    fn painted(view: &BuddyView, elapsed_ms: u64) -> Framebuffer {
        painted_on(&LANDSCAPE, view, elapsed_ms)
    }

    fn painted_on(layout: &Layout, view: &BuddyView, elapsed_ms: u64) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(layout.canvas);
        render(&mut fb, layout, view, elapsed_ms).expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: an unrendered canvas is blank, so every "paints pixels" below means something.
    #[test]
    fn an_unrendered_canvas_is_blank() {
        assert_eq!(Framebuffer::new().lit_pixels(), 0);
    }

    /// One: the home screen paints.
    #[test]
    fn the_home_screen_paints_pixels() {
        assert!(painted(&busy(), 0).lit_pixels() > 0);
    }

    /// THE HEADLINE SWAP: a pending prompt replaces the band, and nothing else — the picture
    /// changes, and it is not merely a different transcript.
    #[test]
    fn a_pending_prompt_replaces_the_band() {
        assert_ne!(painted(&busy(), 0).pixels(), painted(&asking(), 0).pixels());
    }

    /// The creature keeps its place across the swap: painting the approval screen over a home
    /// screen at the same instant leaves the creature's rows untouched, so the two renders agree
    /// above the band.
    #[test]
    fn the_creature_stays_put_when_the_band_changes() {
        // Only the band may differ: the mood and the session counts feed the status rows, which
        // sit above it, so they are held equal to the quiet view.
        let mut same_mood: BuddyView = asking();
        same_mood.persona = busy().persona;
        same_mood.sessions_waiting = busy().sessions_waiting;
        let quiet: Framebuffer = painted(&busy(), 0);
        let asking: Framebuffer = painted(&same_mood, 0);
        // `pixels` is row-major, so the first `band_top` rows are the first
        // `band_top * width` entries — everything above the band.
        let above: usize =
            LANDSCAPE.row_y(LANDSCAPE.band_first_row) as usize * LANDSCAPE.canvas.width as usize;
        assert_eq!(
            &quiet.pixels()[..above],
            &asking.pixels()[..above],
            "the picture above the band moved when only the band should have"
        );
    }

    /// The creature animates on the home screen: the same view, one frame-hold later, differs.
    #[test]
    fn the_creature_animates_at_home() {
        let hold: u64 = u64::from(
            creature::selected(busy().species, busy().persona, 0)
                .expect("index 0 is registered")
                .frame()
                .hold_ms(),
        );
        assert_ne!(
            painted(&busy(), 0).pixels(),
            painted(&busy(), hold).pixels()
        );
    }

    /// Nothing escapes either canvas, in either band state.
    #[test]
    fn nothing_escapes_either_canvas() {
        assert_eq!(painted(&busy(), 0).escaped(), 0);
        assert_eq!(painted(&asking(), 0).escaped(), 0);
        assert_eq!(painted_on(&PORTRAIT, &busy(), 0).escaped(), 0);
        assert_eq!(painted_on(&PORTRAIT, &asking(), 0).escaped(), 0);
    }

    /// Turning the board draws a different picture on a differently-shaped canvas, rather than
    /// the landscape one clipped.
    #[test]
    fn a_quarter_turn_paints_on_the_taller_canvas() {
        let flat: Framebuffer = painted_on(&LANDSCAPE, &busy(), 0);
        let turned: Framebuffer = painted_on(&PORTRAIT, &busy(), 0);
        assert_ne!(flat.size(), turned.size());
        assert!(turned.lit_pixels() > 0);
    }
}
