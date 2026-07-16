#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # host-core
//!
//! Framework-free host-metrics core for the M5StickC Plus host monitor.
//!
//! Pure `no_std`, zero dependencies, no allocation: just the CPU/memory value
//! types, the wire parser that reads a host's [node_exporter] scrape, the rate
//! arithmetic that turns two scrapes into a busy percentage, the bounded history
//! the graph is drawn from, and the staleness rule. Everything here is
//! deterministic and host-testable — the Xtensa side only performs the HTTP GET
//! and feeds the bytes in.
//!
//! [node_exporter]: https://github.com/prometheus/node_exporter
//!
//! ## Hexagon
//! - **Entities**: [`Percent`] and [`Sample`] — a CPU/memory reading, each value's
//!   `0..=100` invariant enforced at construction. A [`Sample`] pairs the two, so one
//!   graph column is one value object.
//! - **Entities / policy**: [`prometheus`] — [`RawScrape`], the cumulative counters
//!   one scrape yields after summing, and [`ScrapeAccumulator`], the pure line-at-a-
//!   time reduction the firmware feeds a large `/metrics` body through with bounded
//!   memory. Bespoke to this exporter's text format, so it stays in the domain rather
//!   than a shared platform crate.
//! - **Control**: [`step`] — the sampling use case. CPU load is a *rate* — the busy
//!   fraction between two cumulative counter reads — so this fold is stateful in the
//!   previous scrape's counters; the first scrape yields nothing, the second onward a
//!   [`Sample`]. Memory is a level, read straight from one scrape. A pure function of
//!   its inputs.
//! - **Entities**: [`History`] — the bounded rolling window of recent [`Sample`]s the
//!   sparkline is drawn from. A plain fixed array (`Copy + Eq`), single-writer, evict-
//!   oldest: the graph's retention window, not a thread-shared stream.
//! - **Entities**: [`Status`] and [`HostFault`] — what a consumer learns when it asks
//!   the cache about the host. Like the plant monitor's `Observation`, it carries two
//!   orthogonal facts a bare `Option` cannot: whether the *poller* is alive, and
//!   whether the *host* answered.
//! - **Control / policy**: [`observe`] — the staleness rule that turns a cached
//!   [`Reading`] into a [`Status`], so a host that goes dark never keeps reporting its
//!   last percentages and an unreachable host is never mistaken for a dead poller.
//! - **Entities**: [`HostState`] — the history + status a reader observes at one instant,
//!   the value the shell hands the display each tick (the analog of the plant
//!   `Observation`), kept here so neither side depends on the other.
//! - **Ports**: [`MetricsSource`] — the driven interface the firmware's HTTP adapter
//!   implements, with [`MetricsFault`] classifying its failures into domain faults.
//!   (The display port is the board-generic `platform_core::Screen`, rendering a
//!   `host_display::Glass` wrapper over the history + status — so the pixels are no
//!   longer a host-core concern.)

pub mod freshness;
pub mod history;
pub mod percent;
pub mod ports;
pub mod prometheus;
pub mod rate;
pub mod sample;
pub mod state;
pub mod status;

pub use freshness::{observe, Outcome, Reading, Tick};
pub use history::History;
pub use percent::Percent;
pub use ports::{MetricsFault, MetricsSource};
pub use prometheus::{parse, ParseError, RawScrape, ScrapeAccumulator};
pub use rate::{step, PollState};
pub use sample::Sample;
pub use state::HostState;
pub use status::{HostFault, Status};
