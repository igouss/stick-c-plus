//! [`Magnified`] — a [`DrawTarget`] that draws everything `scale`× larger.
//!
//! # Why this exists
//!
//! The board carries exactly one font, `FONT_10X20`, and it is the right size for a row of
//! labels and readings. It is the wrong size for the rare value that is the *entire reason the
//! screen is up*: a pairing passkey is six digits, read across a desk, typed on another machine,
//! inside a thirty-second window that BlueZ closes without asking. At 10×20 those digits occupy
//! a seventh of the panel's height, and a device that shows a secret nobody can read has not
//! shown it. (Reported from the glass: "the text was a little difficult for me to read".)
//!
//! # Why a target rather than a bigger font
//!
//! Embedding a second, larger font would cost flash for one screen, and would still be a fixed
//! size chosen in advance. Scaling the *target* instead leaves every existing drawing primitive
//! untouched and works for all of them: [`text_field`](crate::text_field) draws into this the
//! same way it draws into a panel, so the field padding that erases a stale value, the centring,
//! and the overflow check all keep working, magnified. Nothing in `text.rs` had to learn about
//! scale, and any future screen that needs a large anything gets it for free.
//!
//! # The contract
//!
//! One source pixel becomes a `scale`×`scale` block, and the whole picture is then offset by
//! `origin` in *target* space. A `scale` of 0 or 1 is not a special case in the arithmetic —
//! 1 is the identity, and 0 draws nothing, which is the same thing the sprite renderer does
//! with a zero scale.
//!
//! Bounds are the caller's business, exactly as they are when drawing to a panel directly: this
//! reports its own size as the inner target's, divided down, so a caller that lays out inside
//! [`DrawTarget::bounding_box`] stays inside the glass.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

/// A draw target that magnifies everything drawn into it by an integer factor.
///
/// See the module docs. Borrows the inner target for the duration of the drawing.
pub struct Magnified<'a, D> {
    inner: &'a mut D,
    scale: u32,
    origin: Point,
}

impl<'a, D> Magnified<'a, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    /// Magnify `inner` by `scale`, placing the magnified picture's top-left at `origin`.
    pub fn new(inner: &'a mut D, scale: u32, origin: Point) -> Self {
        Magnified {
            inner,
            scale,
            origin,
        }
    }

    /// The size a picture may be, in *source* pixels, to fit the inner target from `origin`.
    ///
    /// The reason this type reports a divided-down `bounding_box`: a caller doing its own
    /// layout asks in the same units it draws in.
    fn source_size(&self) -> Size {
        if self.scale == 0 {
            return Size::zero();
        }
        let inner: Size = self.inner.bounding_box().size;
        let across: u32 = inner.width.saturating_sub(self.origin.x.max(0) as u32) / self.scale;
        let down: u32 = inner.height.saturating_sub(self.origin.y.max(0) as u32) / self.scale;
        Size::new(across, down)
    }
}

impl<D> Dimensions for Magnified<'_, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), self.source_size())
    }
}

impl<D> DrawTarget for Magnified<'_, D>
where
    D: DrawTarget<Color = Rgb565>,
{
    type Color = Rgb565;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        if self.scale == 0 {
            return Ok(());
        }
        // One filled rectangle per source pixel. `fill_solid` rather than a pixel loop so an
        // adapter with a windowed write (which the ST7789 has) issues one transaction per block
        // instead of scale² of them.
        for Pixel(point, colour) in pixels {
            let block: Rectangle = Rectangle::new(
                Point::new(
                    self.origin.x + point.x * self.scale as i32,
                    self.origin.y + point.y * self.scale as i32,
                ),
                Size::new(self.scale, self.scale),
            );
            self.inner.fill_solid(&block, colour)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Framebuffer;
    use crate::{text_field, FieldAlign};

    const CANVAS: Size = Size::new(240, 135);

    fn magnified_text(scale: u32) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(CANVAS);
        {
            let mut big: Magnified<'_, Framebuffer> =
                Magnified::new(&mut fb, scale, Point::new(0, 0));
            text_field(
                &mut big,
                Point::new(0, 0),
                Rgb565::WHITE,
                6,
                FieldAlign::Left,
                format_args!("482913"),
            )
            .expect("a framebuffer render cannot fail");
        }
        fb
    }

    /// Zero: a zero scale paints nothing, like the sprite renderer's zero scale.
    #[test]
    fn a_zero_scale_paints_nothing() {
        assert_eq!(magnified_text(0).lit_pixels(), 0);
    }

    /// One: at scale 1 the magnifier is the identity — the same pixels the inner target
    /// would have received directly.
    #[test]
    fn scale_one_is_the_identity() {
        let mut direct: Framebuffer = Framebuffer::sized(CANVAS);
        text_field(
            &mut direct,
            Point::new(0, 0),
            Rgb565::WHITE,
            6,
            FieldAlign::Left,
            format_args!("482913"),
        )
        .expect("a framebuffer render cannot fail");
        assert_eq!(magnified_text(1).pixels(), direct.pixels());
    }

    /// Many: scaling multiplies the painted area by exactly scale² — the property that says
    /// the digits actually got bigger rather than merely moved.
    #[test]
    fn scaling_multiplies_the_painted_area_by_the_square_of_the_scale() {
        let single: usize = magnified_text(1).lit_pixels();
        assert_eq!(magnified_text(2).lit_pixels(), single * 4);
        assert_eq!(magnified_text(3).lit_pixels(), single * 9);
    }

    /// A magnified picture stays on the glass: nothing escapes the inner canvas.
    #[test]
    fn a_magnified_picture_stays_on_the_canvas() {
        assert_eq!(magnified_text(3).escaped(), 0);
    }

    /// The origin offsets in target space, so a caller can centre the magnified picture.
    #[test]
    fn the_origin_places_the_magnified_picture() {
        let mut shifted: Framebuffer = Framebuffer::sized(CANVAS);
        {
            let mut big: Magnified<'_, Framebuffer> =
                Magnified::new(&mut shifted, 2, Point::new(40, 20));
            text_field(
                &mut big,
                Point::new(0, 0),
                Rgb565::WHITE,
                6,
                FieldAlign::Left,
                format_args!("482913"),
            )
            .expect("a framebuffer render cannot fail");
        }
        assert_eq!(shifted.lit_pixels(), magnified_text(2).lit_pixels());
        assert_ne!(shifted.pixels(), magnified_text(2).pixels());
        assert_eq!(shifted.escaped(), 0);
    }

    /// The reported bounding box is in source pixels, so a caller laying out inside it and
    /// then drawing there cannot run off the inner target.
    #[test]
    fn the_bounding_box_is_reported_in_source_pixels() {
        let mut fb: Framebuffer = Framebuffer::sized(CANVAS);
        let big: Magnified<'_, Framebuffer> = Magnified::new(&mut fb, 3, Point::new(0, 0));
        assert_eq!(big.bounding_box().size, Size::new(80, 45));
    }
}
