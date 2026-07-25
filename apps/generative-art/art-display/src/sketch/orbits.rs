//! The orbits sketch: the comet of diamond blooms, projected onto the panel and filled cell by cell
//! in its greys.
//!
//! A faithful move of the source *Dwitter* onto the glass. The picture is a pure function of the
//! elapsed clock: [`orbits_core::frame`] turns the clock into a virtual frame, and
//! [`orbits_core::for_each_cell`] walks the source's `50×50` grid of cells, handing each its grey —
//! the brightest diamond over it, grained by a fixed texture. This module lands each cell on the
//! panel as the source draws it: a filled `10×10` source-square in a neutral grey. Where the plume
//! plots points and the fan fills triangles, the orbits tiles the canvas with axis-aligned
//! rectangles, so it carries a small rectangle fill.
//!
//! ## Fitting the grid to the glass
//!
//! The orbits fills its whole square canvas, so — like the fan — the projection is a **fill-height**
//! scale: the `500`-tall source mapped to the panel's `240` rows, which crops the width to the
//! panel's `135` and keeps the cells square rather than squashing the whole square into a portrait
//! letterbox. The comet sweeps the entire canvas over its cycle, so the crop only ever hides part of
//! its travel, never the comet itself for long. A cell no diamond reaches has a grey of exactly
//! zero; since the canvas was already flooded with the ground, those cells are **skipped** rather
//! than painted black — which leaves the fill touching only the lit part of the frame, a small
//! fraction of the grid, and is what keeps the expensive-looking sketch cheap on the glass.

use core::ops::Range;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::Size;
use orbits_core::{for_each_cell, frame, COLS, STEP};

use crate::canvas::Canvas;
use crate::colour::grey_to_rgb565;
use platform_core::Tick;

/// How the source's `500×500` canvas lands on a `canvas`-shaped panel: a uniform fill-height scale
/// and the offset that centres the source square on the panel.
///
/// The scale is the panel height over the source size, so the full height is used and the width is
/// cropped. The offset places the source's centre at the panel's middle — vertically that is exact
/// (the source maps `[0, SOURCE]` onto `[0, height]`), and horizontally it slides the
/// wider-than-panel row of cells so its middle sits centred.
struct Projection {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

impl Projection {
    /// Derive the fill-height projection for a `canvas`-shaped panel.
    fn for_canvas(canvas: Size) -> Self {
        let scale: f32 = canvas.height as f32 / orbits_core::SOURCE;
        Self {
            scale,
            offset_x: canvas.width as f32 / 2.0 - orbits_core::SOURCE / 2.0 * scale,
            offset_y: canvas.height as f32 / 2.0 - orbits_core::SOURCE / 2.0 * scale,
        }
    }

    /// Where a source-space x lands on the panel, rounded to the nearest pixel.
    fn px(&self, source_x: f32) -> i32 {
        libm::roundf(source_x * self.scale + self.offset_x) as i32
    }

    /// Where a source-space y lands on the panel, rounded to the nearest pixel.
    fn py(&self, source_y: f32) -> i32 {
        libm::roundf(source_y * self.scale + self.offset_y) as i32
    }

    /// The band of grid columns whose cells land on the `canvas`-wide panel — the crop's saving.
    ///
    /// The fill-height projection crops only the width, so every row is on the panel but only a band
    /// of columns is; inverting the projection at the panel's left and right edges gives the
    /// source-x window on view, and dividing by [`STEP`] the columns that touch it. Computed once per
    /// frame and handed to the walk, so the bloom is paid only for cells that can be seen — the
    /// off-panel columns, nearly half the grid, are never evaluated. Widened by a column at each end
    /// (`floor` on the left, `+ 1` on the right) so a cell straddling an edge is included; the walk
    /// clamps the band to the grid.
    fn visible_columns(&self, canvas: Size) -> Range<u16> {
        let left: f32 = -self.offset_x / self.scale; // source x at panel x = 0
        let right: f32 = (canvas.width as f32 - self.offset_x) / self.scale; // source x at the edge
        let lo: i32 = libm::floorf(left / STEP as f32) as i32;
        let hi: i32 = libm::floorf(right / STEP as f32) as i32 + 1;
        lo.clamp(0, COLS as i32) as u16..hi.clamp(0, COLS as i32) as u16
    }
}

/// Draw the orbits at `elapsed` into `canvas`, which the caller has reset to the ground.
///
/// Walks [`orbits_core`]'s grid — the thirty orbit centres hoisted out once per frame — and fills
/// each lit cell's projected rectangle in its grey. Generic over the [`Canvas`], so the orbits lands
/// identically in a host [`Frame`](crate::Frame) for the goldens and in the firmware's wire-order
/// buffer on the glass. A cell whose grey is zero is left as the flooded ground; the rest are
/// filled in source draw order, so overlapping cells layer as the source layers them.
pub fn render<C: Canvas>(canvas: &mut C, elapsed: Tick, size: Size) {
    let frame: f32 = frame(elapsed);
    let projection: Projection = Projection::for_canvas(size);
    let columns: Range<u16> = projection.visible_columns(size);
    for_each_cell(frame, columns, |col: u16, row: u16, grey: f32| {
        if grey <= 0.0 {
            return; // an unreached cell stays the ground — no black rectangle to paint.
        }
        let x0: i32 = projection.px((col * STEP) as f32);
        let x1: i32 = projection.px(((col + 1) * STEP) as f32);
        let y0: i32 = projection.py((row * STEP) as f32);
        let y1: i32 = projection.py(((row + 1) * STEP) as f32);
        fill_rect(canvas, x0, y0, x1, y1, grey_to_rgb565(grey));
    });
}

/// Fill the axis-aligned rectangle `[x0, x1) × [y0, y1)` in `colour`.
///
/// Adjacent cells share an edge — cell `col`'s right pixel is cell `col + 1`'s left, both rounded
/// from the same source coordinate — so the grid tiles the panel with no seam and no overlap. Each
/// pixel goes through [`Canvas::set`], which clips anything off the canvas, so a cell the fill-height
/// crop sends past the edge simply draws the part that lands.
fn fill_rect<C: Canvas>(canvas: &mut C, x0: i32, y0: i32, x1: i32, y1: i32, colour: Rgb565) {
    for y in y0..y1 {
        for x in x0..x1 {
            canvas.set(x, y, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use crate::gallery::{canvas_size, GROUND_COLOUR};
    use embedded_graphics::pixelcolor::RgbColor;
    use platform_core::ScreenRotation;
    use platform_display::testing::Framebuffer;

    /// The portrait quarter turn the gallery is pinned to.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// A moment with the comet's head well inside the crop — a representative still, not the sparse
    /// origin frame.
    const LIT_MS: Tick = 700;

    /// Blit the orbits at `elapsed` into a fresh portrait framebuffer — the whole host path.
    fn painted(elapsed: Tick) -> Framebuffer {
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        frame.reset(canvas, GROUND_COLOUR);
        render(&mut frame, elapsed, canvas);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb
    }

    /// One: the comet puts ink on the glass — the lit cells are filled, not skipped.
    #[test]
    fn the_comet_paints_pixels() {
        assert!(painted(LIT_MS).lit_pixels() > 0);
    }

    /// The orbits is a **grey** sketch: every lit pixel is a neutral grey, red equal to blue, with
    /// no colour cast — the monochrome ramp the fan's radial spectrum could never be. A colour leak
    /// (a hue where a brightness belongs) would trip this.
    #[test]
    fn every_lit_pixel_is_grey() {
        let fb: Framebuffer = painted(LIT_MS);
        assert!(
            fb.pixels()
                .iter()
                .filter(|&&c: &&Rgb565| c != Rgb565::BLACK)
                .all(|c: &Rgb565| c.r() == c.b()),
            "a lit pixel wore a colour cast"
        );
    }

    /// Nothing the orbits draws escapes the canvas — every projected cell, including the ones the
    /// fill-height crop sends off the edge, is clipped to the panel.
    #[test]
    fn nothing_escapes_the_canvas() {
        assert_eq!(painted(LIT_MS).escaped(), 0);
    }

    /// Many: three moments of the orbits paint three distinct pictures — the comet actually drifts,
    /// the sketch does not freeze.
    #[test]
    fn the_comet_drifts_over_time() {
        let a: Framebuffer = painted(0);
        let b: Framebuffer = painted(LIT_MS);
        let c: Framebuffer = painted(2 * LIT_MS);
        assert_ne!(a.pixels(), b.pixels());
        assert_ne!(b.pixels(), c.pixels());
        assert_ne!(a.pixels(), c.pixels());
    }
}
