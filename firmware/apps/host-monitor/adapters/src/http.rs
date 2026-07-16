//! `http` — scrape node_exporter over ESP-IDF's HTTP client.
//!
//! The driven adapter for the [`MetricsSource`] port: one `poll` performs `GET
//! http://<host>:9100/metrics`, streams the response **a chunk at a time** through the
//! pure [`ScrapeAccumulator`] (so the multi-kilobyte body is never resident), and returns
//! the [`RawScrape`] the domain's rate arithmetic turns into a percentage. All the
//! parsing lives inward in `host-core`; this crate only moves bytes.
//!
//! ## Bounded memory
//!
//! The body is read into a small fixed chunk buffer and split into lines on the fly. A
//! line is accumulated in a fixed buffer and handed to [`observe_line`] at each newline;
//! a line longer than [`LINE_MAX`] (never one of the short metrics the parser cares
//! about — those are well under 100 bytes) is skipped rather than truncated, so a
//! truncated value can never corrupt a sum. Nothing scales with the response size.
//!
//! [`MetricsSource`]: host_core::MetricsSource
//! [`ScrapeAccumulator`]: host_core::ScrapeAccumulator
//! [`observe_line`]: host_core::ScrapeAccumulator::observe_line

use core::time::Duration;

use esp_idf_svc::http::client::{Configuration, EspHttpConnection};
use esp_idf_svc::http::Method;
use esp_idf_sys::EspError;
use host_core::{HostFault, MetricsFault, MetricsSource, ParseError, RawScrape, ScrapeAccumulator};

/// How long a scrape may take before it is treated as a failure.
///
/// A powered-off host must surface as [`HostFault::Unreachable`] promptly rather than
/// hanging the poller thread, so the request is bounded. Generous enough for a busy
/// host's exporter to answer, short enough that the display flips to "unreachable" within
/// a couple of poll periods.
const SCRAPE_TIMEOUT: Duration = Duration::from_secs(5);

/// The response read-chunk buffer, in bytes. The body is drained through this.
const CHUNK: usize = 512;

/// The longest single metric line the parser is fed. node_exporter's lines are short
/// (the CPU and memory metrics are well under 100 bytes); a longer line — a filesystem or
/// GC metric with a long label set — is one this parser ignores anyway, so overflowing it
/// is skipped, not truncated.
const LINE_MAX: usize = 256;

/// Why a scrape failed.
///
/// Classifies into a domain [`HostFault`] via [`MetricsFault`]: a transport failure is
/// [`Unreachable`](HostFault::Unreachable) (the host did not answer), while a non-200
/// status or an unparseable body is [`Malformed`](HostFault::Malformed) (the host answered
/// but not with a usable scrape).
#[derive(Debug)]
pub enum ScrapeError {
    /// The HTTP transport failed — connection refused, timed out, DNS, TLS. The host did
    /// not answer.
    Http(EspError),
    /// The host answered with a non-200 status — the wrong endpoint, or an error page.
    Status(u16),
    /// The body was reached but was not a usable node_exporter scrape.
    Parse(ParseError),
}

impl core::fmt::Display for ScrapeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ScrapeError::Http(err) => write!(f, "scrape transport failed: {err}"),
            ScrapeError::Status(status) => write!(f, "scrape returned HTTP {status}, not 200"),
            ScrapeError::Parse(err) => write!(f, "scrape body was not usable: {err}"),
        }
    }
}

impl std::error::Error for ScrapeError {}

impl From<EspError> for ScrapeError {
    fn from(err: EspError) -> Self {
        ScrapeError::Http(err)
    }
}

impl MetricsFault for ScrapeError {
    fn fault(&self) -> HostFault {
        match self {
            // A transport failure means the host did not answer at all.
            ScrapeError::Http(_) => HostFault::Unreachable,
            // A wrong status or an unparseable body means it answered, but uselessly.
            ScrapeError::Status(_) | ScrapeError::Parse(_) => HostFault::Malformed,
        }
    }
}

/// A [`MetricsSource`] that scrapes a fixed node_exporter URL over HTTP.
///
/// Holds only the URL; each [`poll`](MetricsSource::poll) opens a fresh connection, so
/// there is no stale socket state to manage between cycles (a poll is seconds apart). The
/// composition root builds one from the host address baked in at build time.
pub struct HttpMetricsSource {
    url: String,
    config: Configuration,
}

impl HttpMetricsSource {
    /// A source that scrapes `http://<address>/metrics`, where `address` is `host:port`
    /// (e.g. `"192.168.1.10:9100"`).
    pub fn new(address: &str) -> Self {
        Self {
            url: format!("http://{address}/metrics"),
            config: Configuration {
                timeout: Some(SCRAPE_TIMEOUT),
                ..Default::default()
            },
        }
    }

    /// The URL this source scrapes — for the composition root's boot log.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl MetricsSource for HttpMetricsSource {
    type Error = ScrapeError;

    fn poll(&mut self) -> Result<RawScrape, ScrapeError> {
        let mut connection: EspHttpConnection = EspHttpConnection::new(&self.config)?;
        connection.initiate_request(Method::Get, &self.url, &[])?;
        connection.initiate_response()?;

        let status: u16 = connection.status();
        if status != 200 {
            return Err(ScrapeError::Status(status));
        }

        fold_response(&mut connection)
    }
}

/// Drain the response body through the pure accumulator, one line at a time.
///
/// Reads the body in [`CHUNK`]-sized reads, splitting on `\n` and feeding each complete
/// line to [`observe_line`](ScrapeAccumulator::observe_line). Bounded memory: one chunk
/// buffer and one line buffer, neither scaling with the response.
fn fold_response(connection: &mut EspHttpConnection) -> Result<RawScrape, ScrapeError> {
    let mut accumulator: ScrapeAccumulator = ScrapeAccumulator::new();
    let mut chunk: [u8; CHUNK] = [0; CHUNK];
    let mut line: [u8; LINE_MAX] = [0; LINE_MAX];
    let mut line_len: usize = 0;
    let mut overflowed: bool = false;

    loop {
        let read: usize = connection.read(&mut chunk).map_err(ScrapeError::Http)?;
        if read == 0 {
            break; // end of body
        }
        for &byte in &chunk[..read] {
            if byte == b'\n' {
                if !overflowed {
                    accumulator.observe_line(&line[..line_len]);
                }
                line_len = 0;
                overflowed = false;
            } else if line_len < LINE_MAX {
                line[line_len] = byte;
                line_len += 1;
            } else {
                // Too long to be one of the parser's short metrics: skip the whole line
                // rather than feed a truncated value that could corrupt a sum.
                overflowed = true;
            }
        }
    }

    // A final line with no trailing newline (rare, but node_exporter's last line may lack
    // one on some setups).
    if !overflowed && line_len > 0 {
        accumulator.observe_line(&line[..line_len]);
    }

    accumulator.finish().map_err(ScrapeError::Parse)
}
