//! [`Masked`] — a [`DrawTarget`] that refuses the pixels another layer has claimed.
//!
//! # Why this exists
//!
//! There is no framebuffer between this crate and the glass: every write reaches the panel over
//! SPI and the eye sees it. So when a compositor draws a screen and then draws a panel *over*
//! part of it, the owner sees the covered region twice — once as the screen, once as the panel —
//! on **every** repaint. On a screen that repaints on an animation clock that is not a one-off
//! transition, it is a region flashing twenty times a second for as long as the overlay is open.
//!
//! The rule that makes flicker impossible is *every pixel is painted at most once per frame*, and
//! a layered picture can only keep it if the layer underneath is told what the layer above has
//! claimed. That is this target: the screen is drawn through it, the claimed rectangle is
//! dropped, and the overlay then paints those pixels for the first and only time.
//!
//! # Why a target rather than a flag on each renderer
//!
//! A screen would otherwise have to know it was being overlaid, and every screen would have to
//! know it separately — five renderers each carrying a clipping rule that the compositor is the
//! only party that actually knows. Masking the target instead leaves every renderer exactly as
//! it is, and composes: what is drawn is unchanged, only where it may land.
//!
//! # The contract
//!
//! Writes inside `claimed` are dropped; writes outside pass through untouched. The reported size
//! is the inner target's, because the picture's geometry has not changed — only its visibility.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

/// A draw target that drops everything landing inside a rectangle another layer will paint.
///
/// See the module docs. Borrows the inner target for the duration of the drawing.
pub struct Masked<'a, D> {
    inner: &'a mut D,
    claimed: Rectangle,
}

impl<'a, D> Masked<'a, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    /// Draw into `inner`, dropping every pixel that falls inside `claimed`.
    pub fn new(inner: &'a mut D, claimed: Rectangle) -> Self {
        Masked { inner, claimed }
    }
}

impl<D> Dimensions for Masked<'_, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    /// The inner target's box: masking hides pixels, it does not shrink the canvas, and a
    /// renderer laying out against the bounding box must still lay out against the real one.
    fn bounding_box(&self) -> Rectangle {
        self.inner.bounding_box()
    }
}

impl<D> DrawTarget for Masked<'_, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    type Color = Rgb565;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        // Filtered rather than clipped: the claimed rectangle is in the middle of the canvas, so
        // there is no sub-rectangle to hand down — only a predicate, per pixel.
        let claimed: Rectangle = self.claimed;
        self.inner.draw_iter(
            pixels
                .into_iter()
                .filter(|Pixel(at, _): &Pixel<Rgb565>| !claimed.contains(*at)),
        )
    }

    /// A run of colours, kept as one transfer whenever the claim does not cut into it.
    ///
    /// The creature is drawn this way — one call of several thousand pixels — so the difference
    /// between forwarding it and filtering it per pixel is the difference between one address
    /// window and dozens, on the screen that repaints most often. The straddling case still has
    /// to go pixel by pixel, because the surviving region is not a rectangle and the colours
    /// arrive in one row-major run that cannot be re-cut without buffering it, which this board
    /// has no memory to do.
    fn fill_contiguous<I>(&mut self, area: &Rectangle, colours: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let hidden: Rectangle = area.intersection(&self.claimed);
        if hidden.is_zero_sized() {
            return self.inner.fill_contiguous(area, colours);
        }
        if hidden.size == area.size {
            // Nothing survives, so the colours are never consumed — the sprite behind them is
            // not decoded at all, which is the larger half of the saving.
            return Ok(());
        }
        let claimed: Rectangle = self.claimed;
        self.inner.draw_iter(
            area.points()
                .zip(colours)
                .filter(|(at, _): &(Point, Rgb565)| !claimed.contains(*at))
                .map(|(at, colour): (Point, Rgb565)| Pixel(at, colour)),
        )
    }

    /// A solid fill, kept whole wherever the claim does not cut it.
    ///
    /// Overridden because the default would not: [`DrawTarget::fill_solid`] falls through to
    /// [`DrawTarget::fill_contiguous`] and then to [`draw_iter`](DrawTarget::draw_iter), which
    /// turns one rectangle into a stream of individual pixels. On this panel a rectangle costs an
    /// *address window* — one CASET/RASET/RAMWR whatever its area — while a pixel stream is
    /// re-batched a hundred pixels at a time. A full-width backdrop strip is one window drawn
    /// directly and over a hundred drawn through an unaugmented mask, on a screen that repaints
    /// at the creature's cadence for as long as an overlay is open.
    ///
    /// So the rectangle is kept as a rectangle: forwarded whole when the claim misses it,
    /// dropped without drawing a pixel when the claim swallows it, and otherwise cut into the
    /// (at most four) pieces that survive — each still one fill.
    fn fill_solid(&mut self, area: &Rectangle, colour: Self::Color) -> Result<(), Self::Error> {
        let hidden: Rectangle = area.intersection(&self.claimed);
        if hidden.is_zero_sized() {
            return self.inner.fill_solid(area, colour);
        }
        if hidden.size == area.size {
            return Ok(());
        }
        for piece in surviving(area, &hidden) {
            if !piece.is_zero_sized() {
                self.inner.fill_solid(&piece, colour)?;
            }
        }
        Ok(())
    }
}

/// The pieces of `area` left once `hidden` is cut out of it: the strip above, the strip below,
/// and the strips either side of what remains between them.
///
/// `hidden` must already be the intersection of the two, so it is inside `area` on every axis and
/// the arithmetic below cannot go negative. Four pieces rather than a general polygon because
/// that is all a rectangle minus a rectangle can be, and each one stays a single fill.
fn surviving(area: &Rectangle, hidden: &Rectangle) -> [Rectangle; 4] {
    let left: i32 = area.top_left.x;
    let top: i32 = area.top_left.y;
    let right: i32 = left + area.size.width as i32;
    let bottom: i32 = top + area.size.height as i32;

    let cut_left: i32 = hidden.top_left.x;
    let cut_top: i32 = hidden.top_left.y;
    let cut_right: i32 = cut_left + hidden.size.width as i32;
    let cut_bottom: i32 = cut_top + hidden.size.height as i32;

    let band: u32 = (cut_bottom - cut_top) as u32;
    [
        // Above the cut, full width.
        Rectangle::new(
            Point::new(left, top),
            Size::new(area.size.width, (cut_top - top) as u32),
        ),
        // Below it, full width.
        Rectangle::new(
            Point::new(left, cut_bottom),
            Size::new(area.size.width, (bottom - cut_bottom) as u32),
        ),
        // Beside it, only as tall as the cut itself.
        Rectangle::new(
            Point::new(left, cut_top),
            Size::new((cut_left - left) as u32, band),
        ),
        Rectangle::new(
            Point::new(cut_right, cut_top),
            Size::new((right - cut_right) as u32, band),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Framebuffer;

    /// The rectangle the layer above claims, in these tests.
    const CLAIMED: Rectangle = Rectangle::new(Point::new(4, 4), Size::new(4, 4));

    fn drawn(points: &[Point]) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(Size::new(16, 16));
        {
            let mut masked: Masked<'_, Framebuffer> = Masked::new(&mut fb, CLAIMED);
            masked
                .draw_iter(points.iter().map(|at: &Point| Pixel(*at, Rgb565::WHITE)))
                .expect("memory writes cannot fail");
        }
        fb
    }

    /// Zero: a pixel inside the claim never reaches the glass.
    #[test]
    fn a_pixel_the_layer_above_claimed_is_dropped() {
        assert_eq!(drawn(&[Point::new(5, 5)]).lit_pixels(), 0);
    }

    /// One: a pixel outside it passes through untouched.
    #[test]
    fn a_pixel_outside_the_claim_passes_through() {
        let fb: Framebuffer = drawn(&[Point::new(1, 1)]);
        assert_eq!(fb.lit_pixels(), 1);
        assert_eq!(fb.pixel(1, 1), Rgb565::WHITE);
    }

    /// Many, and the edges: the claim includes its top-left corner and excludes the pixel past
    /// its bottom-right — the off-by-one that would leave a one-pixel line of the screen
    /// underneath showing through the overlay's border.
    #[test]
    fn the_claim_holds_its_own_corners_and_nothing_beyond_them() {
        let fb: Framebuffer = drawn(&[
            Point::new(4, 4),
            Point::new(7, 7),
            Point::new(3, 4),
            Point::new(8, 7),
        ]);
        assert_eq!(fb.lit_pixels(), 2);
        assert_eq!(fb.pixel(3, 4), Rgb565::WHITE);
        assert_eq!(fb.pixel(8, 7), Rgb565::WHITE);
    }

    /// Fill a rectangle through the mask, and separately pixel by pixel, and compare. The whole
    /// point of the `fill_solid` override is that it changes only the number of transfers, never
    /// the picture — so the two must be indistinguishable.
    fn filled(area: Rectangle) -> (Framebuffer, Framebuffer) {
        let mut whole: Framebuffer = Framebuffer::sized(Size::new(16, 16));
        {
            let mut masked: Masked<'_, Framebuffer> = Masked::new(&mut whole, CLAIMED);
            masked
                .fill_solid(&area, Rgb565::WHITE)
                .expect("memory writes cannot fail");
        }
        let mut per_pixel: Framebuffer = Framebuffer::sized(Size::new(16, 16));
        {
            let mut masked: Masked<'_, Framebuffer> = Masked::new(&mut per_pixel, CLAIMED);
            masked
                .draw_iter(area.points().map(|at: Point| Pixel(at, Rgb565::WHITE)))
                .expect("memory writes cannot fail");
        }
        (whole, per_pixel)
    }

    /// Zero: a fill the claim swallows whole paints nothing — and, unlike the default path, does
    /// not rasterize the pixels first only to drop every one of them.
    #[test]
    fn a_fill_inside_the_claim_paints_nothing() {
        let (whole, per_pixel) = filled(Rectangle::new(Point::new(5, 5), Size::new(2, 2)));
        assert_eq!(whole.lit_pixels(), 0);
        assert_eq!(whole.pixels(), per_pixel.pixels());
    }

    /// One: a fill the claim misses is forwarded intact.
    #[test]
    fn a_fill_clear_of_the_claim_reaches_the_glass_whole() {
        let (whole, per_pixel) = filled(Rectangle::new(Point::new(10, 10), Size::new(3, 3)));
        assert_eq!(whole.lit_pixels(), 9);
        assert_eq!(whole.pixels(), per_pixel.pixels());
    }

    /// Many: a fill straddling the claim is cut into pieces, and the surviving picture is exactly
    /// the one the per-pixel path would have produced — including the pixels beside the claim,
    /// which the four-way split is the only part of the arithmetic that can get wrong.
    #[test]
    fn a_fill_straddling_the_claim_paints_its_complement_exactly() {
        let (whole, per_pixel) = filled(Rectangle::new(Point::new(2, 2), Size::new(9, 9)));
        assert_eq!(whole.pixels(), per_pixel.pixels());
        assert_eq!(
            whole.lit_pixels(),
            9 * 9 - 4 * 4,
            "the claim kept its own pixels"
        );
        assert_eq!(whole.pixel(3, 5), Rgb565::WHITE, "left of the claim");
        assert_eq!(whole.pixel(9, 5), Rgb565::WHITE, "right of it");
        assert_eq!(whole.pixel(5, 3), Rgb565::WHITE, "above it");
        assert_eq!(whole.pixel(5, 9), Rgb565::WHITE, "below it");
        assert_eq!(whole.pixel(5, 5), Rgb565::BLACK, "inside it");
    }

    /// A fill covering the whole canvas — the `clear()` shape — still leaves the claim untouched.
    #[test]
    fn a_full_canvas_fill_still_spares_the_claim() {
        let (whole, per_pixel) = filled(Rectangle::new(Point::zero(), Size::new(16, 16)));
        assert_eq!(whole.pixels(), per_pixel.pixels());
        assert_eq!(whole.lit_pixels(), 16 * 16 - 4 * 4);
    }

    /// A contiguous run cut by the claim keeps every surviving colour on the pixel it belonged
    /// to. The hazard is the ordering inside the override: the colours arrive as one row-major
    /// run with no positions of their own, so they must be zipped to the area's points *before*
    /// anything is dropped. Filtering first would slide every colour after the claim onto the
    /// wrong pixel — a sheared sprite, which no count of lit pixels would notice.
    #[test]
    fn a_contiguous_run_cut_by_the_claim_keeps_its_colours_in_place() {
        let area: Rectangle = Rectangle::new(Point::new(2, 2), Size::new(9, 9));
        // A distinct colour per position, so a shear cannot coincidentally still match.
        let shade = |index: usize| Rgb565::new((index % 32) as u8, (index % 64) as u8, 1);
        let colours: Vec<Rgb565> = (0..area.size.width as usize * area.size.height as usize)
            .map(shade)
            .collect();

        let mut whole: Framebuffer = Framebuffer::sized(Size::new(16, 16));
        {
            let mut masked: Masked<'_, Framebuffer> = Masked::new(&mut whole, CLAIMED);
            masked
                .fill_contiguous(&area, colours.iter().copied())
                .expect("memory writes cannot fail");
        }

        let mut per_pixel: Framebuffer = Framebuffer::sized(Size::new(16, 16));
        {
            let mut masked: Masked<'_, Framebuffer> = Masked::new(&mut per_pixel, CLAIMED);
            masked
                .draw_iter(
                    area.points()
                        .zip(colours.iter().copied())
                        .map(|(at, colour): (Point, Rgb565)| Pixel(at, colour)),
                )
                .expect("memory writes cannot fail");
        }

        assert_eq!(whole.pixels(), per_pixel.pixels());
        // And spot-check a pixel PAST the claim, which is where a shear would first show.
        assert_eq!(whole.pixel(9, 9), shade(7 * 9 + 7));
    }

    /// The canvas a renderer lays out against is the real one — masking hides pixels, it does not
    /// move the edges the layout arithmetic is checked against.
    #[test]
    fn the_masked_target_reports_the_canvas_it_actually_draws_on() {
        let mut fb: Framebuffer = Framebuffer::sized(Size::new(16, 16));
        let masked: Masked<'_, Framebuffer> = Masked::new(&mut fb, CLAIMED);
        assert_eq!(masked.bounding_box().size, Size::new(16, 16));
    }
}
