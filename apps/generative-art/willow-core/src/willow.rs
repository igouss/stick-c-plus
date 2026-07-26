//! The curtain: hanging tendrils that sway in a travelling wind-wave.
//!
//! An **original** generative frond, not a port. The willow is a curtain of [`STRAND_COUNT`]
//! tendrils, each hanging from the top edge and drooping to a ragged bottom. A tendril is a chain of
//! [`POINT_COUNT`] points; each point sways sideways by a sine of the phase, and three things make
//! that sway read as *wind through a willow* rather than a metronome:
//!
//! - **the tip sways more than the root.** The sway is scaled by `s^1.5` along the tendril (`s` from
//!   `0` at the anchored root to `1` at the free tip), so the root barely stirs and the tip streams.
//! - **the wave travels down the tendril.** The sine's phase advances with `s` ([`WAVE_NUMBER`]), so
//!   a gust visibly runs from root to tip rather than the whole strand swinging rigidly.
//! - **neighbouring tendrils lag.** Each strand's phase is offset by its index ([`STRAND_LAG`]), so
//!   the curtain ripples across its width instead of moving as one sheet.
//!
//! There is no state and no history — the curtain at phase `φ` is [`Curtain::sway`]`(φ, table)` and
//! nothing else.
//!
//! ## Authored in fractions, not pixels
//!
//! Being original, the willow is beholden to no source canvas, so it is authored in **normalised
//! `[0, 1]` fractions**: `x` a fraction of the panel width, `y` a fraction of its height. The display
//! scales those onto whatever panel it has, so the aspect is the display's to apply and never this
//! crate's to assume — the sway, a horizontal motion, is a fraction of the width, and the droop a
//! fraction of the height. There is no projection-and-crop step as the ports need, because there is
//! no square source to fit.
//!
//! ## Cheap by construction
//!
//! Everything that does not depend on the phase is folded into the [`Curtain`] once at startup: each
//! strand's root x, its phase lag and its droop length; each point's depth `s`, its sway gain
//! `s^1.5` and its share of the travelling wave. A frame then costs, per point, one add, one
//! [`SinTable`] lookup and a couple of multiplies — no `sqrt`, no transcendental, no division on the
//! hot path. At a few thousand points that is a fraction of the plume's budget, so the curtain runs
//! fast and smooth.

use platform_numerics::SinTable;

/// How many tendrils hang across the curtain's width. Evenly spaced, so the panel's width is divided
/// into this many columns and one strand falls down the middle of each.
pub const STRAND_COUNT: usize = 24;

/// How many points make up one tendril from root to tip. Dense enough that the display's short
/// bridging run between consecutive points draws a near-solid strand rather than a dotted line.
pub const POINT_COUNT: usize = 112;

/// The tip's sway, as a fraction of the panel width: at full deflection a tip swings this far from
/// its resting column. The root's sway is zero and it grows as `s^1.5` to here.
const SWAY: f32 = 0.13;

/// How many radians of the travelling wave span one tendril, root to tip: the sine's phase advances
/// by this much from `s = 0` to `s = 1`, so a little under two full waves ride down a strand and a
/// gust is seen to travel rather than the whole strand swinging as one.
const WAVE_NUMBER: f32 = 1.7 * core::f32::consts::TAU;

/// The phase lag between neighbouring tendrils, in radians: strand `i` is offset by `i · STRAND_LAG`,
/// so the curtain ripples across its width instead of moving as a single rigid sheet.
const STRAND_LAG: f32 = 0.7;

/// The shortest and the span of a tendril's droop, as fractions of the panel height: a strand's
/// length is `SHORTEST_DROOP + hash · DROOP_SPAN`, so the curtain's bottom is ragged, not a ruled
/// line.
const SHORTEST_DROOP: f32 = 0.72;
const DROOP_SPAN: f32 = 0.26;

/// One point on a tendril at a phase: where it sits on the panel, in `[0, 1]` fractions, and how far
/// down its strand it is.
///
/// Carried by value — a plain result. Turning the fractions into pixels and the depth into a foliage
/// colour is the display crate's business; this crate says only where the point is and how deep.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StrandPoint {
    /// The point's x as a fraction of the panel width. The sway can carry it a little past `[0, 1]`;
    /// the display clips what falls off the edge.
    pub x: f32,
    /// The point's y as a fraction of the panel height, in `[0, 1]` — `depth` scaled by the strand's
    /// droop length.
    pub y: f32,
    /// How far down the strand this point is, `0.0` at the root and `1.0` at the tip — the depth the
    /// display ramps from deep green to a light tip.
    pub depth: f32,
}

/// One strand's phase-invariant shape: where it is anchored and how long it droops.
#[derive(Clone, Copy)]
struct Strand {
    /// The resting x of the strand's root, a fraction of the panel width — the column it hangs down.
    root_x: f32,
    /// The strand's phase offset, `i · STRAND_LAG`, that ripples the curtain across its width.
    lag: f32,
    /// The strand's droop length as a fraction of the panel height, in `[SHORTEST_DROOP, …]`.
    length: f32,
}

/// One point-index's phase-invariant shape: how deep it is and how it takes the wave.
#[derive(Clone, Copy)]
struct Node {
    /// The point's depth `s = index / (POINT_COUNT - 1)`, `0.0` root to `1.0` tip — also its colour
    /// depth.
    depth: f32,
    /// The point's sway gain `s^1.5`, so the tip streams and the root barely stirs.
    gain: f32,
    /// The point's share of the travelling wave, `s · WAVE_NUMBER`, added into the sway's phase.
    wave: f32,
}

/// The curtain's phase-invariant capital, folded once at startup: every strand's anchor and every
/// point-index's shape, so a frame is only the sway.
///
/// Held by the display for the life of the app and swept each frame by [`sway`](Self::sway). Fixed
/// arrays, so the whole curtain is a few kilobytes on no heap at all.
pub struct Curtain {
    /// The [`STRAND_COUNT`] tendrils' anchors and lengths.
    strands: [Strand; STRAND_COUNT],
    /// The [`POINT_COUNT`] point-indices' depths, gains and wave shares.
    nodes: [Node; POINT_COUNT],
}

impl Curtain {
    /// Fold the curtain's phase-invariant shape once: the strands' anchors and droop lengths, and
    /// each point-index's depth, sway gain and wave share. The `sqrt` of the sway gain and the length
    /// hash are paid here, at startup, never per frame.
    pub fn new() -> Self {
        let strands: [Strand; STRAND_COUNT] = core::array::from_fn(|i: usize| Strand {
            root_x: (i as f32 + 0.5) / STRAND_COUNT as f32,
            lag: i as f32 * STRAND_LAG,
            length: SHORTEST_DROOP + DROOP_SPAN * length_hash(i),
        });
        let nodes: [Node; POINT_COUNT] = core::array::from_fn(|index: usize| {
            let s: f32 = index as f32 / (POINT_COUNT - 1) as f32;
            Node {
                depth: s,
                gain: s * libm::sqrtf(s), // s^1.5 — the tip streams, the root barely stirs
                wave: s * WAVE_NUMBER,
            }
        });
        Self { strands, nodes }
    }

    /// One point of one strand at phase `φ`, read through `table`.
    ///
    /// The whole per-frame cost of the willow, per point: the sway is `gain · sin(φ + wave + lag)`,
    /// one [`SinTable`] lookup; `x` is the root plus that sway scaled by [`SWAY`], and `y` is the
    /// depth scaled by the strand's droop length. No `sqrt`, no division — the shape was folded at
    /// startup.
    fn point(&self, strand: &Strand, node: &Node, phi: f32, table: &SinTable) -> StrandPoint {
        let sway: f32 = node.gain * table.sin(phi + node.wave + strand.lag);
        StrandPoint {
            x: strand.root_x + SWAY * sway,
            y: node.depth * strand.length,
            depth: node.depth,
        }
    }

    /// Every point of every tendril at phase `φ`, strand by strand, root to tip.
    ///
    /// The curtain's whole picture at a phase: the outer sweep is the strands across the width, the
    /// inner the points down each strand. Borrows `self` and `table` for the life of the iterator —
    /// the render loop's shape: build the curtain once, sweep it each frame.
    pub fn sway<'a>(
        &'a self,
        phi: f32,
        table: &'a SinTable,
    ) -> impl Iterator<Item = StrandPoint> + 'a {
        self.strands.iter().flat_map(move |strand: &Strand| {
            self.nodes
                .iter()
                .map(move |node: &Node| self.point(strand, node, phi, table))
        })
    }
}

impl Default for Curtain {
    fn default() -> Self {
        Self::new()
    }
}

/// A strand's length hash in `[0, 1)`: a deterministic pseudo-random value from its index, so the
/// curtain's bottom is ragged. The classic `frac(sin(i · 12.9898) · 43758.5453)` shader hash, paid
/// once per strand at startup.
fn length_hash(index: usize) -> f32 {
    let mixed: f32 = libm::sinf(index as f32 * 12.9898) * 43_758.547;
    mixed - libm::floorf(mixed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The sway is one table sine scaled by `s^1.5`, so a point's x must track that `f64` reference
    /// within the table's tolerance, with a little room for the gain's own `f32` `sqrt`.
    const X_TOLERANCE: f64 = 2e-3;

    /// Zero: a root point (depth zero) does not sway at all — its gain is `s^1.5 = 0`, so it sits on
    /// its resting column whatever the phase, the fixed anchor a hanging strand needs.
    #[test]
    fn a_root_point_never_sways() {
        let curtain: Curtain = Curtain::new();
        let table: SinTable = SinTable::new();
        let strand: &Strand = &curtain.strands[5];
        let root: &Node = &curtain.nodes[0];
        let a: StrandPoint = curtain.point(strand, root, 0.0, &table);
        let b: StrandPoint = curtain.point(strand, root, 1.7, &table);
        assert_eq!(a.x, strand.root_x);
        assert_eq!(a.x, b.x);
    }

    /// One: the tip sways further than a mid point at the same phase — the `s^1.5` gain grows with
    /// depth, which is what makes the curtain stream at the bottom and hang still at the top. Chosen
    /// at a phase where the raw sine is near its peak for both, so the gains are what separate them.
    #[test]
    fn the_tip_sways_more_than_the_middle() {
        let curtain: Curtain = Curtain::new();
        let table: SinTable = SinTable::new();
        let strand: &Strand = &curtain.strands[0];
        // Strand 0 has lag 0; pick a phase near a sine peak so both points swing the same way.
        let phi: f32 = core::f32::consts::FRAC_PI_2;
        let mid: StrandPoint = curtain.point(strand, &curtain.nodes[POINT_COUNT / 2], phi, &table);
        let tip: StrandPoint = curtain.point(strand, &curtain.nodes[POINT_COUNT - 1], phi, &table);
        let mid_sway: f32 = (mid.x - strand.root_x).abs();
        let tip_sway: f32 = (tip.x - strand.root_x).abs();
        assert!(tip_sway > mid_sway, "tip {tip_sway} !> mid {mid_sway}");
    }

    /// Many: the curtain is the whole set of points — every strand, every node.
    #[test]
    fn the_curtain_is_every_point() {
        let curtain: Curtain = Curtain::new();
        let table: SinTable = SinTable::new();
        let count: usize = curtain.sway(0.0, &table).count();
        assert_eq!(count, STRAND_COUNT * POINT_COUNT);
    }

    /// The strands are ordered across the width: root xs strictly increase, so the curtain spans the
    /// panel left to right with no two tendrils sharing a column.
    #[test]
    fn the_strands_span_the_width_in_order() {
        let curtain: Curtain = Curtain::new();
        let ordered: bool = curtain
            .strands
            .windows(2)
            .all(|pair: &[Strand]| pair[0].root_x < pair[1].root_x);
        assert!(ordered, "strand roots are not left-to-right");
    }

    proptest! {
        /// Many: across a strand, a depth and a full turn of phase, a point's x tracks the `f64`
        /// reference `root + SWAY · s^1.5 · sin(φ + s · WAVE_NUMBER + i · STRAND_LAG)` within
        /// tolerance — the sway the whole picture rests on, pinned everywhere the animation drives it.
        #[test]
        fn a_point_tracks_the_reference(
            strand_index in 0usize..STRAND_COUNT,
            node_index in 0usize..POINT_COUNT,
            phi in 0.0f32..core::f32::consts::TAU,
        ) {
            let curtain: Curtain = Curtain::new();
            let table: SinTable = SinTable::new();
            let strand: &Strand = &curtain.strands[strand_index];
            let node: &Node = &curtain.nodes[node_index];
            let got: StrandPoint = curtain.point(strand, node, phi, &table);

            let s: f64 = node_index as f64 / (POINT_COUNT - 1) as f64;
            let gain: f64 = s.powf(1.5);
            let arg: f64 = phi as f64 + s * WAVE_NUMBER as f64 + strand_index as f64 * STRAND_LAG as f64;
            let want_x: f64 = strand.root_x as f64 + SWAY as f64 * gain * arg.sin();
            prop_assert!((got.x as f64 - want_x).abs() < X_TOLERANCE, "x {} vs {}", got.x, want_x);
        }
    }
}
