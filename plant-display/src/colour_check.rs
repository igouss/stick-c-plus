//! The colour self-test screen: three bands that make a channel swap falsifiable.
//!
//! Text on this panel had only ever been white on black, and neither white nor black
//! can reveal a red/blue swap: the two channels are symmetric in `0xFFFF` and
//! `0x0000`. So a wrong `ColorOrder` sat in the display adapter from the day it
//! landed and was never once observed — until the first genuinely coloured pixel
//! shipped, the red `FAULT` line, and came out blue.
//!
//! ## The limit of this screen, stated plainly
//!
//! Drawn into a **host** framebuffer, these bands prove only that this crate asks for
//! red where it means red. The channel swap lives *below* [`DrawTarget`] — in the
//! panel's MADCTL byte, which the driven adapter sets — so a host render is red no
//! matter what the glass would do. Only [`colour_bands`] drawn on the **real panel**,
//! through the production init path, can answer the colour-order question. See
//! `firmware/bins/plant-monitor/src/display_colour_check.rs`.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

use crate::error::RenderError;
use crate::layout::{text_overlay, TEXT_X};

/// The bands, top to bottom, each labelled with the colour it is *meant* to be.
const BANDS: [(Rgb565, &str); 3] = [
    (Rgb565::RED, "RED"),
    (Rgb565::GREEN, "GREEN"),
    (Rgb565::BLUE, "BLUE"),
];

/// Inset of a band's label from the band's own top edge.
const LABEL_INSET_Y: i32 = 8;

/// Paint three full-width bands — **red, green, blue**, top to bottom — each labelled
/// in white with the colour it is meant to be.
///
/// Read the result on the glass like this:
///
/// - **R / G / B in order** — the colour order is right.
/// - **bands read B / G / R** — red and blue are swapped: the `ColorOrder` in the
///   display adapter is wrong for this panel. Green is invariant under that swap,
///   which is what makes the diagnosis unambiguous.
/// - **green looks magenta** (and red cyan, blue yellow) — the inversion is wrong, not
///   the order. This should be impossible while white renders white, which is why the
///   labels are drawn in white.
pub fn colour_bands<D>(target: &mut D) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let size: Size = target.bounding_box().size;
    let band_height: u32 = size.height / 3;

    // `try_for_each` over the three bands: no index arithmetic outside the enumerate,
    // and the first draw error short-circuits rather than painting a half-screen.
    BANDS.into_iter().enumerate().try_for_each(
        |(index, (colour, label)): (usize, (Rgb565, &str))| {
            let top: i32 = (index as u32 * band_height) as i32;
            Rectangle::new(Point::new(0, top), Size::new(size.width, band_height))
                .into_styled(PrimitiveStyle::with_fill(colour))
                .draw(target)
                .map_err(RenderError::Draw)?;
            // A white label is unaffected by a red/blue swap, so it stays readable and
            // names what the band is *supposed* to be.
            text_overlay(
                target,
                Point::new(TEXT_X, top + LABEL_INSET_Y),
                Rgb565::WHITE,
                label,
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::SCREEN_SIZE;
    use crate::testing::Framebuffer;

    fn painted() -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        colour_bands(&mut fb).expect("a framebuffer render cannot fail");
        fb
    }

    /// The pixel at the vertical middle of band `index`, away from its label.
    fn band_colour(fb: &Framebuffer, index: u32) -> Rgb565 {
        let band_height: u32 = SCREEN_SIZE.height / 3;
        let y: u32 = index * band_height + band_height / 2;
        // Sample at the far right, well clear of the left-inset white label.
        let x: u32 = SCREEN_SIZE.width - 1;
        fb.pixels()[(y * SCREEN_SIZE.width + x) as usize]
    }

    /// The three bands are the three primaries, in order. This is what a correct panel
    /// must reproduce; if the glass disagrees with this, the panel's colour order is
    /// wrong, not this code.
    #[test]
    fn the_bands_are_red_green_and_blue_top_to_bottom() {
        let fb: Framebuffer = painted();
        assert_eq!(band_colour(&fb, 0), Rgb565::RED);
        assert_eq!(band_colour(&fb, 1), Rgb565::GREEN);
        assert_eq!(band_colour(&fb, 2), Rgb565::BLUE);
    }

    /// Red and blue must be distinguishable in the framebuffer, or the screen could
    /// not diagnose a swap even in principle.
    #[test]
    fn the_red_and_blue_bands_differ() {
        let fb: Framebuffer = painted();
        assert_ne!(band_colour(&fb, 0), band_colour(&fb, 2));
    }

    /// The labels are white, and white is invariant under a red/blue swap — that is
    /// the property that makes them readable on a miswired panel.
    #[test]
    fn white_is_invariant_under_a_red_blue_swap() {
        let white: Rgb565 = Rgb565::WHITE;
        let swapped: Rgb565 = Rgb565::new(white.b(), white.g(), white.r());
        assert_eq!(swapped, white);
    }

    /// Green is invariant under a red/blue swap, which is what makes a B/G/R reading
    /// an unambiguous diagnosis rather than a guess.
    #[test]
    fn green_is_invariant_under_a_red_blue_swap() {
        let green: Rgb565 = Rgb565::GREEN;
        let swapped: Rgb565 = Rgb565::new(green.b(), green.g(), green.r());
        assert_eq!(swapped, green);
    }

    /// Red is *not* invariant, so the top band is where a swap becomes visible.
    #[test]
    fn red_is_not_invariant_under_a_red_blue_swap() {
        let red: Rgb565 = Rgb565::RED;
        let swapped: Rgb565 = Rgb565::new(red.b(), red.g(), red.r());
        assert_ne!(swapped, red);
    }

    #[test]
    fn no_band_escapes_the_canvas() {
        assert_eq!(painted().escaped(), 0);
    }
}
