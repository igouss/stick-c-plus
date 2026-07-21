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
//!   the render loop reads from, stamped with when the pose was last confirmed.
//! - [`spawn_sampler`] — the high-rate poll loop: read the IMU, fold the reading through the
//!   pure smoother, and publish the pose it implies against the injected clock.
//!
//! ## How a dead sensor becomes visible
//!
//! A failed IMU read is logged and skipped, and the last good pose stays on the glass — the
//! right call for the flaky single transaction this is overwhelmingly likely to be, and the
//! same fail-visible policy the render loop and the power-watch already follow.
//!
//! What keeps that from becoming a frozen-but-plausible readout is that a skip does not
//! restamp. Every publication carries the time it was confirmed, every read is handed an age,
//! and [`Signal`](orientation_core::Signal) turns that age into a verdict the screen draws
//! distinctly. So the two cases separate themselves without the sampler needing to tell them
//! apart: a glitch costs a couple of missed refreshes and nothing more, while a sensor that
//! stops answering ages its last pose out and the glass says `NO SIGNAL`.

mod sampler;
mod shared;

pub use sampler::{spawn_sampler, SamplerConfig, SamplerTask, SAMPLER_STACK_SIZE, SAMPLE_PERIOD};
pub use shared::SharedOrientation;
