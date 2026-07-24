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

/// A canvas of `Rgb565`, plus a count of writes that fell outside it and a count of the ones
/// that repainted a pixel this frame had already painted a different colour.
///
/// The canvas is [`SCREEN_SIZE`] by default and any size on request. A screen drawn at a
/// quarter turn paints into a canvas with the panel's dimensions swapped, and its
/// [`escaped`](Framebuffer::escaped) count only means "nothing was clipped" if the canvas it
/// is counted against is the one that screen is actually drawn on.
///
/// ## The flicker counter
///
/// There is no framebuffer between this crate and the glass: every write reaches the panel, and
/// the eye sees each one. So a renderer that clears a region and then draws into it shows the
/// owner the cleared state — for a whole SPI transfer, once per repaint. That is exactly what
/// *flicker* is, and [`overpainted`](Framebuffer::overpainted) counts it: a pixel written twice
/// in one frame with two different colours. Writing the same colour twice is invisible and is
/// not counted; it is waste, not a defect.
///
/// The count is per **frame**, so a test that renders twice into one canvas calls
/// [`start_frame`](Framebuffer::start_frame) between the renders — the second render legitimately
/// repaints what the first one left.
pub struct Framebuffer {
    size: Size,
    pixels: Vec<Rgb565>,
    escaped: usize,
    painted: Vec<bool>,
    overpainted: usize,
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
            painted: vec![false; area],
            overpainted: 0,
        }
    }

    /// Begin a new frame: forget which pixels this one has painted, and reset the flicker count.
    ///
    /// Only a test that renders more than once into the same canvas needs it. A second render is
    /// *meant* to repaint what the first one left — that is erase-in-place, not flicker — and
    /// without this the two frames would be judged as one.
    pub fn start_frame(&mut self) {
        self.painted
            .iter_mut()
            .for_each(|seen: &mut bool| *seen = false);
        self.overpainted = 0;
    }

    /// Every pixel, row-major — the whole picture, for comparing two renders.
    pub fn pixels(&self) -> &[Rgb565] {
        &self.pixels
    }

    /// The colour at `(x, y)`.
    ///
    /// For a test asking about one *place* rather than the whole picture — "is the border on
    /// the edge?", "is this corner marked?" — where indexing `pixels()` by hand would put
    /// row-major arithmetic in the assertion and bury what is being claimed.
    ///
    /// Panics outside the canvas, deliberately: a test that reads a pixel which cannot exist
    /// is asking a malformed question, and silently handing back black would let it pass.
    pub fn pixel(&self, x: u32, y: u32) -> Rgb565 {
        assert!(
            x < self.size.width && y < self.size.height,
            "({x}, {y}) is outside the {} x {} canvas",
            self.size.width,
            self.size.height
        );
        self.pixels[(y * self.size.width + x) as usize]
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

    /// How many pixels this frame has painted at all, whatever colour.
    ///
    /// The complement of [`overpainted`](Framebuffer::overpainted): together they say a region
    /// was covered *and* covered once — which is the whole claim behind "this renderer owns its
    /// band", and a claim [`lit_pixels`](Framebuffer::lit_pixels) cannot make, because a pixel
    /// deliberately painted background is indistinguishable from one never painted at all.
    pub fn painted(&self) -> usize {
        self.painted.iter().filter(|seen: &&bool| **seen).count()
    }

    /// How many pixels this frame painted a second time in a different colour — every one of
    /// them a flicker the owner would see, because there is no framebuffer between here and the
    /// glass. A renderer that paints each region once, opaquely, scores zero.
    pub fn overpainted(&self) -> usize {
        self.overpainted
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
        if self.painted[index] && self.pixels[index] != colour {
            self.overpainted += 1;
        }
        self.painted[index] = true;
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

    /// Zero, one, many for the flicker counter: a pixel painted once is not a flicker, the same
    /// colour written again is invisible and is not one either, and a *different* colour over a
    /// pixel this frame already painted is exactly one.
    #[test]
    fn only_a_second_colour_on_the_same_pixel_counts_as_a_flicker() {
        let mut fb: Framebuffer = Framebuffer::new();
        let at: Point = Point::new(3, 4);

        fb.draw_iter([Pixel(at, Rgb565::WHITE)])
            .expect("memory writes cannot fail");
        assert_eq!(fb.overpainted(), 0, "one paint is not a flicker");

        fb.draw_iter([Pixel(at, Rgb565::WHITE)])
            .expect("memory writes cannot fail");
        assert_eq!(fb.overpainted(), 0, "the same colour twice is invisible");

        fb.draw_iter([Pixel(at, Rgb565::BLACK), Pixel(at, Rgb565::WHITE)])
            .expect("memory writes cannot fail");
        assert_eq!(
            fb.overpainted(),
            2,
            "clear-then-draw is two visible changes"
        );
    }

    /// A second render into the same canvas is erase-in-place, not flicker — so the counter is
    /// per frame, and `start_frame` is what separates them.
    #[test]
    fn a_new_frame_forgets_what_the_previous_one_painted() {
        let mut fb: Framebuffer = Framebuffer::new();
        let at: Point = Point::new(1, 1);

        fb.draw_iter([Pixel(at, Rgb565::WHITE)])
            .expect("memory writes cannot fail");
        fb.start_frame();
        fb.draw_iter([Pixel(at, Rgb565::BLACK)])
            .expect("memory writes cannot fail");

        assert_eq!(fb.overpainted(), 0);
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
