//! History — the bounded rolling window of recent samples the graph is drawn from.
//!
//! A sparkline shows the last screen-width samples, oldest at the left, newest at the
//! right, scrolling as new ones arrive. That is exactly a fixed-capacity queue that
//! drops its oldest element when full — so [`History`] is a plain array of
//! [`CAPACITY`] samples plus a length, and [`push`] appends, evicting the oldest once
//! the window is full.
//!
//! ## Why a plain array, not `heapless::Vec` or a ring buffer
//!
//! The history *is* the app's rendered state: the board-generic render loop
//! (`platform_runtime::spawn_display`) requires that state to be `Copy + Eq` and
//! takes it *by value* every tick. A `heapless::Vec` is `Clone` but never `Copy`, so
//! it cannot carry the state; a plain `[Sample; CAPACITY]` can. And because the
//! samples are always stored in order (a full push shifts the whole array left by
//! one), [`samples`] hands back a straight `&[Sample]` oldest-to-newest with no ring
//! bookkeeping — which is exactly what the sparkline wants. The shift is `O(CAPACITY)`
//! of two-byte copies once per poll (seconds apart), which is free.
//!
//! This is a *single-writer, synchronous* structure — the poller thread pushes, a
//! reader snapshots the whole value — not a thread-shared, lock-free, drop-counting
//! stream. That distinction is why it lives here as its own small type rather than
//! reaching for general observability-stream infrastructure.
//!
//! [`push`]: History::push
//! [`samples`]: History::samples

use crate::sample::Sample;

/// How many samples the window retains — one per graph column.
///
/// The sparkline draws one column per sample, so this is both the retention depth and
/// the graph's pixel width. The display's layout asserts, at compile time, that its
/// graph is exactly this wide, so the two can never drift.
pub const CAPACITY: usize = 120;

/// A bounded, in-order window of the most recent [`Sample`]s.
///
/// `Copy + Eq`, so it can be the app's [`Animated`](platform_core::Animated) state and
/// be compared tick-to-tick for change suppression. Samples are stored oldest-first in
/// `buf[..len]`; [`push`](Self::push) evicts the oldest once `len` reaches
/// [`CAPACITY`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct History {
    buf: [Sample; CAPACITY],
    len: usize,
}

impl History {
    /// An empty window — no samples yet.
    pub const fn new() -> Self {
        Self {
            buf: [Sample::ZERO; CAPACITY],
            len: 0,
        }
    }

    /// Append `sample` as the newest, evicting the oldest if the window is full.
    ///
    /// While the window is filling, this just grows `buf[..len]`. Once full, the whole
    /// array shifts left by one (dropping the oldest) and `sample` takes the last
    /// slot — so `buf[..len]` stays in strict oldest-to-newest order for [`samples`].
    ///
    /// [`samples`]: Self::samples
    pub fn push(&mut self, sample: Sample) {
        if self.len < CAPACITY {
            self.buf[self.len] = sample;
            self.len += 1;
        } else {
            self.buf.rotate_left(1);
            self.buf[CAPACITY - 1] = sample;
        }
    }

    /// The retained samples, oldest first — the slice the sparkline plots.
    pub fn samples(&self) -> &[Sample] {
        &self.buf[..self.len]
    }

    /// The newest sample, or `None` while the window is empty.
    pub fn latest(&self) -> Option<Sample> {
        self.buf[..self.len].last().copied()
    }

    /// How many samples are retained, `0..=`[`CAPACITY`].
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the window holds no samples yet.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The fixed retention depth — [`CAPACITY`].
    pub const fn capacity() -> usize {
        CAPACITY
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::percent::Percent;

    /// A sample whose CPU percent is `n` (memory fixed), so a test can track exactly
    /// which samples the window kept by their CPU value.
    fn sample(n: u8) -> Sample {
        Sample::new(
            Percent::new(n).expect("test value is 0..=100"),
            Percent::ZERO,
        )
    }

    /// The CPU values the window holds, oldest first — the sequence a graph would plot.
    fn cpu_series(history: &History) -> Vec<u8> {
        history
            .samples()
            .iter()
            .map(|sample: &Sample| sample.cpu().value())
            .collect()
    }

    #[test]
    fn an_empty_window_holds_nothing() {
        let history: History = History::new();
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
        assert_eq!(history.latest(), None);
        assert_eq!(cpu_series(&history), Vec::<u8>::new());
    }

    #[test]
    fn one_push_is_the_only_and_latest_sample() {
        let mut history: History = History::new();
        history.push(sample(42));
        assert_eq!(history.len(), 1);
        assert_eq!(cpu_series(&history), vec![42]);
        assert_eq!(history.latest(), Some(sample(42)));
    }

    #[test]
    fn many_pushes_stay_in_oldest_to_newest_order() {
        let mut history: History = History::new();
        history.push(sample(1));
        history.push(sample(2));
        history.push(sample(3));
        assert_eq!(cpu_series(&history), vec![1, 2, 3]);
        assert_eq!(history.latest(), Some(sample(3)));
    }

    #[test]
    fn the_window_never_exceeds_its_capacity() {
        let mut history: History = History::new();
        for _ in 0..(CAPACITY + 50) {
            history.push(sample(7));
        }
        assert_eq!(history.len(), CAPACITY, "the window is bounded");
    }

    #[test]
    fn a_full_window_scrolls_dropping_the_oldest() {
        // Fill exactly, then push one more: the first sample falls off the left and
        // the new one appears on the right, order preserved.
        let mut history: History = History::new();
        for n in 0..CAPACITY {
            history.push(sample((n % 100) as u8));
        }
        let before: Vec<u8> = cpu_series(&history);
        history.push(sample(99));

        let after: Vec<u8> = cpu_series(&history);
        assert_eq!(after.len(), CAPACITY);
        assert_eq!(
            &after[..CAPACITY - 1],
            &before[1..],
            "everything shifted left by one"
        );
        assert_eq!(after[CAPACITY - 1], 99, "the newest is on the right");
        assert_eq!(history.latest(), Some(sample(99)));
    }
}
