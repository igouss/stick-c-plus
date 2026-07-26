//! The willow sketch: the curtain of tendrils, scaled onto the panel and plotted in its foliage
//! greens.
//!
//! The gallery's one original piece. The picture is a pure function of the elapsed clock:
//! [`willow_core::phase`] turns the clock into a sway phase, and [`willow_core::Curtain::sway`]
//! sweeps the curtain into a stream of [`StrandPoint`]s — each a point on a tendril, in normalised
//! `[0, 1]` fractions, with a depth from root to tip. This module scales each fraction onto the
//! panel and plots it in the green its depth names, deep at the root and light at the drooping tip.
//! Where the plume plots a projected field and the fan fills triangles, the willow plots a curtain
//! of points, bridging each to the next so the tendrils read as strands and not dotted lines.
//!
//! ## Fitting the curtain to the glass — nothing to fit
//!
//! Unlike the ported sketches, the willow is authored for no particular canvas: its points arrive in
//! `[0, 1]` fractions of the panel's width and height. So there is no square source to letterbox or
//! crop — the fractions scale straight onto whatever panel the render is handed, `x` by its width and
//! `y` by its height, and the aspect is applied here rather than assumed in the domain. A tip the
//! sway carries a little past the edge is clipped by [`Canvas::set`]; nothing else falls off.

use embedded_graphics::prelude::Size;
use willow_core::{phase, Curtain, SinTable, StrandPoint};

use crate::canvas::Canvas;
use crate::colour::blend;
use platform_core::Tick;

/// The tendril colour at its **root**: a deep willow green. `(34, 74, 28)` on `[0, 255]`, as `[0, 1]`
/// fractions — the shaded top of the curtain.
const ROOT_GREEN: (f32, f32, f32) = (0.133, 0.290, 0.110);

/// The tendril colour at its **tip**: a light yellow-green. `(165, 210, 95)` on `[0, 255]`, as
/// `[0, 1]` fractions — the sunlit drooping ends. A point's depth ramps between these two.
const TIP_GREEN: (f32, f32, f32) = (0.647, 0.824, 0.373);

/// Draw the willow at `elapsed` into `canvas`, which the caller has reset to the ground.
///
/// Sweeps [`willow_core`]'s curtain — its phase-invariant shape folded once, held by the gallery —
/// and plots each point, scaled from its `[0, 1]` fractions onto the panel and coloured by its depth.
/// Generic over the [`Canvas`], so the willow lands identically in a host [`Frame`](crate::Frame) for
/// the goldens and in the firmware's wire-order buffer on the glass. Each point is bridged one pixel
/// down to the next so a near-vertical tendril draws solid rather than dotted; [`Canvas::set`] clips
/// whatever the sway carries off the edge.
pub fn render<C: Canvas>(
    canvas: &mut C,
    curtain: &Curtain,
    table: &SinTable,
    elapsed: Tick,
    size: Size,
) {
    let phi: f32 = phase(elapsed);
    let width: f32 = size.width as f32;
    let height: f32 = size.height as f32;
    curtain.sway(phi, table).for_each(|point: StrandPoint| {
        let px: i32 = libm::roundf(point.x * width) as i32;
        let py: i32 = libm::roundf(point.y * height) as i32;
        let colour = blend(ROOT_GREEN, TIP_GREEN, point.depth);
        // The point, and a bridge one pixel down to close the gap to the next point on the strand —
        // the tendrils hang near-vertical, so a one-pixel vertical run draws them solid.
        canvas.set(px, py, colour);
        canvas.set(px, py + 1, colour);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use crate::gallery::{canvas_size, GROUND_COLOUR};
    use embedded_graphics::pixelcolor::Rgb565;
    use embedded_graphics::prelude::RgbColor;
    use platform_core::ScreenRotation;
    use platform_display::testing::Framebuffer;

    /// The portrait quarter turn the gallery is pinned to.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// Blit the willow at `elapsed` into a fresh portrait framebuffer — the whole host path.
    fn painted(elapsed: Tick) -> Framebuffer {
        let table: SinTable = SinTable::new();
        let curtain: Curtain = Curtain::new();
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        frame.reset(canvas, GROUND_COLOUR);
        render(&mut frame, &curtain, &table, elapsed, canvas);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb
    }

    /// One: the curtain puts ink on the glass — the tendrils are plotted, not skipped.
    #[test]
    fn the_curtain_paints_pixels() {
        assert!(painted(0).lit_pixels() > 0);
    }

    /// The willow is **green**: every lit pixel has more green than red and more green than blue — a
    /// foliage cast, never the plume's cyan-white or the fan's full spectrum. A colour leak would
    /// trip this.
    #[test]
    fn every_lit_pixel_is_green() {
        let fb: Framebuffer = painted(0);
        assert!(
            fb.pixels()
                .iter()
                .filter(|&&c: &&Rgb565| c != Rgb565::BLACK)
                // Green field is six bits (0..63), red and blue five (0..31); halve green to compare
                // on the same scale, so the test asks a true "greener than", not a wider-field
                // artefact.
                .all(|c: &Rgb565| c.g() / 2 >= c.r() && c.g() / 2 >= c.b()),
            "a lit pixel was not green-dominant"
        );
    }

    /// Nothing the curtain draws escapes the canvas — a tip the sway carries past the edge is clipped
    /// to the panel.
    #[test]
    fn nothing_escapes_the_canvas() {
        assert_eq!(painted(willow_core::PERIOD_MS / 4).escaped(), 0);
    }

    /// Many: three moments of the willow paint three distinct pictures — the curtain actually sways,
    /// it does not freeze.
    #[test]
    fn the_curtain_sways_over_time() {
        let a: Framebuffer = painted(0);
        let b: Framebuffer = painted(willow_core::PERIOD_MS / 4);
        let c: Framebuffer = painted(willow_core::PERIOD_MS / 2);
        assert_ne!(a.pixels(), b.pixels());
        assert_ne!(b.pixels(), c.pixels());
        assert_ne!(a.pixels(), c.pixels());
    }
}
