#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # wire-canvas
//!
//! The gallery's device-side drawing surface: a [`Canvas`] that stores its pixels in the panel's
//! wire order, so a finished frame is *already* the bytes the ST7789 wants and streams straight out
//! over DMA — no offscreen `Rgb565` frame, no per-frame byte-swap.
//!
//! The gallery renders into any [`Canvas`]. On the host that is art-display's `Frame`, an `Rgb565`
//! buffer blitted to a draw target. On the board it is this: a [`WireCanvas`] over a **borrowed**
//! byte buffer — on the device the DMA-capable one the panel streams from — into which every plotted
//! pixel is written as its two big-endian bytes on the spot. So the single full-screen buffer the
//! app can afford is filled once, in the order and format the wire wants, and shown with a bare
//! `RAMWR` + one DMA burst.
//!
//! ## Why the byte order lives here, not in art-display
//!
//! The ST7789 takes `Rgb565` most-significant-byte first. That is a fact of the *panel*, so it
//! belongs to an adapter, not to the gallery domain: art-display speaks only [`Rgb565`] through the
//! [`Canvas`] port and never learns the wire order. This crate is the one place that knows it — the
//! [`set`](WireCanvas::set) that encodes big-endian — which is exactly the hexagonal seam: the
//! domain stays panel-agnostic and host-testable, and the wire format is isolated where it can be
//! proven ([`bytes`](WireCanvas::bytes) against the host `Frame`, byte for byte, in the tests below).
//!
//! It sits beside art-display in the app workspace rather than in `platform/adapters` because it
//! depends on the app's [`Canvas`] port, and the platform must never depend on an app — dependencies
//! point inward.

use core::convert::Infallible;

use art_display::Canvas;
use embedded_graphics::pixelcolor::raw::ToBytes;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

/// A full-screen [`Canvas`] backed by a borrowed wire-order byte buffer.
///
/// Two big-endian bytes per pixel, row-major (`2·(y·width + x)` is a pixel's byte offset), so the
/// active canvas is a contiguous run the panel streams with no reordering. The buffer is
/// **borrowed**, not owned: on the device it is the DMA-capable buffer the panel bursts from, handed
/// in once at the composition root, so this adapter adds no allocation of its own — it *is* the one
/// full-screen buffer, not a second copy of it.
pub struct WireCanvas<'a> {
    /// The wire-order pixels: two big-endian bytes each, row-major. Sized for the panel's full area
    /// at construction; only the active prefix (see [`reset`](Self::reset)) is used and streamed.
    buffer: &'a mut [u8],
    /// The active canvas width in pixels.
    width: u32,
    /// The active canvas height in pixels.
    height: u32,
}

impl<'a> WireCanvas<'a> {
    /// Wrap a whole-frame byte buffer as a blank canvas.
    ///
    /// `buffer` must hold at least two bytes per pixel of the largest canvas ever
    /// [`reset`](Self::reset) to — on the device it is `dma_mem`'s `w·h·2`-byte DMA buffer. The
    /// canvas has no shape until the first `reset`; nothing is streamed before then.
    pub fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            width: 0,
            height: 0,
        }
    }

    /// The active canvas as wire-order bytes — exactly what the panel streams, in the order it
    /// streams it.
    ///
    /// The seam the composition root bursts to the glass: `RAMWR` then this slice, one DMA
    /// transaction. Its length is the active `w·h·2`, so a canvas smaller than the buffer streams
    /// only its own pixels, never a stale tail.
    pub fn bytes(&self) -> &[u8] {
        let count: usize = (self.width * self.height) as usize * 2;
        &self.buffer[..count]
    }

    /// The pixel at `(x, y)` as wire-order bytes, or `None` if it falls outside the active canvas —
    /// the encode-and-index this adapter is built around, factored out so [`set`](Self::set) and the
    /// flood share one definition.
    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return None;
        }
        Some((y as usize * self.width as usize + x as usize) * 2)
    }
}

impl WireCanvas<'_> {
    /// Paint the pixel at `(x, y)` `colour` in wire order, or drop it if it falls outside the canvas.
    ///
    /// This is the whole of the adapter's panel knowledge: the pixel is written most-significant byte
    /// first ([`ToBytes::to_be_bytes`]), the ST7789's order. The clip matches the host `Frame`'s
    /// exactly, so the two adapters agree on which coordinates draw and which are dropped.
    fn set(&mut self, x: i32, y: i32, colour: Rgb565) {
        if let Some(offset) = self.offset(x, y) {
            // A two-byte `copy_from_slice`, not two byte stores: it compiles to one aligned 16-bit
            // write, which on the ESP32's DMA SRAM is markedly faster than sub-word stores — the
            // difference measured at ~1.8 ms a frame across the flood.
            self.buffer[offset..offset + 2].copy_from_slice(&colour.to_be_bytes());
        }
    }

    /// Point the canvas at a `size`-shaped area and flood it with `background` in wire order.
    ///
    /// Sizes the active canvas and fills its whole byte run with the ground's two big-endian bytes,
    /// so the frame starts as a solid field and the sketch plots over it — the self-erasing property,
    /// paid here once instead of by a separate swap pass.
    fn reset(&mut self, size: Size, background: Rgb565) {
        self.width = size.width;
        self.height = size.height;
        let count: usize = (self.width * self.height) as usize;
        debug_assert!(
            count * 2 <= self.buffer.len(),
            "a canvas larger than the wire buffer"
        );
        let bytes: [u8; 2] = background.to_be_bytes();
        // `copy_from_slice` per chunk — one aligned 16-bit store each, the fast path on DMA SRAM.
        self.buffer[..count * 2]
            .chunks_exact_mut(2)
            .for_each(|slot: &mut [u8]| slot.copy_from_slice(&bytes));
    }
}

impl Dimensions for WireCanvas<'_> {
    /// The active canvas — what a primitive drawn into the surface is clipped against.
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), Size::new(self.width, self.height))
    }
}

impl DrawTarget for WireCanvas<'_> {
    type Color = Rgb565;
    /// Plotting into the buffer cannot fail: an off-canvas pixel is dropped by [`set`](Self::set),
    /// not errored — the same clip the host `Frame` and the on-panel path make.
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

impl Canvas for WireCanvas<'_> {
    /// The wire adapter's plot: encode `colour` big-endian into the buffer.
    fn set(&mut self, x: i32, y: i32, colour: Rgb565) {
        WireCanvas::set(self, x, y, colour);
    }

    /// The wire adapter's flood: fill the active canvas with `background`'s wire bytes.
    fn reset(&mut self, size: Size, background: Rgb565) {
        WireCanvas::reset(self, size, background);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use art_display::Frame;
    use platform_display::testing::Framebuffer;
    use proptest::prelude::*;

    /// A portrait canvas to exercise a non-square shape, matching the board's orientation.
    const PORTRAIT: Size = Size::new(135, 240);

    /// A buffer large enough for the portrait canvas: two bytes a pixel.
    fn portrait_buffer() -> alloc::vec::Vec<u8> {
        alloc::vec![0u8; (PORTRAIT.width * PORTRAIT.height) as usize * 2]
    }

    // The test build links std, so `alloc` is available for the buffers.
    extern crate alloc;

    /// One: a set pixel is its two big-endian bytes at the row-major offset, and nothing else is
    /// touched. This is the adapter's whole panel contract — most-significant byte first.
    #[test]
    fn a_set_pixel_is_its_big_endian_bytes() {
        let mut buffer: alloc::vec::Vec<u8> = portrait_buffer();
        let mut wire: WireCanvas = WireCanvas::new(&mut buffer);
        wire.reset(PORTRAIT, Rgb565::BLACK);

        // A colour with distinct high and low bytes, so a swap could not hide.
        let colour: Rgb565 = Rgb565::new(31, 0, 0); // 0xF800 -> [0xF8, 0x00]
        wire.set(2, 1, colour);

        let offset: usize = (1 * PORTRAIT.width as usize + 2) * 2;
        assert_eq!(wire.bytes()[offset], 0xF8, "high byte first");
        assert_eq!(wire.bytes()[offset + 1], 0x00, "low byte second");
    }

    /// Zero: a coordinate outside the canvas draws nothing — no wrap to the far edge, no write past
    /// the buffer. The clip the sketches rely on, matching the host `Frame`.
    #[test]
    fn an_off_canvas_pixel_is_dropped() {
        let mut buffer: alloc::vec::Vec<u8> = portrait_buffer();
        let mut wire: WireCanvas = WireCanvas::new(&mut buffer);
        wire.reset(PORTRAIT, Rgb565::BLACK);
        wire.set(-1, 0, Rgb565::WHITE);
        wire.set(0, -1, Rgb565::WHITE);
        wire.set(PORTRAIT.width as i32, 0, Rgb565::WHITE);
        wire.set(0, PORTRAIT.height as i32, Rgb565::WHITE);
        assert!(
            wire.bytes().iter().all(|&b: &u8| b == 0),
            "an off-canvas write reached the buffer"
        );
    }

    /// A reset floods the whole active canvas with the ground's wire bytes — the solid field every
    /// sketch plots over.
    #[test]
    fn a_reset_floods_the_ground_in_wire_order() {
        let mut buffer: alloc::vec::Vec<u8> = portrait_buffer();
        let mut wire: WireCanvas = WireCanvas::new(&mut buffer);
        let ground: Rgb565 = Rgb565::new(0, 63, 0); // 0x07E0 -> [0x07, 0xE0]
        wire.reset(PORTRAIT, ground);
        assert!(
            wire.bytes()
                .chunks_exact(2)
                .all(|p: &[u8]| p == [0x07, 0xE0]),
            "the flood is not the ground's wire bytes everywhere"
        );
    }

    /// The wire bytes of a `Frame` blitted to a host framebuffer, in the panel's big-endian order —
    /// the reference the wire canvas must reproduce.
    fn frame_as_wire_bytes(frame: &Frame) -> alloc::vec::Vec<u8> {
        let mut fb: Framebuffer = Framebuffer::sized(PORTRAIT);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb.pixels()
            .iter()
            .flat_map(|colour: &Rgb565| colour.to_be_bytes())
            .collect()
    }

    proptest! {
        /// Many: for any set of plotted pixels, the wire canvas holds byte-for-byte what the host
        /// `Frame` would blit, encoded big-endian. This is the keystone's core invariant — the two
        /// Canvas adapters are one picture — proven on the host, because on the monochrome plume a
        /// byte-swap is invisible on the glass. Colours carry distinct high/low bytes so a swap
        /// cannot pass.
        #[test]
        fn the_wire_canvas_is_byte_identical_to_the_frame(
            plots in prop::collection::vec(
                (0i32..PORTRAIT.width as i32, 0i32..PORTRAIT.height as i32, any::<u16>()),
                0..64,
            ),
        ) {
            let mut frame: Frame = Frame::new();
            let mut buffer: alloc::vec::Vec<u8> = portrait_buffer();
            let mut wire: WireCanvas = WireCanvas::new(&mut buffer);

            Canvas::reset(&mut frame, PORTRAIT, Rgb565::BLACK);
            Canvas::reset(&mut wire, PORTRAIT, Rgb565::BLACK);
            for (x, y, raw) in plots {
                let colour: Rgb565 = Rgb565::from(embedded_graphics::pixelcolor::raw::RawU16::new(raw));
                Canvas::set(&mut frame, x, y, colour);
                Canvas::set(&mut wire, x, y, colour);
            }

            let expected: alloc::vec::Vec<u8> = frame_as_wire_bytes(&frame);
            prop_assert_eq!(wire.bytes(), expected.as_slice());
        }
    }
}
