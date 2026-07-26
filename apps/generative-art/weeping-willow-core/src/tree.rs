//! The tree: the wood and the foliage, assembled — the piece's public face.
//!
//! A [`Tree`] owns the still [`Skeleton`](crate::skeleton) and the moving [`Foliage`](crate::foliage)
//! and hands the display exactly two things: the [`wood`](Tree::wood) to stroke in bark, folded once
//! and never changing, and the [`sway`](Tree::sway) — every frond point at a phase, the canopy's
//! whole motion. Both are pure: the tree holds no clock and no state, so the same phase always draws
//! the same tree, and the piece is verified on the host and cross-compiles to Xtensa unchanged.

use platform_numerics::SinTable;

use crate::foliage::{Foliage, FrondPoint};
use crate::geometry::Segment;
use crate::skeleton::Skeleton;

/// A whole weeping willow: the wood folded once and the foliage draped each frame.
///
/// The phase-invariant capital of the piece — the trunk and boughs, and each frond's depth-shape —
/// all folded at construction onto the **heap** (see the crate root: the fold must not build a
/// multi-kilobyte array on the composition root's 8 KiB main stack). The `Tree` itself is then just a
/// few pointers — cheap to move into the display thread's closure by value.
pub struct Tree {
    /// The still wood and the frond anchors, folded once.
    skeleton: Skeleton,
    /// The frond depth-shape, draped over the skeleton's anchors each frame.
    foliage: Foliage,
}

impl Tree {
    /// Fold the whole tree once: the wood and the foliage's depth-shape. Everything phase-invariant
    /// is paid here, so a frame pays only its sway.
    pub fn new() -> Self {
        Self {
            skeleton: Skeleton::new(),
            foliage: Foliage::new(),
        }
    }

    /// The wood's limbs, for the display to stroke in bark — the trunk's, then each bough's. Static:
    /// the same every frame, so the display may stroke it straight without a phase.
    pub fn wood(&self) -> &[Segment] {
        self.skeleton.wood()
    }

    /// Every point of every frond at phase `φ`, read through `table` — the canopy's whole motion.
    ///
    /// The render loop's per-frame call: drape the folded foliage over the folded anchors at this
    /// frame's phase. Borrows `self` and `table` for the life of the iterator.
    pub fn sway<'a>(
        &'a self,
        phi: f32,
        table: &'a SinTable,
    ) -> impl Iterator<Item = FrondPoint> + 'a {
        self.foliage.drape(self.skeleton.anchors(), phi, table)
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foliage::POINT_COUNT;
    use crate::skeleton::{FROND_COUNT, WOOD_SEGMENTS};

    /// One: a tree stands on wood — its limb list is the whole frame, trunk and boughs, so the piece
    /// is a tree and not a bare curtain.
    #[test]
    fn a_tree_stands_on_its_wood() {
        let tree: Tree = Tree::new();
        assert_eq!(tree.wood().len(), WOOD_SEGMENTS);
    }

    /// Many: the swept canopy is every frond's every point — the whole foliage at a phase.
    #[test]
    fn the_sway_is_the_whole_canopy() {
        let tree: Tree = Tree::new();
        let table: SinTable = SinTable::new();
        let count: usize = tree.sway(0.0, &table).count();
        assert_eq!(count, FROND_COUNT * POINT_COUNT);
    }

    /// The canopy actually sways: two different phases draw two different sets of points, so the tree
    /// moves in the wind rather than freezing.
    #[test]
    fn the_canopy_sways_with_the_phase() {
        let tree: Tree = Tree::new();
        let table: SinTable = SinTable::new();
        let still: Vec<FrondPoint> = tree.sway(0.0, &table).collect();
        let blown: Vec<FrondPoint> = tree.sway(1.0, &table).collect();
        assert_ne!(
            still, blown,
            "the canopy did not sway as the phase advanced"
        );
    }
}
