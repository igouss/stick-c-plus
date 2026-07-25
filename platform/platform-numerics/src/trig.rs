//! The sine table — the plume's answer to a chip with no fast transcendental.
//!
//! The field ([`crate::field`]) evaluates six sines and cosines per point, five thousand
//! points a frame: thirty thousand transcendentals every repaint. `libm::sinf` is honest but
//! it is *software* — the ESP32's FPU multiplies and adds in hardware but has no sine — so
//! computing them the obvious way spends the whole frame budget inside libm.
//!
//! So this crate spends the transcendentals **once**, at startup, filling a table; the hot
//! path then reads it. A lookup is a multiply, a floor, a mask and a lerp — all of which the
//! FPU and ALU do in a handful of cycles — in place of a polynomial evaluation with range
//! reduction. The table is the single largest reason the animation keeps its cadence.
//!
//! ## Why a table and not an incremental oscillator
//!
//! Two of the six arguments (`k = 4·cos(i/21)` and `sin(i/19)`) advance linearly in the point
//! index, so a rotate-by-a-fixed-angle recurrence could produce them with no table at all.
//! But the other four are *data-dependent* — they turn on `d`, on the phase `t`, on `k`
//! itself — and a recurrence cannot touch them. A table serves all six through one code path,
//! and, unlike a recurrence stepped five thousand times a frame, it cannot drift: entry
//! *n* is exactly `sin(2πn/N)` every frame, forever.
//!
//! ## Why linear interpolation
//!
//! A raw nearest-entry lookup off a 2048-point table has a worst-case error around the size
//! of one table step — visible as a faint banding in a smooth sweep. One lerp between
//! neighbours drops the error by more than two orders of magnitude for the cost of one
//! multiply and one add, which is what [`SinTable::sin`] does. The result is
//! [proven](SinTable::sin) to sit within a small tolerance of `libm` across the whole circle.

use alloc::vec;
use alloc::vec::Vec;
use core::f32::consts::{FRAC_PI_2, TAU};

/// The table's size as a power of two, so the wrap-around after interpolation is a mask
/// (`& (LEN - 1)`) rather than a modulo. Eleven bits — 2048 entries — is the smallest table
/// whose interpolated error is invisible on the glass; see the module docs.
const BITS: usize = 11;

/// How many samples of one turn the table holds: `2^BITS`.
pub const LEN: usize = 1 << BITS;

/// The mask that wraps a table index into `0..LEN` — valid because [`LEN`] is a power of two.
const MASK: usize = LEN - 1;

/// Index units per radian: a whole turn (`TAU`) spans [`LEN`] entries.
const INDEX_PER_RADIAN: f32 = LEN as f32 / TAU;

/// A full circle of sines, sampled `LEN` times.
///
/// Built once — [`SinTable::new`] pays the [`LEN`] `libm::sinf` calls at startup so the render
/// loop never pays one. The samples live in a **heap** allocation (`LEN × 4` bytes, 8 KiB at
/// 2048 entries), deliberately: the table is far larger than a task stack can spare, and
/// building it as a stack array before moving it to the heap would overflow a bring-up stack
/// on the way there. So the buffer is heap-allocated *first* and filled in place — the 8 KiB
/// never sits on the stack. Found on the metal: an earlier stack-resident build double-faulted
/// the ESP32 into a boot loop before the first frame.
pub struct SinTable {
    samples: Vec<f32>,
}

impl SinTable {
    /// Fill the table: entry *n* is `sin(2πn / LEN)`.
    ///
    /// This is the *only* place `libm`'s transcendental is called in anger. The buffer is
    /// allocated on the heap up front (`vec!`) and filled in place, so the 8 KiB of samples is
    /// never a stack temporary — see the type docs. A `while` loop rather than an iterator so
    /// the whole thing reads as "a table being filled" at a glance; it runs once, at bring-up.
    pub fn new() -> Self {
        let mut samples: Vec<f32> = vec![0.0; LEN];
        let mut n: usize = 0;
        while n < LEN {
            samples[n] = libm::sinf(n as f32 * (TAU / LEN as f32));
            n += 1;
        }
        Self { samples }
    }

    /// `sin(theta)` for any real `theta`, read from the table with linear interpolation.
    ///
    /// The argument is scaled into index units and reduced onto the ring with `floorf` + a
    /// power-of-two mask, so a negative or many-turns-large angle is as cheap as a small one —
    /// which matters, because the field feeds this angles like `e·14 − d·3 + 2t` that grow
    /// without bound as the animation runs.
    ///
    /// The two neighbouring entries are blended by the fractional index, so the returned value
    /// tracks the true sine to within a small tolerance everywhere on the circle rather than
    /// stepping between 2048 discrete levels.
    pub fn sin(&self, theta: f32) -> f32 {
        let index: f32 = theta * INDEX_PER_RADIAN;
        let whole: f32 = libm::floorf(index);
        let frac: f32 = index - whole;

        // Reduce the integer part onto the ring with a power-of-two mask, not `rem_euclid`. The
        // table spans a whole turn in `LEN` entries and `LEN` is a power of two, so the ring
        // index is the low `BITS` bits of `floor(index)`, which `& MASK` extracts — and in two's
        // complement that is the correct non-negative residue even for a negative angle
        // (`-1 & MASK == LEN - 1`). This deletes the per-lookup `f32 → i64` conversion and 64-bit
        // `rem_euclid`, both software-emulated on this FPU-without-integer-divide Xtensa core;
        // measured at six lookups a point over five thousand points, they were the bulk of a
        // frame's compute. Masking only the low bits, the cast's sign-extension is irrelevant, so
        // this reads the same on the 32-bit target and the 64-bit host.
        let lo: usize = (whole as i32) as usize & MASK;
        let hi: usize = (lo + 1) & MASK;

        self.samples[lo] * (1.0 - frac) + self.samples[hi] * frac
    }

    /// `cos(theta)`, as the sine a quarter turn ahead — one table serves both.
    pub fn cos(&self, theta: f32) -> f32 {
        self.sin(theta + FRAC_PI_2)
    }
}

impl Default for SinTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The interpolated table must track the true sine everywhere. This is the tolerance the
    /// whole optimisation rests on: if a lookup drifts further than this from `libm`, the
    /// picture is wrong, not merely cheap.
    const TOLERANCE: f32 = 1e-3;

    /// Zero: the table's origin is the sine's origin.
    #[test]
    fn sine_of_zero_is_zero() {
        assert!(SinTable::new().sin(0.0).abs() < TOLERANCE);
    }

    /// One: a single named angle lands where trigonometry says it must — a quarter turn is the
    /// crest.
    #[test]
    fn sine_of_a_quarter_turn_is_one() {
        assert!((SinTable::new().sin(FRAC_PI_2) - 1.0).abs() < TOLERANCE);
    }

    /// One, for the sibling: cosine is the sine a quarter turn ahead, so cosine of zero is the
    /// crest. This is the whole of `cos`'s correctness.
    #[test]
    fn cosine_of_zero_is_one() {
        assert!((SinTable::new().cos(0.0) - 1.0).abs() < TOLERANCE);
    }

    proptest! {
        /// Many: across a wide span of angles the interpolated lookup tracks `libm::sinf` within
        /// tolerance. The range reaches ±2000 rad on purpose — the field feeds this angles that
        /// grow with the phase `t` (roughly `2t`, hundreds of radians after minutes of running),
        /// so the reduction is proven exactly where the animation drives it, not just near the
        /// origin. The mask reduction must agree with the true sine over the whole span; a
        /// regression there would band or tear a long-running frond.
        #[test]
        fn the_table_tracks_libm_across_the_circle(theta in -2000.0f32..2000.0) {
            let table: SinTable = SinTable::new();
            prop_assert!((table.sin(theta) - libm::sinf(theta)).abs() < TOLERANCE);
            prop_assert!((table.cos(theta) - libm::cosf(theta)).abs() < TOLERANCE);
        }

        /// Many, on the ring: an angle and the same angle a whole turn away read the same, so a
        /// forever-growing argument never walks off the table or accumulates a seam.
        #[test]
        fn a_full_turn_reads_the_same(theta in -10.0f32..10.0) {
            let table: SinTable = SinTable::new();
            prop_assert!((table.sin(theta) - table.sin(theta + TAU)).abs() < TOLERANCE);
        }
    }
}
