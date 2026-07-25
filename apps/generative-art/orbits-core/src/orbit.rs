//! The orbit centres: thirty points that trace a bouncing path across the canvas, and the diamond
//! bloom each one throws.
//!
//! The whole sketch is a stack of thirty [L1](https://en.wikipedia.org/wiki/Taxicab_geometry)
//! "diamond" blooms, each centred on a point that sweeps the canvas as a triangle wave of the
//! frame. Later orbits are phase-shifted and fall off steeper, so the thirty centres string
//! themselves into a bright head with a sharp tail — a comet drifting through the square. The
//! [`bloom`] at a cell is the brightest diamond over it; nothing else.
//!
//! ## The port's one real idea
//!
//! The source computes each centre with `W/PI * acos(cos(theta))`. But `acos(cos(theta))` **is a
//! triangle wave** — it has no business calling a transcendental. It rises `0 -> pi` and falls
//! `pi -> 0` every `2*pi`, which is exactly [`tri`]: a wrap onto `[0, 2*pi)` and a fold about `pi`.
//! So the centres cost a floor and an abs, not an `acos` and a `cos`. And because a centre depends
//! only on the frame (never on the cell), the thirty of them are computed **once per frame** here
//! and read by every cell — where the source recomputes all thirty inside its per-cell loop,
//! `50 x 50 x 30` times over.
//!
//! One more fold pays off on the metal. The taxicab distance `|cx - x| + |cy - y|` separates: its
//! x-part is constant down a whole column, and an orbit whose diamond does not reach a column
//! (`SOURCE - |cx - x|·falloff <= 0`) can light no cell in it. So a [`Column`] is folded once per
//! column — the x-part collapsed into each orbit's `base`, the orbits that cannot reach dropped —
//! and every row then costs only the y-part of the few orbits that survive. Since the comet is small
//! in `x`, most columns keep only a handful of the thirty. Those three moves are the whole
//! optimisation; the rest is arithmetic.
//!
//! ## Provenance
//!
//! A faithful port of a 280-character *Dwitter*, kept to its coordinates so the picture is the same
//! creature:
//!
//! ```text
//! f=0
//! draw=_=>{
//!   f++||createCanvas(W=500,W); noStroke()
//!   for(x=0;x<W;x+=10)for(y=0;y<W;y+=10){
//!     c=0
//!     for(n=0;n<30;n++){
//!       C=$=>W/PI*acos(cos((f-n)*2*$(1)/30))
//!       c=max(c, W-(abs(C(cos)-x)+abs(C(sin)-y))*(3+n))
//!     }
//!     fill(c*noise(x,y)); rect(x,y,10)
//!   }
//! }
//! ```
//!
//! Here `C(cos)` and `C(sin)` are the centre's x and y: the same triangle wave driven at the two
//! incommensurate rates `2*cos(1)/30` and `2*sin(1)/30`, so the centre traces a bouncing Lissajous
//! path rather than a straight diagonal. `(3 + n)` is orbit `n`'s falloff — the diamond shrinks as
//! `n` grows — and `(f - n)` is its phase lag along that path.

use core::f32::consts::{PI, TAU};

/// The side of the source's square canvas, `W` in the *Dwitter*. The centres sweep across it and a
/// bloom's brightness is measured against it; the display scales it onto the panel.
pub const SOURCE: f32 = 500.0;

/// How many diamond blooms are stacked into the picture — the source's `for(n=0;n<30;n++)`. The
/// head (orbit 0) is the widest and slowest to lag; the tail (orbit 29) is the sharpest and most
/// delayed.
pub const ORBITS: usize = 30;

/// The rate the centre's **x** sweeps, per frame: the source's `2*cos(1)/30`. `cos(1)` is a bare
/// constant in the source (`$(1)` with `$ = cos`), so this is a fixed number, not a function of
/// anything — evaluated once here rather than every centre.
const RATE_X: f32 = 2.0 * 0.540_302_3 / 30.0;

/// The rate the centre's **y** sweeps, per frame: the source's `2*sin(1)/30`. It is incommensurate
/// with [`RATE_X`] (`sin(1)/cos(1)` is irrational), which is what turns the two triangle waves into
/// a path that bounces around the square instead of retracing one line.
const RATE_Y: f32 = 2.0 * 0.841_471 / 30.0;

/// One orbit's centre in the source's `500x500` canvas space at a frame.
///
/// Carried by value — a plain pair, not an object. The projection of the point onto the panel is
/// the display crate's business; this crate says only where the centre is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Centre {
    /// The centre's x in `[0, SOURCE]`, a triangle wave of the frame at [`RATE_X`].
    pub x: f32,
    /// The centre's y in `[0, SOURCE]`, a triangle wave of the frame at [`RATE_Y`].
    pub y: f32,
}

/// `acos(cos(theta))` without the transcendentals: a triangle wave in `[0, pi]`, period `2*pi`.
///
/// It rises linearly `0 -> pi` as `theta` goes `0 -> pi`, then falls `pi -> 0` as `theta` goes
/// `pi -> 2*pi`, and repeats — exactly what `acos(cos(theta))` traces, to floating-point rounding.
/// Computed as the fold of the wrapped angle about `pi`: wrap `theta` onto `[0, 2*pi)`, then take
/// `pi - |wrapped - pi|`. No sine table and no `acos`; a floor and an abs.
fn tri(theta: f32) -> f32 {
    let wrapped: f32 = theta - TAU * libm::floorf(theta / TAU); // onto [0, 2*pi)
    PI - libm::fabsf(wrapped - PI)
}

/// The thirty orbit centres at `frame` — the frame-invariant work, done once so every cell can read
/// it.
///
/// Orbit `n` lags the head by `n` frames (`frame - n`) and rides the two triangle waves at
/// [`RATE_X`] and [`RATE_Y`], scaled from the wave's `[0, pi]` onto the canvas's `[0, SOURCE]`. The
/// array is small (thirty pairs) and returned by value; the render loop builds it per frame and
/// lends it to [`bloom`].
pub fn centres(frame: f32) -> [Centre; ORBITS] {
    core::array::from_fn(|n: usize| {
        let age: f32 = frame - n as f32;
        Centre {
            x: SOURCE / PI * tri(age * RATE_X),
            y: SOURCE / PI * tri(age * RATE_Y),
        }
    })
}

/// Each orbit's falloff, `(3 + n)` — the rate its diamond dims with taxicab distance. Built once at
/// compile time so the per-cell loop reads it rather than casting `n` every pixel.
const FALLOFF: [f32; ORBITS] = {
    let mut falloff: [f32; ORBITS] = [0.0; ORBITS];
    let mut n: usize = 0;
    while n < ORBITS {
        falloff[n] = (n + 3) as f32;
        n += 1;
    }
    falloff
};

/// One orbit's contribution as seen from a fixed column: its brightness where the column meets the
/// orbit's own row, and how fast it dims moving away in `y`.
///
/// The taxicab distance `|cx - x| + |cy - y|` splits into an x-part that is **constant down a
/// column** and a y-part that is not. Fixing the column collapses the x-part into [`base`] — the
/// orbit's value at `y = cy`, `SOURCE - |cx - x|·falloff` — leaving only `base - |cy - y|·falloff`
/// to evaluate per cell. An orbit is kept only when its `base` is positive; a non-positive one can
/// never beat the zero floor at any `y`, so dropping it changes no pixel.
#[derive(Clone, Copy)]
struct ActiveOrbit {
    /// The orbit's brightness where this column crosses its centre row — `SOURCE - |cx - x|·falloff`,
    /// always positive for a kept orbit.
    base: f32,
    /// The orbit centre's y, the row its diamond peaks on.
    cy: f32,
    /// The orbit's [`FALLOFF`], carried alongside so the per-cell loop needs no second array.
    falloff: f32,
}

/// A column's worth of the bloom: the orbits that can light *any* cell in this column, with their
/// x-distance already folded in.
///
/// Built once per column by [`Column::at`] and read down every row by [`Column::bloom`], this is the
/// hoist that makes the sketch cheap: the source recomputes the whole taxicab distance for every
/// cell and every orbit, where here the x-part is paid once per column and the dead orbits — usually
/// most of the thirty, since the comet is small in `x` — are dropped before the row loop even
/// starts.
pub struct Column {
    /// The kept orbits for this column, densely packed; only the first [`count`](Self::count) are
    /// meaningful. A fixed array, so a column costs no heap.
    active: [ActiveOrbit; ORBITS],
    /// How many of [`active`](Self::active) are real.
    count: usize,
}

impl Column {
    /// Fold the column at source-x `x` out of the thirty `centres`: keep each orbit whose diamond
    /// reaches this column (`base > 0`), with its x-distance already collapsed into its `base`.
    pub fn at(x: f32, centres: &[Centre; ORBITS]) -> Self {
        let mut active: [ActiveOrbit; ORBITS] = [ActiveOrbit {
            base: 0.0,
            cy: 0.0,
            falloff: 0.0,
        }; ORBITS];
        let mut count: usize = 0;
        for (n, centre) in centres.iter().enumerate() {
            let base: f32 = SOURCE - libm::fabsf(centre.x - x) * FALLOFF[n];
            if base > 0.0 {
                active[count] = ActiveOrbit {
                    base,
                    cy: centre.y,
                    falloff: FALLOFF[n],
                };
                count += 1;
            }
        }
        Self { active, count }
    }

    /// The brightest diamond over the cell at source-y `y` in this column: `max(0, max_orbit(base -
    /// |cy - y|·falloff))`, the source's `c` for this cell.
    ///
    /// Only the column's kept orbits are visited, and only the y-part of each diamond is evaluated —
    /// the x-part is already in `base`. Starting from a zero floor means a cell no diamond reaches is
    /// exactly dark, which the display reads as ground and skips. Pure `f32` subtract, multiply, abs
    /// and compare — no division, no rounding, the operations this board's FPU does in stride.
    pub fn bloom(&self, y: f32) -> f32 {
        let mut brightest: f32 = 0.0;
        for orbit in &self.active[..self.count] {
            let value: f32 = orbit.base - libm::fabsf(orbit.cy - y) * orbit.falloff;
            if value > brightest {
                brightest = value;
            }
        }
        brightest
    }
}

/// The brightest diamond over the source point `(x, y)`, across all [`ORBITS`] centres: the value
/// `c` in `[0, SOURCE]` the source builds with `c = max(c, W - L1 * (3 + n))`.
///
/// The single-cell convenience: it folds the column with [`Column::at`] then reads one row with
/// [`Column::bloom`], so it is the exact path the grid walks, one cell at a time — the fidelity tests
/// pin *this*, and the grid's hoisting cannot drift from it. A caller shading a whole column should
/// build the [`Column`] once and read every row, not call this per cell.
pub fn bloom(x: f32, y: f32, centres: &[Centre; ORBITS]) -> f32 {
    Column::at(x, centres).bloom(y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The triangle wave stands in for a transcendental, so it must track `acos(cos(.))` tightly.
    /// `1e-2` (a hundredth of a pixel on the `[0, 500]` canvas) is `f32`'s room over the `f64`
    /// reference — chiefly the cancellation in wrapping a large angle onto `[0, 2*pi)` — while still
    /// catching any real drift, which a wrong rate or a lost period would show in whole pixels. The
    /// bloom test reuses the same centres in both precisions, so it needs only rounding room.
    const WAVE_TOLERANCE: f64 = 1e-2;
    const BLOOM_TOLERANCE: f64 = 1e-2;

    /// Zero: at phase zero the triangle wave is at the bottom of its climb — `acos(cos(0)) = 0`.
    #[test]
    fn the_wave_is_zero_at_zero() {
        assert!(
            tri(0.0).abs() < WAVE_TOLERANCE as f32,
            "tri(0) = {}",
            tri(0.0)
        );
    }

    /// One: half a turn is the wave's peak — `acos(cos(pi)) = pi`, the top of the triangle.
    #[test]
    fn the_wave_peaks_at_half_turn() {
        assert!(
            (tri(PI) - PI).abs() < WAVE_TOLERANCE as f32,
            "tri(pi) = {}",
            tri(PI)
        );
    }

    /// Many: a full turn returns the wave to the bottom — the period is `2*pi`, so `tri(2*pi)` is
    /// `tri(0)`, and a wave off by a period would drift the whole comet.
    #[test]
    fn the_wave_repeats_every_turn() {
        assert!(
            tri(TAU).abs() < WAVE_TOLERANCE as f32,
            "tri(2*pi) = {}",
            tri(TAU)
        );
    }

    /// Every centre lands inside the canvas: the wave maps `[0, pi]` onto `[0, SOURCE]`, so no orbit
    /// can wander off the square, at frame zero or any frame the animation drives.
    #[test]
    fn centres_stay_on_the_canvas() {
        let frames: [f32; 3] = [0.0, 37.0, 250.5];
        let on_canvas = |frame: f32| -> bool {
            centres(frame)
                .iter()
                .all(|c: &Centre| (0.0..=SOURCE).contains(&c.x) && (0.0..=SOURCE).contains(&c.y))
        };
        assert!(frames.into_iter().all(on_canvas));
    }

    /// Zero: a point no diamond reaches is exactly dark — the bloom starts at zero and a cell far
    /// outside every diamond never rises above it, which is what lets the display skip it.
    #[test]
    fn a_far_point_is_dark() {
        // Pin every centre near the origin, then probe the far corner: `SOURCE - L1*(3+n)` is deeply
        // negative for all thirty, so the max stays at its zero floor.
        let cs: [Centre; ORBITS] = core::array::from_fn(|_| Centre { x: 0.0, y: 0.0 });
        assert_eq!(bloom(SOURCE, SOURCE, &cs), 0.0);
    }

    /// One: a point sitting on the head orbit's centre is at full brightness — `L1 = 0` makes its
    /// contribution `SOURCE`, and no diamond can exceed that.
    #[test]
    fn the_head_centre_is_full_brightness() {
        let cs: [Centre; ORBITS] = centres(40.0);
        let head: Centre = cs[0];
        assert_eq!(bloom(head.x, head.y, &cs), SOURCE);
    }

    /// The `f64` ground truth for one centre coordinate: the source's `W/PI * acos(cos(rate * age))`
    /// in full precision.
    fn centre_reference(age: f64, rate: f64) -> f64 {
        500.0 / core::f64::consts::PI * (rate * age).cos().acos()
    }

    proptest! {
        /// Many: across a wide span of frames, the `f32` triangle-wave centres track the `f64`
        /// `acos(cos(.))` reference within tolerance — the port's headline claim, that the wave *is*
        /// the transcendental, pinned everywhere the animation drives it.
        #[test]
        fn centres_track_the_transcendental(frame in -400.0f32..400.0) {
            let cs: [Centre; ORBITS] = centres(frame);
            for (n, centre) in cs.iter().enumerate() {
                let age: f64 = frame as f64 - n as f64;
                let want_x: f64 = centre_reference(age, 2.0 * 1.0f64.cos() / 30.0);
                let want_y: f64 = centre_reference(age, 2.0 * 1.0f64.sin() / 30.0);
                prop_assert!((centre.x as f64 - want_x).abs() < WAVE_TOLERANCE, "x {} vs {}", centre.x, want_x);
                prop_assert!((centre.y as f64 - want_y).abs() < WAVE_TOLERANCE, "y {} vs {}", centre.y, want_y);
            }
        }

        /// Many: the bloom equals the source's `max(0, max_n(W - L1*(3+n)))` computed in `f64`,
        /// anywhere on the canvas for any arrangement of centres — the diamond stack is faithful,
        /// not just plausible.
        #[test]
        fn the_bloom_tracks_the_reference(
            x in 0.0f32..SOURCE,
            y in 0.0f32..SOURCE,
            frame in 0.0f32..400.0,
        ) {
            let cs: [Centre; ORBITS] = centres(frame);
            let mut want: f64 = 0.0;
            for (n, centre) in cs.iter().enumerate() {
                let l1: f64 = (centre.x as f64 - x as f64).abs() + (centre.y as f64 - y as f64).abs();
                let value: f64 = 500.0 - l1 * (3 + n) as f64;
                if value > want {
                    want = value;
                }
            }
            prop_assert!((bloom(x, y, &cs) as f64 - want).abs() < BLOOM_TOLERANCE, "{} vs {}", bloom(x, y, &cs), want);
        }
    }
}
