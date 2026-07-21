#![forbid(unsafe_code)]
//! # orientation-shell
//!
//! The orientation readout's imperative shell: the shared pose cache and the sampler thread
//! that fills it.
//!
//! Everything here is `std` (a background thread, `Arc`/`Mutex`), but nothing is ESP-specific
//! — `std::thread` and `Arc` run on the host under `cargo test` *and* cross-compile to
//! `xtensa-esp32-espidf` unchanged (the esp-idf std maps `std::thread` onto a FreeRTOS task).
//! So the whole sample-smooth-publish cycle is proven off the metal against a fake
//! [`Imu`](platform_core::Imu), and a firmware bin supplies only the real MPU6886 adapter.
//!
//! - [`SharedOrientation`] — the one-writer, one-reader cache the sampler publishes into and
//!   the render loop snapshots from.
//! - [`spawn_sampler`] — the high-rate poll loop: read the IMU, fold the reading through the
//!   pure smoother, and publish the pose it implies.
//!
//! ## A known limit
//!
//! A failed IMU read is logged and skipped, and the last good pose stays on the glass — the
//! right call for the flaky single transaction this is overwhelmingly likely to be, and the
//! same fail-visible policy the render loop and the power-watch already follow. The cost is
//! that a *persistently* dead sensor leaves a frozen-but-plausible readout, findable in the
//! log rather than on the screen. Surfacing that on the glass wants a staleness clock and a
//! "no signal" state on the view; it is tracked rather than quietly ignored.

mod sampler;
mod shared;

pub use sampler::{spawn_sampler, SamplerConfig, SamplerTask, SAMPLER_STACK_SIZE, SAMPLE_PERIOD};
pub use shared::SharedOrientation;
