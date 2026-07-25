#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # wire-canvas
//!
//! The gallery's device-side drawing surface: a [`Canvas`] that stores its pixels in the panel's
//! wire order, so a finished frame is *already* the bytes the ST7789 wants and streams straight out
//! over DMA — no offscreen `Rgb565` frame, no per-frame conversion pass.
//!
//! The gallery renders into any [`Canvas`]. On the host that is art-display's `Frame`, an `Rgb565`
//! buffer blitted to a draw target. On the board it is this: a [`WireCanvas`] over a **borrowed**
//! byte buffer — on the device the DMA-capable one the panel streams from — into which every plotted
//! pixel is packed in the panel's format on the spot. So the single full-screen buffer the app can
//! afford is filled once, in the order and format the wire wants, and shown with a bare `RAMWR` +
//! one DMA burst.
//!
//! ## The wire format is packed 12-bit RGB444
//!
//! The panel is driven in its 12-bit-per-pixel mode (`COLMOD = 0x03`): three 4-bit channels a pixel,
//! **two pixels packed into three bytes** — `[R0 G0][B0 R1][G1 B1]`. That is 25% fewer bytes across
//! the bus than 16-bit `Rgb565` and 25% less heap per full-screen buffer, which is what lets a
//! *second* full-screen buffer fit for the double-buffer. On the monochrome plume the loss is nil
//! (white `0xFFFF` → `0xFFF`, still full white); on a colour sketch it is a single low bit per
//! channel. An `Rgb565` is quantised to RGB444 here by truncation (see [`rgb444`]).
//!
//! Because a byte is shared by two pixels (the middle byte of every three carries one pixel's blue
//! and the next pixel's red), a single-pixel [`set`](WireCanvas::set) is a read-modify-write that
//! preserves the neighbour's nibble. Plotting is single-threaded, so there is no aliasing — the
//! render thread owns the whole buffer for the frame.
//!
//! ## Why the wire format lives here, not in art-display
//!
//! The packing is a fact of the *panel*, so it belongs to an adapter, not to the gallery domain:
//! art-display speaks only [`Rgb565`] through the [`Canvas`] port and never learns the wire format.
//! This crate is the one place that knows it — the [`set`](WireCanvas::set) that quantises and packs
//! — which is exactly the hexagonal seam: the domain stays panel-agnostic and host-testable, and the
//! wire format is isolated where it can be proven ([`bytes`](WireCanvas::bytes) against the host
//! `Frame` quantised to 12-bit, byte for byte, in the tests below).
//!
//! It sits beside art-display in the app workspace rather than in `platform/adapters` because it
//! depends on the app's [`Canvas`] port, and the platform must never depend on an app — dependencies
//! point inward.

use core::convert::Infallible;

use art_display::Canvas;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

/// The panel's 12-bit representation of an `Rgb565`: the high four bits of each channel, as three
/// `0..=15` nibbles `(r, g, b)`.
///
/// Quantise by truncation — drop the low bit of red and blue, the low two of green. On the white
/// plume this is exact (`0xFFFF` → `0xFFF`); on a colour sketch it costs a single LSB per channel,
/// which the panel's 12-bit mode could not have shown anyway. The single source of truth for the
/// downsample, shared by [`WireCanvas::set`], the flood, and the tests' reference packing.
fn rgb444(colour: Rgb565) -> (u8, u8, u8) {
    (colour.r() >> 1, colour.g() >> 2, colour.b() >> 1)
}

/// A full-screen [`Canvas`] backed by a borrowed wire-order byte buffer.
///
/// Packed 12-bit RGB444, row-major, two pixels per three bytes, so the active canvas is a contiguous
/// run the panel streams with no reordering. The buffer is **borrowed**, not owned: on the device it
/// is the DMA-capable buffer the panel bursts from, handed in once at the composition root, so this
/// adapter adds no allocation of its own — it *is* the one full-screen buffer, not a second copy.
pub struct WireCanvas<'a> {
    /// The wire-order pixels: packed 12-bit RGB444, row-major, two pixels per three bytes. Sized for
    /// the panel's full area at construction; only the active prefix (see [`reset`](Self::reset)) is
    /// used and streamed.
    buffer: &'a mut [u8],
    /// The active canvas width in pixels.
    width: u32,
    /// The active canvas height in pixels.
    height: u32,
}

impl<'a> WireCanvas<'a> {
    /// Wrap a whole-frame byte buffer as a blank canvas.
    ///
    /// `buffer` must hold at least `w·h·3/2` bytes for the largest canvas ever
    /// [`reset`](Self::reset) to — on the device it is `dma_mem`'s DMA buffer. The canvas has no
    /// shape until the first `reset`; nothing is streamed before then.
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
    /// transaction. Its length is the active `w·h·3/2`, so a canvas smaller than the buffer streams
    /// only its own pixels, never a stale tail.
    pub fn bytes(&self) -> &[u8] {
        &self.buffer[..self.wire_len()]
    }

    /// The active canvas's wire length in bytes: `w·h·3/2`, two 12-bit pixels per three bytes.
    ///
    /// The panel area is an even pixel count (a portrait 135×240 or its landscape turn — 32 400
    /// either way), so the halving is exact and no pixel straddles the end of the buffer.
    fn wire_len(&self) -> usize {
        (self.width * self.height) as usize / 2 * 3
    }
}

impl WireCanvas<'_> {
    /// Paint the pixel at `(x, y)` `colour` in wire order, or drop it if it falls outside the canvas.
    ///
    /// This is the whole of the adapter's panel knowledge: the colour is quantised to 12-bit
    /// ([`rgb444`]) and packed into the pixel's three nibbles. The middle byte of each two-pixel
    /// group is shared, so writing one pixel of the pair is a read-modify-write that leaves the
    /// other's nibble intact. The clip matches the host `Frame`'s exactly, so the two adapters agree
    /// on which coordinates draw and which are dropped.
    fn set(&mut self, x: i32, y: i32, colour: Rgb565) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let pixel: usize = y as usize * self.width as usize + x as usize;
        let (r, g, b): (u8, u8, u8) = rgb444(colour);
        // Each two-pixel group is three bytes at `base = (pixel / 2) · 3`. The even pixel of the pair
        // owns the first byte and the high nibble of the second; the odd pixel owns the low nibble of
        // the second and the whole third — so only the second (shared) byte needs the read-modify.
        let base: usize = pixel / 2 * 3;
        if pixel % 2 == 0 {
            self.buffer[base] = (r << 4) | g;
            self.buffer[base + 1] = (b << 4) | (self.buffer[base + 1] & 0x0F);
        } else {
            self.buffer[base + 1] = (self.buffer[base + 1] & 0xF0) | r;
            self.buffer[base + 2] = (g << 4) | b;
        }
    }

    /// Point the canvas at a `size`-shaped area and flood it with `background` in wire order.
    ///
    /// Sizes the active canvas and fills its whole byte run with the ground's packed 12-bit pattern,
    /// so the frame starts as a solid field and the sketch plots over it — the self-erasing property,
    /// paid here once. A uniform colour repeats every two pixels, i.e. every three bytes, so the
    /// flood is one `[u8; 3]` pattern stamped across the run.
    fn reset(&mut self, size: Size, background: Rgb565) {
        self.width = size.width;
        self.height = size.height;
        debug_assert!(
            (self.width * self.height).is_multiple_of(2),
            "the panel area must be an even pixel count for 12-bit packing"
        );
        let bytes: usize = self.wire_len();
        debug_assert!(
            bytes <= self.buffer.len(),
            "a canvas larger than the wire buffer"
        );
        let (r, g, b): (u8, u8, u8) = rgb444(background);
        // Two consecutive pixels of the same colour: [R G][B R][G B].
        let pattern: [u8; 3] = [(r << 4) | g, (b << 4) | r, (g << 4) | b];
        self.buffer[..bytes]
            .chunks_exact_mut(3)
            .for_each(|slot: &mut [u8]| slot.copy_from_slice(&pattern));
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
    /// The wire adapter's plot: quantise `colour` to 12-bit and pack it into the buffer.
    fn set(&mut self, x: i32, y: i32, colour: Rgb565) {
        WireCanvas::set(self, x, y, colour);
    }

    /// The wire adapter's flood: fill the active canvas with `background`'s packed 12-bit pattern.
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

    /// A buffer large enough for the portrait canvas: `w·h·3/2` bytes, two 12-bit pixels per three.
    fn portrait_buffer() -> alloc::vec::Vec<u8> {
        alloc::vec![0u8; (PORTRAIT.width * PORTRAIT.height) as usize / 2 * 3]
    }

    // The test build links std, so `alloc` is available for the buffers.
    extern crate alloc;

    /// One: a two-pixel group packs into its three bytes, and the shared middle byte carries *both*
    /// pixels' nibbles — the read-modify-write of the second `set` leaves the first's blue intact.
    /// This is the adapter's whole panel contract: `[R0 G0][B0 R1][G1 B1]`.
    #[test]
    fn a_two_pixel_group_packs_into_three_bytes() {
        let mut buffer: alloc::vec::Vec<u8> = portrait_buffer();
        let mut wire: WireCanvas = WireCanvas::new(&mut buffer);
        wire.reset(PORTRAIT, Rgb565::BLACK);

        // Pixels 0 and 1 of row 0 share byte 1: pixel 0's blue in its high nibble, pixel 1's red in
        // its low nibble. Distinct channels so a mispack cannot hide.
        wire.set(0, 0, Rgb565::new(31, 0, 31)); // r4=15, g4=0, b4=15
        wire.set(1, 0, Rgb565::new(0, 63, 0)); // r4=0,  g4=15, b4=0

        assert_eq!(wire.bytes()[0], 0xF0, "pixel 0: R<<4 | G");
        assert_eq!(wire.bytes()[1], 0xF0, "byte 1: pixel 0 B<<4 | pixel 1 R");
        assert_eq!(wire.bytes()[2], 0xF0, "pixel 1: G<<4 | B");
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

    /// A reset floods the whole active canvas with the ground's packed 12-bit pattern — the solid
    /// field every sketch plots over, one three-byte group per two pixels.
    #[test]
    fn a_reset_floods_the_ground_in_wire_order() {
        let mut buffer: alloc::vec::Vec<u8> = portrait_buffer();
        let mut wire: WireCanvas = WireCanvas::new(&mut buffer);
        let ground: Rgb565 = Rgb565::new(0, 63, 0); // r4=0, g4=15, b4=0 -> [0x0F, 0x00, 0xF0]
        wire.reset(PORTRAIT, ground);
        assert!(
            wire.bytes()
                .chunks_exact(3)
                .all(|p: &[u8]| p == [0x0F, 0x00, 0xF0]),
            "the flood is not the ground's packed wire bytes everywhere"
        );
    }

    /// The wire bytes of a `Frame` blitted to a host framebuffer, quantised to 12-bit and packed in
    /// the panel's two-pixels-per-three-bytes order — the reference the wire canvas must reproduce.
    fn frame_as_wire_bytes(frame: &Frame) -> alloc::vec::Vec<u8> {
        let mut fb: Framebuffer = Framebuffer::sized(PORTRAIT);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb.pixels()
            .chunks_exact(2)
            .flat_map(|pair: &[Rgb565]| {
                let (r0, g0, b0): (u8, u8, u8) = rgb444(pair[0]);
                let (r1, g1, b1): (u8, u8, u8) = rgb444(pair[1]);
                [(r0 << 4) | g0, (b0 << 4) | r1, (g1 << 4) | b1]
            })
            .collect()
    }

    proptest! {
        /// Many: for any set of plotted pixels, the wire canvas holds byte-for-byte what the host
        /// `Frame` would blit, quantised to 12-bit and packed. This is the keystone's core invariant
        /// — the two Canvas adapters are one picture (to the panel's own colour depth) — proven on
        /// the host, because the per-pixel read-modify-write pack streamed point-by-point must equal
        /// the pairwise pack of the finished frame. Colours span the whole `u16` so a mispack or a
        /// clobbered shared nibble cannot pass.
        #[test]
        fn the_wire_canvas_is_the_frame_quantised_to_12_bit(
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
