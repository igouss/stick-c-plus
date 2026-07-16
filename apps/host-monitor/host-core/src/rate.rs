//! The sampling use case (Control), as a pure Functional Core.
//!
//! One [`step`] folds a [`RawScrape`] into a [`Sample`]. It is a *pure function* of
//! its inputs — no socket, no clock, no interior mutability — so the whole sampling
//! policy runs on the host with plain values. The firmware shell is the only thing
//! that touches the network: it polls the [`MetricsSource`](crate::MetricsSource)
//! adapter, hands the raw scrape here, and publishes the [`Sample`].
//!
//! ## CPU is a rate; memory is a level
//!
//! Memory usage is read straight from one scrape (`used = 1 - avail/total`). CPU
//! usage cannot be: `node_cpu_seconds_total` is a *cumulative* counter, so a single
//! read says only how much idle time has ever elapsed, not how busy the host is
//! *now*. The busy fraction is the change between two reads — `1 - Δidle/Δtotal` —
//! which makes this fold **stateful**: it carries the previous scrape's counters in a
//! [`PollState`]. The first scrape only primes that state and yields nothing; the
//! second scrape onward yields a [`Sample`]. The ratio is self-normalising (idle and
//! total advance on the same clock), so no wall-time is needed.

use crate::percent::Percent;
use crate::prometheus::RawScrape;
use crate::sample::Sample;

/// The previous scrape's CPU counters — the fold's only carried state.
///
/// `None` before the first scrape. Kept across the network gap of a failed poll too:
/// the next successful scrape's delta then spans that gap, which reports the *average*
/// busy fraction over it — a sensible reading, not a glitch.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct PollState {
    prev: Option<CpuCounters>,
}

impl PollState {
    /// The state before any scrape has been seen.
    pub const fn new() -> Self {
        Self { prev: None }
    }
}

/// A scrape's cumulative CPU counters, in seconds.
#[derive(Clone, Copy, PartialEq, Debug)]
struct CpuCounters {
    idle: f64,
    total: f64,
}

impl CpuCounters {
    /// The counters carried by `scrape`.
    fn of(scrape: RawScrape) -> Self {
        Self {
            idle: scrape.cpu_idle_secs,
            total: scrape.cpu_total_secs,
        }
    }
}

/// Fold `scrape` into a [`Sample`], relative to `state`.
///
/// Returns the state to carry into the next step (always the current scrape's
/// counters), and the [`Sample`] to publish — `Some` once a CPU rate can be computed
/// (the second scrape onward), `None` on the very first scrape, which only primes the
/// state. Memory is always available, but a `Sample` deliberately withholds it until
/// CPU can join it: showing a memory bar beside a CPU bar pinned at a fictitious zero
/// would misread as "idle", so the first interval shows nothing rather than a lie.
///
/// Pure and total: the same `(state, scrape)` always yields the same result, and no
/// input — a degenerate memory total, a counter that did not advance, a node_exporter
/// restart that reset the counters backwards — can panic. Each such case falls back to
/// a clamped [`Percent`], never a division by zero.
pub fn step(state: PollState, scrape: RawScrape) -> (PollState, Option<Sample>) {
    let now: CpuCounters = CpuCounters::of(scrape);
    let mem: Percent = mem_percent(scrape.mem_total, scrape.mem_avail);

    let sample: Option<Sample> = state
        .prev
        .and_then(|prev: CpuCounters| cpu_percent(prev, now))
        .map(|cpu: Percent| Sample::new(cpu, mem));

    (PollState { prev: Some(now) }, sample)
}

/// The busy fraction between two counter reads, or `None` if it cannot be computed.
///
/// `busy = (Δtotal - Δidle) / Δtotal`. A non-advancing or backwards `Δtotal` (two
/// scrapes too close together, or a node_exporter restart) has no interval to divide
/// by, so it yields `None` — the caller keeps the fresh counters as the new baseline
/// and simply skips a sample rather than reporting a bogus figure.
fn cpu_percent(prev: CpuCounters, now: CpuCounters) -> Option<Percent> {
    let delta_total: f64 = now.total - prev.total;
    let delta_idle: f64 = now.idle - prev.idle;
    if delta_total <= 0.0 {
        return None;
    }
    let busy_fraction: f64 = (delta_total - delta_idle) / delta_total;
    Some(percent(busy_fraction))
}

/// The used fraction of memory: `1 - avail/total`, clamped. A degenerate total
/// (never seen from a live host) reports empty rather than dividing by zero.
fn mem_percent(total: f64, avail: f64) -> Percent {
    if total <= 0.0 {
        return Percent::ZERO;
    }
    percent(1.0 - avail / total)
}

/// Turn a `0.0..=1.0` fraction into a rounded [`Percent`], clamped.
///
/// Rounds half-up without `f64::round` (which `core` lacks on the no_std target):
/// a saturating float-to-int cast truncates, so `+ 0.5` first gives round-to-nearest,
/// and [`Percent::clamped`] pins the result into `0..=100` (a `1.005` fraction from a
/// counter glitch cannot escape). A `NaN` fraction casts to `0`.
fn percent(fraction: f64) -> Percent {
    let rounded: i32 = (fraction * 100.0 + 0.5) as i32;
    Percent::clamped(rounded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scrape with the given CPU counters and a fixed 16 GB / 8 GB memory (50 %
    /// used) — so a test can vary the CPU axis without restating memory each time.
    fn scrape(idle: f64, total: f64) -> RawScrape {
        RawScrape {
            cpu_idle_secs: idle,
            cpu_total_secs: total,
            mem_total: 16.0e9,
            mem_avail: 8.0e9,
        }
    }

    #[test]
    fn the_first_scrape_primes_the_state_and_reports_nothing() {
        // Zero prior scrapes: CPU is a rate and there is no interval yet, so no
        // Sample is produced — only the baseline is remembered.
        let (state, sample): (PollState, Option<Sample>) =
            step(PollState::new(), scrape(1000.0, 2000.0));
        assert_eq!(sample, None, "no rate can exist from a single scrape");
        assert_eq!(
            state,
            PollState {
                prev: Some(CpuCounters {
                    idle: 1000.0,
                    total: 2000.0
                })
            }
        );
    }

    #[test]
    fn a_second_scrape_reports_the_busy_fraction() {
        // Over the interval, total advanced 100 s and idle advanced 25 s, so the host
        // was busy 75 s of 100 → 75 %. Memory is the fixed 50 %.
        let (state, _): (PollState, _) = step(PollState::new(), scrape(1000.0, 2000.0));
        let (_, sample): (_, Option<Sample>) = step(state, scrape(1025.0, 2100.0));
        let sample: Sample = sample.expect("the second scrape yields a Sample");
        assert_eq!(sample.cpu(), Percent::new(75).unwrap());
        assert_eq!(sample.mem(), Percent::new(50).unwrap());
    }

    #[test]
    fn a_fully_idle_interval_is_zero_percent() {
        // All of the elapsed CPU-time was idle: 100 s total, 100 s idle → 0 % busy.
        let (state, _): (PollState, _) = step(PollState::new(), scrape(1000.0, 2000.0));
        let (_, sample): (_, Option<Sample>) = step(state, scrape(1100.0, 2100.0));
        assert_eq!(sample.unwrap().cpu(), Percent::ZERO);
    }

    #[test]
    fn a_fully_busy_interval_is_one_hundred_percent() {
        // No idle time accrued over the interval: 100 s total, 0 s idle → 100 % busy.
        let (state, _): (PollState, _) = step(PollState::new(), scrape(1000.0, 2000.0));
        let (_, sample): (_, Option<Sample>) = step(state, scrape(1000.0, 2100.0));
        assert_eq!(sample.unwrap().cpu(), Percent::FULL);
    }

    #[test]
    fn a_non_advancing_counter_reports_no_cpu_but_keeps_the_baseline() {
        // Two identical scrapes (Δtotal = 0): no interval to divide by, so no Sample,
        // and the baseline is still refreshed for next time.
        let (state, _): (PollState, _) = step(PollState::new(), scrape(1000.0, 2000.0));
        let (next, sample): (PollState, Option<Sample>) = step(state, scrape(1000.0, 2000.0));
        assert_eq!(sample, None);
        assert_eq!(
            next,
            PollState {
                prev: Some(CpuCounters {
                    idle: 1000.0,
                    total: 2000.0
                })
            }
        );
    }

    #[test]
    fn a_counter_reset_reports_no_cpu_rather_than_a_glitch() {
        // node_exporter restarted: the new counters are *below* the old ones, so
        // Δtotal is negative. That yields no Sample, not a wild percentage — and the
        // reset scrape becomes the new baseline, so the next interval is clean.
        let (state, _): (PollState, _) = step(PollState::new(), scrape(9000.0, 18000.0));
        let (next, sample): (PollState, Option<Sample>) = step(state, scrape(10.0, 20.0));
        assert_eq!(sample, None);
        assert_eq!(
            next,
            PollState {
                prev: Some(CpuCounters {
                    idle: 10.0,
                    total: 20.0
                })
            }
        );
    }

    #[test]
    fn memory_is_read_straight_from_the_scrape() {
        // 16 GB total, 4 GB available → 75 % used. Independent of the CPU rate.
        let raw: RawScrape = RawScrape {
            cpu_idle_secs: 1.0,
            cpu_total_secs: 2.0,
            mem_total: 16.0e9,
            mem_avail: 4.0e9,
        };
        let (state, _): (PollState, _) = step(PollState::new(), scrape(1000.0, 2000.0));
        let (_, sample): (_, Option<Sample>) = step(
            state,
            RawScrape {
                cpu_idle_secs: 1010.0,
                cpu_total_secs: 2020.0,
                ..raw
            },
        );
        assert_eq!(sample.unwrap().mem(), Percent::new(75).unwrap());
    }

    #[test]
    fn a_degenerate_memory_total_is_empty_not_a_panic() {
        assert_eq!(mem_percent(0.0, 0.0), Percent::ZERO);
    }
}
