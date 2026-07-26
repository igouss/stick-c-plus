//! The gallery renderer: reset the canvas, draw the selected sketch into it, hand it back to be
//! shown.
//!
//! Device-independent by construction — it plots into any [`Canvas`], so the on-target wire-order
//! buffer and a host [`Frame`](crate::Frame) render *the same code*. The renderer owns just one
//! thing across frames: the [`FrondCompute`] port the plume's cloud is evaluated through (built
//! once, at construction). It does **not** own the pixels: the canvas is supplied each frame by the
//! composition root, which is what lets the device plot straight into a single DMA buffer (no
//! offscreen copy, no per-frame byte-swap) while the host plots into an `Rgb565` frame it blits — one
//! renderer, two adapters, one picture.
//!
//! The dispatch is a single exhaustive match on the selected [`Sketch`]: each arm draws one piece.
//! All five are rasterised for real now — the plume, the squares, the fan, the orbits and the willow;
//! the gallery is complete and there are no placeholders left. Because the match is exhaustive,
//! adding a sketch to the running order forces a new arm here — a new piece cannot be silently left
//! undrawn.

use alloc::boxed::Box;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_core::{ScreenRotation, Tick};
use platform_display::SCREEN_SIZE;
use platform_numerics::SinTable;
use plume_core::phase;

use crate::canvas::Canvas;
use crate::frond::{FrondCompute, SerialFrond};
use crate::sketch::plume::ScreenPoint;
use crate::sketch::{fan, orbits, plume, squares, willow};
use crate::view::GalleryView;
use willow_core::Curtain;

/// The colour the plume's frond is drawn in — re-exported from the plume sketch so a caller
/// recolouring the frond has one place to look.
pub use crate::sketch::plume::PLUME_COLOUR;

/// The ground every sketch is drawn over: the panel's black. The canvas is flooded with this each
/// frame before the sketch plots into it, which is what makes the animation self-erasing.
pub const GROUND_COLOUR: Rgb565 = Rgb565::BLACK;

/// The canvas the gallery is drawn on at `rotation` — the panel's dimensions, swapped for a
/// quarter turn.
///
/// The gallery is a portrait picture, so a composition root pins it to a portrait rotation; but
/// the render honours whatever it is handed, laying the sketch into the matching canvas shape,
/// because the rotation is the render loop's to supply and not this crate's to assume.
pub const fn canvas_size(rotation: ScreenRotation) -> Size {
    match rotation {
        ScreenRotation::Deg0 | ScreenRotation::Deg180 => SCREEN_SIZE,
        ScreenRotation::Deg90 | ScreenRotation::Deg270 => {
            Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width)
        }
    }
}

/// The gallery renderer: the frond-compute port, held for the life of the app.
///
/// The port's capital (the sine table and the precomputed field it holds) lives on the **heap**, so
/// the renderer itself is a small handle — cheap to move into the display thread's closure. It holds
/// no frame buffer: the canvas is passed to [`paint_into`](Self::paint_into) each frame, so the one
/// full-screen buffer the app needs lives at the composition root (a single DMA buffer on the
/// device), never duplicated here. The plume's cloud is never buffered either: it streams through the
/// port straight into the canvas a point at a time.
pub struct Gallery {
    /// How the plume's point cloud is evaluated: [`SerialFrond`] on one core by default, or the
    /// firmware's two-core implementation when injected via [`with_frond`](Self::with_frond). Held
    /// for the life of the app because a parallel implementation owns a persistent worker.
    frond: Box<dyn FrondCompute>,
    /// The sine table the non-plume sketches read their trigonometry from, built once and shared by
    /// every sketch that breathes on a table sine (the squares' grid, the fan's fold, the willow's
    /// sway). The plume carries its own inside its [`frond`](Self::frond) port, which owns it for
    /// the worker thread; this is the one the render thread lends the rest. The orbits reads no
    /// table — its centres are a triangle wave — so it takes nothing from here.
    table: SinTable,
    /// The willow's phase-invariant curtain — its strands' anchors and its points' shapes — folded
    /// once at construction and swept each frame. Small fixed arrays, held here for the app's life so
    /// a willow frame pays only its sway, never the fold.
    curtain: Curtain,
}

impl Gallery {
    /// Build the renderer on the one-core default frond — the pure path every host renderer drives.
    pub fn new() -> Self {
        Self::with_frond(Box::new(SerialFrond::new()))
    }

    /// Build the renderer around a frond-compute port — the seam the firmware uses to inject its
    /// two-core evaluation — and the shared sine table the other sketches read.
    pub fn with_frond(frond: Box<dyn FrondCompute>) -> Self {
        Self {
            frond,
            table: SinTable::new(),
            curtain: Curtain::new(),
        }
    }

    /// Draw the selected sketch into `canvas`: flood the ground, then dispatch on the selected
    /// sketch to plot its picture.
    ///
    /// The canvas is the caller's — a host [`Frame`](crate::Frame) for the goldens, the firmware's
    /// wire-order DMA buffer on the glass. The renderer treats them alike: it resets and plots
    /// through the [`Canvas`] port and never learns which it holds. `elapsed` is the clock *since
    /// this sketch became current* — the render loop resets it on a switch because the view's
    /// [`anchor`](platform_core::Animated::anchor) is the sketch — so each piece animates from the
    /// start of its own motion.
    ///
    /// After this returns the caller shows the canvas its own way: the host blits it to a draw
    /// target, the device streams its bytes over DMA. Both drew the identical picture here; only the
    /// move to the glass differs.
    pub fn paint_into<C: Canvas>(
        &mut self,
        canvas: &mut C,
        view: GalleryView,
        elapsed: Tick,
        rotation: ScreenRotation,
    ) {
        let size: Size = canvas_size(rotation);
        canvas.reset(size, GROUND_COLOUR);

        use art_core::Sketch;
        match view.current() {
            Sketch::Plume => {
                // Evaluate and project the frond at this frame's phase through the port (one core or
                // two) and plot each drawable pixel as it arrives — streamed, never buffered.
                let t: f32 = phase(elapsed);
                self.frond.evaluate(t, size, &mut |point: ScreenPoint| {
                    plume::plot_screen(canvas, point)
                });
            }
            Sketch::Squares => squares::render(canvas, &self.table, elapsed, size),
            Sketch::Fan => fan::render(canvas, &self.table, elapsed, size),
            Sketch::Orbits => orbits::render(canvas, elapsed, size),
            Sketch::Willow => willow::render(canvas, &self.curtain, &self.table, elapsed, size),
        }
    }
}

impl Default for Gallery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use art_core::Sketch;
    use platform_display::testing::Framebuffer;
    use plume_core::FRAME_MS;

    /// The portrait quarter turn the gallery is pinned to on the board.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// Paint `sketch` at `elapsed` into a fresh portrait frame, then blit it — the whole host path:
    /// dispatch, canvas, blit, the same code every host renderer drives.
    fn painted(sketch: Sketch, elapsed: Tick) -> Framebuffer {
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        Gallery::new().paint_into(&mut frame, GalleryView::new(sketch), elapsed, PORTRAIT);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb
    }

    /// One: the plume, rendered through the gallery, puts ink on the glass.
    #[test]
    fn the_plume_paints_pixels() {
        assert!(painted(Sketch::Plume, 0).lit_pixels() > 0);
    }

    /// The whole canvas is painted every frame — sketch plus ground — which is what makes the blit
    /// one contiguous window and the animation self-erasing.
    #[test]
    fn every_pixel_is_painted_each_frame() {
        let canvas: Size = canvas_size(PORTRAIT);
        assert_eq!(
            painted(Sketch::Plume, 0).painted(),
            (canvas.width * canvas.height) as usize
        );
    }

    /// Nothing escapes the canvas at any selected sketch — the blit streams exactly the panel
    /// area, so no sketch's dispatch can drive a write past the edge.
    #[test]
    fn no_sketch_escapes_the_canvas() {
        for &sketch in &Sketch::ALL {
            assert_eq!(painted(sketch, 0).escaped(), 0, "{sketch:?}");
        }
    }

    /// Many: three phases of the plume paint three distinct pictures — the gallery actually
    /// animates the selected piece, it does not freeze it.
    #[test]
    fn the_plume_animates_through_the_gallery() {
        let a: Framebuffer = painted(Sketch::Plume, 0);
        let b: Framebuffer = painted(Sketch::Plume, 20 * FRAME_MS);
        let c: Framebuffer = painted(Sketch::Plume, 60 * FRAME_MS);
        assert_ne!(a.pixels(), b.pixels());
        assert_ne!(b.pixels(), c.pixels());
        assert_ne!(a.pixels(), c.pixels());
    }

    /// Two different selected sketches paint two different pictures — the dispatch actually
    /// switches what is drawn, so a button press changes the glass.
    #[test]
    fn selecting_a_different_sketch_changes_the_picture() {
        assert_ne!(
            painted(Sketch::Plume, 0).pixels(),
            painted(Sketch::Squares, 0).pixels(),
            "the dispatch did not switch pieces"
        );
    }

    /// A frame fully replaces the one before it: painting one sketch then another into the same
    /// canvas leaves exactly the second, not a smear of both — the self-erasing property that lets
    /// the gallery repaint forever with no clear and no ghosting, across a switch.
    #[test]
    fn a_frame_fully_replaces_the_one_before_it() {
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        let mut gallery: Gallery = Gallery::new();
        gallery.paint_into(&mut frame, GalleryView::new(Sketch::Squares), 0, PORTRAIT);
        gallery.paint_into(&mut frame, GalleryView::new(Sketch::Plume), 0, PORTRAIT);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        assert_eq!(
            fb.pixels(),
            painted(Sketch::Plume, 0).pixels(),
            "the previous sketch was not fully erased — the gallery would smear across a switch"
        );
    }
}
