//! The gallery renderer: reset the frame, draw the selected sketch into it, blit it once.
//!
//! Device-independent by construction — it draws into any [`DrawTarget`], so the on-target panel
//! and a host framebuffer render *the same code*. The renderer owns two things across frames: the
//! [`SinTable`] every sketch reads its trigonometry from (built once, at construction), and the
//! [`Frame`] each sketch is plotted into (reset and refilled each frame). Both are held rather
//! than rebuilt because building either every frame is exactly the cost the design removes.
//!
//! The dispatch is a single exhaustive match on the selected [`Sketch`]: each arm draws one piece.
//! Today only the plume is rasterised for real; the other four draw their honest
//! [`placeholder`](crate::sketch::placeholder). Because the match is exhaustive, adding a sketch
//! to the running order forces a new arm here — a new piece cannot be silently left undrawn.

use alloc::boxed::Box;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_core::{ScreenRotation, Tick};
use platform_display::{RenderError, SCREEN_SIZE};
use plume_core::{phase, FieldPoint};

use crate::frame::Frame;
use crate::frond::{FrondCompute, SerialFrond};
use crate::sketch::{placeholder, plume};
use crate::view::GalleryView;

/// The colour the plume's frond is drawn in — re-exported from the plume sketch so a caller
/// recolouring the frond has one place to look.
pub use crate::sketch::plume::PLUME_COLOUR;

/// The ground every sketch is drawn over: the panel's black. The frame is flooded with this each
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

/// The gallery renderer: the frond-compute port and the offscreen frame, both held for the life of
/// the app.
///
/// The port's capital (the sine table and the precomputed field it holds) and the frame's pixels
/// live on the **heap** (each owns a `Vec`), so the renderer itself is a small handle — cheap to
/// move into the display thread's closure, and with no large buffer ever placed on a stack, which
/// on bring-up would overflow it. The plume's cloud is never buffered here: it streams through the
/// port straight into the frame a point at a time (see [`paint`](Self::paint)).
pub struct Gallery {
    /// How the plume's point cloud is evaluated: [`SerialFrond`] on one core by default, or the
    /// firmware's two-core implementation when injected via [`with_frond`](Self::with_frond). Held
    /// for the life of the app because a parallel implementation owns a persistent worker.
    frond: Box<dyn FrondCompute>,
    /// The frame the selected sketch is plotted into and blitted from.
    frame: Frame,
}

impl Gallery {
    /// Build the renderer on the one-core default frond — the pure path every host renderer drives.
    pub fn new() -> Self {
        Self::with_frond(|| Box::new(SerialFrond::new()))
    }

    /// Build the renderer around a frond-compute port — the seam the firmware uses to inject its
    /// two-core evaluation.
    ///
    /// The port is built by the `frond` closure *after* the frame buffer, deliberately: the frame
    /// is one of the two full-screen buffers the app needs a contiguous run for, and on the ESP32's
    /// pool-fragmented ~300 KiB heap the big buffers must claim their runs before the smaller
    /// scattered allocations (the field, a parallel frond's worker buffer) carve the pools up. So
    /// the frame is allocated first, then the frond.
    pub fn with_frond(frond: impl FnOnce() -> Box<dyn FrondCompute>) -> Self {
        let frame: Frame = Frame::new();
        let frond: Box<dyn FrondCompute> = frond();
        Self { frond, frame }
    }

    /// Draw the selected sketch into the offscreen frame and return it as a contiguous
    /// [`Rgb565`] slice — the picture, computed but not yet on any glass.
    ///
    /// The sequence is: flood the frame with the ground, then dispatch on the selected sketch to
    /// plot its picture into the frame. `elapsed` is the clock *since this sketch became current*
    /// — the render loop resets it on a switch because the view's
    /// [`anchor`](platform_core::Animated::anchor) is the sketch — so each piece animates from the
    /// start of its own motion.
    ///
    /// Split from the blit so a panel adapter can take these pixels and push them to the glass
    /// itself (a DMA burst), while the host renderers go on through [`render`](Self::render). Both
    /// paths compute the identical frame here; only the final move to the glass differs.
    pub fn paint(
        &mut self,
        view: GalleryView,
        elapsed: Tick,
        rotation: ScreenRotation,
    ) -> &[Rgb565] {
        let canvas: Size = canvas_size(rotation);
        self.frame.reset(canvas, GROUND_COLOUR);

        use art_core::Sketch;
        match view.current() {
            Sketch::Plume => {
                // Evaluate the frond at this frame's phase through the port (one core or two) and
                // plot each point as it arrives — streamed, never buffered. `frame` is reborrowed
                // as a disjoint field so the plot closure and the frond borrow do not contend.
                let t: f32 = phase(elapsed);
                let frame: &mut Frame = &mut self.frame;
                self.frond.evaluate(t, &mut |point: FieldPoint| {
                    plume::plot_point(frame, point, canvas)
                });
            }
            Sketch::Squares => placeholder::render(&mut self.frame, Sketch::Squares, canvas),
            Sketch::Fan => placeholder::render(&mut self.frame, Sketch::Fan, canvas),
            Sketch::Orbits => placeholder::render(&mut self.frame, Sketch::Orbits, canvas),
            Sketch::Willow => placeholder::render(&mut self.frame, Sketch::Willow, canvas),
        }

        self.frame.pixels()
    }

    /// Render the selected sketch and blit the whole frame to `target` in one window.
    ///
    /// [`paint`](Self::paint) then [`blit`](Frame::blit): the device-independent path every host
    /// renderer (the screenshots, the goldens) drives, drawing the same code the panel does.
    pub fn render<D>(
        &mut self,
        target: &mut D,
        view: GalleryView,
        elapsed: Tick,
        rotation: ScreenRotation,
    ) -> Result<(), RenderError<D::Error>>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        // Compute into the frame; the returned borrow ends at the semicolon, freeing the frame
        // for the blit below.
        let _ = self.paint(view, elapsed, rotation);
        self.frame.blit(target)
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
    use art_core::Sketch;
    use platform_display::testing::Framebuffer;
    use plume_core::FRAME_MS;

    /// The portrait quarter turn the gallery is pinned to on the board.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// Paint `sketch` at `elapsed` into a fresh portrait framebuffer, through the whole gallery
    /// path — dispatch, frame, blit — the same path the panel adapter drives.
    fn painted(sketch: Sketch, elapsed: Tick) -> Framebuffer {
        let canvas: Size = canvas_size(PORTRAIT);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        Gallery::new()
            .render(&mut fb, GalleryView::new(sketch), elapsed, PORTRAIT)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// One: the plume, rendered through the gallery, puts ink on the glass.
    #[test]
    fn the_plume_paints_pixels() {
        assert!(painted(Sketch::Plume, 0).lit_pixels() > 0);
    }

    /// The `paint` seam returns exactly the canvas the `render`/blit path streams — same pixels,
    /// same order — so the panel adapter's direct DMA burst and the host blit stay one picture.
    #[test]
    fn paint_returns_the_pixels_the_blit_would_stream() {
        let canvas: Size = canvas_size(PORTRAIT);
        let mut gallery: Gallery = Gallery::new();
        let direct: alloc::vec::Vec<Rgb565> = gallery
            .paint(GalleryView::new(Sketch::Plume), 0, PORTRAIT)
            .to_vec();

        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        gallery
            .render(&mut fb, GalleryView::new(Sketch::Plume), 0, PORTRAIT)
            .expect("a framebuffer render cannot fail");

        assert_eq!(
            direct.len(),
            (canvas.width * canvas.height) as usize,
            "paint returns the whole canvas"
        );
        assert_eq!(
            fb.pixels(),
            direct.as_slice(),
            "the blit streamed a different picture than paint returned"
        );
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

    /// A frame fully replaces the one before it: rendering one sketch then another into the same
    /// buffer leaves exactly the second, not a smear of both — the self-erasing property that lets
    /// the gallery repaint forever with no clear and no ghosting, across a switch.
    #[test]
    fn a_frame_fully_replaces_the_one_before_it() {
        let canvas: Size = canvas_size(PORTRAIT);
        let mut reused: Framebuffer = Framebuffer::sized(canvas);
        let mut gallery: Gallery = Gallery::new();
        gallery
            .render(&mut reused, GalleryView::new(Sketch::Squares), 0, PORTRAIT)
            .expect("the placeholder");
        gallery
            .render(&mut reused, GalleryView::new(Sketch::Plume), 0, PORTRAIT)
            .expect("the frond over it");
        assert_eq!(
            reused.pixels(),
            painted(Sketch::Plume, 0).pixels(),
            "the previous sketch was not fully erased — the gallery would smear across a switch"
        );
    }
}
