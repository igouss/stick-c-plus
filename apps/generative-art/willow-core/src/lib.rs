#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # willow-core
//!
//! The pure heart of the willow sketch: a curtain of hanging tendrils that sway in a travelling
//! wind-wave, drawn as a function of nothing but an animation phase. An **original** generative
//! frond — not a port of a *Dwitter* like the squares, fan and orbits, but designed for this
//! pipeline from the first line. No framework, no I/O, no state — the curtain at phase `φ` is
//! [`Curtain::sway`]`(φ, table)` and nothing else — so the whole thing is verified on the host and
//! cross-compiles to Xtensa unchanged.
//!
//! - [`Curtain`] — the phase-invariant capital (each strand's anchor and droop, each point's depth
//!   and wave share), folded once at startup, then swept each frame into [`StrandPoint`]s.
//! - [`phase`] — the animation clock: wall-clock milliseconds to a sway phase on the ring `[0, 2π)`,
//!   wrapping so an `f32` never loses precision however long the wind blows.
//!
//! ## What makes it a willow, and cheap
//!
//! Three touches turn a row of swinging strings into wind through a willow: the tip sways more than
//! the root (`s^1.5`), the wave travels down each strand ([`WAVE_NUMBER`](crate)), and neighbouring
//! strands lag in phase so the curtain ripples across its width. Being original, it is authored in
//! normalised `[0, 1]` fractions — the display scales them onto the panel, so there is no square
//! source to fit and crop. And everything phase-invariant is folded into the [`Curtain`] at startup,
//! so a frame costs, per point, one add, one [`SinTable`](platform_numerics::SinTable) lookup and a
//! couple of multiplies — no `sqrt`, no transcendental, no division on the hot path. What is worth
//! proving is that the sway [tracks its reference](willow) and the phase [stays on the ring](phase);
//! both are pinned by tests.

mod phase;
mod willow;

pub use phase::{phase, PERIOD_MS};
pub use willow::{Curtain, StrandPoint, POINT_COUNT, STRAND_COUNT};

// The sine table lives in the shared platform, so every sketch in the gallery reads its trigonometry
// from one proven copy. Re-exported so `willow_core::SinTable` names the type a caller passes into
// [`Curtain::sway`] without depending on `platform-numerics` directly.
pub use platform_numerics::SinTable;
