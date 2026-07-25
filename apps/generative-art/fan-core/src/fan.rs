//! The folding triangles: radial columns that fold open and closed in a wave from the centre.
//!
//! The whole sketch is a grid of triangles, each a pure function of the phase. A triangle is
//! anchored at its base `(x, y)`; its apex swings out and back as the phase advances, so the
//! triangle folds open and closed like a paper fan. Because a triangle's fold phase is offset by
//! its distance from the centre, the folds run out of step into a wave spreading outward — and
//! because its hue is set by that same distance, the wave is a ring of colour sweeping the wheel.
//! There is no state and no history — the fan at phase `φ` is [`cells`]`(φ, table)` and nothing
//! else.
//!
//! ## Provenance
//!
//! A faithful port of a 280-character *Dwitter*, kept to its coordinates so the picture is the
//! same creature:
//!
//! ```text
//! f=0
//! draw=_=>{
//!   f++||createCanvas(W=500,W); background(0); colorMode(HSB,1); d=43
//!   for(x=0;x<W;x+=d)for(y=-x/2,t=1;y<W;y+=25,t*=-1){
//!     fill((r=dist(x,y,250,250))/W%1,1,1)
//!     s=sin((f+r)/30)
//!     quad(x,y, x+d*t*s,y+25*t, x,y+50*t)
//!   }
//! }
//! ```
//!
//! The source draws, per cell, a triangle `(x, y)`–`(x + d·t·s, y + 25·t)`–`(x, y + 50·t)` filled
//! at hue `dist(x,y,250,250)/W`, where `s = sin((f + r)/30)` folds the apex and `t` alternates the
//! column's triangles up and down. Two things about the port, both because they are the *shape of
//! the computation* rather than the shape of the glass:
//!
//! - **The fan is computed in the source's `500×500` space.** The centre, the column spacing and
//!   the triangle metrics are the source's, so it is the same creature; the projection onto the
//!   `135×240` panel (a fill-height scale, cropping the width) is the display crate's business,
//!   exactly as the plume projects its own canvas.
//! - **`f32`, and a table sine.** Every value is single-precision, for the ESP32's FPU, and the one
//!   sine a triangle needs is read from the shared [`SinTable`]. The distance is an honest `sqrt` —
//!   it sets both the hue and the fold's phase — paid per cell, not per frame elsewhere.

use platform_numerics::SinTable;

/// The side of the source's square canvas, `W` in the *Dwitter*. Distances are taken within it and
/// the hue wraps once across it; the display scales it onto the panel.
pub const SOURCE: f32 = 500.0;

/// The centre the fan radiates from — the source's `(250, 250)`, where the hue is zero (red) and
/// the fold has no distance offset.
pub const CENTRE: f32 = SOURCE / 2.0;

/// The horizontal spacing between columns of triangles — the source's `d = 43`, also the reach of
/// a fully-folded apex (`x + d·t·s` at `|s| = 1`).
pub const COLUMN_STEP: f32 = 43.0;

/// The vertical step between triangles within a column — the source's `25`. A triangle spans two
/// of these (`y + 50·t`), so consecutive triangles overlap by one step, which is the interlock the
/// fan is woven from.
pub const ROW_STEP: f32 = 25.0;

/// How far the fold's phase is offset per unit of distance from the centre: the source's `/30` in
/// `sin((f + r)/30)`, applied here as `r / FOLD_DIVISOR`. It is what turns a single global phase
/// into a wave — triangles at different radii fold at different times.
const FOLD_DIVISOR: f32 = 30.0;

/// One triangle of the fan at a phase: its three vertices in source canvas space, and its hue.
///
/// Carried by value — a plain result, not an object. The projection of the vertices onto the panel
/// and the turn of the hue into an [`Rgb565`](https://docs.rs/embedded-graphics) colour are the
/// display crate's business; this crate says only where the triangle is and what colour it wears.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    /// The triangle's three vertices in the source's `500×500` canvas space, in the source's order:
    /// the base `(x, y)`, the folding apex, then the tip two row-steps along.
    pub verts: [(f32, f32); 3],
    /// The HSB hue in `[0, 1)`, `dist(x, y, centre)/SOURCE` wrapped onto the ring — red at the
    /// centre, sweeping the wheel outward. Saturation and value are always one, so the hue alone
    /// names the colour.
    pub hue: f32,
}

/// The triangle whose base sits at `(x, y)`, the `k`-th in its column, at phase `φ`, through
/// `table`.
///
/// `k` alternates the triangle up (`t = +1`) or down (`t = -1`) exactly as the source's `t *= -1`
/// does. The distance from the centre sets both the hue (wrapped onto `[0, 1)`) and the fold's
/// phase offset; the fold itself, `s = sin(φ + r/FOLD_DIVISOR)`, is one table lookup.
fn triangle(x: f32, k: u32, y: f32, phi: f32, table: &SinTable) -> Cell {
    let t: f32 = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
    let dx: f32 = x - CENTRE;
    let dy: f32 = y - CENTRE;
    let r: f32 = libm::sqrtf(dx * dx + dy * dy);

    let ratio: f32 = r / SOURCE;
    let hue: f32 = ratio - libm::floorf(ratio); // (r / SOURCE) % 1, and r ≥ 0

    let s: f32 = table.sin(phi + r / FOLD_DIVISOR);
    let apex: (f32, f32) = (x + COLUMN_STEP * t * s, y + ROW_STEP * t);
    let tip: (f32, f32) = (x, y + 2.0 * ROW_STEP * t);
    Cell {
        verts: [(x, y), apex, tip],
        hue,
    }
}

/// Every triangle of the fan at phase `φ`: the source's `for(x)for(y)` sweep, in that order.
///
/// The outer loop steps columns across the canvas by [`COLUMN_STEP`] while `x < SOURCE`; the inner
/// loop walks a column from `y = -x/2` down by [`ROW_STEP`] while `y < SOURCE`, alternating each
/// triangle up and down. Yielded in exactly that order because the triangles overlap and the source
/// layers the later one on top, so the display must draw them in the same order. Borrows `table`
/// for the life of the iterator — the render loop's shape: build the table once, sweep it each
/// frame.
pub fn cells<'a>(phi: f32, table: &'a SinTable) -> impl Iterator<Item = Cell> + 'a {
    let columns = (0u32..)
        .map(|i: u32| i as f32 * COLUMN_STEP)
        .take_while(|&x: &f32| x < SOURCE);
    columns.flat_map(move |x: f32| {
        (0u32..)
            .map(move |k: u32| (k, -x / 2.0 + k as f32 * ROW_STEP))
            .take_while(|&(_, y): &(u32, f32)| y < SOURCE)
            .map(move |(k, y): (u32, f32)| triangle(x, k, y, phi, table))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The fold and the hue are the port's only table/float-sensitive steps, so they must track an
    /// `f64` reference within the table's tolerance, with margin for the fold's `43×` lever on the
    /// sine error.
    const VERT_TOLERANCE: f64 = 0.1;
    const HUE_TOLERANCE: f64 = 1e-4;

    /// The `f64` ground truth: the source's triangle and hue in full precision.
    fn reference(x: f64, k: u32, y: f64, phi: f64) -> ([(f64, f64); 3], f64) {
        let t: f64 = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
        let r: f64 = ((x - 250.0).powi(2) + (y - 250.0).powi(2)).sqrt();
        let hue: f64 = (r / 500.0).rem_euclid(1.0);
        let s: f64 = (phi + r / 30.0).sin();
        (
            [(x, y), (x + 43.0 * t * s, y + 25.0 * t), (x, y + 50.0 * t)],
            hue,
        )
    }

    /// Zero: a triangle at the very centre with phase zero is *closed* — its folding apex sits
    /// directly above its base (no horizontal swing, `sin(0) = 0`), and its hue is zero, the red
    /// the fan radiates from.
    #[test]
    fn the_centre_triangle_is_closed_and_red() {
        let cell: Cell = triangle(CENTRE, 0, CENTRE, 0.0, &SinTable::new());
        assert!(cell.hue.abs() < HUE_TOLERANCE as f32, "hue {}", cell.hue);
        // apex x equals base x — the fold is shut.
        assert!((cell.verts[1].0 - cell.verts[0].0).abs() < 1e-3);
    }

    /// One: hue grows with distance from the centre — a near triangle is redder (smaller hue) than
    /// a far one, before the wheel wraps. This is the radial colouring, in one comparison.
    #[test]
    fn hue_grows_outward_from_the_centre() {
        let table: SinTable = SinTable::new();
        let near: Cell = triangle(CENTRE, 0, CENTRE + 10.0, 0.0, &table);
        let far: Cell = triangle(0.0, 0, 0.0, 0.0, &table);
        assert!(near.hue < far.hue, "near {} !< far {}", near.hue, far.hue);
    }

    /// Many: a triangle folds as the phase advances — the same triangle at two phases has its apex
    /// in two places, so a fan that ignored `φ` could never pass.
    #[test]
    fn a_triangle_folds_with_the_phase() {
        let table: SinTable = SinTable::new();
        let shut: Cell = triangle(86.0, 3, 100.0, 0.0, &table);
        let open: Cell = triangle(86.0, 3, 100.0, 1.0, &table);
        assert_ne!(shut.verts[1], open.verts[1]);
    }

    proptest! {
        /// Many: across the canvas and a full turn of phase, the table-and-`f32` triangle and hue
        /// track the `f64` reference within tolerance. A regression in the fold's sine or the hue
        /// wrap would drift the picture; this pins both everywhere the animation drives them.
        #[test]
        fn the_triangle_tracks_the_reference(
            x in 0.0f32..SOURCE,
            k in 0u32..40,
            y in -250.0f32..SOURCE,
            phi in 0.0f32..core::f32::consts::TAU,
        ) {
            let cell: Cell = triangle(x, k, y, phi, &SinTable::new());
            let (verts, hue): ([(f64, f64); 3], f64) = reference(x as f64, k, y as f64, phi as f64);
            for (got, want) in cell.verts.iter().zip(verts.iter()) {
                prop_assert!((got.0 as f64 - want.0).abs() < VERT_TOLERANCE, "x {} vs {}", got.0, want.0);
                prop_assert!((got.1 as f64 - want.1).abs() < VERT_TOLERANCE, "y {} vs {}", got.1, want.1);
            }
            prop_assert!((cell.hue as f64 - hue).abs() < HUE_TOLERANCE, "hue {} vs {}", cell.hue, hue);
        }
    }
}
