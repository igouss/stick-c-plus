//! The plume field: a point cloud that *is* the picture, as a pure function of the phase.
//!
//! The whole animation is one parametric field. For an animation phase `t`, evaluating
//! [`point`] over the index range yields a cloud of points in a 400×400 canvas space; drawn as
//! dots, that cloud is a feathered frond, and as `t` advances the barbs sweep and the frond
//! breathes. There is no state and no history — frame *N* is [`plume`]`(phase(N))`, computed
//! from nothing but the phase — which is what lets the render loop treat it as a value and the
//! tests treat it as a function.
//!
//! ## Provenance
//!
//! The field is a port of a 280-character *Dwitter* by its parametric form, kept faithful to
//! the coordinates so the picture is the same creature:
//!
//! ```text
//! a=(x,y,d=mag(k=4*cos(x/21),e=y/8-20))=>
//!   circle((q=3*sin(k*2)+.3/k+sin(y/19)*k*(9+2*sin(e*14-d*3+t*2)))+50*cos(c=d-t)+200,
//!          q*sin(c)+d*39-475, k*k>15?2:1)
//! t=0,draw=$=>{...for(t+=PI/240,i=1e4;i--;)a(i,i/235)}
//! ```
//!
//! Two things change for the panel, both decided *here* because both are about the shape of
//! the computation rather than the shape of the glass:
//!
//! - **Half the points.** The original plots ten thousand; on a canvas a fifth of the area
//!   most of them land on a pixel another already lit. [`POINT_COUNT`] is five thousand — every
//!   other index — which was checked against the ten-thousand render to preserve the frond. See
//!   [`STEP`].
//! - **Table trigonometry, `f32` throughout.** Every `sin`/`cos` is read from the
//!   [`SinTable`](crate::SinTable); every value is single-precision, for the ESP32's FPU. The
//!   one thing that is *not* a table lookup is `mag`, a genuine `sqrtf` — the honest magnitude
//!   the field is built around.

use crate::trig::SinTable;

/// The index the original loop counts down from — the widest `x` the field is evaluated at.
pub const START: u32 = 10_000;

/// The stride between evaluated indices: every other one.
///
/// This is the point-budget optimisation in one number. The original steps by one; stepping by
/// two halves the transcendentals and the dots for a picture the eye cannot tell apart at
/// 135×240, because on that canvas the discarded points overwhelmingly collided with a kept
/// one. Widening it further (three, four) starts to thin the barbs — two is the edge.
pub const STEP: u32 = 2;

/// How many points make up one frame of the frond: [`START`] / [`STEP`].
pub const POINT_COUNT: u32 = START / STEP;

/// One point of the frond, in the original's 400×400 canvas coordinates.
///
/// Carried by value — it is a plain result, not an object. The projection onto the panel (and
/// the decision to keep or clip it) is the display crate's business; this crate only says where
/// the point *is* and whether it is a fat one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FieldPoint {
    /// Horizontal position in canvas space. Roughly centred on 200, but a degenerate index can
    /// send it to infinity — see [`point`].
    pub x: f32,
    /// Vertical position in canvas space.
    pub y: f32,
    /// Whether the original draws this point as a two-pixel dot rather than one — the barbs'
    /// bright spines, where `k² > 15`.
    pub wide: bool,
}

/// The field at index `i` and phase `t`, through `table`.
///
/// A direct transcription of the *Dwitter* expression, with the sines and cosines routed
/// through the table. The intermediate names (`k`, `e`, `d`, `q`, `c`) are the original's, kept
/// so the port is checkable against its source line by line rather than rewritten into
/// something whose equivalence would have to be argued.
///
/// ## The degenerate index
///
/// `q` contains `0.3 / k`, and `k = 4·cos(i/21)` passes through zero roughly a hundred and
/// fifty times across the index range. At those indices `q` is ±∞ and so is the point — which
/// is *correct*: the original's `circle` at infinity simply draws nothing, and a projection
/// that checks `is_finite` reproduces that exactly. No angle fed to the table is ever infinite
/// (`c = d − t` is always finite), so the infinity stays a coordinate, never a table index.
pub fn point(i: u32, t: f32, table: &SinTable) -> FieldPoint {
    let x: f32 = i as f32;
    let y: f32 = x * (1.0 / 235.0);

    let k: f32 = 4.0 * table.cos(x * (1.0 / 21.0));
    let e: f32 = y * (1.0 / 8.0) - 20.0;
    let d: f32 = libm::sqrtf(k * k + e * e);

    let swirl: f32 = table.sin(e * 14.0 - d * 3.0 + t * 2.0);
    let q: f32 =
        3.0 * table.sin(k * 2.0) + 0.3 / k + table.sin(y * (1.0 / 19.0)) * k * (9.0 + 2.0 * swirl);

    let c: f32 = d - t;
    FieldPoint {
        x: q + 50.0 * table.cos(c) + 200.0,
        y: q * table.sin(c) + d * 39.0 - 475.0,
        wide: k * k > 15.0,
    }
}

/// Every point of the frond at phase `t`: [`point`] over the strided index range.
///
/// [`POINT_COUNT`] points at indices `STEP, 2·STEP, …, START`. Borrows `table` for the life of
/// the iterator, which is exactly the render loop's shape: build the table once, then sweep it
/// each frame.
pub fn plume<'a>(t: f32, table: &'a SinTable) -> impl Iterator<Item = FieldPoint> + 'a {
    (1..=POINT_COUNT).map(move |n: u32| point(n * STEP, t, table))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trig::SinTable;
    use proptest::prelude::*;

    /// A reference evaluation in `f64` through the standard library's real transcendentals —
    /// the ground truth the table-and-`f32` field is checked against. This is the original
    /// computation, unoptimised, so a discrepancy is the optimisation's fault by construction.
    fn reference(i: u32, t: f64) -> (f64, f64) {
        let x: f64 = i as f64;
        let y: f64 = x / 235.0;
        let k: f64 = 4.0 * (x / 21.0).cos();
        let e: f64 = y / 8.0 - 20.0;
        let d: f64 = (k * k + e * e).sqrt();
        let q: f64 = 3.0 * (k * 2.0).sin()
            + 0.3 / k
            + (y / 19.0).sin() * k * (9.0 + 2.0 * (e * 14.0 - d * 3.0 + t * 2.0).sin());
        let c: f64 = d - t;
        (q + 50.0 * c.cos() + 200.0, q * c.sin() + d * 39.0 - 475.0)
    }

    /// Many: the frond is made of exactly [`POINT_COUNT`] points — the halved budget, counted.
    #[test]
    fn the_frond_is_the_reduced_point_budget() {
        assert_eq!(plume(0.0, &SinTable::new()).count(), POINT_COUNT as usize);
    }

    /// One: a well-behaved index produces a finite point near the canvas centre, so the field
    /// is actually drawing something and not just clipping everything off the edge.
    #[test]
    fn a_typical_point_is_finite_and_on_canvas() {
        let p: FieldPoint = point(5_000, 0.0, &SinTable::new());
        assert!(p.x.is_finite() && p.y.is_finite());
        assert!(p.x > 0.0 && p.x < 400.0, "x = {}", p.x);
        assert!(p.y > 0.0 && p.y < 400.0, "y = {}", p.y);
    }

    /// The frond breathes: the same index at two phases is two different points. A field that
    /// ignored `t` would pass every count test and never animate.
    #[test]
    fn a_point_moves_with_the_phase() {
        let table: SinTable = SinTable::new();
        assert_ne!(point(5_000, 0.0, &table), point(5_000, 1.0, &table));
    }

    proptest! {
        /// Many: for every well-conditioned index the table-and-`f32` point tracks the `f64`
        /// reference within a pixel. Indices near a `k`-zero are excluded here — there the point
        /// is ±∞ by design, which the projection clips; its finiteness is its own test below.
        #[test]
        fn the_field_tracks_the_reference(i in 1u32..=START, t in 0.0f32..core::f32::consts::TAU) {
            let k: f64 = 4.0 * (i as f64 / 21.0).cos();
            prop_assume!(k.abs() > 0.2); // away from the degenerate `0.3 / k`

            let p: FieldPoint = point(i, t, &SinTable::new());
            let (rx, ry): (f64, f64) = reference(i, t as f64);
            // A pixel of agreement in canvas space — well under the 0.75× down-scale to the
            // panel, so no discrepancy this small can move a projected dot.
            prop_assert!((p.x as f64 - rx).abs() < 1.0, "x: {} vs {}", p.x, rx);
            prop_assert!((p.y as f64 - ry).abs() < 1.0, "y: {} vs {}", p.y, ry);
        }
    }
}
