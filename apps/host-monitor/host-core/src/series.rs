//! Series — one host's bounded `0..=100`% sequence, oldest-first, gaps preserved.
//!
//! The hostpulse endpoint has already done the PromQL `rate()`, so a host's CPU or
//! memory arm arrives ready to plot: a run of integer percents on a fixed grid, oldest
//! at the left, newest at the right, with a `null` wherever a scrape was missing. A
//! [`Series`] is exactly that — a fixed array of [`Option`]`<`[`Percent`]`>` plus a
//! length — and it is the column data one sparkline draws.
//!
//! ## Why a plain array, not a `Vec`
//!
//! Each poll carries the whole window, so the shell *replaces* a series rather than
//! accumulating into it — there is no rolling push here. But the frame the series lives
//! in is the app's rendered state, which the board-generic render loop
//! (`platform_runtime::spawn_display`) requires to be `Copy + Eq` and takes *by value*
//! every tick. A `Vec` is never `Copy`; a plain `[Option<Percent>; MAX_SAMPLES]` is. So
//! this is a bounded value object, not a growable buffer, for the same reason the old
//! `History` was — the render loop's contract, not a memory-scarcity trick.
//!
//! ## A gap is not a zero
//!
//! A `null` in the wire is a *missing sample*, not `0%` — the host was momentarily
//! unscraped, not idle. It is kept as [`None`] so the display can skip it (an empty
//! column) and [`latest`](Series::latest) can look past it to the last real reading,
//! rather than a stray `0` dragging a graph or a label to the floor.
//!
//! Pure and `no_std`: the clamping of a wire integer into `0..=100` is the one policy
//! here, and it is exercised on the host.

use crate::percent::Percent;

/// How many samples one series retains — the window's depth.
///
/// The wire's grid length is `~= window_s / step_s + 1`; the homelab's default 900 s
/// window at a 30 s step is 31 samples, and this leaves headroom for a finer step (a
/// 15 s step is 61). A payload longer than this keeps its **newest** [`MAX_SAMPLES`] —
/// the recent past is what the graph shows — rather than truncating the newest away.
pub const MAX_SAMPLES: usize = 64;

/// A host's bounded `0..=100`% sequence, oldest-first, with gaps kept as [`None`].
///
/// `Copy + Eq`, so it can ride inside the frame the render loop compares tick-to-tick.
/// Samples live in `buf[..len]`; [`from_wire`](Self::from_wire) clamps each present
/// value into a [`Percent`] and keeps the newest [`MAX_SAMPLES`] when the window is
/// longer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Series {
    buf: [Option<Percent>; MAX_SAMPLES],
    len: usize,
}

impl Series {
    /// The empty series — no samples.
    pub const EMPTY: Series = Series {
        buf: [None; MAX_SAMPLES],
        len: 0,
    };

    /// An empty series.
    pub const fn new() -> Self {
        Self::EMPTY
    }

    /// Build a series from wire values, clamping each present value into `0..=100` and
    /// keeping gaps as [`None`].
    ///
    /// A present integer is coerced with [`Percent::clamped`], so a `150` from a glitched
    /// exporter becomes `100` and a negative becomes `0` — a value that fell outside the
    /// contract is pinned, never dropped or panicked on. A [`None`] stays a gap. When
    /// `values` is longer than [`MAX_SAMPLES`] the newest tail is kept, because the graph
    /// draws the recent past; a shorter window fills from the start.
    pub fn from_wire(values: &[Option<i32>]) -> Self {
        let mut series: Series = Series::EMPTY;
        // Keep the newest MAX_SAMPLES: skip the oldest overflow so the tail survives.
        let skip: usize = values.len().saturating_sub(MAX_SAMPLES);
        for value in &values[skip..] {
            series.buf[series.len] = value.map(Percent::clamped);
            series.len += 1;
        }
        series
    }

    /// The retained samples, oldest first — the slice a sparkline plots.
    pub fn samples(&self) -> &[Option<Percent>] {
        &self.buf[..self.len]
    }

    /// The newest *present* reading, skipping trailing gaps — the value a label states.
    ///
    /// Returns [`None`] only when every sample is a gap (a down host) or the series is
    /// empty, in which case there is no current value to show.
    pub fn latest(&self) -> Option<Percent> {
        self.buf[..self.len].iter().rev().find_map(|s| *s)
    }

    /// How many samples are retained, `0..=`[`MAX_SAMPLES`].
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the series holds no samples at all.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether every sample is a gap (or there are none) — the series carries no data.
    ///
    /// A host whose CPU *and* memory series are both `all_gaps` is down; the display draws
    /// it as "no data" rather than a flat graph at zero.
    pub fn all_gaps(&self) -> bool {
        self.buf[..self.len].iter().all(|s| s.is_none())
    }
}

impl Default for Series {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `u8` values a series holds, gaps rendered as `None` — for comparing to a slice.
    fn values(series: &Series) -> Vec<Option<u8>> {
        series
            .samples()
            .iter()
            .map(|s: &Option<Percent>| s.map(Percent::value))
            .collect()
    }

    #[test]
    fn an_empty_series_holds_nothing() {
        let series: Series = Series::from_wire(&[]);
        assert!(series.is_empty());
        assert_eq!(series.len(), 0);
        assert_eq!(series.latest(), None);
        assert!(series.all_gaps(), "no samples is vacuously all-gaps");
    }

    #[test]
    fn one_value_is_the_only_and_latest_sample() {
        let series: Series = Series::from_wire(&[Some(42)]);
        assert_eq!(series.len(), 1);
        assert_eq!(values(&series), vec![Some(42)]);
        assert_eq!(series.latest(), Some(Percent::new(42).unwrap()));
    }

    #[test]
    fn many_values_stay_in_oldest_to_newest_order() {
        let series: Series = Series::from_wire(&[Some(1), Some(2), Some(3)]);
        assert_eq!(values(&series), vec![Some(1), Some(2), Some(3)]);
        assert_eq!(series.latest(), Some(Percent::new(3).unwrap()));
    }

    #[test]
    fn a_gap_is_kept_not_zeroed() {
        // The null between 11 and 10 must survive as a gap, and `latest` must look past a
        // trailing gap to the last real reading.
        let series: Series = Series::from_wire(&[Some(11), None, Some(10), None]);
        assert_eq!(values(&series), vec![Some(11), None, Some(10), None]);
        assert_eq!(
            series.latest(),
            Some(Percent::new(10).unwrap()),
            "latest skips the trailing gap"
        );
        assert!(!series.all_gaps());
    }

    #[test]
    fn an_all_null_series_is_all_gaps_with_no_latest() {
        let series: Series = Series::from_wire(&[None, None, None]);
        assert_eq!(series.len(), 3, "the gaps are retained, not dropped");
        assert!(series.all_gaps());
        assert_eq!(series.latest(), None);
    }

    #[test]
    fn out_of_range_values_are_clamped_not_dropped() {
        let series: Series = Series::from_wire(&[Some(-5), Some(150), Some(50)]);
        assert_eq!(values(&series), vec![Some(0), Some(100), Some(50)]);
    }

    #[test]
    fn a_window_longer_than_capacity_keeps_its_newest_tail() {
        // MAX_SAMPLES + 3 values numbered by position; the oldest three fall off the front
        // and the newest MAX_SAMPLES survive, still in order.
        let raw: Vec<Option<i32>> = (0..(MAX_SAMPLES as i32 + 3))
            .map(|n: i32| Some(n % 101))
            .collect();
        let series: Series = Series::from_wire(&raw);
        assert_eq!(series.len(), MAX_SAMPLES, "the series is bounded");
        // First kept value is the 4th raw value (index 3), last is the final raw value.
        let got: Vec<Option<u8>> = values(&series);
        assert_eq!(got.first().copied().unwrap(), Some(3));
        assert_eq!(
            got.last().copied().unwrap(),
            Some(((MAX_SAMPLES as i32 + 2) % 101) as u8)
        );
    }
}
