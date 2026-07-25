#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # fan-core
//!
//! The pure heart of the fan sketch: radial columns of triangles that fold open and closed in a
//! wave spreading from the centre, each hued by its distance from that centre, drawn as a function
//! of nothing but an animation phase. No framework, no I/O, no state — the fan at phase `φ` is
//! [`cells`]`(φ, table)` and nothing else — so the whole thing is verified on the host and
//! cross-compiles to Xtensa unchanged.
//!
//! - [`fan`] — the folding triangles: [`cells`] sweeps the source's `500×500` grid of them, each a
//!   [`Cell`] of three canvas-space vertices and a hue. A faithful port of a *Dwitter*. The fold is
//!   one [`SinTable`](platform_numerics::SinTable) lookup; the hue and the fold's phase both turn
//!   on the triangle's distance from the centre, an honest `sqrt` paid per cell.
//! - [`phase`] — the animation clock: wall-clock milliseconds to a phase on the ring `[0, 2π)`,
//!   wrapping so an `f32` never loses precision however long the animation runs.
//!
//! ## Optimised for the chip, honestly
//!
//! The M5StickC Plus is a classic ESP32 (Xtensa LX6): a single-precision **FPU**, and **no
//! SIMD**. The grid is a few hundred triangles, so a frame is a few hundred `sqrt`s and table
//! sines — cheap enough that there is no field to precompute here (unlike the plume's seven and a
//! half thousand points); the colour, the pricier per-cell work, is baked once by the display, not
//! here. Every value is `f32`, for the FPU. What is worth proving is that the table-and-`f32` fold
//! [tracks an `f64` reference](fan); the phase [stays on the ring](phase).

extern crate alloc;

mod fan;
mod phase;

pub use fan::{cells, Cell, CENTRE, COLUMN_STEP, ROW_STEP, SOURCE};
pub use phase::{phase, PERIOD_MS};

// The sine table lives in the shared platform, so every sketch in the gallery reads its
// trigonometry from one proven copy. Re-exported so `fan_core::SinTable` names the type a caller
// passes into [`cells`] without depending on `platform-numerics` directly.
pub use platform_numerics::SinTable;
