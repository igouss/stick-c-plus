//! A meter: `cells` boxes in a row, `lit` of them alight.
//!
//! One primitive for all three of the pet screen's readings — the mood hearts, the fed pips,
//! the energy bars — because they are the same picture in three colours, and three separate
//! drawing routines would drift into three slightly different pictures.
//!
//! ## The unlit cells are drawn, not left blank
//!
//! A meter's *length* carries as much as its fill: two lit cells out of five is a poor mood,
//! and two out of two is a perfect one. Drawing the dark cells is what tells those apart at a
//! glance. It is also what erases a longer previous reading in place, the same way an opaque
//! text field does — so a meter dropping from four to two needs no clear.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use platform_display::RenderError;

use crate::palette;

/// One cell's side, in pixels.
pub const CELL: u32 = 8;

/// The gap between two cells.
pub const GAP: u32 = 3;

/// How wide a meter of `cells` reaches, so a layout can place what follows it.
pub const fn width(cells: usize) -> u32 {
    match cells {
        0 => 0,
        n => (CELL + GAP) * n as u32 - GAP,
    }
}

/// Draw a meter of `cells` at `origin`, the first `lit` of them in `colour`.
///
/// `lit` beyond `cells` is clamped rather than refused: a tier is a domain number and a meter is
/// a picture of it, and a picture that panicked because the domain grew a sixth tier would take
/// the glass down over a cosmetic disagreement.
pub fn meter<D>(
    target: &mut D,
    origin: Point,
    cells: usize,
    lit: usize,
    colour: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let lit: usize = lit.min(cells);
    let size: Size = Size::new(CELL, CELL);
    (0..cells).try_for_each(|index: usize| {
        let at: Point = origin + Point::new(((CELL + GAP) * index as u32) as i32, 0);
        let fill: Rgb565 = if index < lit {
            colour
        } else {
            palette::METER_DARK
        };
        Rectangle::new(at, size)
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(target)
            .map_err(RenderError::Draw)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_display::testing::Framebuffer;

    const ORIGIN: Point = Point::new(0, 0);
    const CELLS: usize = 5;

    fn painted(cells: usize, lit: usize) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        meter(&mut fb, ORIGIN, cells, lit, palette::METER_LIT)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero cells paint nothing at all.
    #[test]
    fn a_meter_of_no_cells_paints_nothing() {
        assert_eq!(painted(0, 0).lit_pixels(), 0);
        assert_eq!(width(0), 0);
    }

    /// Zero LIT cells still paint the meter — the dark cells are what give it a length.
    #[test]
    fn an_empty_meter_still_shows_its_length() {
        assert!(painted(CELLS, 0).lit_pixels() > 0);
    }

    /// One, and many: the picture changes with the reading, so a meter actually reports.
    #[test]
    fn each_reading_paints_a_different_picture() {
        assert_ne!(painted(CELLS, 0).pixels(), painted(CELLS, 1).pixels());
        assert_ne!(painted(CELLS, 1).pixels(), painted(CELLS, 3).pixels());
        assert_ne!(painted(CELLS, 3).pixels(), painted(CELLS, CELLS).pixels());
    }

    /// A reading past the end is clamped to full rather than refused — a domain that grows a
    /// sixth tier must not take the glass down.
    #[test]
    fn a_reading_past_the_end_is_clamped_to_full() {
        assert_eq!(
            painted(CELLS, CELLS + 3).pixels(),
            painted(CELLS, CELLS).pixels()
        );
    }

    /// The declared width is where the meter actually ends, so a layout can place a label after
    /// it without measuring pixels of its own.
    #[test]
    fn the_declared_width_is_where_the_meter_ends() {
        assert_eq!(width(CELLS), (CELL + GAP) * CELLS as u32 - GAP);
        assert_eq!(width(1), CELL);
    }
}
