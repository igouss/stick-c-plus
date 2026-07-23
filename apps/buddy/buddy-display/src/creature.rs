//! Drawing the creature — and what to draw when there is no creature to draw.
//!
//! The `(species, state) -> art` binding lives in [`buddy_creature`], not here: this crate owns
//! the SCREENS and the compositing, and asks the creature crate what to composite. All this
//! module adds is the two things the crate next door deliberately does not have — a
//! [`DrawTarget`] and a fallback.
//!
//! ## The fallback is not an error
//!
//! [`buddy_creature::resolve`] is total and answers `None` for the GIF sentinel and for any
//! index past the registry — a persisted NVS value written by a future firmware with more
//! species, read back by this one. Neither is a fault: the buddy is still running, the glass
//! still has to say something. So a missing species draws a plain outlined box where the
//! creature would be, which reads as "no art for this pet" rather than as a blank panel or a
//! panic on the render path.

use buddy_core::{PersonaState, SpeciesIndex};
use buddy_creature::{Selected, Species};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use platform_display::{sprite, RenderError};

use crate::layout::{SPRITE_EXTENT, SPRITE_SCALE};
use crate::palette;

/// What the creature crate chose for this `(species, state, elapsed_ms)`, or `None` when the
/// species has no built-in art.
pub fn selected(species: SpeciesIndex, state: PersonaState, elapsed_ms: u64) -> Option<Selected> {
    buddy_creature::resolve(species)
        .map(|species: &'static Species| buddy_creature::current(species, state, elapsed_ms))
}

/// Paint the creature for `(species, state)` at `origin`, or the no-art placeholder.
///
/// Drawn **opaque** (see [`sprite::draw_onto`]): each frame overwrites its own box against the
/// background, so an animating creature never smears the frame before it.
pub fn draw<D>(
    target: &mut D,
    species: SpeciesIndex,
    state: PersonaState,
    elapsed_ms: u64,
    origin: Point,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match selected(species, state, elapsed_ms) {
        Some(selected) => sprite::draw_onto(
            target,
            selected.sprite,
            selected.frame(),
            origin,
            SPRITE_SCALE,
            palette::BACKGROUND,
        ),
        None => placeholder(target, origin),
    }
}

/// The box drawn where a creature with no built-in art would stand.
fn placeholder<D>(target: &mut D, origin: Point) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let box_size: Size = Size::new(SPRITE_EXTENT, SPRITE_EXTENT);
    // Fill first, then outline: the fill is what erases whatever frame stood here before, which
    // is the same job the opaque sprite draw does on the branch above.
    Rectangle::new(origin, box_size)
        .into_styled(PrimitiveStyle::with_fill(palette::BACKGROUND))
        .draw(target)
        .map_err(RenderError::Draw)?;
    Rectangle::new(origin, box_size)
        .into_styled(PrimitiveStyle::with_stroke(palette::DIM, 2))
        .draw(target)
        .map_err(RenderError::Draw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_core::GIF_SENTINEL;
    use platform_display::testing::Framebuffer;

    const CLAUDEPIX: SpeciesIndex = SpeciesIndex::new(0);
    const NO_ART: SpeciesIndex = SpeciesIndex::new(GIF_SENTINEL);
    const ORIGIN: Point = Point::new(4, 4);

    fn painted(species: SpeciesIndex, elapsed_ms: u64) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        draw(&mut fb, species, PersonaState::Busy, elapsed_ms, ORIGIN)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// One: the registered species draws its art.
    #[test]
    fn a_registered_species_paints_its_creature() {
        assert!(painted(CLAUDEPIX, 0).lit_pixels() > 0);
    }

    /// The creature animates: the same state, one frame-hold later, is a different picture.
    #[test]
    fn the_creature_animates_as_the_clock_advances() {
        let start: Selected =
            selected(CLAUDEPIX, PersonaState::Busy, 0).expect("index 0 is registered");
        let hold: u64 = u64::from(start.frame().hold_ms());
        assert_ne!(
            painted(CLAUDEPIX, 0).pixels(),
            painted(CLAUDEPIX, hold).pixels()
        );
    }

    /// Zero art: the GIF sentinel resolves to nothing, and draws the placeholder instead of
    /// panicking or leaving the panel blank.
    #[test]
    fn a_species_with_no_built_in_art_draws_a_placeholder() {
        assert!(selected(NO_ART, PersonaState::Busy, 0).is_none());
        assert!(painted(NO_ART, 0).lit_pixels() > 0);
    }

    /// The placeholder is not the creature — a fallback that happened to look like the art
    /// would hide the fact that the species was never resolved.
    #[test]
    fn the_placeholder_is_not_the_creature() {
        assert_ne!(painted(NO_ART, 0).pixels(), painted(CLAUDEPIX, 0).pixels());
    }

    /// Nothing escapes the canvas, on either branch.
    #[test]
    fn neither_the_creature_nor_the_placeholder_escapes_the_canvas() {
        assert_eq!(painted(CLAUDEPIX, 0).escaped(), 0);
        assert_eq!(painted(NO_ART, 0).escaped(), 0);
    }
}
