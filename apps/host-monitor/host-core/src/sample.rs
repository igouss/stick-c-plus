//! Sample — one host reading: a CPU busy percent paired with a memory used percent.
//!
//! The value object the graph is built from — one [`Sample`] is one column in each
//! of the two sparklines. It carries only derived percentages; the raw counters it
//! came from live in [`RawScrape`](crate::prometheus::RawScrape) and are spent by the
//! time a `Sample` exists. `Copy + Eq` and one byte per field, so the whole
//! [`History`](crate::History) it accumulates into stays a cheap plain array.

use crate::percent::Percent;

/// A single host reading: how busy the CPU was, and how full memory was.
///
/// Both are [`Percent`]s, so a sample is two bytes and the history that holds a
/// windowful of them is a small `Copy` array. `cpu` is the busy fraction over the
/// last poll interval (a *rate* — see [`step`](crate::step)); `mem` is the used
/// fraction at the moment of the scrape (a *level*).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Sample {
    cpu: Percent,
    mem: Percent,
}

impl Sample {
    /// The empty reading — both idle/empty. The fill value for a not-yet-written
    /// [`History`](crate::History) slot; never rendered, since the history tracks how
    /// many slots are valid.
    pub const ZERO: Sample = Sample {
        cpu: Percent::ZERO,
        mem: Percent::ZERO,
    };

    /// Pair a CPU busy percent with a memory used percent.
    pub const fn new(cpu: Percent, mem: Percent) -> Self {
        Self { cpu, mem }
    }

    /// The CPU busy percentage over the last interval.
    pub const fn cpu(self) -> Percent {
        self.cpu
    }

    /// The memory used percentage at the scrape.
    pub const fn mem(self) -> Percent {
        self.mem
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_carries_both_percentages() {
        let cpu: Percent = Percent::new(42).unwrap();
        let mem: Percent = Percent::new(71).unwrap();
        let sample: Sample = Sample::new(cpu, mem);
        assert_eq!(sample.cpu(), cpu);
        assert_eq!(sample.mem(), mem);
    }

    #[test]
    fn the_zero_sample_is_empty_on_both_axes() {
        assert_eq!(Sample::ZERO.cpu(), Percent::ZERO);
        assert_eq!(Sample::ZERO.mem(), Percent::ZERO);
    }
}
