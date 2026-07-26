//! The foliage: slender fronds that hang from the boughs and stream in a travelling wind-wave.
//!
//! Where the [`skeleton`](crate::skeleton) is the still wood, the foliage is the motion. A frond
//! hangs from an [`Anchor`] on a bough and falls straight down to its droop length; along the way
//! each point sways sideways by a sine of the phase, and three touches make that sway read as *wind
//! through a willow* rather than a swinging rope:
//!
//! - **the tip streams, the anchor holds.** The sway is scaled by `s^1.5` down the frond (`s` from
//!   `0` at the anchored top to `1` at the free tip), so the point where it joins the bough barely
//!   stirs and the tip streams.
//! - **the wave travels down the frond.** The sine's phase advances with `s` ([`WAVE_NUMBER`]), so a
//!   gust is seen to run from bough to tip rather than the whole frond swinging rigidly.
//! - **the canopy ripples across its width.** Each frond's phase is lagged by its anchor's `x`
//!   ([`Anchor::lag`](crate::skeleton)), so a gust travels across the tree instead of every frond
//!   moving as one.
//!
//! There is no state and no history — a frond point at phase `φ` is a pure function of its anchor,
//! its depth and `φ`. Every depth-shape (`s`, the `s^1.5` gain, the wave share) is folded once into
//! the [`Foliage`] at startup, so a frame costs, per point, one add, one [`SinTable`] lookup and a
//! couple of multiplies — no `sqrt`, no transcendental, no division on the hot path.

use alloc::boxed::Box;
use alloc::vec::Vec;

use platform_numerics::SinTable;

use crate::skeleton::Anchor;

/// How many points make up one frond from anchor to tip. Dense enough that the display's short
/// bridging run between consecutive points draws a near-solid frond rather than a dotted line.
pub const POINT_COUNT: usize = 72;

/// The tip's sway, as a fraction of the panel width: at full deflection a tip swings this far from
/// its resting column. The anchor's sway is zero and it grows as `s^1.5` to here.
const SWAY: f32 = 0.09;

/// How many radians of the travelling wave span one frond, anchor to tip: the sine's phase advances
/// by this much from `s = 0` to `s = 1`, so a little over one and a half full waves ride down a frond
/// and a gust is seen to travel rather than the whole frond swinging as one.
const WAVE_NUMBER: f32 = 1.6 * core::f32::consts::TAU;

/// One point on a frond at a phase: where it sits on the panel, in `[0, 1]` fractions, and how far
/// down its frond it is.
///
/// Carried by value — a plain result. Turning the fractions into pixels and the depth into a foliage
/// colour is the display crate's business; this crate says only where the point is and how deep.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FrondPoint {
    /// The point's x as a fraction of the panel width. The sway can carry it a little past `[0, 1]`;
    /// the display clips what falls off the edge.
    pub x: f32,
    /// The point's y as a fraction of the panel height — the anchor's `y` plus `depth` scaled by the
    /// frond's droop length.
    pub y: f32,
    /// How far down the frond this point is, `0.0` at the anchor and `1.0` at the tip — the depth the
    /// display ramps from deep green to a light tip.
    pub depth: f32,
}

/// One point-index's phase-invariant shape: how deep it is and how it takes the wave.
///
/// Shared by every frond — the depth ladder is the same down each — so it is folded once, not per
/// frond.
#[derive(Clone, Copy)]
struct Node {
    /// The point's depth `s = index / (POINT_COUNT - 1)`, `0.0` anchor to `1.0` tip — also its colour
    /// depth.
    depth: f32,
    /// The point's sway gain `s^1.5`, so the tip streams and the anchor barely stirs.
    gain: f32,
    /// The point's share of the travelling wave, `s · WAVE_NUMBER`, added into the sway's phase.
    wave: f32,
}

/// The foliage's phase-invariant capital: the depth-shape of a frond, folded once at startup and
/// draped over every anchor each frame.
///
/// Just the [`POINT_COUNT`] nodes — the anchors are the [`skeleton`](crate::skeleton)'s — on a **heap**
/// slice, `collect`ed at construction so it is never a stack temporary (see the crate root). Held by
/// the [`Tree`](crate::Tree) and swept each frame by [`drape`](Self::drape).
pub(crate) struct Foliage {
    /// The [`POINT_COUNT`] point-indices' depths, gains and wave shares, shared by every frond.
    nodes: Box<[Node]>,
}

impl Foliage {
    /// Fold the frond's depth-shape once: each point-index's depth, sway gain `s^1.5` and wave share.
    /// The `sqrt` of the gain is paid here, at startup, never per frame. `collect`ed onto the heap so
    /// the fold builds no array on the stack.
    pub(crate) fn new() -> Self {
        let nodes: Box<[Node]> = (0..POINT_COUNT)
            .map(|index: usize| {
                let s: f32 = index as f32 / (POINT_COUNT - 1) as f32;
                Node {
                    depth: s,
                    gain: s * libm::sqrtf(s), // s^1.5 — the tip streams, the anchor barely stirs
                    wave: s * WAVE_NUMBER,
                }
            })
            .collect::<Vec<Node>>()
            .into();
        Self { nodes }
    }

    /// One point of one frond at phase `φ`, read through `table`.
    ///
    /// The whole per-frame cost of the foliage, per point: the sway is `gain · sin(φ + wave + lag)`,
    /// one [`SinTable`] lookup; `x` is the anchor plus that sway scaled by [`SWAY`], and `y` is the
    /// anchor's depth plus this point's depth scaled by the frond's droop length. No `sqrt`, no
    /// division — the shape was folded at startup.
    fn point(&self, anchor: &Anchor, node: &Node, phi: f32, table: &SinTable) -> FrondPoint {
        let sway: f32 = node.gain * table.sin(phi + node.wave + anchor.lag);
        FrondPoint {
            x: anchor.base.x + SWAY * sway,
            y: anchor.base.y + node.depth * anchor.length,
            depth: node.depth,
        }
    }

    /// Every point of every frond hanging from `anchors` at phase `φ`, frond by frond, anchor to tip.
    ///
    /// The canopy's whole picture at a phase: the outer sweep is the fronds across the boughs, the
    /// inner the points down each frond. Borrows `self` and `table` for the life of the iterator —
    /// the render loop's shape: fold the foliage once, drape it each frame.
    pub(crate) fn drape<'a>(
        &'a self,
        anchors: &'a [Anchor],
        phi: f32,
        table: &'a SinTable,
    ) -> impl Iterator<Item = FrondPoint> + 'a {
        anchors.iter().flat_map(move |anchor: &Anchor| {
            self.nodes
                .iter()
                .map(move |node: &Node| self.point(anchor, node, phi, table))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skeleton::Skeleton;
    use proptest::prelude::*;

    /// The sway is one table sine scaled by `s^1.5`, so a point's x must track that `f64` reference
    /// within the table's tolerance, with a little room for the gain's own `f32` `sqrt`.
    const X_TOLERANCE: f64 = 2e-3;

    /// Zero: an anchor point (depth zero) does not sway at all — its gain is `s^1.5 = 0`, so it sits
    /// on the bough whatever the phase, the fixed join a hanging frond needs.
    #[test]
    fn an_anchor_point_never_sways() {
        let skeleton: Skeleton = Skeleton::new();
        let foliage: Foliage = Foliage::new();
        let table: SinTable = SinTable::new();
        let anchor: &Anchor = &skeleton.anchors()[7];
        let top: &Node = &foliage.nodes[0];
        let a: FrondPoint = foliage.point(anchor, top, 0.0, &table);
        let b: FrondPoint = foliage.point(anchor, top, 1.7, &table);
        assert_eq!(a.x, anchor.base.x);
        assert_eq!(a.x, b.x);
    }

    /// The frond hangs downward: its tip sits below its anchor — `y` grows with depth — so a frond
    /// droops and never rises.
    #[test]
    fn a_frond_hangs_below_its_anchor() {
        let skeleton: Skeleton = Skeleton::new();
        let foliage: Foliage = Foliage::new();
        let table: SinTable = SinTable::new();
        let anchor: &Anchor = &skeleton.anchors()[3];
        let top: FrondPoint = foliage.point(anchor, &foliage.nodes[0], 0.4, &table);
        let tip: FrondPoint = foliage.point(anchor, &foliage.nodes[POINT_COUNT - 1], 0.4, &table);
        assert!(tip.y > top.y, "tip y {} !> anchor y {}", tip.y, top.y);
    }

    /// One: the tip sways further than a mid point at the same phase — the `s^1.5` gain grows with
    /// depth, which is what makes the frond stream at its end and hang still at the bough. Chosen at a
    /// phase near the sine's peak for both, so the gains are what separate them.
    #[test]
    fn the_tip_sways_more_than_the_middle() {
        let skeleton: Skeleton = Skeleton::new();
        let foliage: Foliage = Foliage::new();
        let table: SinTable = SinTable::new();
        // Anchor 0's lag is small; pick a phase near a sine peak so both points swing the same way.
        let anchor: &Anchor = &skeleton.anchors()[0];
        let phi: f32 = core::f32::consts::FRAC_PI_2;
        let mid: FrondPoint = foliage.point(anchor, &foliage.nodes[POINT_COUNT / 2], phi, &table);
        let tip: FrondPoint = foliage.point(anchor, &foliage.nodes[POINT_COUNT - 1], phi, &table);
        let mid_sway: f32 = (mid.x - anchor.base.x).abs();
        let tip_sway: f32 = (tip.x - anchor.base.x).abs();
        assert!(tip_sway > mid_sway, "tip {tip_sway} !> mid {mid_sway}");
    }

    /// Many: the draped canopy is the whole set of points — every anchor, every node.
    #[test]
    fn the_canopy_is_every_point() {
        let skeleton: Skeleton = Skeleton::new();
        let foliage: Foliage = Foliage::new();
        let table: SinTable = SinTable::new();
        let count: usize = foliage.drape(skeleton.anchors(), 0.0, &table).count();
        assert_eq!(count, skeleton.anchors().len() * POINT_COUNT);
    }

    proptest! {
        /// Many: across the anchors, a depth and a full turn of phase, a point's x tracks the `f64`
        /// reference `base.x + SWAY · s^1.5 · sin(φ + s · WAVE_NUMBER + lag)` within tolerance — the
        /// sway the whole picture rests on, pinned everywhere the animation drives it.
        #[test]
        fn a_point_tracks_the_reference(
            anchor_index in 0usize..crate::skeleton::FROND_COUNT,
            node_index in 0usize..POINT_COUNT,
            phi in 0.0f32..core::f32::consts::TAU,
        ) {
            let skeleton: Skeleton = Skeleton::new();
            let foliage: Foliage = Foliage::new();
            let table: SinTable = SinTable::new();
            let anchor: &Anchor = &skeleton.anchors()[anchor_index];
            let node: &Node = &foliage.nodes[node_index];
            let got: FrondPoint = foliage.point(anchor, node, phi, &table);

            let s: f64 = node_index as f64 / (POINT_COUNT - 1) as f64;
            let gain: f64 = s.powf(1.5);
            let arg: f64 = phi as f64 + s * WAVE_NUMBER as f64 + anchor.lag as f64;
            let want_x: f64 = anchor.base.x as f64 + SWAY as f64 * gain * arg.sin();
            prop_assert!((got.x as f64 - want_x).abs() < X_TOLERANCE, "x {} vs {}", got.x, want_x);
        }
    }
}
