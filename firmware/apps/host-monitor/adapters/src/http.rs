//! `http` — fetch the hostpulse endpoint over ESP-IDF's HTTP client.
//!
//! The driven adapter for the [`PulseSource`] port: one `poll` performs
//! `GET http://<endpoint>/pulse` with an `Authorization: Bearer <token>` header, reads the
//! small fixed JSON body, and hands the bytes to the pure [`parse_pulse`] codec, which folds
//! them into the [`Pulse`] frame the display draws. Deciding *what the bytes mean* lives in
//! `host-wire` (host-tested); this crate only moves bytes and classifies failures.
//!
//! ## Bounded memory
//!
//! The endpoint returns one small frame — three hosts × two short `%`-series — so the whole
//! body is read into a heap buffer capped at [`BODY_MAX`] and parsed at once (the old
//! node_exporter path had to stream a multi-kilobyte scrape; this one does not). A body that
//! would exceed the cap is refused as [`FetchError::TooLarge`] rather than grown without
//! bound.
//!
//! ## The token never leaks
//!
//! The bearer token is held only inside the source (in the `Authorization` header value) and
//! never appears in [`FetchError`], its [`Display`], or [`url`](HttpPulseSource::url) — so a
//! logged fetch failure or a boot line can never print it. [`HttpPulseSource`] deliberately
//! does **not** derive `Debug` for the same reason.
//!
//! [`PulseSource`]: host_core::PulseSource
//! [`parse_pulse`]: host_wire::parse_pulse
//! [`Display`]: core::fmt::Display

use core::time::Duration;

use esp_idf_svc::http::client::{Configuration, EspHttpConnection};
use esp_idf_svc::http::Method;
use esp_idf_sys::EspError;
use host_core::{HostFault, Pulse, PulseFault, PulseSource};
use host_wire::{parse_pulse, WireError};

/// How long a fetch may take before it is treated as a failure.
///
/// An endpoint that is off the LAN must surface as [`HostFault::Unreachable`] promptly rather
/// than hanging the poller thread, so the request is bounded. Generous enough for the control
/// node to answer, short enough that the display flips to "unreachable" within a couple of
/// poll periods.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// The response read-chunk buffer, in bytes. The body is drained through this.
const CHUNK: usize = 512;

/// The largest `/pulse` body accepted, in bytes. One frame of three hosts is ~1–2 KB; this
/// cap keeps a wrong or hostile endpoint from growing the buffer without bound.
const BODY_MAX: usize = 8 * 1024;

/// Why a fetch failed.
///
/// Classifies into a domain [`HostFault`] via [`PulseFault`]: a transport failure or a `502`
/// (the endpoint's own `prometheus_unavailable`) is [`Unreachable`](HostFault::Unreachable)
/// — no frame could be had, so the last good one is kept — while any other non-200, an
/// over-large body, or a body that is not a frame is [`Malformed`](HostFault::Malformed) (the
/// endpoint answered, but not usefully). Holds no token, so it is safe to log.
#[derive(Debug)]
pub enum FetchError {
    /// The HTTP transport failed — connection refused, timed out, DNS. The endpoint did not
    /// answer.
    Http(EspError),
    /// The endpoint answered with a non-200 status. `502` is its own backend being down.
    Status(u16),
    /// The body exceeded [`BODY_MAX`] before it ended — not the small frame we expect.
    TooLarge,
    /// The body was read but was not a usable pulse frame.
    Parse(WireError),
}

impl core::fmt::Display for FetchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FetchError::Http(err) => write!(f, "pulse fetch transport failed: {err}"),
            FetchError::Status(status) => {
                write!(f, "pulse endpoint returned HTTP {status}, not 200")
            }
            FetchError::TooLarge => write!(f, "pulse body exceeded {BODY_MAX} bytes"),
            FetchError::Parse(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<EspError> for FetchError {
    fn from(err: EspError) -> Self {
        FetchError::Http(err)
    }
}

impl PulseFault for FetchError {
    fn fault(&self) -> HostFault {
        match self {
            // A transport failure, or the endpoint reporting its own backend down (502),
            // means no frame could be had: unreachable, keep the last good frame.
            FetchError::Http(_) => HostFault::Unreachable,
            FetchError::Status(502) => HostFault::Unreachable,
            // Any other bad status or unusable body: it answered, but not with a frame.
            FetchError::Status(_) | FetchError::TooLarge | FetchError::Parse(_) => {
                HostFault::Malformed
            }
        }
    }
}

/// A [`PulseSource`] that fetches the hostpulse endpoint over HTTP.
///
/// Holds the URL, the bearer header value, and the client config; each
/// [`poll`](PulseSource::poll) opens a fresh connection, so there is no stale socket state
/// between cycles (a poll is seconds apart). The composition root builds one from the endpoint
/// and token baked in at build time. Not `Debug` — the header value carries the secret.
pub struct HttpPulseSource {
    url: String,
    authorization: String,
    config: Configuration,
}

impl HttpPulseSource {
    /// A source that fetches `http://<endpoint>/pulse` with `Authorization: Bearer <token>`.
    ///
    /// `endpoint` is `host:port` (e.g. `"10.0.0.10:9099"`); `token` is the bearer secret. The
    /// token is stored only inside the header value and is never exposed again.
    pub fn new(endpoint: &str, token: &str) -> Self {
        Self {
            url: format!("http://{endpoint}/pulse"),
            authorization: format!("Bearer {token}"),
            config: Configuration {
                timeout: Some(FETCH_TIMEOUT),
                ..Default::default()
            },
        }
    }

    /// The URL this source fetches — for the composition root's boot log. Carries no token.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl PulseSource for HttpPulseSource {
    type Error = FetchError;

    fn poll(&mut self) -> Result<Pulse, FetchError> {
        let mut connection: EspHttpConnection = EspHttpConnection::new(&self.config)?;
        let headers: [(&str, &str); 1] = [("Authorization", self.authorization.as_str())];
        connection.initiate_request(Method::Get, &self.url, &headers)?;
        connection.initiate_response()?;

        let status: u16 = connection.status();
        if status != 200 {
            return Err(FetchError::Status(status));
        }

        let body: Vec<u8> = read_body(&mut connection)?;
        parse_pulse(&body).map_err(FetchError::Parse)
    }
}

/// Read the whole response body into a bounded heap buffer.
///
/// Reads in [`CHUNK`]-sized reads until the connection is drained, refusing a body that would
/// exceed [`BODY_MAX`]. The frame is small, so holding it entire is cheap — and the codec
/// needs the whole body anyway (JSON is not line-oriented).
fn read_body(connection: &mut EspHttpConnection) -> Result<Vec<u8>, FetchError> {
    let mut body: Vec<u8> = Vec::new();
    let mut chunk: [u8; CHUNK] = [0; CHUNK];

    loop {
        let read: usize = connection.read(&mut chunk).map_err(FetchError::Http)?;
        if read == 0 {
            break; // end of body
        }
        if body.len() + read > BODY_MAX {
            return Err(FetchError::TooLarge);
        }
        body.extend_from_slice(&chunk[..read]);
    }

    Ok(body)
}
