#![forbid(unsafe_code)]
//! # host-shell
//!
//! The host monitor's imperative shell — the part that owns the frame cache and the poller
//! thread yet stays device-independent, so it is verified on the host with the same
//! discipline as the pure domain.
//!
//! The pure core (`host-core`) decides *what a frame is* (the clamping/gap transform) and
//! *when a cached reading has gone stale*. This crate drives that core against the outside
//! world:
//!
//! - [`SharedMetrics`] — the last good [`Pulse`](host_core::Pulse) frame plus the latest
//!   fetch outcome, shared between the poller (the one writer) and the display (reader).
//!   Poison-tolerant, so a panic in any holder can neither corrupt the cache nor crash the
//!   others.
//! - [`spawn_poller`] — the sized background thread: fetch the injected
//!   [`PulseSource`](host_core::PulseSource) adapter → replace the cached frame with the one
//!   it returned (each fetch carries the whole window, so there is nothing to accumulate) or,
//!   on failure, publish the classified fault. It owns the timing and the cache but takes an
//!   injected [`Clock`](platform_core::Clock), so the domain stays pure and the composition
//!   root supplies the one `Monotonic` every thread shares.
//!
//! The display is `platform_runtime::spawn_display` fed a source that reads this cache (a
//! [`HostState`](host_core::HostState) snapshot) and a `Screen` that paints a
//! `host_display::Glass`.
//!
//! ## Host-testable shell
//!
//! Everything here is `std`, but nothing is ESP-specific: `std::thread`, `Arc`, `Mutex` and
//! `Instant` all run on the host under `cargo test` *and* cross-compile to
//! `xtensa-esp32-espidf` unchanged (esp-idf maps `std::thread` onto a FreeRTOS task). So the
//! staleness-on-death and panic-isolation guarantees are proven here, off the metal; the
//! composition root only has to wire the real HTTP [`PulseSource`](host_core::PulseSource)
//! adapter in.

mod poller;
mod shared;

pub use poller::{
    spawn_poller, Poller, PollerConfig, MAX_POLL_PERIOD, MIN_POLL_PERIOD, POLLER_STACK_SIZE,
    STALENESS_PERIODS,
};
pub use shared::SharedMetrics;
