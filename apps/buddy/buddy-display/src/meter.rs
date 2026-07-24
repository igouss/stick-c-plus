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
use embedded_graphics::primitives::Rectangle;
use platform_display::RenderError;

use crate::backdrop;
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

/// Draw a meter of `cells` filling the `height`-tall band whose top-left is `origin`, the first
/// `lit` of them in `colour`.
///
/// The cells are centred in the band and **the whole band is painted**: the air above and below
/// the cells, and the gaps between them, are background. That is what lets a caller treat a meter
/// as one opaque rectangle — it erases what it replaces without a clear beforehand, and so never
/// paints a pixel twice in a frame. See [`backdrop`](crate::backdrop) for why that matters.
///
/// `lit` beyond `cells` is clamped rather than refused: a tier is a domain number and a meter is
/// a picture of it, and a picture that panicked because the domain grew a sixth tier would take
/// the glass down over a cosmetic disagreement.
pub fn meter<D>(
    target: &mut D,
    origin: Point,
    height: u32,
    cells: usize,
    lit: usize,
    colour: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let lit: usize = lit.min(cells);
    // Cells on the band's midline: a meter sitting on the text's cap height rather than its
    // middle is what makes a label and its meter read as two separate things.
    let drop: i32 = (height.saturating_sub(CELL) / 2) as i32;
    let span: u32 = width(cells);

    // The background above and below the cell row, painted as the complement of the row itself
    // rather than as two hand-computed strips — the same discipline, and the same arithmetic,
    // every other renderer on this glass uses.
    backdrop::behind(
        target,
        Rectangle::new(origin, Size::new(span, height)),
        [Rectangle::new(
            origin + Point::new(0, drop),
            Size::new(span, CELL),
        )],
        palette::BACKGROUND,
    )?;

    (0..cells).try_for_each(|index: usize| {
        let at: Point = origin + Point::new(((CELL + GAP) * index as u32) as i32, drop);
        let colour: Rgb565 = if index < lit {
            colour
        } else {
            palette::METER_DARK
        };
        backdrop::fill(target, Rectangle::new(at, Size::new(CELL, CELL)), colour)?;
        // The gap after this cell, so the band has no unpainted column in it. The last cell has
        // no gap after it — `width` measures to the last cell's right edge, and painting one
        // there would reach past the band the caller reserved.
        let gap: u32 = if index + 1 < cells { GAP } else { 0 };
        backdrop::fill(
            target,
            Rectangle::new(at + Point::new(CELL as i32, 0), Size::new(gap, CELL)),
            palette::BACKGROUND,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_display::testing::Framebuffer;

    const ORIGIN: Point = Point::new(0, 0);
    const CELLS: usize = 5;
    /// The band a gauge gives a meter — one text row of the board's font.
    const BAND: u32 = 20;

    fn painted(cells: usize, lit: usize) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        meter(&mut fb, ORIGIN, BAND, cells, lit, palette::METER_LIT)
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

    /// A meter owns its whole band and paints every pixel of it exactly once — which is what
    /// lets a gauge treat it as an opaque rectangle and paint no background underneath it.
    /// Without that, the row is cleared and then drawn, and the reading blinks on every repaint.
    #[test]
    fn a_meter_paints_its_whole_band_once() {
        let mut fb: Framebuffer = Framebuffer::new();
        meter(&mut fb, ORIGIN, BAND, CELLS, 2, palette::METER_LIT)
            .expect("a framebuffer render cannot fail");

        assert_eq!(fb.overpainted(), 0, "a pixel was painted twice");
        assert_eq!(
            fb.painted(),
            (width(CELLS) * BAND) as usize,
            "the meter left a pixel of its own band unpainted, so something behind it shows"
        );
    }
}
