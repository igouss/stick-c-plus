//! The node_exporter scrape parser — a Prometheus text body reduced to the four
//! numbers the host monitor needs.
//!
//! A host's `node_exporter` answers `GET /metrics` with the Prometheus text
//! exposition format: hundreds of `# HELP`/`# TYPE` comment lines and thousands of
//! `metric{labels} value` samples, tens of kilobytes in all. The device cannot hold
//! that body in its scarce SRAM, so this parser is a *streaming reduction*: the
//! firmware adapter reads the socket a line at a time and folds each line into a
//! [`ScrapeAccumulator`], which keeps only a running sum. The whole body is never
//! resident.
//!
//! Four metrics matter, everything else is ignored:
//!
//! - `node_cpu_seconds_total{cpu="…",mode="…"}` — a per-CPU, per-mode cumulative
//!   counter of seconds. Summed over *every* line gives total CPU-seconds elapsed;
//!   summed over just `mode="idle"` gives idle CPU-seconds. The busy fraction between
//!   two scrapes is `1 - Δidle/Δtotal` — see [`step`](crate::step).
//! - `node_memory_MemTotal_bytes` / `node_memory_MemAvailable_bytes` — the memory
//!   level, read straight (`used = 1 - avail/total`).
//!
//! ## Why floating point here, when the plant core is integer-only
//!
//! node_exporter formats its `float64` sample values with Go's shortest-round-trip
//! `'g'` formatting, which emits **scientific notation** for large magnitudes —
//! `node_memory_MemTotal_bytes` prints as e.g. `1.66508544e+10`, not `16650854400`.
//! Hand-rolled integer parsing would mis-read those, so values are parsed with
//! `core`'s [`f64`] `FromStr` (which is in `core`, not `std`, so it cross-compiles to
//! the no_std domain) and the counters are kept as `f64`. The magnitudes involved
//! (seconds since boot, bytes of RAM) are whole numbers well inside `f64`'s exact
//! integer range (`2^53`), so no precision is lost.
//!
//! ## Tolerant reader
//!
//! A malformed value on a line is *skipped*, not fatal: a single weird line from a
//! future node_exporter must not crash a desk display. [`finish`] fails only when a
//! whole required metric never appeared — a scrape that is missing CPU or memory
//! entirely is not something to paper over with zeros.
//!
//! [`finish`]: ScrapeAccumulator::finish

use core::fmt;

/// The cumulative counters one node_exporter scrape yields, after summing.
///
/// `cpu_idle_secs` and `cpu_total_secs` are monotonic counters (seconds since boot,
/// summed across CPUs); their *difference* between two scrapes is what
/// [`step`](crate::step) turns into a busy percentage. `mem_total`/`mem_avail` are a
/// level, read as-is. Not `Eq` — it holds `f64` — which is fine: a `RawScrape` is
/// transient, consumed by the fold, never stored in the `Eq`-compared history.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RawScrape {
    /// Idle CPU-seconds since boot, summed over every CPU.
    pub cpu_idle_secs: f64,
    /// Total CPU-seconds since boot, summed over every CPU and every mode.
    pub cpu_total_secs: f64,
    /// Total physical memory, in bytes.
    pub mem_total: f64,
    /// Memory available for new allocations without swapping, in bytes.
    pub mem_avail: f64,
}

/// Why a scrape could not be reduced to a [`RawScrape`].
///
/// Each names a *whole metric* that never appeared — the tolerant reader skips
/// malformed individual lines silently, so reaching one of these means the body was
/// not a node_exporter scrape at all (wrong port, an error page, a truncated read),
/// which a caller should surface rather than treat as "the host is idle".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParseError {
    /// No `node_cpu_seconds_total` line was seen.
    MissingCpu,
    /// No `node_memory_MemTotal_bytes` line was seen.
    MissingMemTotal,
    /// No `node_memory_MemAvailable_bytes` line was seen.
    MissingMemAvailable,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingCpu => {
                f.write_str("no node_cpu_seconds_total in the scrape — not a node_exporter body?")
            }
            ParseError::MissingMemTotal => {
                f.write_str("no node_memory_MemTotal_bytes in the scrape")
            }
            ParseError::MissingMemAvailable => {
                f.write_str("no node_memory_MemAvailable_bytes in the scrape")
            }
        }
    }
}

/// A running reduction of a node_exporter scrape, fed one line at a time.
///
/// The firmware adapter builds one of these, calls [`observe_line`] for each line it
/// reads off the socket, then [`finish`] — so the multi-kilobyte body flows through
/// at one line of resident memory. Host tests drive it the same way (or via the
/// [`parse`] convenience), so the streaming path is the tested path.
///
/// [`observe_line`]: ScrapeAccumulator::observe_line
/// [`finish`]: ScrapeAccumulator::finish
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrapeAccumulator {
    cpu_idle_secs: f64,
    cpu_total_secs: f64,
    mem_total: Option<f64>,
    mem_avail: Option<f64>,
    saw_cpu: bool,
}

impl ScrapeAccumulator {
    /// A fresh accumulator with every running total at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one raw line of the scrape into the running totals.
    ///
    /// Bytes, not `&str`, because that is what the adapter reads off the socket; a
    /// line that is not valid UTF-8 is simply ignored (Prometheus text is ASCII).
    /// Comment lines (`#…`) and blank lines are ignored, as is any metric other than
    /// the four this parser cares about. A line whose value does not parse is
    /// skipped — see the module's tolerant-reader note.
    pub fn observe_line(&mut self, line: &[u8]) {
        let Ok(text) = core::str::from_utf8(line) else {
            return;
        };
        let text: &str = text.trim();
        if text.is_empty() || text.starts_with('#') {
            return;
        }

        // `identifier value [timestamp]` — take the identifier and the first value
        // token; a trailing timestamp (which node_exporter does not emit by default)
        // is ignored.
        let mut tokens = text.split_ascii_whitespace();
        let (Some(identifier), Some(value_token)) = (tokens.next(), tokens.next()) else {
            return;
        };

        // Split `name{labels}` into the metric name and its label block (empty when
        // the metric is unlabelled, as the memory gauges are).
        let (name, labels): (&str, &str) = match identifier.split_once('{') {
            Some((name, labels)) => (name, labels),
            None => (identifier, ""),
        };

        match name {
            "node_cpu_seconds_total" => {
                if let Ok(seconds) = value_token.parse::<f64>() {
                    self.cpu_total_secs += seconds;
                    self.saw_cpu = true;
                    if labels.contains("mode=\"idle\"") {
                        self.cpu_idle_secs += seconds;
                    }
                }
            }
            "node_memory_MemTotal_bytes" => {
                if let Ok(bytes) = value_token.parse::<f64>() {
                    self.mem_total = Some(bytes);
                }
            }
            "node_memory_MemAvailable_bytes" => {
                if let Ok(bytes) = value_token.parse::<f64>() {
                    self.mem_avail = Some(bytes);
                }
            }
            _ => {}
        }
    }

    /// Conclude the reduction, or say which whole metric was missing.
    ///
    /// Succeeds once at least one CPU line and both memory gauges have been seen. A
    /// missing metric is a [`ParseError`], never a silent zero — see the type's docs.
    pub fn finish(self) -> Result<RawScrape, ParseError> {
        if !self.saw_cpu {
            return Err(ParseError::MissingCpu);
        }
        let mem_total: f64 = self.mem_total.ok_or(ParseError::MissingMemTotal)?;
        let mem_avail: f64 = self.mem_avail.ok_or(ParseError::MissingMemAvailable)?;
        Ok(RawScrape {
            cpu_idle_secs: self.cpu_idle_secs,
            cpu_total_secs: self.cpu_total_secs,
            mem_total,
            mem_avail,
        })
    }
}

/// Parse a whole scrape body at once — the convenience over [`ScrapeAccumulator`].
///
/// Splits `body` on newlines and folds every line through the same accumulator the
/// streaming adapter uses, so a host test proves the exact path the device runs. The
/// firmware prefers the streaming accumulator (it never holds the whole body); tests
/// and any caller that already has the bytes use this.
pub fn parse(body: &[u8]) -> Result<RawScrape, ParseError> {
    let mut accumulator: ScrapeAccumulator = ScrapeAccumulator::new();
    for line in body.split(|&byte: &u8| byte == b'\n') {
        accumulator.observe_line(line);
    }
    accumulator.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small but representative scrape: two CPUs with the usual modes, the two
    /// memory gauges (one in scientific notation, as node_exporter really emits), and
    /// noise lines — comments, a blank line, and unrelated metrics — the parser must
    /// step over. `MemTotal` here is `1.6e10` = 16 GB; `MemAvailable` `8e9` = 8 GB.
    const SCRAPE: &str = "\
# HELP node_cpu_seconds_total Seconds the CPUs spent in each mode.
# TYPE node_cpu_seconds_total counter
node_cpu_seconds_total{cpu=\"0\",mode=\"idle\"} 1000.5
node_cpu_seconds_total{cpu=\"0\",mode=\"user\"} 200
node_cpu_seconds_total{cpu=\"0\",mode=\"system\"} 100
node_cpu_seconds_total{cpu=\"1\",mode=\"idle\"} 900.5
node_cpu_seconds_total{cpu=\"1\",mode=\"user\"} 250
node_cpu_seconds_total{cpu=\"1\",mode=\"system\"} 149

# HELP node_memory_MemTotal_bytes Total memory.
# TYPE node_memory_MemTotal_bytes gauge
node_memory_MemTotal_bytes 1.6e+10
node_memory_MemAvailable_bytes 8e+09
node_filesystem_avail_bytes{device=\"/dev/sda1\"} 5.0e+11
";

    #[test]
    fn a_scrape_sums_idle_and_total_cpu_seconds() {
        let raw: RawScrape = parse(SCRAPE.as_bytes()).expect("a valid scrape");
        // idle = 1000.5 + 900.5 = 1901; total = every mode of every cpu:
        // 1000.5+200+100 + 900.5+250+149 = 2600.
        assert_eq!(raw.cpu_idle_secs, 1901.0);
        assert_eq!(raw.cpu_total_secs, 2600.0);
    }

    #[test]
    fn a_scrape_reads_the_memory_gauges_including_scientific_notation() {
        let raw: RawScrape = parse(SCRAPE.as_bytes()).expect("a valid scrape");
        assert_eq!(raw.mem_total, 1.6e10);
        assert_eq!(raw.mem_avail, 8.0e9);
    }

    #[test]
    fn comments_blanks_and_unrelated_metrics_are_ignored() {
        // The filesystem metric in the fixture must not leak into any total; if it
        // did, cpu_total or mem would be off. Proven indirectly by the exact sums
        // above, and directly here: a body of only noise parses to MissingCpu.
        let noise: &str = "# just a comment\n\nnode_load1 0.42\n";
        assert_eq!(parse(noise.as_bytes()), Err(ParseError::MissingCpu));
    }

    #[test]
    fn a_missing_cpu_metric_is_an_error() {
        let body: &str = "node_memory_MemTotal_bytes 100\nnode_memory_MemAvailable_bytes 50\n";
        assert_eq!(parse(body.as_bytes()), Err(ParseError::MissingCpu));
    }

    #[test]
    fn a_missing_mem_total_is_an_error() {
        let body: &str =
            "node_cpu_seconds_total{mode=\"idle\"} 10\nnode_memory_MemAvailable_bytes 50\n";
        assert_eq!(parse(body.as_bytes()), Err(ParseError::MissingMemTotal));
    }

    #[test]
    fn a_missing_mem_available_is_an_error() {
        let body: &str =
            "node_cpu_seconds_total{mode=\"idle\"} 10\nnode_memory_MemTotal_bytes 100\n";
        assert_eq!(parse(body.as_bytes()), Err(ParseError::MissingMemAvailable));
    }

    #[test]
    fn a_malformed_value_is_skipped_not_fatal() {
        // The idle line's value is garbage, so it is skipped; the user line still
        // counts toward total. idle stays 0, total = 5. A tolerant reader keeps going.
        let body: &str = "\
node_cpu_seconds_total{mode=\"idle\"} not-a-number
node_cpu_seconds_total{mode=\"user\"} 5
node_memory_MemTotal_bytes 100
node_memory_MemAvailable_bytes 40
";
        let raw: RawScrape = parse(body.as_bytes()).expect("the good lines still parse");
        assert_eq!(
            raw.cpu_idle_secs, 0.0,
            "the malformed idle line was skipped"
        );
        assert_eq!(raw.cpu_total_secs, 5.0);
    }

    #[test]
    fn a_trailing_timestamp_is_ignored() {
        // Prometheus permits `metric value timestamp`; take the value, drop the rest.
        let body: &str = "\
node_cpu_seconds_total{mode=\"idle\"} 7 1600000000000
node_memory_MemTotal_bytes 100 1600000000000
node_memory_MemAvailable_bytes 40 1600000000000
";
        let raw: RawScrape = parse(body.as_bytes()).expect("valid with timestamps");
        assert_eq!(raw.cpu_idle_secs, 7.0);
        assert_eq!(raw.mem_total, 100.0);
    }

    #[test]
    fn the_streaming_accumulator_matches_the_whole_body_parse() {
        // The adapter feeds lines one at a time; that must reach the same result as
        // parsing the whole body. Same input, two paths, one answer.
        let mut accumulator: ScrapeAccumulator = ScrapeAccumulator::new();
        for line in SCRAPE.lines() {
            accumulator.observe_line(line.as_bytes());
        }
        assert_eq!(accumulator.finish(), parse(SCRAPE.as_bytes()));
    }

    #[test]
    fn non_utf8_lines_are_ignored() {
        // A stray non-UTF-8 line (a truncated multibyte read) is dropped, not fatal.
        let mut accumulator: ScrapeAccumulator = ScrapeAccumulator::new();
        accumulator.observe_line(&[0xff, 0xfe, 0xfd]);
        accumulator.observe_line(b"node_cpu_seconds_total{mode=\"idle\"} 3");
        accumulator.observe_line(b"node_memory_MemTotal_bytes 100");
        accumulator.observe_line(b"node_memory_MemAvailable_bytes 40");
        assert_eq!(
            accumulator.finish(),
            Ok(RawScrape {
                cpu_idle_secs: 3.0,
                cpu_total_secs: 3.0,
                mem_total: 100.0,
                mem_avail: 40.0,
            })
        );
    }
}
