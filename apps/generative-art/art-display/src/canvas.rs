//! The gallery's drawing surface: the port a sketch plots its full-screen picture into.
//!
//! A sketch does not know where its pixels end up — a host framebuffer for the goldens, or a
//! DMA-capable buffer on its way to the panel. It knows only how to *draw*: flood the ground, set a
//! pixel, lay `embedded-graphics` text. This trait is that seam. The gallery resets the canvas and
//! plots into it through this port and nothing more, so *what the canvas is made of* — an
//! [`Rgb565`] `Vec` the host blits, or the panel's wire-order bytes streamed straight over DMA — is
//! the composition root's choice, not the renderer's.
//!
//! Keeping the port in [`Rgb565`] is what lets this crate stay panel-agnostic: the frame's byte
//! order on the wire (the ST7789 is big-endian) is a fact of the *adapter*, never of the domain.
//! The host's [`Frame`](crate::Frame) implements this over an `Rgb565` buffer; the firmware's
//! wire-order adapter implements it over raw bytes. Both present the same [`Rgb565`] face, so the
//! gallery draws one picture and each adapter carries it to the glass its own way.
//!
//! A [`Canvas`] is a [`DrawTarget`] first — so the not-yet-built sketches can lay their placeholder
//! text through `embedded-graphics` — with two additions the gallery leans on every frame: a fast
//! per-pixel [`set`](Canvas::set) for the plume's five thousand dots (an iterator of one-pixel
//! `draw_iter` calls would pay dispatch on each), and a [`reset`](Canvas::reset) that both sizes the
//! surface to the active canvas and floods it with the ground.

use core::convert::Infallible;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

/// A full-screen [`Rgb565`] drawing surface the gallery plots one frame into.
///
/// [`DrawTarget`] with `Error = Infallible`, because plotting into a buffer cannot fail — an
/// off-canvas pixel is *clipped*, not errored (exactly as an off-panel draw is nothing on the
/// glass) — so the gallery's plot path carries no error handling. The two extra methods are the
/// hot path: [`set`](Self::set) plots one pixel without iterator dispatch, and
/// [`reset`](Self::reset) points the surface at a new canvas shape and floods it.
pub trait Canvas: DrawTarget<Color = Rgb565, Error = Infallible> {
    /// Paint the pixel at `(x, y)` `colour`, or drop it if it falls outside the active canvas.
    ///
    /// The plume's per-point plot and the self-erasing flood both go through here; an off-canvas
    /// coordinate — a flung barb tip, a cell past the edge — simply does not draw.
    fn set(&mut self, x: i32, y: i32, colour: Rgb565);

    /// Point the surface at a `size`-shaped canvas and flood it with `background`.
    ///
    /// Called once at the top of every frame: it sets the active dimensions [`set`](Self::set) and
    /// the sketch clip against, and fills that area with the ground the frame is drawn over — which
    /// is what makes the animation erase itself, no diff of what changed.
    fn reset(&mut self, size: Size, background: Rgb565);
}
