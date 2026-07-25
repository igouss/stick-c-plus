#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # squares-core
//!
//! The pure heart of the squares sketch: a grid of nested-square frames that breathe and flip
//! sign in a diagonal wave, drawn as a function of nothing but an animation phase. No framework,
//! no I/O, no state — cell `(col, row)` at phase `φ` breathes by one sine and nothing else — so
//! the whole thing is verified on the host and cross-compiles to Xtensa unchanged.
//!
//! - [`grid`] — the breathing grid: [`amplitude`] for one cell's signed breath at a phase, and
//!   [`cells`] for the whole grid, a faithful port of a *Dwitter*. The grid's shape ([`COLS`] ×
//!   [`ROWS`]) is the artwork's structure and lives here; a cell's *pixel* size is the display
//!   crate's business.
//! - [`phase`] — the animation clock: wall-clock milliseconds to a phase on the ring `[0, 2π)`,
//!   wrapping so an `f32` never loses precision however long the animation runs.
//!
//! ## Optimised for the chip, honestly
//!
//! The M5StickC Plus is a classic ESP32 (Xtensa LX6): a single-precision **FPU**, and **no
//! SIMD**. The grid is small — a few dozen cells — so there is no field to precompute here; a
//! cell's per-frame cost is one [`SinTable`](platform_numerics::SinTable) lookup of a per-cell
//! phase offset that is a single multiply-add, not a table to hoist. Every value is `f32`, for
//! the FPU. The one thing worth proving is that the table-and-`f32` breath [tracks an `f64`
//! reference](grid); the phase [stays on the ring](phase).

extern crate alloc;

mod grid;
mod phase;

pub use grid::{amplitude, cells, Cell, COLS, OFFSET_STEP, ROWS};
pub use phase::{phase, PERIOD_MS};

// The sine table lives in the shared platform, so every sketch in the gallery reads its
// trigonometry from one proven copy. Re-exported so `squares_core::SinTable` names the type a
// caller passes into [`amplitude`]/[`cells`] without depending on `platform-numerics` directly.
pub use platform_numerics::SinTable;
