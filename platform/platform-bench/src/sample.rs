//! One timed observation, and whether the suspect was active while it was taken.

use core::time::Duration;

/// One timing, marked with whether the **suspect** was active for it.
///
/// The mark is what turns a set of timings into an experiment. Timing a paint tells you it took
/// 60 ms; timing it *and recording that a jingle was sounding* tells you why — but only if the
/// same run also holds the samples where no jingle sounded, to compare against. So the mark
/// travels with the sample rather than the bench keeping two arrays, which could not survive
/// being sorted.
///
/// What counts as "the suspect" is the bench tool's business, not this crate's: a jingle in
/// flight, a thread still running, a sensor mid-read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// How long the timed work took.
    pub took: Duration,
    /// Whether the suspect was active for the whole or part of that time.
    pub during_suspect: bool,
}

impl Sample {
    /// A sample taken while the suspect was active.
    pub const fn during(took: Duration) -> Sample {
        Sample {
            took,
            during_suspect: true,
        }
    }

    /// A sample taken while the suspect was idle.
    pub const fn between(took: Duration) -> Sample {
        Sample {
            took,
            during_suspect: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_taken_during_the_suspect_is_marked() {
        assert!(Sample::during(Duration::from_millis(60)).during_suspect);
    }

    #[test]
    fn a_sample_taken_between_is_not_marked() {
        assert!(!Sample::between(Duration::from_millis(21)).during_suspect);
    }

    /// The two constructors differ only in the mark — the timing survives either.
    #[test]
    fn both_constructors_keep_the_timing() {
        let took: Duration = Duration::from_micros(21_300);
        assert_eq!(Sample::during(took).took, took);
        assert_eq!(Sample::between(took).took, took);
    }
}
