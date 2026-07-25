//! The offscreen `Rgb565` frame — where a full-screen animation is made cheap.
//!
//! A sketch is a full-screen picture that changes every frame. Drawn straight onto the panel,
//! each cell or dot is its own addressed SPI window — thousands of tiny transactions a frame,
//! which the bus cannot carry at any watchable rate. So a sketch is plotted here instead, into an
//! `Rgb565` frame held in RAM, and the whole frame is then handed to the panel as **one**
//! contiguous window ([`blit`](Frame::blit)). The picture crosses the wire as a single streamed
//! fill, the way a video frame does, rather than as thousands of pokes.
//!
//! `Rgb565`, not the one-bit mask the monochrome plume used on its own, because the gallery holds
//! colour sketches (the fan's HSB quads, the orbits' grey bloom) that a single bit could not
//! carry. One frame shape serves them all: the plume simply plots white into it.
//!
//! Refilling the whole frame every frame is also what makes the animation **erase itself**:
//! nothing of the previous picture survives into the next, so a moving image never smears and
//! needs no diff of what changed.
//!
//! The frame is a [`DrawTarget`] in its own right, so a sketch may either plot pixels straight in
//! ([`set`](Frame::set)) or draw through `embedded-graphics` primitives (the not-yet-built
//! sketches render their placeholder text this way) — both land in the same buffer, blitted once.

use alloc::vec;
use alloc::vec::Vec;
use core::convert::Infallible;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_display::{RenderError, SCREEN_SIZE};

/// The largest frame the panel ever needs: its full area, either way up. Sized once so a rotation
/// change reuses the buffer rather than reallocating.
const MAX_PIXELS: usize = (SCREEN_SIZE.width * SCREEN_SIZE.height) as usize;

/// An `Rgb565` drawing surface, sized to the active canvas and blitted whole.
///
/// The pixels are row-major (x fastest), so index `y·width + x` is both the plot address and the
/// position the blit streams it out at — one order, used for both, so the two cannot disagree.
/// The buffer lives on the **heap** (a `Vec`), so the frame itself is a small handle and no
/// panel-sized buffer is ever placed on a stack, which on bring-up would overflow it.
pub struct Frame {
    /// The pixels, row-major. Allocated once at [`MAX_PIXELS`]; only the active prefix is used.
    pixels: Vec<Rgb565>,
    /// The active canvas width in pixels.
    width: u32,
    /// The active canvas height in pixels.
    height: u32,
}

impl Frame {
    /// A blank frame at the panel's native landscape, its buffer sized for either way up.
    pub fn new() -> Self {
        Self {
            pixels: vec![Rgb565::BLACK; MAX_PIXELS],
            width: SCREEN_SIZE.width,
            height: SCREEN_SIZE.height,
        }
    }

    /// Point the frame at a `size`-shaped area and flood it with `background`, reusing the buffer.
    ///
    /// The area must fit the buffer the frame was built with — it always does, because the only
    /// sizes asked for are the panel's two ways up, both bounded by [`MAX_PIXELS`]. The assert
    /// makes that a checked fact rather than a silent overrun if a future caller forgets. Only the
    /// active prefix is flooded; the blit streams exactly that prefix, so no stale pixel from a
    /// differently-shaped previous frame can reach the glass.
    pub fn reset(&mut self, size: Size, background: Rgb565) {
        debug_assert!(
            (size.width * size.height) as usize <= MAX_PIXELS,
            "a frame larger than the panel"
        );
        self.width = size.width;
        self.height = size.height;
        let count: usize = (self.width * self.height) as usize;
        self.pixels[..count]
            .iter_mut()
            .for_each(|pixel: &mut Rgb565| *pixel = background);
    }

    /// Paint the pixel at `(x, y)` `colour`, or drop it if it falls outside the canvas.
    ///
    /// Clipping here is the sketch's safety net: a point that lands off the panel — a flung barb
    /// tip, a cell past the edge — simply does not draw, the same nothing an off-canvas draw is in
    /// the source sketches.
    pub fn set(&mut self, x: i32, y: i32, colour: Rgb565) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let index: usize = y as usize * self.width as usize + x as usize;
        self.pixels[index] = colour;
    }

    /// The active canvas as a contiguous row-major `Rgb565` slice — exactly the pixels [`blit`]
    /// streams, in the order it streams them.
    ///
    /// The seam for a panel adapter that pushes the frame to the glass *itself* — swapping to the
    /// wire's byte order and bursting it over DMA — rather than draw through the generic
    /// [`blit`](Self::blit)/[`DrawTarget`] path. Same bytes, same order; only who moves them, and
    /// how fast, differs. Host renderers keep using [`blit`], so both paths stay one picture.
    pub fn pixels(&self) -> &[Rgb565] {
        let count: usize = (self.width * self.height) as usize;
        &self.pixels[..count]
    }

    /// Stream the whole active canvas to `target` as one contiguous fill — the single transaction
    /// the whole module exists to make possible.
    pub fn blit<D>(&self, target: &mut D) -> Result<(), RenderError<D::Error>>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let area: Rectangle = Rectangle::new(Point::zero(), Size::new(self.width, self.height));
        let count: usize = (self.width * self.height) as usize;
        target
            .fill_contiguous(&area, self.pixels[..count].iter().copied())
            .map_err(RenderError::Draw)
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

impl Dimensions for Frame {
    /// The active canvas — what a primitive drawn into the frame is clipped against.
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), Size::new(self.width, self.height))
    }
}

impl DrawTarget for Frame {
    type Color = Rgb565;
    /// Plotting into RAM cannot fail: an off-canvas pixel is dropped by [`set`](Frame::set), not
    /// an error, exactly as the on-panel path clips rather than faults.
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        pixels
            .into_iter()
            .for_each(|Pixel(coord, colour): Pixel<Rgb565>| self.set(coord.x, coord.y, colour));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_display::testing::Framebuffer;

    /// A portrait canvas to exercise a non-native frame shape.
    const PORTRAIT: Size = Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width);

    /// Zero: a freshly reset frame is a solid field of its background — the baseline every sketch
    /// plots over.
    #[test]
    fn a_reset_frame_is_solid_background() {
        let mut frame: Frame = Frame::new();
        frame.reset(PORTRAIT, Rgb565::BLACK);
        let mut fb: Framebuffer = Framebuffer::sized(PORTRAIT);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        assert_eq!(fb.lit_pixels(), 0, "a black frame lights nothing");
    }

    /// One: a single set pixel reaches the glass, and nothing else does.
    #[test]
    fn one_set_pixel_reaches_the_glass() {
        let mut frame: Frame = Frame::new();
        frame.reset(PORTRAIT, Rgb565::BLACK);
        frame.set(3, 4, Rgb565::WHITE);
        let mut fb: Framebuffer = Framebuffer::sized(PORTRAIT);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        assert_eq!(fb.lit_pixels(), 1, "exactly the one set pixel is lit");
    }

    /// A pixel outside the canvas is dropped, not wrapped to the far edge or an out-of-bounds
    /// write — the clip the sketches rely on.
    #[test]
    fn an_off_canvas_pixel_is_dropped() {
        let mut frame: Frame = Frame::new();
        frame.reset(PORTRAIT, Rgb565::BLACK);
        frame.set(-1, 0, Rgb565::WHITE);
        frame.set(0, -1, Rgb565::WHITE);
        frame.set(PORTRAIT.width as i32, 0, Rgb565::WHITE);
        frame.set(0, PORTRAIT.height as i32, Rgb565::WHITE);
        let mut fb: Framebuffer = Framebuffer::sized(PORTRAIT);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        assert_eq!(fb.lit_pixels(), 0, "no off-canvas write reaches the glass");
    }

    /// The blit streams exactly the active canvas — every pixel of it, and no more — so a blit
    /// after a resize never leaks a pixel of the taller previous frame.
    #[test]
    fn the_blit_covers_exactly_the_active_canvas() {
        let mut frame: Frame = Frame::new();
        frame.reset(PORTRAIT, Rgb565::WHITE);
        let mut fb: Framebuffer = Framebuffer::sized(PORTRAIT);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        assert_eq!(
            fb.painted(),
            (PORTRAIT.width * PORTRAIT.height) as usize,
            "the whole canvas is painted in one blit"
        );
        assert_eq!(fb.escaped(), 0, "nothing is streamed past the canvas");
    }
}
