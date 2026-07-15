#![forbid(unsafe_code)]
//! # plant-shell
//!
//! The plant monitor's imperative shell — the part that owns the moisture cache and the
//! sampler thread yet stays device-independent, so it is verified on the host with the same
//! discipline as the pure domain.
//!
//! The pure core (`plant-core`) decides *what* a reading means and *when* a cached one has
//! gone stale. This crate drives that core against the outside world:
//!
//! - [`SharedMoisture`] — the latest reading, shared between the sampler (the one writer) and
//!   the display + native-API server (readers). Poison-tolerant, so a panic in any one holder
//!   can neither corrupt the cache nor crash the others.
//! - [`spawn_sampler`] — the sized background thread: read the ADC adapter → fold through the
//!   pure [`step`](plant_core::sampler::step) → publish the latest moisture. It owns the
//!   timing and the cache but takes an injected [`Clock`](platform_core::Clock), so the domain
//!   stays pure and the composition root supplies the one `Monotonic` every thread shares.
//!
//! The board-generic machinery this once held — the `Monotonic` clock and the change-
//! suppressing display render loop — now lives in `platform-runtime`, shared with every app;
//! the plant monitor's display is `platform_runtime::spawn_display` fed a source that reads
//! this cache and a `Screen` that paints a `plant_display::Glass`.
//!
//! ## Host-testable shell
//!
//! Everything here is `std`, but nothing is ESP-specific: `std::thread`, `Arc`, `Mutex` and
//! `Instant` all run on the host under `cargo test` *and* cross-compile to
//! `xtensa-esp32-espidf` unchanged (esp-idf maps `std::thread` onto a FreeRTOS task). So the
//! staleness-on-death and panic-isolation guarantees are proven here, off the metal; the
//! composition root only has to wire the real [`EarthUnit`](plant_core::SoilSensor) adapter in.

mod sampler;
mod shared;

pub use sampler::{spawn_sampler, Sampler, SamplerConfig};
pub use shared::SharedMoisture;
