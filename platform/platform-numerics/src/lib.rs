#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # platform-numerics
//!
//! Board-generic numerics for a chip with no fast transcendental. The one thing here is the
//! [`SinTable`] — a startup-built sine table the platform's animated sketches read all their
//! trigonometry from, so the render loop pays a table lookup where it would otherwise pay a
//! software sine.
//!
//! It lived inside `plume-core` while the plume was the only animation on the board. The
//! generative-art gallery made it shared: every sketch — the plume, the squares, the fan, the
//! orbits — evaluates trigonometry on the same hot path, and one proven table serves them all
//! rather than a copy per app. So it is lifted here, `context = "shared"`, where any bounded
//! context may depend on it without a context violation.
//!
//! - [`SinTable`] — a full circle of sines sampled once at startup, read with linear
//!   interpolation; [proven](SinTable::sin) within a small tolerance of `libm` across the
//!   whole circle.

extern crate alloc;

mod trig;

pub use trig::{SinTable, LEN};
