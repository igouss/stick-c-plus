#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # orbits-core
//!
//! The pure heart of the orbits sketch: a grid of grey cells lit by the brightest of thirty
//! sweeping L1 "diamond" blooms, grained by a static noise texture — a comet with a sharp tail
//! drifting across the canvas, drawn as a function of nothing but a virtual frame. No framework, no
//! I/O, no state — the orbits at frame `f` are [`for_each_cell`]`(f, …)` and nothing else — so the
//! whole thing is verified on the host and cross-compiles to Xtensa unchanged.
//!
//! - [`centres`] / [`Column`] / [`bloom`] — the thirty orbit [`Centre`]s and the diamond each throws:
//!   the picture's geometry. The centres are the port's real idea (see below); a [`Column`] folds a
//!   column's worth of them for cheap reading; the bloom is the `max(0, max_n(SOURCE - L1·(3 + n)))`
//!   the source builds per cell.
//! - [`noise`] — the fixed grain a cell is multiplied by, a pure hash of its coordinates.
//! - [`for_each_cell`] — the picture: bloom times grain, clamped, walked column by column over the
//!   [`COLS`]×[`COLS`] grid of [`STEP`]-unit cells, with the frame's centres hoisted out once.
//! - [`frame`] — the clock: elapsed milliseconds to the source's virtual `f`.
//!
//! ## Optimised for the chip, honestly
//!
//! The M5StickC Plus is a classic ESP32 (Xtensa LX6): a single-precision **FPU**, and **no SIMD**.
//! The source's `50×50` grid over thirty orbits is expensive only if taken literally — it
//! recomputes each orbit's centre inside the per-cell loop, and computes those centres with
//! `acos(cos(.))`. This crate does neither. It recognises `acos(cos(θ))` as a **triangle wave** (a
//! floor and an abs, no transcendental — proven to track the reference in [`orbit`]'s tests); it
//! hoists the thirty frame-invariant centres **out** of the per-cell loop; and it folds the taxicab
//! distance per [`Column`], paying the x-part once a column and dropping the orbits that cannot reach
//! it, so a cell costs only the y-part of the few surviving orbits and a max — all `f32`
//! add/multiply/abs the FPU does in stride, no division, no sine table. Unlike the source there is no
//! `noise` buffer to allocate either: the grain is hashed on read, which keeps the sketch off a heap
//! that is already tight. What is worth proving is that the wave *is* the transcendental and the
//! bloom *is* the source's; both are pinned by property tests.

mod clock;
mod field;
mod noise;
mod orbit;

pub use clock::{frame, FRAME_MS};
pub use field::{for_each_cell, COLS, STEP};
pub use noise::noise;
pub use orbit::{bloom, centres, Centre, Column, ORBITS, SOURCE};
