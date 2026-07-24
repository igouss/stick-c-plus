//! The background behind a picture — painted only where the picture itself is not.
//!
//! There is no framebuffer between this crate and the glass. Every write reaches the panel over
//! SPI and the eye sees it, so a renderer that clears a region and *then* draws into it shows
//! the owner the cleared state for the length of a transfer, every time it repaints. That is
//! what flicker is on this board: not a slow paint, but a paint that says two things.
//!
//! The rule this module exists to make cheap is therefore: **every pixel is painted once per
//! frame**. An opaque text field already covers its own rectangle, so a screen never has to
//! clear the rows it is about to write — only the pixels *between* and *around* them. [`behind`]
//! paints exactly those, given the rectangles the picture will cover.
//!
//! The bands must be disjoint and top to bottom, which is not a restriction in practice: every
//! screen on this board is rows on one grid, and a row is a band.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use platform_display::RenderError;

/// Fill `area` with `colour` everywhere the `covered` bands are not.
///
/// `covered` is what the caller is about to paint itself: each entry is a horizontal band, they
/// do not overlap, and they run top to bottom. Anything outside `area` is ignored, so a caller
/// may hand over a band that hangs past an edge.
///
/// An iterator rather than a slice, so a screen whose bands are a run of grid rows maps a range
/// straight in — nothing on this render path allocates, on a board with 300 KiB of SRAM.
///
/// The complement is drawn as at most `2 * covered + 1` rectangles: the strip above each band,
/// the strips either side of it, and the strip below the last one.
pub fn behind<D, B>(
    target: &mut D,
    area: Rectangle,
    covered: B,
    colour: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
    B: IntoIterator<Item = Rectangle>,
{
    let mut y: i32 = area.top_left.y;
    let bottom: i32 = area.top_left.y + area.size.height as i32;
    let left: i32 = area.top_left.x;
    let right: i32 = left + area.size.width as i32;

    for band in covered.into_iter() {
        let top: i32 = band.top_left.y.clamp(y, bottom);
        let band_bottom: i32 = (band.top_left.y + band.size.height as i32).clamp(top, bottom);
        let band_left: i32 = band.top_left.x.clamp(left, right);
        let band_right: i32 = (band.top_left.x + band.size.width as i32).clamp(band_left, right);

        strip(target, left, y, right - left, top - y, colour)?;
        strip(
            target,
            left,
            top,
            band_left - left,
            band_bottom - top,
            colour,
        )?;
        strip(
            target,
            band_right,
            top,
            right - band_right,
            band_bottom - top,
            colour,
        )?;
        y = band_bottom;
    }
    strip(target, left, y, right - left, bottom - y, colour)
}

/// One strip of background from signed extents, skipped entirely when it has no area.
///
/// Signed because the complement arithmetic above genuinely produces negative widths — a band
/// wider than the area it sits in leaves `right - band_right` below zero — and the guard here is
/// what stops one becoming an enormous `u32`. Callers holding a real rectangle want [`fill`].
fn strip<D>(
    target: &mut D,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    colour: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    if width <= 0 || height <= 0 {
        return Ok(());
    }
    fill(
        target,
        Rectangle::new(Point::new(x, y), Size::new(width as u32, height as u32)),
        colour,
    )
}

/// One filled rectangle, skipped when it has no area — the crate's single spelling of "paint
/// this region this colour", so a renderer needing one does not open-code the styled-draw and
/// the error mapping a fifth time.
pub(crate) fn fill<D>(
    target: &mut D,
    area: Rectangle,
    colour: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    if area.is_zero_sized() {
        return Ok(());
    }
    area.into_styled(PrimitiveStyle::with_fill(colour))
        .draw(target)
        .map_err(RenderError::Draw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette;
    use platform_display::testing::Framebuffer;

    /// The canvas these tests reason about — small enough to count pixels by hand.
    const AREA: Rectangle = Rectangle::new(Point::new(0, 0), Size::new(10, 10));

    fn painted(covered: &[Rectangle]) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(Size::new(10, 10));
        behind(&mut fb, AREA, covered.iter().copied(), palette::PRIMARY)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero bands: the whole area is background, painted once.
    #[test]
    fn nothing_covered_paints_the_whole_area_once() {
        let fb: Framebuffer = painted(&[]);
        assert_eq!(fb.lit_pixels(), 100);
        assert_eq!(fb.overpainted(), 0);
    }

    /// One band: everything but the band, and the band is left untouched for its owner to paint.
    #[test]
    fn one_band_is_left_for_its_own_renderer() {
        let band: Rectangle = Rectangle::new(Point::new(2, 3), Size::new(5, 2));
        let fb: Framebuffer = painted(&[band]);

        assert_eq!(fb.lit_pixels(), 100 - 10, "the band kept its ten pixels");
        assert_eq!(fb.pixel(2, 3), Rgb565::BLACK, "inside the band");
        assert_eq!(fb.pixel(1, 3), palette::PRIMARY, "left of the band");
        assert_eq!(fb.pixel(7, 4), palette::PRIMARY, "right of the band");
        assert_eq!(fb.pixel(2, 2), palette::PRIMARY, "above the band");
        assert_eq!(fb.pixel(2, 5), palette::PRIMARY, "below the band");
        assert_eq!(fb.overpainted(), 0);
    }

    /// Many: two bands with a gap between them, and the gap is filled — the shape every screen
    /// on this board has, because the row grid leaves a pixel of air between rows.
    #[test]
    fn two_bands_keep_their_pixels_and_the_gap_between_them_is_filled() {
        let bands: [Rectangle; 2] = [
            Rectangle::new(Point::new(0, 1), Size::new(10, 2)),
            Rectangle::new(Point::new(0, 4), Size::new(10, 2)),
        ];
        let fb: Framebuffer = painted(&bands);

        assert_eq!(fb.lit_pixels(), 100 - 40);
        assert_eq!(fb.pixel(5, 1), Rgb565::BLACK, "the first band");
        assert_eq!(fb.pixel(5, 3), palette::PRIMARY, "the gap between them");
        assert_eq!(fb.pixel(5, 4), Rgb565::BLACK, "the second band");
        assert_eq!(fb.overpainted(), 0);
    }

    /// A band hanging past the area is clipped to it rather than refused — and nothing escapes
    /// the canvas, which is the failure that would otherwise be invisible.
    #[test]
    fn a_band_reaching_past_the_area_is_clipped_to_it() {
        let over: Rectangle = Rectangle::new(Point::new(-4, 8), Size::new(20, 20));
        let fb: Framebuffer = painted(&[over]);

        assert_eq!(fb.escaped(), 0);
        assert_eq!(fb.lit_pixels(), 80, "the two rows the band covers are left");
        assert_eq!(fb.overpainted(), 0);
    }
}
