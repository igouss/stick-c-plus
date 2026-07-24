#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # plume-core
//!
//! The pure heart of the plume app: a slow-breathing feathered frond, drawn as a cloud of
//! points that is a function of nothing but an animation phase. No framework, no I/O, no state
//! — frame *N* is [`plume`]`(`[`phase`]`(N))` and nothing else — so the whole thing is verified
//! on the host and cross-compiles to Xtensa unchanged.
//!
//! - [`field`] — the parametric point field: [`point`] for one point, [`plume`] for the whole
//!   frond at a phase, in the original 400×400 canvas space. A faithful port of a *Dwitter*,
//!   with the point budget halved to what a 135×240 panel can show.
//! - [`phase`] — the animation clock: wall-clock milliseconds to a phase on the ring `[0, 2π)`,
//!   reproducing the source's per-frame step and wrapping so an `f32` never loses precision.
//! - [`SinTable`] — the startup-built sine table the field's trigonometry is read from, so the
//!   render loop pays no per-point transcendental.
//!
//! ## Optimised for the chip, honestly
//!
//! The M5StickC Plus is a classic ESP32 (Xtensa LX6): a single-precision **FPU**, and **no
//! SIMD** — the vector instructions are an ESP32-S3 feature, and even there they would need
//! `unsafe` this workspace forbids. So the speed here comes from what the chip actually has,
//! not from a vector unit it does not:
//!
//! - the [`SinTable`] turns thirty thousand software sines a frame into table reads;
//! - [`STEP`] halves the point budget to what the smaller panel can distinguish;
//! - every value is `f32`, so the field runs on the FPU rather than in soft-float `f64`.
//!
//! Each of those is a property test away from proof: the table [tracks
//! `libm`](SinTable::sin), the field [tracks an `f64` reference](field), the phase [stays on
//! the ring](phase).

mod field;
mod phase;
mod trig;

pub use field::{plume, point, FieldPoint, POINT_COUNT, START, STEP};
pub use phase::{phase, FRAME_MS, PERIOD_FRAMES, PHASE_PER_FRAME};
pub use trig::SinTable;
