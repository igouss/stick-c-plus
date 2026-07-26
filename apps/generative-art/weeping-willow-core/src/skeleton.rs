//! The wood: a trunk that forks into arcing boughs, and the anchors the foliage hangs from.
//!
//! The tree's static frame, folded once at startup and never touched again per frame. It is two
//! things the [`Tree`](crate::Tree) hands on:
//!
//! - **the wood** — a trunk rising from the ground to a fork, and [`BOUGH_COUNT`] boughs arcing up
//!   and out from that fork, both as a flat list of tapering [`Segment`]s the display strokes in bark.
//! - **the anchors** — points sampled along the boughs from which the foliage's fronds hang. Each
//!   carries where it is, how far its frond droops, and the phase lag that ripples the wind across
//!   the canopy.
//!
//! Everything here is pure geometry in `[0, 1]` fractions and is computed at construction: the bough
//! Béziers, the ragged droop lengths, the per-anchor lags. A frame pays none of it.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::geometry::{Point2, Segment};

/// The trunk's foot, at the bottom-centre of the panel — the tree stands on the ground.
const BASE: Point2 = Point2::new(0.5, 1.0);
/// The fork, where the trunk ends and the boughs begin — a little above the panel's middle, so the
/// canopy has the upper half and the trunk the lower.
const FORK: Point2 = Point2::new(0.5, 0.52);

/// How many limbs the trunk is drawn as. A handful, so its taper from foot to fork is stepwise-smooth
/// without paying for limbs no one can see.
const TRUNK_SEGMENTS: usize = 3;
/// The trunk's half-width at the foot and at the fork, as fractions of the panel width — fat at the
/// ground, narrowing to meet the boughs.
const TRUNK_FOOT_HALF_WIDTH: f32 = 0.025;
const TRUNK_FORK_HALF_WIDTH: f32 = 0.010;

/// How many boughs fork from the trunk. Odd, so one bough climbs straight up the centre and the rest
/// fan evenly to either side.
pub(crate) const BOUGH_COUNT: usize = 5;
/// How many limbs each bough is drawn as — enough that its arc reads as a curve, not a dogleg.
const BOUGH_SEGMENTS: usize = 6;
/// A bough's half-width at the fork and at its tip, as fractions of the panel width — thin where it
/// leaves the trunk, thinner still where the fronds take over.
const BOUGH_FORK_HALF_WIDTH: f32 = 0.011;
const BOUGH_TIP_HALF_WIDTH: f32 = 0.0038;

/// The left and right edges of the canopy: the outermost bough tips reach these fractions of the
/// width, so the crown spans the middle three-quarters of the panel.
const RIM_LEFT_X: f32 = 0.12;
const RIM_RIGHT_X: f32 = 0.88;
/// The height of the crown: the centre bough tip reaches this fraction from the top, and the outer
/// tips droop [`CROWN_DROP`] lower, so the canopy is a rounded dome rather than a flat line.
const CROWN_Y: f32 = 0.34;
const CROWN_DROP: f32 = 0.14;
/// How far above its straight chord a bough bows, as a fraction of the height — the gentle upward arc
/// that makes a bough climb before the fronds fall from it, the shape a weeping willow's branches
/// take. Kept small, so no bough overshoots into a bare spike above the canopy.
const ARC: f32 = 0.06;

/// How many fronds hang from each bough, spaced along its length.
pub(crate) const ANCHORS_PER_BOUGH: usize = 7;
/// Where along a bough the fronds start hanging: fronds drape from `[ANCHOR_T_START, 1]` of each
/// bough — nearly from the fork, so the foliage covers the wood rather than leaving a bare limb
/// standing through the canopy's centre.
const ANCHOR_T_START: f32 = 0.10;
/// The height the fronds' tips gather around, as a fraction of the panel: a frond is just long enough
/// to reach here from its anchor, so the foliage has a hem near the panel's four-fifths rather than
/// each frond stopping at its own random depth.
const BASELINE_Y: f32 = 0.82;
/// The shortest a frond may droop and the extra a length-hash may add, both fractions of the height —
/// so even a low anchor's frond hangs a little, and the hem is ragged rather than ruled.
const MIN_DROOP: f32 = 0.14;
const RAGGED: f32 = 0.10;
/// How many radians of phase lag span the canopy's full width: a frond's lag is its `x` fraction
/// times this, so a gust is seen to travel across the tree left to right rather than every frond
/// swinging as one.
const SPREAD_LAG: f32 = 5.0;

/// How many fronds make up the whole canopy — one per anchor, [`ANCHORS_PER_BOUGH`] on each of
/// [`BOUGH_COUNT`] boughs.
pub const FROND_COUNT: usize = BOUGH_COUNT * ANCHORS_PER_BOUGH;
/// How many limbs the whole wood is drawn as — the trunk's, plus each bough's.
pub const WOOD_SEGMENTS: usize = TRUNK_SEGMENTS + BOUGH_COUNT * BOUGH_SEGMENTS;

/// One frond's phase-invariant shape: where it hangs from, how far it droops, and its share of the
/// travelling wind.
#[derive(Clone, Copy)]
pub(crate) struct Anchor {
    /// The point on a bough the frond hangs from, in `[0, 1]` fractions.
    pub(crate) base: Point2,
    /// The frond's droop length as a fraction of the panel height — how far below its anchor the
    /// tip falls.
    pub(crate) length: f32,
    /// The frond's phase offset, `base.x · SPREAD_LAG`, that ripples the wind across the canopy.
    pub(crate) lag: f32,
}

/// The tree's static frame: the wood to stroke and the anchors to hang fronds from, folded once.
///
/// Both are **heap** slices, `collect`ed at construction so no full array is ever a stack temporary
/// (the crate is built on the 8 KiB main task; see the crate root). Held by the [`Tree`](crate::Tree)
/// for the app's life and never rebuilt — a frame reads them, it does not fold them.
pub(crate) struct Skeleton {
    /// Every limb of the wood — the trunk's, then each bough's — in one flat heap slice.
    wood: Box<[Segment]>,
    /// Every frond's anchor, [`ANCHORS_PER_BOUGH`] per bough.
    anchors: Box<[Anchor]>,
}

impl Skeleton {
    /// Fold the whole frame once: the trunk and bough limbs with their tapers, and the anchors with
    /// their droop lengths and lags. The Béziers, the ragged-hem hashes and the lags are all paid
    /// here, at startup, never per frame. `collect`ed onto the heap element by element, so the fold
    /// never builds an array on the stack.
    pub(crate) fn new() -> Self {
        let wood: Box<[Segment]> = (0..WOOD_SEGMENTS)
            .map(limb)
            .collect::<Vec<Segment>>()
            .into();
        let anchors: Box<[Anchor]> = (0..FROND_COUNT).map(anchor).collect::<Vec<Anchor>>().into();
        Self { wood, anchors }
    }

    /// The wood's limbs, for the display to stroke in bark.
    pub(crate) fn wood(&self) -> &[Segment] {
        &self.wood
    }

    /// The frond anchors, for the foliage to drape and sway.
    pub(crate) fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }
}

/// A straight interpolation of two scalars — `a` at `t = 0`, `b` at `t = 1`. The tapers and the
/// crown spread are built from it; paid at startup only.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The `bough`th bough's Bézier: its fork end, its upward control point, and its tip.
///
/// The tip fans across the rim by the bough's index and droops lower toward the edges, so the crown
/// is a dome; the control point sits above the fork–tip chord by [`ARC`], so the bough bows upward
/// before the fronds fall from it.
fn bough_curve(bough: usize) -> (Point2, Point2, Point2) {
    let fraction: f32 = bough as f32 / (BOUGH_COUNT - 1) as f32; // 0 at the left bough, 1 at the right
    let tip_x: f32 = lerp(RIM_LEFT_X, RIM_RIGHT_X, fraction);
    let from_centre: f32 = 2.0 * libm::fabsf(fraction - 0.5); // 0 at the centre bough, 1 at the edges
    let tip: Point2 = Point2::new(tip_x, CROWN_Y + CROWN_DROP * from_centre);
    let chord_mid: Point2 = FORK.lerp(tip, 0.5);
    let control: Point2 = Point2::new(chord_mid.x, chord_mid.y - ARC); // above the chord: bows upward
    (FORK, control, tip)
}

/// A point on a quadratic Bézier at parameter `u`, from `f` through control `c` to `t`.
fn bezier(f: Point2, c: Point2, t: Point2, u: f32) -> Point2 {
    let inv: f32 = 1.0 - u;
    let weight_f: f32 = inv * inv;
    let weight_c: f32 = 2.0 * inv * u;
    let weight_t: f32 = u * u;
    Point2::new(
        weight_f * f.x + weight_c * c.x + weight_t * t.x,
        weight_f * f.y + weight_c * c.y + weight_t * t.y,
    )
}

/// The `flat`th limb of the wood: a trunk limb for the first [`TRUNK_SEGMENTS`], then a bough limb.
///
/// The flat index is decomposed back to which limb it is — a trunk step, or bough `b`'s step `k` —
/// so the whole wood is one `from_fn` sweep with no heap and no intermediate vector.
fn limb(flat: usize) -> Segment {
    if flat < TRUNK_SEGMENTS {
        let u0: f32 = flat as f32 / TRUNK_SEGMENTS as f32;
        let u1: f32 = (flat + 1) as f32 / TRUNK_SEGMENTS as f32;
        Segment {
            a: BASE.lerp(FORK, u0),
            b: BASE.lerp(FORK, u1),
            half_width_a: lerp(TRUNK_FOOT_HALF_WIDTH, TRUNK_FORK_HALF_WIDTH, u0),
            half_width_b: lerp(TRUNK_FOOT_HALF_WIDTH, TRUNK_FORK_HALF_WIDTH, u1),
        }
    } else {
        let within: usize = flat - TRUNK_SEGMENTS;
        let (f, c, t): (Point2, Point2, Point2) = bough_curve(within / BOUGH_SEGMENTS);
        let step: usize = within % BOUGH_SEGMENTS;
        let u0: f32 = step as f32 / BOUGH_SEGMENTS as f32;
        let u1: f32 = (step + 1) as f32 / BOUGH_SEGMENTS as f32;
        Segment {
            a: bezier(f, c, t, u0),
            b: bezier(f, c, t, u1),
            half_width_a: lerp(BOUGH_FORK_HALF_WIDTH, BOUGH_TIP_HALF_WIDTH, u0),
            half_width_b: lerp(BOUGH_FORK_HALF_WIDTH, BOUGH_TIP_HALF_WIDTH, u1),
        }
    }
}

/// The `flat`th frond anchor: sampled along its bough's outer length, drooping just far enough to
/// reach the hem plus a ragged hash, and lagged by its `x` so the wind travels across the canopy.
fn anchor(flat: usize) -> Anchor {
    let (f, c, t): (Point2, Point2, Point2) = bough_curve(flat / ANCHORS_PER_BOUGH);
    let step: usize = flat % ANCHORS_PER_BOUGH;
    let along: f32 = lerp(
        ANCHOR_T_START,
        1.0,
        step as f32 / (ANCHORS_PER_BOUGH - 1) as f32,
    );
    let base: Point2 = bezier(f, c, t, along);
    let reach: f32 = BASELINE_Y - base.y; // the anchor sits above the hem, so this is positive
    Anchor {
        base,
        length: libm::fmaxf(reach, MIN_DROOP) + RAGGED * length_hash(flat),
        lag: base.x * SPREAD_LAG,
    }
}

/// A frond's ragged-hem hash in `[0, 1)`: a deterministic pseudo-random value from its index, so the
/// canopy's bottom is uneven. The classic `frac(sin(i · 12.9898) · 43758.5453)` shader hash, paid
/// once per frond at startup.
fn length_hash(index: usize) -> f32 {
    let mixed: f32 = libm::sinf(index as f32 * 12.9898) * 43_758.547;
    mixed - libm::floorf(mixed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// One: the trunk rises from the ground — its first limb's foot is the panel's bottom-centre, so
    /// the tree is rooted, not floating.
    #[test]
    fn the_trunk_rises_from_the_ground() {
        let skeleton: Skeleton = Skeleton::new();
        assert_eq!(skeleton.wood()[0].a, BASE);
    }

    /// The fork joins wood to wood: the trunk's last limb ends exactly where every bough's first limb
    /// begins, so the boughs grow out of the trunk and do not float above it.
    #[test]
    fn the_boughs_grow_from_the_forks_top() {
        let skeleton: Skeleton = Skeleton::new();
        let trunk_top: Point2 = skeleton.wood()[TRUNK_SEGMENTS - 1].b;
        assert_eq!(trunk_top, FORK);
        let first_bough_limb: Point2 = skeleton.wood()[TRUNK_SEGMENTS].a;
        assert_eq!(first_bough_limb, FORK);
    }

    /// The wood narrows as it climbs: the trunk's foot is wider than any bough's tip, so the tree
    /// tapers from a stout base to fine branch ends.
    #[test]
    fn the_wood_tapers_from_foot_to_tip() {
        let skeleton: Skeleton = Skeleton::new();
        let foot: f32 = skeleton.wood()[0].half_width_a;
        let finest_tip: f32 = skeleton
            .wood()
            .iter()
            .map(|limb: &Segment| limb.half_width_b)
            .fold(f32::INFINITY, f32::min);
        assert!(foot > finest_tip, "foot {foot} !> finest tip {finest_tip}");
    }

    /// The boughs spread both sides of the trunk: some anchor hangs left of centre and some to the
    /// right, so the canopy is a crown and not a single vertical stripe.
    #[test]
    fn the_canopy_spreads_both_sides_of_the_trunk() {
        let skeleton: Skeleton = Skeleton::new();
        let leftmost: f32 = skeleton
            .anchors()
            .iter()
            .map(|anchor: &Anchor| anchor.base.x)
            .fold(f32::INFINITY, f32::min);
        let rightmost: f32 = skeleton
            .anchors()
            .iter()
            .map(|anchor: &Anchor| anchor.base.x)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(leftmost < 0.5, "nothing hangs left of centre: {leftmost}");
        assert!(
            rightmost > 0.5,
            "nothing hangs right of centre: {rightmost}"
        );
    }

    /// Many: the wood is every limb — the trunk's and every bough's — and the canopy every frond.
    #[test]
    fn the_frame_is_every_limb_and_every_frond() {
        let skeleton: Skeleton = Skeleton::new();
        assert_eq!(skeleton.wood().len(), WOOD_SEGMENTS);
        assert_eq!(skeleton.anchors().len(), FROND_COUNT);
    }

    proptest! {
        /// Many: every anchor hangs inside the panel and above the hem — its base is in the unit
        /// square, and its frond's tip (`base.y + length`) does not fall off the bottom edge — so no
        /// frond is ever anchored or drooped off-panel.
        #[test]
        fn every_frond_hangs_on_the_panel(flat in 0usize..FROND_COUNT) {
            let skeleton: Skeleton = Skeleton::new();
            let anchor: &Anchor = &skeleton.anchors()[flat];
            prop_assert!((0.0..=1.0).contains(&anchor.base.x), "x {}", anchor.base.x);
            prop_assert!((0.0..BASELINE_Y).contains(&anchor.base.y), "y {}", anchor.base.y);
            let tip_y: f32 = anchor.base.y + anchor.length;
            prop_assert!(tip_y <= 1.0, "tip y {tip_y} falls off the bottom");
        }
    }
}
