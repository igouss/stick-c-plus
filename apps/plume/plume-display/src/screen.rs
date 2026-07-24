//! The plume screen: plot the frond of the moment into the frame, then blit the frame.
//!
//! Device-independent by construction — it draws into any [`DrawTarget`], so the on-target
//! panel and a host framebuffer render *the same code*. The render owns two things across
//! frames: the [`SinTable`] the field's trigonometry is read from (built once, at
//! construction), and the [`Canvas`] the dots are plotted into (cleared and refilled each
//! frame). Both are held rather than rebuilt because building either every frame is exactly
//! the cost the design removes.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_core::{ScreenRotation, Tick};
use platform_display::{RenderError, SCREEN_SIZE};
use plume_core::{phase, plume, SinTable};

use crate::canvas::Canvas;
use crate::project::project;
use crate::view::PlumeView;

/// The colour the lit frond is drawn in. White, faithful to the source's translucent-white
/// dots — but a single named constant, so the frond recolours with one edit and no change to a
/// pixel of the frame beneath it.
pub const PLUME_COLOUR: Rgb565 = Rgb565::WHITE;

/// The colour behind the frond: the panel's black ground.
pub const GROUND_COLOUR: Rgb565 = Rgb565::BLACK;

/// The canvas the plume is drawn on at `rotation` — the panel's dimensions, swapped for a
/// quarter turn.
///
/// The plume is a portrait picture, so a composition root pins it to a portrait rotation; but
/// the render honours whatever it is handed, laying the frond into the matching canvas shape,
/// because the rotation is the render loop's to supply and not this crate's to assume.
pub const fn canvas_size(rotation: ScreenRotation) -> Size {
    match rotation {
        ScreenRotation::Deg0 | ScreenRotation::Deg180 => SCREEN_SIZE,
        ScreenRotation::Deg90 | ScreenRotation::Deg270 => {
            Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width)
        }
    }
}

/// The plume renderer: the sine table and the offscreen frame, held for the life of the app.
///
/// Both the table's samples and the frame's bits live on the **heap** (each owns a `Vec`), so
/// the renderer itself is a small handle — cheap to move into the display thread's closure, and
/// with no large buffer ever placed on a stack, which on bring-up would overflow it.
pub struct Plume {
    /// The startup-built trigonometry the field is evaluated through.
    table: SinTable,
    /// The one-bit frame the frond is plotted into and blitted from.
    canvas: Canvas,
}

impl Plume {
    /// Build the renderer: pay for the sine table once, here, and never again.
    pub fn new() -> Self {
        Self {
            table: SinTable::new(),
            canvas: Canvas::new(),
        }
    }

    /// Render the frond for the state that has been current for `elapsed` milliseconds.
    ///
    /// The state itself ([`PlumeView`]) carries nothing — the picture is a pure function of the
    /// elapsed clock through [`phase`], so the frond of this instant is decided by the time
    /// alone. The sequence is: clear the frame, plot every projected point of the frond into it
    /// (a fat point lighting its neighbour too), then blit the whole frame in one window.
    pub fn render<D>(
        &mut self,
        target: &mut D,
        _view: PlumeView,
        elapsed: Tick,
        rotation: ScreenRotation,
    ) -> Result<(), RenderError<D::Error>>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let canvas: Size = canvas_size(rotation);
        self.canvas.reset(canvas);

        let t: f32 = phase(elapsed);
        for point in plume(t, &self.table) {
            if let Some((x, y)) = project(point, canvas) {
                self.canvas.set(x, y);
                // A fat point is the original's two-pixel dot — the barbs' bright spine.
                if point.wide {
                    self.canvas.set(x + 1, y);
                }
            }
        }

        self.canvas.blit(target, PLUME_COLOUR, GROUND_COLOUR)
    }
}

impl Default for Plume {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_display::testing::Framebuffer;
    use plume_core::FRAME_MS;

    /// The portrait quarter turn the plume is pinned to on the board.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// A framebuffer shaped for `rotation` — the only canvas an escape count means anything
    /// against.
    fn canvas_at(rotation: ScreenRotation) -> Framebuffer {
        Framebuffer::sized(canvas_size(rotation))
    }

    /// Paint the frond at `elapsed` into a fresh portrait framebuffer.
    fn painted(elapsed: Tick) -> Framebuffer {
        painted_at(elapsed, PORTRAIT)
    }

    /// Paint the frond at `elapsed` and `rotation` into a fresh matching framebuffer.
    fn painted_at(elapsed: Tick, rotation: ScreenRotation) -> Framebuffer {
        let mut fb: Framebuffer = canvas_at(rotation);
        Plume::new()
            .render(&mut fb, PlumeView, elapsed, rotation)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: an unrendered canvas is blank — the baseline every other assertion rests on.
    #[test]
    fn an_unrendered_canvas_is_blank() {
        assert_eq!(Framebuffer::new().lit_pixels(), 0);
    }

    /// One: a rendered frond puts ink on the glass.
    #[test]
    fn a_rendered_frond_paints_pixels() {
        assert!(painted(0).lit_pixels() > 0);
    }

    /// The whole canvas is painted every frame — lit frond plus black ground — which is what
    /// makes the blit one contiguous window and the animation self-erasing.
    #[test]
    fn every_pixel_is_painted_each_frame() {
        let canvas: Size = canvas_size(PORTRAIT);
        assert_eq!(
            painted(0).painted(),
            (canvas.width * canvas.height) as usize
        );
    }

    /// Many: three phases of the breath paint three distinct fronds — the animation actually
    /// moves.
    #[test]
    fn three_phases_paint_three_distinct_fronds() {
        let a: Framebuffer = painted(0);
        let b: Framebuffer = painted(20 * FRAME_MS);
        let c: Framebuffer = painted(60 * FRAME_MS);
        assert_ne!(a.pixels(), b.pixels());
        assert_ne!(b.pixels(), c.pixels());
        assert_ne!(a.pixels(), c.pixels());
    }

    /// Nothing escapes the canvas: every frond point either lands on the panel or is clipped by
    /// the canvas, at several phases, so a barb tip flung off the edge never becomes an
    /// out-of-bounds draw.
    #[test]
    fn no_frond_escapes_the_canvas() {
        (0..8u64).for_each(|frame: u64| {
            assert_eq!(painted(frame * 7 * FRAME_MS).escaped(), 0);
        });
    }

    /// The animation erases itself: painting one frame and then another over it leaves exactly
    /// the second frame, not a smear of both. This is what lets the plume repaint forever with
    /// no clear and no ghosting.
    #[test]
    fn a_frame_fully_replaces_the_one_before_it() {
        let later: Tick = 30 * FRAME_MS;

        let mut reused: Framebuffer = canvas_at(PORTRAIT);
        let mut plume: Plume = Plume::new();
        plume
            .render(&mut reused, PlumeView, 0, PORTRAIT)
            .expect("the first frond");
        plume
            .render(&mut reused, PlumeView, later, PORTRAIT)
            .expect("the second frond over it");

        assert_eq!(
            reused.pixels(),
            painted(later).pixels(),
            "the previous frond was not fully erased — the animation would smear"
        );
    }

    /// The frond breathes on a period and returns: a full period later is the same picture, the
    /// display-side proof of the phase's wrap.
    #[test]
    fn a_full_period_returns_to_the_same_frond() {
        let period_ms: Tick = plume_core::PERIOD_FRAMES * FRAME_MS;
        assert_eq!(painted(0).pixels(), painted(period_ms).pixels());
    }

    /// The picture is a function of the elapsed clock, not of the wall clock: two renders at the
    /// same elapsed time are identical however far apart they are asked.
    #[test]
    fn the_same_elapsed_paints_the_same_frond() {
        assert_eq!(
            painted(13 * FRAME_MS).pixels(),
            painted(13 * FRAME_MS).pixels()
        );
    }
}
