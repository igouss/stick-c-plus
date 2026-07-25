//! The fan sketch: the folding triangles, projected onto the panel and filled in their colours.
//!
//! A faithful move of the source *Dwitter* onto the glass. The picture is a pure function of the
//! elapsed clock: [`fan_core::phase`] turns the clock into a phase, [`fan_core::cells`] turns the
//! phase into the folding triangles in the source's `500×500` space, and this module lands each on
//! the panel and fills it in the colour its hue names. Where the plume plots single points and the
//! squares fill axis-aligned rectangles, the fan fills arbitrary triangles — so it carries its own
//! small triangle rasteriser.
//!
//! ## Fitting the fan to the glass
//!
//! The fan fills its whole square canvas, and its distinguished point is the centre it radiates
//! from. So the projection is a **fill-height** scale — the `500`-tall source mapped to the panel's
//! `240` rows — which crops the width to the panel's `135`, keeping the red core centred and the
//! columns running the full height. Half the source's triangles fall outside the panel and are
//! clipped; the fan is cheap enough (a few hundred triangles) that computing the cropped ones costs
//! nothing worth saving, and cropping is what keeps the on-panel triangles at the source's
//! proportions rather than squeezing the whole square into a portrait letterbox. The centre maps to
//! the panel centre exactly, since the source centre is half its size and the scale is the panel
//! height over that size.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::Size;
use fan_core::{cells, phase, Cell, SinTable, CENTRE, SOURCE};

use crate::canvas::Canvas;
use crate::colour::hsv_to_rgb565;
use platform_core::Tick;

/// How the source's `500×500` canvas lands on a `canvas`-shaped panel: a uniform fill-height scale
/// and the offset that centres the source centre on the panel.
///
/// The scale is the panel height over the source size, so the full height is used and the width is
/// cropped. The offset places the source's [`CENTRE`] at the panel's middle — vertically that is
/// exact (the centre is half the source and the scale is the panel height over the source), and
/// horizontally it slides the wider-than-panel row of columns so its middle sits centred.
struct Projection {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

impl Projection {
    /// Derive the fill-height projection for a `canvas`-shaped panel.
    fn for_canvas(canvas: Size) -> Self {
        let scale: f32 = canvas.height as f32 / SOURCE;
        Self {
            scale,
            offset_x: canvas.width as f32 / 2.0 - CENTRE * scale,
            offset_y: canvas.height as f32 / 2.0 - CENTRE * scale,
        }
    }

    /// Where a source-space vertex lands on the panel, rounded to the nearest pixel.
    fn apply(&self, vertex: (f32, f32)) -> (i32, i32) {
        (
            libm::roundf(vertex.0 * self.scale + self.offset_x) as i32,
            libm::roundf(vertex.1 * self.scale + self.offset_y) as i32,
        )
    }
}

/// Draw the fan at `elapsed` into `canvas`, which the caller has reset to the ground.
///
/// Projects each of [`fan_core`]'s folding triangles onto the panel and fills it in the colour its
/// hue names. Generic over the [`Canvas`], so the fan lands identically in a host
/// [`Frame`](crate::Frame) for the goldens and in the firmware's wire-order buffer on the glass.
/// The triangles arrive in the source's draw order and are filled as they arrive, so an overlapping
/// pair layers exactly as the source layers it.
pub fn render<C: Canvas>(canvas: &mut C, table: &SinTable, elapsed: Tick, size: Size) {
    let phi: f32 = phase(elapsed);
    let projection: Projection = Projection::for_canvas(size);
    cells(phi, table).for_each(|cell: Cell| {
        let colour: Rgb565 = hsv_to_rgb565(cell.hue);
        let verts: [(i32, i32); 3] = [
            projection.apply(cell.verts[0]),
            projection.apply(cell.verts[1]),
            projection.apply(cell.verts[2]),
        ];
        fill_triangle(canvas, verts, size, colour);
    });
}

/// The doubled signed area of triangle `a b c` — positive for one winding, negative for the other,
/// zero when the three are colinear. Also the edge function that decides which side of edge `a→b`
/// a point `p` lies on, and it steps by a constant in `x` and in `y`, which is what lets the fill
/// walk it by addition rather than recompute it per pixel.
fn edge(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> i32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Fill the triangle `verts` in `colour`, clipped to the `size`-shaped canvas.
///
/// A half-plane fill walked by **integer addition only** — no multiply, divide, or `roundf` in the
/// loop, all of which are slow on this soft-divide FPU (a per-row `f32` scanline fill measured
/// *slower* than this despite touching fewer pixels). The three edge functions are linear, so each
/// steps by a constant across a row and down a column; the fill seeds them at the bounding box's
/// corner and increments. A pixel is inside when all three are non-negative, tested in one branch by
/// OR-ing them and checking the sign bit. The triangle is first wound so its interior is the
/// non-negative side. The scan is the bounding box **clamped to the canvas**, so a triangle the
/// fill-height crop throws off the panel costs only the pixels it covers on-glass; a degenerate
/// (colinear) triangle has zero area and draws nothing — the source's fully-shut fold.
fn fill_triangle<C: Canvas>(canvas: &mut C, verts: [(i32, i32); 3], size: Size, colour: Rgb565) {
    let [a, b, c]: [(i32, i32); 3] = verts;
    let area: i32 = edge(a, b, c);
    if area == 0 {
        return; // colinear — a shut fold, nothing to fill.
    }
    // Wind counter-clockwise (positive area) so a point is inside when every edge function is
    // non-negative; swapping two vertices flips the winding.
    let [a, b, c]: [(i32, i32); 3] = if area < 0 { [a, c, b] } else { [a, b, c] };

    let min_x: i32 = a.0.min(b.0).min(c.0).max(0);
    let max_x: i32 = a.0.max(b.0).max(c.0).min(size.width as i32 - 1);
    let min_y: i32 = a.1.min(b.1).min(c.1).max(0);
    let max_y: i32 = a.1.max(b.1).max(c.1).min(size.height as i32 - 1);
    if min_x > max_x || min_y > max_y {
        return; // wholly off the panel after clipping.
    }

    // Each edge function's step per pixel across (`_dx`) and down (`_dy`), and its value at the
    // clamped top-left corner. `edge(u, v, p)` increases by `u.y - v.y` per `+x` and by `v.x - u.x`
    // per `+y`, so a whole triangle is filled with adds after these three seeds.
    let corner: (i32, i32) = (min_x, min_y);
    let (mut w0_row, w0_dx, w0_dy): (i32, i32, i32) = (edge(b, c, corner), b.1 - c.1, c.0 - b.0);
    let (mut w1_row, w1_dx, w1_dy): (i32, i32, i32) = (edge(c, a, corner), c.1 - a.1, a.0 - c.0);
    let (mut w2_row, w2_dx, w2_dy): (i32, i32, i32) = (edge(a, b, corner), a.1 - b.1, b.0 - a.0);

    for y in min_y..=max_y {
        let (mut w0, mut w1, mut w2): (i32, i32, i32) = (w0_row, w1_row, w2_row);
        for x in min_x..=max_x {
            // Inside when none of the three edge functions is negative: OR-ing them puts a set sign
            // bit iff any is negative, so one non-negative test covers all three.
            if (w0 | w1 | w2) >= 0 {
                canvas.set(x, y, colour);
            }
            w0 += w0_dx;
            w1 += w1_dx;
            w2 += w2_dx;
        }
        w0_row += w0_dy;
        w1_row += w1_dy;
        w2_row += w2_dy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use crate::gallery::{canvas_size, GROUND_COLOUR};
    use alloc::collections::BTreeSet;
    use embedded_graphics::pixelcolor::RgbColor;
    use platform_core::ScreenRotation;
    use platform_display::testing::Framebuffer;

    /// The portrait quarter turn the gallery is pinned to.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// Blit the fan at `elapsed` into a fresh portrait framebuffer — the whole host path.
    fn painted(elapsed: Tick) -> Framebuffer {
        let table: SinTable = SinTable::new();
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        frame.reset(canvas, GROUND_COLOUR);
        render(&mut frame, &table, elapsed, canvas);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb
    }

    /// The number of distinct non-black colours in a framebuffer — the fan's colourfulness, which a
    /// grey or monochrome sketch could not have.
    fn distinct_colours(fb: &Framebuffer) -> usize {
        fb.pixels()
            .iter()
            .filter(|&&c: &&Rgb565| c != Rgb565::BLACK)
            .map(|c: &Rgb565| (c.r(), c.g(), c.b()))
            .collect::<BTreeSet<(u8, u8, u8)>>()
            .len()
    }

    /// One: the fan puts ink on the glass — the triangles are filled, not skipped.
    #[test]
    fn the_fan_paints_pixels() {
        assert!(painted(0).lit_pixels() > 0);
    }

    /// The fan is *colourful* — it wears many distinct hues at once, the radial spectrum a
    /// monochrome sketch could never draw. Far more than a handful, so the HSB colouring is real.
    #[test]
    fn the_fan_is_many_coloured() {
        assert!(
            distinct_colours(&painted(0)) > 16,
            "only {} colours",
            distinct_colours(&painted(0))
        );
    }

    /// Nothing the fan draws escapes the canvas — every projected triangle, including the ones the
    /// fill-height crop sends off the edge, is clipped to the panel.
    #[test]
    fn nothing_escapes_the_canvas() {
        assert_eq!(painted(fan_core::PERIOD_MS / 3).escaped(), 0);
    }

    /// Many: three moments of the fan paint three distinct pictures — the triangles actually fold,
    /// the sketch does not freeze.
    #[test]
    fn the_fan_folds_over_time() {
        let a: Framebuffer = painted(0);
        let b: Framebuffer = painted(fan_core::PERIOD_MS / 4);
        let c: Framebuffer = painted(fan_core::PERIOD_MS / 2);
        assert_ne!(a.pixels(), b.pixels());
        assert_ne!(b.pixels(), c.pixels());
        assert_ne!(a.pixels(), c.pixels());
    }
}
