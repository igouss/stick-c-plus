#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # host-core
//!
//! Framework-free host-metrics core for the M5StickC Plus host monitor.
//!
//! Pure `no_std`, zero dependencies, no allocation: the `0..=100`% value type, the bounded
//! per-host series the sparklines are drawn from, the whole-frame [`Pulse`] a single
//! hostpulse fetch yields, the pure transform that clamps and gap-handles a payload into
//! one, and the staleness rule. Everything here is deterministic and host-testable — the
//! Xtensa side only performs the HTTP `GET`, deserializes the JSON, and hands the frame in.
//!
//! ## Why this shrank
//!
//! The monitor used to scrape each host's node_exporter and do the PromQL `rate()` on the
//! device — a streaming Prometheus-text parser, a stateful CPU-rate fold, a rolling
//! history. The hostpulse endpoint now does the `rate()` server-side and returns a
//! ready-to-plot per-host CPU/mem series for every host in one bearer-gated call, so all of
//! that arithmetic is gone. The domain's job is now only to *hold N hosts × two `%`-series
//! and hand them to the display*.
//!
//! ## Hexagon
//! - **Entities**: [`Percent`] — a CPU/memory reading, its `0..=100` invariant enforced at
//!   construction; [`Series`] — one host's bounded, oldest-first `%` sequence with gaps kept
//!   as [`None`]; [`HostSeries`] — a named host's CPU + memory series; [`Pulse`] — one whole
//!   fetch's grid plus every host's series. All `Copy + Eq`, so the frame can be the render
//!   loop's by-value state.
//! - **Control / policy**: [`PulseBuilder`] — the pure JSON→model transform (minus the JSON):
//!   an adapter parses the wire and pushes each host's raw values, and the builder owns the
//!   clamping (into `0..=100`) and gap handling (a `null` stays a gap, never a `0`). A pure
//!   function of its inputs.
//! - **Entities**: [`Status`] and [`HostFault`] — what a consumer learns when it asks the
//!   cache about the endpoint. Like the plant monitor's `Observation`, it carries two
//!   orthogonal facts a bare `Option` cannot: whether the *poller* is alive, and whether the
//!   *endpoint* answered.
//! - **Control / policy**: [`observe`] — the staleness rule that turns a cached [`Reading`]
//!   into a [`Status`], so a poller that dies is never mistaken for a live one.
//! - **Entities**: [`HostState`] — the last good frame + status a reader observes at one
//!   instant, the value the shell hands the display each tick, kept here so neither side
//!   depends on the other.
//! - **Ports**: [`PulseSource`] — the driven interface the firmware's HTTP adapter
//!   implements, with [`PulseFault`] classifying its failures into domain faults. (The
//!   display port is the board-generic `platform_core::Screen`, rendering a
//!   `host_display::Glass` over the frame + status — so the pixels are no longer a host-core
//!   concern.)

pub mod freshness;
pub mod hostseries;
pub mod name;
pub mod percent;
pub mod ports;
pub mod pulse;
pub mod series;
pub mod state;
pub mod status;

pub use freshness::{observe, Outcome, Reading, Tick};
pub use hostseries::HostSeries;
pub use name::HostName;
pub use percent::Percent;
pub use ports::{PulseFault, PulseSource};
pub use pulse::{Pulse, PulseBuilder, MAX_HOSTS};
pub use series::{Series, MAX_SAMPLES};
pub use state::HostState;
pub use status::{HostFault, Status};
