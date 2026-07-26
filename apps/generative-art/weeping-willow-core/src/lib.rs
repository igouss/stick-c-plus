#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # weeping-willow-core
//!
//! The pure heart of the weeping-willow-*tree* sketch: a rooted tree — a trunk that forks into arcing
//! boughs, from which a canopy of slender fronds cascades down and streams in a travelling wind-wave
//! — drawn as a function of nothing but an animation phase. An **original** generative piece, not a
//! port; and the tree to `willow-core`'s curtain — where that piece hangs its strands from the top
//! edge, this one grows them from real wood. No framework, no I/O, no state, so
//! the whole thing is verified on the host and cross-compiles to Xtensa unchanged.
//!
//! - [`Tree`] — the phase-invariant capital: the still wood (trunk and boughs) and each frond's
//!   depth-shape, folded once at startup. It hands the display the [`wood`](Tree::wood) to stroke and
//!   the [`sway`](Tree::sway) — every frond point at a phase.
//! - [`Swarm`] — a cloud of fireflies that wander the canopy and pulse, some behind the tree and some
//!   in front, on the same phase clock; the scene's life around the still wood and swaying foliage.
//! - [`phase`] — the animation clock: wall-clock milliseconds to a sway phase on the ring `[0, 2π)`,
//!   wrapping so an `f32` never loses precision however long the wind blows.
//!
//! ## What makes it a weeping willow, and cheap
//!
//! It is a tree, not a curtain: a [`Skeleton`](skeleton) of real wood carries the foliage. The trunk
//! rises from the ground and forks into boughs that bow *upward* and spread into a domed crown; the
//! [`Foliage`](foliage) then hangs a frond from points along each bough, and each frond falls to a
//! ragged hem and sways — its tip streaming (`s^1.5`), a wave travelling down it, and neighbouring
//! fronds lagging so a gust ripples across the canopy. Being original, it is authored in normalised
//! `[0, 1]` fractions — the display scales them onto the panel, so there is no square source to fit
//! and crop. And everything phase-invariant — the bough Béziers, the tapers, the droop lengths, the
//! depth-shape — is folded into the [`Tree`] at startup, so a frame costs, per frond point, one add,
//! one [`SinTable`](platform_numerics::SinTable) lookup and a couple of multiplies. The wood is
//! stroked straight from its folded limbs and never recomputed.

// The tree's capital — the wood, the anchors, the foliage's depth-shape and the firefly swarm — is
// built on the **heap**, not in fixed stack arrays: this crate is constructed on the composition
// root's 8 KiB main task, where a multi-kilobyte stack temporary would overflow the frame and corrupt
// the heap before the first line of `main` runs (see `large-buffer-heap-not-stack`). `collect`ing the
// capital lands each element in the heap one at a time, so no N-sized array is ever a stack
// temporary. `alloc` is memory, not I/O, so the crate stays a pure functional core.
extern crate alloc;

mod fireflies;
mod foliage;
mod geometry;
mod phase;
mod skeleton;
mod tree;

pub use fireflies::{Firefly, Swarm, FIREFLY_COUNT};
pub use foliage::{FrondPoint, POINT_COUNT};
pub use geometry::{Point2, Segment};
pub use phase::{phase, PERIOD_MS};
pub use skeleton::{FROND_COUNT, WOOD_SEGMENTS};
pub use tree::Tree;

// The sine table lives in the shared platform, so every sketch in the gallery reads its trigonometry
// from one proven copy. Re-exported so `weeping_willow_core::SinTable` names the type a caller passes
// into [`Tree::sway`] without depending on `platform-numerics` directly.
pub use platform_numerics::SinTable;
