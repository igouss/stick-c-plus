//! A host [`DrawTarget`] that records what was painted — and what escaped.
//!
//! The second adapter for the graphics port, opposite the on-target ST7789 panel.
//! It exists so the crate's rules are proven against real pixels rather than against
//! the arithmetic that produced them.
//!
//! ## Why not `SimulatorDisplay`
//!
//! `embedded_graphics_simulator::SimulatorDisplay` *clips* an out-of-bounds write and
//! returns `Ok`, as [`DrawTarget`] permits. A "nothing is drawn off-screen" test
//! written against it can therefore never fail — a clipped digit would vanish and the
//! test would still pass. That is a false green, so this target counts every escaping
//! pixel instead of swallowing it. (The simulator is still the right tool for saving a
//! PNG, and `examples/screenshots.rs` uses it for exactly that.)

use core::convert::Infallible;

use alloc::vec;
use alloc::vec::Vec;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

use crate::SCREEN_SIZE;

/// A canvas of `Rgb565`, plus a count of writes that fell outside it.
///
/// The canvas is [`SCREEN_SIZE`] by default and any size on request. A screen drawn at a
/// quarter turn paints into a canvas with the panel's dimensions swapped, and its
/// [`escaped`](Framebuffer::escaped) count only means "nothing was clipped" if the canvas it
/// is counted against is the one that screen is actually drawn on.
pub struct Framebuffer {
    size: Size,
    pixels: Vec<Rgb565>,
    escaped: usize,
}

impl Framebuffer {
    /// A blank [`SCREEN_SIZE`] canvas, black as the panel is after its bring-up clear.
    pub fn new() -> Self {
        Self::sized(SCREEN_SIZE)
    }

    /// A blank canvas of `size` — for a screen drawn on something other than the panel's
    /// native landscape.
    pub fn sized(size: Size) -> Self {
        let area: usize = (size.width * size.height) as usize;
        Framebuffer {
            size,
            pixels: vec![Rgb565::BLACK; area],
            escaped: 0,
        }
    }

    /// Every pixel, row-major — the whole picture, for comparing two renders.
    pub fn pixels(&self) -> &[Rgb565] {
        &self.pixels
    }

    /// How many pixels carry ink (are not the black background).
    pub fn lit_pixels(&self) -> usize {
        self.pixels
            .iter()
            .filter(|colour: &&Rgb565| **colour != Rgb565::BLACK)
            .count()
    }

    /// How many writes landed outside the canvas — clipped glyphs, in other words.
    /// A correct layout never produces one.
    pub fn escaped(&self) -> usize {
        self.escaped
    }

    /// Store one pixel, or record that it escaped the canvas.
    fn put(&mut self, at: Point, colour: Rgb565) {
        let inside: bool = at.x >= 0
            && at.y >= 0
            && (at.x as u32) < self.size.width
            && (at.y as u32) < self.size.height;
        if !inside {
            self.escaped += 1;
            return;
        }
        let index: usize = at.y as usize * self.size.width as usize + at.x as usize;
        self.pixels[index] = colour;
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        self.size
    }
}

impl DrawTarget for Framebuffer {
    type Color = Rgb565;
    /// A memory write cannot fail — which is what makes this target's `Ok` mean
    /// "painted", and lets a render test assert on pixels without unwrapping a bus.
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        pixels
            .into_iter()
            .for_each(|Pixel(at, colour): Pixel<Rgb565>| self.put(at, colour));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The escape counter is the whole reason this target exists; prove it counts.
    #[test]
    fn a_pixel_inside_the_canvas_is_stored_and_does_not_escape() {
        let mut fb: Framebuffer = Framebuffer::new();
        fb.draw_iter([Pixel(Point::new(0, 0), Rgb565::WHITE)])
            .expect("memory writes cannot fail");
        assert_eq!(fb.escaped(), 0);
        assert_eq!(fb.lit_pixels(), 1);
    }

    #[test]
    fn a_pixel_beyond_the_right_edge_escapes() {
        let mut fb: Framebuffer = Framebuffer::new();
        fb.draw_iter([Pixel(
            Point::new(SCREEN_SIZE.width as i32, 0),
            Rgb565::WHITE,
        )])
        .expect("memory writes cannot fail");
        assert_eq!(fb.escaped(), 1);
        assert_eq!(fb.lit_pixels(), 0);
    }

    #[test]
    fn a_pixel_at_a_negative_coordinate_escapes() {
        let mut fb: Framebuffer = Framebuffer::new();
        fb.draw_iter([Pixel(Point::new(-1, 0), Rgb565::WHITE)])
            .expect("memory writes cannot fail");
        assert_eq!(fb.escaped(), 1);
    }

    #[test]
    fn a_blank_canvas_carries_no_ink() {
        assert_eq!(Framebuffer::new().lit_pixels(), 0);
    }

    /// The default canvas is the panel's.
    #[test]
    fn a_default_canvas_is_the_panels_size() {
        assert_eq!(Framebuffer::new().size(), SCREEN_SIZE);
    }

    /// A turned canvas counts escapes against *its own* edges, not the panel's. Without this
    /// the escape counter would call a correctly-placed portrait pixel an escape, and — worse
    /// — would silently accept one that ran off the narrow edge.
    #[test]
    fn a_turned_canvas_counts_escapes_against_its_own_edges() {
        let turned: Size = Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width);
        let mut fb: Framebuffer = Framebuffer::sized(turned);

        // Inside the turned canvas, past the panel's landscape bottom edge.
        fb.draw_iter([Pixel(
            Point::new(0, SCREEN_SIZE.height as i32),
            Rgb565::WHITE,
        )])
        .expect("memory writes cannot fail");
        // Outside the turned canvas, inside the panel's landscape width.
        fb.draw_iter([Pixel(Point::new(turned.width as i32, 0), Rgb565::WHITE)])
            .expect("memory writes cannot fail");

        assert_eq!(fb.size(), turned);
        assert_eq!(fb.lit_pixels(), 1);
        assert_eq!(fb.escaped(), 1);
    }
}
