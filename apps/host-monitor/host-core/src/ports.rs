//! Ports — the interfaces the domain requires of the outside world.
//!
//! The driven side of the hexagon. The firmware supplies the adapter — an HTTP client
//! that scrapes the host's node_exporter over the network and feeds the body through
//! the pure [`prometheus`](crate::prometheus) parser ([`MetricsSource`]); the domain
//! depends only on this trait, so dependencies point inward. The display port is the
//! board-generic `platform_core::Screen`, rendering a `host_display::Glass`, so it is
//! not named here.

use crate::prometheus::RawScrape;
use crate::status::HostFault;

/// Classifies an adapter's own poll failure into a domain [`HostFault`].
///
/// The domain must be able to *publish* why a scrape was refused — a fresh fault is
/// what distinguishes an unreachable host from a dead poller — but it must not learn
/// what an `EspError` or an HTTP status is. So the port asks the adapter, the only
/// party that knows what its own failures mean, to say which domain fault each
/// represents. The adapter keeps its rich error for the log line; the domain gets the
/// verdict.
pub trait MetricsFault {
    /// Which domain fault this failure represents.
    fn fault(&self) -> HostFault;
}

/// A source of host metrics: one call, one scrape's worth of cumulative counters.
///
/// Implementations perform the network round-trip (`GET /metrics`) and reduce the
/// response through the pure [`parse`](crate::prometheus::parse) /
/// [`ScrapeAccumulator`](crate::prometheus::ScrapeAccumulator), returning the
/// [`RawScrape`] the [`step`](crate::step) fold turns into a percentage. The port does
/// no rate arithmetic and holds no history, so an adapter stays a thin translation
/// from the wire to four numbers.
///
/// `Error` is associated so an adapter can surface its own failure type — a socket
/// timeout, a non-200 status, a parse error — without the domain naming any concrete
/// transport error. It is bound by [`MetricsFault`] because the domain requires every
/// failure to be classifiable: a scrape that could not be taken must still be
/// *reportable*, or it degrades into silence and a consumer cannot tell it from a dead
/// device.
pub trait MetricsSource {
    /// The adapter's own poll-failure type, classifiable into a [`HostFault`].
    type Error: MetricsFault;

    /// Take one scrape, reduced to its cumulative counters.
    fn poll(&mut self) -> Result<RawScrape, Self::Error>;
}
