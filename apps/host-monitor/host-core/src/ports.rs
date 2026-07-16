//! Ports — the interfaces the domain requires of the outside world.
//!
//! The driven side of the hexagon. The firmware supplies the adapter — an HTTP client that
//! fetches the hostpulse endpoint (`GET /pulse`, bearer-gated) and deserializes the small
//! fixed JSON into a [`Pulse`] frame ([`PulseSource`]); the domain depends only on this
//! trait, so dependencies point inward. The display port is the board-generic
//! `platform_core::Screen`, rendering a `host_display::Glass`, so it is not named here.

use crate::pulse::Pulse;
use crate::status::HostFault;

/// Classifies an adapter's own fetch failure into a domain [`HostFault`].
///
/// The domain must be able to *publish* why a fetch was refused — a fresh fault is what
/// distinguishes an unreachable endpoint from a dead poller — but it must not learn what an
/// `EspError` or an HTTP status is. So the port asks the adapter, the only party that knows
/// what its own failures mean, to say which domain fault each represents. The adapter keeps
/// its rich error for the log line; the domain gets the verdict.
pub trait PulseFault {
    /// Which domain fault this failure represents.
    fn fault(&self) -> HostFault;
}

/// A source of one hostpulse frame: one call, one `GET /pulse` worth of per-host series.
///
/// Implementations perform the network round-trip and deserialize the response into the
/// [`Pulse`] the display draws. The endpoint has already done the PromQL `rate()`, so the
/// port does no arithmetic and holds no history — an adapter stays a thin translation from
/// the wire to a frame.
///
/// `Error` is associated so an adapter can surface its own failure type — a socket timeout,
/// a non-200 status, a JSON parse error — without the domain naming any concrete transport
/// error. It is bound by [`PulseFault`] because the domain requires every failure to be
/// classifiable: a fetch that could not be taken must still be *reportable*, or it degrades
/// into silence and a consumer cannot tell it from a dead device.
pub trait PulseSource {
    /// The adapter's own fetch-failure type, classifiable into a [`HostFault`].
    type Error: PulseFault;

    /// Fetch one frame from the endpoint.
    fn poll(&mut self) -> Result<Pulse, Self::Error>;
}
