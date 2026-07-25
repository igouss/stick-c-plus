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

use alloc::vec::Vec;

use platform_numerics::SinTable;

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
///
/// [`Default`] is the origin, an unlit non-wide point: it is only ever a placeholder to fill a
/// scratch buffer with before [`PlumeField::compute_range`] overwrites every slot, so its value
/// is never read.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
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

/// The per-index invariants of one field point, computed once and reloaded every frame.
///
/// Every frame evaluates the field at the *same* strided indices, so everything in [`point`]
/// that turns only on the index `i` — never on the phase `t` — is the same value on every
/// repaint. Measured on the metal, recomputing it was the bulk of a frame: six table lookups a
/// point dominated, with a `sqrtf` and a `0.3/k` division behind them. This holds the
/// `t`-independent results so a frame recomputes none of them; see [`PlumeField`].
#[derive(Clone, Copy, PartialEq, Debug)]
struct Precomputed {
    /// The `t`-independent part of the swirl angle, `e·14 − d·3`; the frame adds `2t`.
    swirl_base: f32,
    /// The `t`-independent part of `q`, `3·sin(2k) + 0.3/k` — the field's one division, paid once.
    a: f32,
    /// The swirl's coefficient in `q`, `sin(y/19)·k`.
    b: f32,
    /// The magnitude `d = |(k, e)|` — the field's one `sqrtf`, paid once. Reused three ways a
    /// frame: in `c = d − t`, and (already folded into `swirl_base`) the swirl, and `y`'s `d·39`.
    d: f32,
    /// Whether this point is drawn fat — `k² > 15`, the barbs' bright spine.
    wide: bool,
}

/// The `t`-independent results at index `i`, evaluated with the exact operations and grouping
/// [`point`] uses, so reassembling them in [`PlumeField::frame`] reproduces [`point`] bit for bit.
fn precompute(i: u32, table: &SinTable) -> Precomputed {
    let x: f32 = i as f32;
    let y: f32 = x * (1.0 / 235.0);

    let k: f32 = 4.0 * table.cos(x * (1.0 / 21.0));
    let e: f32 = y * (1.0 / 8.0) - 20.0;
    let d: f32 = libm::sqrtf(k * k + e * e);

    Precomputed {
        swirl_base: e * 14.0 - d * 3.0,
        a: 3.0 * table.sin(k * 2.0) + 0.3 / k,
        b: table.sin(y * (1.0 / 19.0)) * k,
        d,
        wide: k * k > 15.0,
    }
}

/// The frond as [`plume`], but built for the panel: the per-index invariants are precomputed
/// once at [`new`](Self::new), so a frame costs only the three genuinely phase-dependent lookups
/// — the swirl and `c`'s sine and cosine — instead of six lookups, a `sqrtf` and a division.
///
/// The saving is exactly the loop-invariant hoist the hardware brief calls for, and it changes
/// nothing on the glass: [`frame`](Self::frame) regroups the *same* operations in the *same*
/// order as [`point`], so it returns a bit-identical cloud (proven in the tests). It holds
/// [`POINT_COUNT`] precomputed points on the heap — a small `f32` record each — cheap to sweep
/// and, like every panel-sized buffer here, never a stack temporary.
pub struct PlumeField {
    points: Vec<Precomputed>,
}

impl PlumeField {
    /// Pay for the frond's per-index invariants once: the six-lookups-a-point,
    /// `sqrtf`-and-division cost of [`point`] is spent here, at construction, and never again on
    /// the render path.
    pub fn new(table: &SinTable) -> Self {
        let points: Vec<Precomputed> = (1..=POINT_COUNT)
            .map(|n: u32| precompute(n * STEP, table))
            .collect();
        Self { points }
    }

    /// The frond at phase `t`: the same [`POINT_COUNT`] points [`plume`] yields, reassembled from
    /// the precomputed invariants with only the three phase-dependent lookups per point.
    ///
    /// [`iter_range`](Self::iter_range) over the whole field — bit-identical to [`plume`]`(t,
    /// table)`, the reference sweep the tests hold to.
    pub fn frame<'a>(
        &'a self,
        t: f32,
        table: &'a SinTable,
    ) -> impl Iterator<Item = FieldPoint> + 'a {
        self.iter_range(0, self.points.len(), t, table)
    }

    /// The frond's points at indices `[start, start + len)`, at phase `t`, as an iterator.
    ///
    /// The streaming primitive both [`frame`](Self::frame) (the whole field) and
    /// [`compute_range`](Self::compute_range) (into a slice) go through, so a range, the whole, and
    /// a filled buffer all evaluate the same [`point_from`] and cannot drift. It lets a caller plot
    /// one core's half of the frond point-by-point with **no intermediate buffer** — the memory
    /// saving that keeps the two-core pipeline inside the ESP32's heap.
    ///
    /// Panics if `start + len` runs past the field — a caller-side indexing bug, kept a hard panic
    /// rather than a silent short sweep.
    pub fn iter_range<'a>(
        &'a self,
        start: usize,
        len: usize,
        t: f32,
        table: &'a SinTable,
    ) -> impl Iterator<Item = FieldPoint> + 'a {
        let t2: f32 = 2.0 * t;
        self.points[start..start + len]
            .iter()
            .map(move |p: &Precomputed| point_from(p, t, t2, table))
    }

    /// Fill `out` with the frond's points at indices `[start, start + out.len())`, at phase `t` —
    /// the buffered form of [`iter_range`](Self::iter_range), for a worker that must *own* its
    /// output because it cannot borrow the caller's frame across a thread. `out[j]` is the point at
    /// precomputed index `start + j`; bit-identical to the matching stretch of [`frame`], so two
    /// cores filling two ranges reassemble the one true frame (proven in the tests).
    ///
    /// Panics if `start + out.len()` runs past the field — a caller-side indexing bug.
    pub fn compute_range(&self, start: usize, t: f32, table: &SinTable, out: &mut [FieldPoint]) {
        let len: usize = out.len();
        out.iter_mut()
            .zip(self.iter_range(start, len, t, table))
            .for_each(|(slot, point): (&mut FieldPoint, FieldPoint)| *slot = point);
    }
}

/// One frame point from its precomputed invariants, at phase `t` — the exact body
/// [`PlumeField::frame`] and [`PlumeField::compute_range`] share, so the whole and any range of it
/// compute the same bits.
///
/// `t2 = 2t` is passed in rather than recomputed, so a sweep hoists the doubling out of the loop
/// once; the three genuinely phase-dependent lookups (the swirl, and `c`'s sine and cosine) are
/// all that remain per point.
fn point_from(p: &Precomputed, t: f32, t2: f32, table: &SinTable) -> FieldPoint {
    let swirl: f32 = table.sin(p.swirl_base + t2);
    let q: f32 = p.a + p.b * (9.0 + 2.0 * swirl);
    let c: f32 = p.d - t;
    FieldPoint {
        x: q + 50.0 * table.cos(c) + 200.0,
        y: q * table.sin(c) + p.d * 39.0 - 475.0,
        wide: p.wide,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_numerics::SinTable;
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

    /// One: the precomputed field yields the whole reduced budget, exactly as [`plume`] does — so
    /// nothing was dropped or doubled in the hoist.
    #[test]
    fn the_fast_field_is_the_reduced_point_budget() {
        let table: SinTable = SinTable::new();
        assert_eq!(
            PlumeField::new(&table).frame(0.0, &table).count(),
            POINT_COUNT as usize
        );
    }

    proptest! {
        /// Many: the precomputed field is **bit-identical** to [`plume`] at every point and every
        /// phase — not merely "within a pixel". The hoist only regroups the same operations in the
        /// same order, so it must return the very same bits; a discrepancy would mean the
        /// regrouping changed the arithmetic, and the picture with it. Compared through `to_bits`
        /// so the field's degenerate ±∞ points (and any NaN) match exactly rather than by `==`,
        /// under which `∞ == ∞` holds but `NaN == NaN` does not.
        #[test]
        fn the_fast_field_is_bit_identical_to_plume(t in 0.0f32..core::f32::consts::TAU) {
            let table: SinTable = SinTable::new();
            let field: PlumeField = PlumeField::new(&table);
            for (slow, fast) in plume(t, &table).zip(field.frame(t, &table)) {
                prop_assert_eq!(slow.x.to_bits(), fast.x.to_bits(), "x differs at t={}", t);
                prop_assert_eq!(slow.y.to_bits(), fast.y.to_bits(), "y differs at t={}", t);
                prop_assert_eq!(slow.wide, fast.wide, "wide differs at t={}", t);
            }
        }
    }

    proptest! {
        /// Many: a **split sweep is bit-identical to the whole**. Filling `[0, split)` and
        /// `[split, POINT_COUNT)` with two [`compute_range`](PlumeField::compute_range) calls yields
        /// exactly the bits [`frame`](PlumeField::frame) produces in one sweep — at every phase and
        /// every split point, including the empty near half (`split = 0`) and the empty far half
        /// (`split = POINT_COUNT`). This is the invariant the dual-core pipeline rests on: two cores
        /// filling two ranges reassemble the one true frame, not an approximation of it. Compared
        /// through `to_bits` so the degenerate ±∞ points match exactly.
        #[test]
        fn a_split_sweep_is_bit_identical_to_the_whole(
            t in 0.0f32..core::f32::consts::TAU,
            split in 0usize..=POINT_COUNT as usize,
        ) {
            let table: SinTable = SinTable::new();
            let field: PlumeField = PlumeField::new(&table);

            let mut out: Vec<FieldPoint> = alloc::vec![FieldPoint::default(); POINT_COUNT as usize];
            let (near, far): (&mut [FieldPoint], &mut [FieldPoint]) = out.split_at_mut(split);
            field.compute_range(0, t, &table, near);
            field.compute_range(split, t, &table, far);

            for (whole, part) in field.frame(t, &table).zip(out.iter()) {
                prop_assert_eq!(whole.x.to_bits(), part.x.to_bits(), "x differs at t={}, split={}", t, split);
                prop_assert_eq!(whole.y.to_bits(), part.y.to_bits(), "y differs at t={}, split={}", t, split);
                prop_assert_eq!(whole.wide, part.wide, "wide differs at t={}, split={}", t, split);
            }
        }
    }
}
