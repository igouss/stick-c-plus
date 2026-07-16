#![forbid(unsafe_code)]
//! # host-shell
//!
//! The host monitor's imperative shell — the part that owns the metrics cache and the
//! poller thread yet stays device-independent, so it is verified on the host with the
//! same discipline as the pure domain.
//!
//! The pure core (`host-core`) decides *what* two scrapes mean (the CPU rate, the memory
//! level) and *when* a cached reading has gone stale. This crate drives that core against
//! the outside world:
//!
//! - [`SharedMetrics`] — the rolling [`History`](host_core::History) plus the latest
//!   scrape outcome, shared between the poller (the one writer) and the display (reader).
//!   Poison-tolerant, so a panic in any holder can neither corrupt the cache nor crash
//!   the others.
//! - [`spawn_poller`] — the sized background thread: poll the injected
//!   [`MetricsSource`](host_core::MetricsSource) adapter → fold the scrape through the
//!   pure stateful [`step`](host_core::step) → push any [`Sample`](host_core::Sample)
//!   into the history and publish the latest status. It owns the timing and the cache but
//!   takes an injected [`Clock`](platform_core::Clock), so the domain stays pure and the
//!   composition root supplies the one `Monotonic` every thread shares.
//!
//! The display is `platform_runtime::spawn_display` fed a source that reads this cache
//! (a [`HostState`](host_core::HostState) snapshot) and a `Screen` that paints a
//! `host_display::Glass`.
//!
//! ## Host-testable shell
//!
//! Everything here is `std`, but nothing is ESP-specific: `std::thread`, `Arc`, `Mutex`
//! and `Instant` all run on the host under `cargo test` *and* cross-compile to
//! `xtensa-esp32-espidf` unchanged (esp-idf maps `std::thread` onto a FreeRTOS task). So
//! the staleness-on-death and panic-isolation guarantees are proven here, off the metal;
//! the composition root only has to wire the real HTTP [`MetricsSource`](host_core::MetricsSource)
//! adapter in.

mod poller;
mod shared;

pub use poller::{
    spawn_poller, Poller, PollerConfig, POLLER_STACK_SIZE, POLL_PERIOD, STALENESS_PERIODS,
};
pub use shared::SharedMetrics;
