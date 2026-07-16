#![forbid(unsafe_code)]
//! # host-adapters
//!
//! The host monitor's driven adapters — the on-target side of the domain's ports.
//!
//!   - [`http`] — the [`HttpPulseSource`](http::HttpPulseSource): fetch the bearer-gated
//!     hostpulse endpoint over ESP-IDF's HTTP client and hand the JSON body to the pure
//!     `host_wire` codec, implementing the [`PulseSource`](host_core::PulseSource) port.
//!
//! Every rule about *what a frame means* stays inward — the wire codec in `host-wire`, the
//! clamping/gap transform and the freshness policy in `host-core`, all host-tested; this
//! crate is the thin on-target shell that performs the network round-trip and hands the bytes
//! across.

pub mod http;

pub use http::{FetchError, HttpPulseSource};
