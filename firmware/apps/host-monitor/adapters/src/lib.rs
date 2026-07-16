#![forbid(unsafe_code)]
//! # host-adapters
//!
//! The host monitor's driven adapters — the on-target side of the domain's ports.
//!
//!   - [`http`] — the [`HttpMetricsSource`](http::HttpMetricsSource): scrape a Fedora
//!     host's `node_exporter` over ESP-IDF's HTTP client and stream the body through the
//!     pure `host_core` parser, implementing the
//!     [`MetricsSource`](host_core::MetricsSource) port.
//!
//! Every rule about *what a scrape means* stays inward in `host-core` (the parser, the
//! rate arithmetic, the freshness policy), host-tested there; this crate is the thin
//! on-target shell that performs the network round-trip and hands the bytes across.

pub mod http;

pub use http::{HttpMetricsSource, ScrapeError};
