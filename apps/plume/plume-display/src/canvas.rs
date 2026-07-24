//! The one-bit offscreen frame — where a full-screen animation is made cheap.
//!
//! A frond is five thousand scattered dots. Drawn straight onto the panel, each dot is its own
//! addressed SPI window — five thousand tiny transactions a frame, which the bus cannot carry
//! at any watchable rate. So the dots are plotted here instead, into a one-bit-per-pixel frame
//! held in RAM, and the whole frame is then handed to the panel as **one** contiguous
//! window ([`blit`](Canvas::blit)). The picture crosses the wire as a single streamed fill, the
//! way a video frame does, rather than as thousands of pokes.
//!
//! One bit per pixel because the choice per pixel is binary — lit or unlit, [solid
//! dots](crate) — so the whole 135×240 frame is 4 KiB, not the 64 KiB an `Rgb565` frame would
//! be. The colour is applied on the way out, in [`blit`], so the frame stays a bare mask and
//! recolouring the frond costs nothing.
//!
//! Clearing and refilling the whole frame every frame is also what makes the animation
//! **erase itself**: nothing of the previous frond survives into the next, so a moving picture
//! never smears and needs no diff of what changed.

use alloc::vec;
use alloc::vec::Vec;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_display::{RenderError, SCREEN_SIZE};

/// The largest frame the panel ever needs: its full area, either way up. Sized once so a
/// rotation change reuses the buffer rather than reallocating.
const MAX_PIXELS: usize = (SCREEN_SIZE.width * SCREEN_SIZE.height) as usize;

/// How many packed bytes hold [`MAX_PIXELS`] one-bit pixels.
const MAX_BYTES: usize = MAX_PIXELS.div_ceil(8);

/// A one-bit-per-pixel drawing surface, sized to the active canvas and blitted whole.
///
/// The bits are row-major (x fastest), so bit index `y·width + x` is both the plot address and
/// the position the blit streams it out at — one order, used for both, so the two cannot
/// disagree.
pub struct Canvas {
    /// Packed pixels, one bit each, row-major. Allocated once at [`MAX_BYTES`]; only the active
    /// prefix is used.
    bits: Vec<u8>,
    /// The active canvas width in pixels.
    width: u32,
    /// The active canvas height in pixels.
    height: u32,
}

impl Canvas {
    /// A blank canvas at the panel's native landscape, its buffer sized for either way up.
    pub fn new() -> Self {
        Self {
            bits: vec![0; MAX_BYTES],
            width: SCREEN_SIZE.width,
            height: SCREEN_SIZE.height,
        }
    }

    /// Point the canvas at a `size`-shaped area and wipe it, reusing the buffer.
    ///
    /// The area must fit the buffer the canvas was built with — it always does, because the
    /// only sizes asked for are the panel's two ways up, both bounded by [`MAX_PIXELS`]. The
    /// assert makes that a checked fact rather than a silent overrun if a future caller forgets.
    pub fn reset(&mut self, size: Size) {
        debug_assert!(
            (size.width * size.height) as usize <= MAX_PIXELS,
            "a canvas larger than the panel"
        );
        self.width = size.width;
        self.height = size.height;
        // Zero the whole buffer: cheaper than computing the active byte span, and it leaves no
        // stale bit from a taller previous frame anywhere the next blit could stream it.
        self.bits.iter_mut().for_each(|byte: &mut u8| *byte = 0);
    }

    /// Light the pixel at `(x, y)`, or drop it if it falls outside the canvas.
    ///
    /// Clipping here is the projection's safety net: a frond point that lands off the panel — a
    /// barb tip, a point flung out by a near-zero `k` — simply does not draw, exactly as the
    /// original's off-canvas `circle` draws nothing.
    pub fn set(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let index: usize = y as usize * self.width as usize + x as usize;
        self.bits[index >> 3] |= 1 << (index & 7);
    }

    /// Whether the pixel at row-major `index` is lit.
    fn lit(&self, index: usize) -> bool {
        self.bits[index >> 3] & (1 << (index & 7)) != 0
    }

    /// Stream the whole active canvas to `target` as one contiguous fill: `on` where a pixel is
    /// lit, `off` everywhere else.
    ///
    /// This is the single transaction the whole module exists to make possible. The colours are
    /// applied here, so the frame itself carries none — recolouring the frond is a change to the
    /// two arguments, not to a pixel of storage.
    pub fn blit<D>(
        &self,
        target: &mut D,
        on: Rgb565,
        off: Rgb565,
    ) -> Result<(), RenderError<D::Error>>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let area: Rectangle = Rectangle::new(Point::zero(), Size::new(self.width, self.height));
        let count: usize = (self.width * self.height) as usize;
        target
            .fill_contiguous(
                &area,
                (0..count).map(|index: usize| if self.lit(index) { on } else { off }),
            )
            .map_err(RenderError::Draw)
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}
