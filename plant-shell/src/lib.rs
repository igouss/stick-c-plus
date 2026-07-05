#![forbid(unsafe_code)]
//! # plant-shell
//!
//! The plant monitor's imperative shell — the part that owns effects (a
//! background thread, a shared cache, a clock) yet stays device-independent, so
//! it is verified on the host with the same discipline as the pure domain.
//!
//! The pure core (`plant-core`) decides *what* a reading means and *when* a
//! cached one has gone stale. This crate drives that core against the outside
//! world:
//!
//! - [`SharedMoisture`] — the latest reading, shared between the sampler (the one
//!   writer) and the display + native-API server (readers). Poison-tolerant, so a
//!   panic in any one holder can neither corrupt the cache nor crash the others.
//! - [`Monotonic`] — one monotonic millisecond clock, the single time base the
//!   writer and readers share so their freshness math agrees.
//! - [`spawn_sampler`] — the sized background thread: read the ADC adapter → fold
//!   through the pure [`step`](plant_core::sampler::step) → publish the latest
//!   moisture. Owns the timing and the cache; the domain stays pure.
//! - [`spawn_display`] — the companion background thread: read the cache → render
//!   the freshest measurement through a [`MoistureDisplay`](plant_core::MoistureDisplay)
//!   adapter (the ST7789 TFT), showing *unavailable* the moment a reading ages out.
//!
//! ## Host-testable shell
//!
//! Everything here is `std`, but nothing here is ESP-specific: `std::thread`,
//! `Arc`, `Mutex` and `Instant` all run on the host under `cargo test` *and*
//! cross-compile to `xtensa-esp32-espidf` unchanged (esp-idf maps `std::thread`
//! onto a FreeRTOS task, honouring [`stack_size`](std::thread::Builder::stack_size)).
//! So the staleness-on-death and panic-isolation guarantees are proven here, off
//! the metal; the composition root only has to wire the real
//! [`EarthUnit`](plant_core::SoilSensor) adapter in.

mod clock;
mod display;
mod sampler;
mod shared;

pub use clock::Monotonic;
pub use display::{spawn_display, DisplayConfig, DisplayTask, DISPLAY_STACK_SIZE, RENDER_PERIOD};
pub use sampler::{
    spawn_sampler, Sampler, SamplerConfig, SAMPLER_STACK_SIZE, SAMPLE_PERIOD, STALENESS_PERIODS,
};
pub use shared::SharedMoisture;
