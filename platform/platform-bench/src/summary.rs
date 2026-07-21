//! The three numbers worth reading from a set of timings, and how many broke the budget.

use core::time::Duration;

use crate::sample::Sample;

/// A set of timings read against the budget they were held to.
///
/// A **median** rather than a mean: one unlucky preemption drags a mean somewhere misleading,
/// and the question a bench asks is what a *typical* sample costs. `max` is kept beside it
/// precisely because the median hides the rare expensive one, and `over_budget` because a
/// count of breaches is the number the production alarm reports — quoting it here is what
/// makes a bench result comparable with a production log.
///
/// The budget is a constructor argument rather than something the reader supplies later, so a
/// summary can never be printed without saying how many samples failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Summary {
    /// How many samples this summarises.
    pub count: usize,
    /// The fastest sample.
    pub min: Duration,
    /// The middle sample — for an even count, the upper of the two middles.
    pub median: Duration,
    /// The slowest sample.
    pub max: Duration,
    /// How many samples exceeded the budget.
    pub over_budget: usize,
}

impl Summary {
    /// Summarise `samples` against `budget`, or [`None`] if there are none.
    ///
    /// [`None`] rather than a zeroed summary: an empty set is not a fast set. A bench that ran
    /// a configuration and captured nothing must say so, because "0.00 ms, fits" reads exactly
    /// like a spectacular result.
    ///
    /// Sorts `samples` in place — the caller's array is scratch space, which is what lets this
    /// run with no allocation on a board with 320 KiB of SRAM.
    pub fn of(samples: &mut [Sample], budget: Duration) -> Option<Summary> {
        samples.sort_unstable_by_key(|sample: &Sample| sample.took);
        let (first, last): (&Sample, &Sample) = (samples.first()?, samples.last()?);
        Some(Summary {
            count: samples.len(),
            min: first.took,
            median: samples[samples.len() / 2].took,
            max: last.took,
            over_budget: samples
                .iter()
                .filter(|sample: &&Sample| sample.took > budget)
                .count(),
        })
    }

    /// Whether the *typical* sample broke the budget — the reading that says a configuration is
    /// broken rather than merely occasionally unlucky.
    pub fn median_over_budget(&self, budget: Duration) -> bool {
        self.median > budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const BUDGET: Duration = Duration::from_millis(50);

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// Zero — an empty set has no summary, and must not masquerade as a fast one.
    #[test]
    fn no_samples_have_no_summary() {
        assert_eq!(Summary::of(&mut [], BUDGET), None);
    }

    /// One — a lone sample is its own min, median and max.
    #[test]
    fn a_lone_sample_is_its_own_min_median_and_max() {
        let mut samples: [Sample; 1] = [Sample::between(ms(21))];
        let summary: Summary = Summary::of(&mut samples, BUDGET).expect("one sample summarises");

        assert_eq!(summary.count, 1);
        assert_eq!(summary.min, ms(21));
        assert_eq!(summary.median, ms(21));
        assert_eq!(summary.max, ms(21));
    }

    /// Many — the three numbers come out of an unsorted set in the right places.
    #[test]
    fn many_samples_report_their_min_median_and_max() {
        let mut samples: [Sample; 3] = [
            Sample::between(ms(60)),
            Sample::between(ms(21)),
            Sample::between(ms(30)),
        ];
        let summary: Summary = Summary::of(&mut samples, BUDGET).expect("three samples summarise");

        assert_eq!(summary.min, ms(21));
        assert_eq!(summary.median, ms(30));
        assert_eq!(summary.max, ms(60));
    }

    /// Zero breaches — a set entirely inside the budget reports none, and is not "over".
    #[test]
    fn a_set_inside_the_budget_reports_no_breaches() {
        let mut samples: [Sample; 2] = [Sample::between(ms(21)), Sample::between(ms(22))];
        let summary: Summary = Summary::of(&mut samples, BUDGET).expect("two samples summarise");

        assert_eq!(summary.over_budget, 0);
        assert!(!summary.median_over_budget(BUDGET));
    }

    /// One breach — THE production shape: a set whose median fits and whose rare sample does
    /// not. The count is what distinguishes "0.8% of paints are blocked" from "the paint is
    /// slow", and a summary that only reported the median would erase the difference.
    #[test]
    fn a_rare_breach_is_counted_without_moving_the_median() {
        let mut samples: [Sample; 3] = [
            Sample::between(ms(21)),
            Sample::between(ms(21)),
            Sample::between(ms(60)),
        ];
        let summary: Summary = Summary::of(&mut samples, BUDGET).expect("three samples summarise");

        assert_eq!(summary.over_budget, 1);
        assert_eq!(summary.median, ms(21));
        assert!(
            !summary.median_over_budget(BUDGET),
            "a rare breach must not read as a broken configuration"
        );
    }

    /// Many breaches — when the typical sample is over, the configuration itself is broken.
    #[test]
    fn a_median_past_the_budget_reads_as_broken() {
        let mut samples: [Sample; 3] = [
            Sample::during(ms(64)),
            Sample::during(ms(67)),
            Sample::during(ms(69)),
        ];
        let summary: Summary = Summary::of(&mut samples, BUDGET).expect("three samples summarise");

        assert_eq!(summary.over_budget, 3);
        assert!(summary.median_over_budget(BUDGET));
    }

    /// A sample exactly on the budget has not broken it — the alarm fires on `>`, and a bench
    /// that disagreed with the production loop by one microsecond would be comparing two
    /// different things.
    #[test]
    fn a_sample_exactly_on_the_budget_is_not_a_breach() {
        let mut samples: [Sample; 1] = [Sample::between(BUDGET)];
        let summary: Summary = Summary::of(&mut samples, BUDGET).expect("one sample summarises");

        assert_eq!(summary.over_budget, 0);
    }

    /// The mark plays no part in the arithmetic: [`Summary`] reads whatever slice it is handed,
    /// and it is [`Split`](crate::Split) that decides which samples that is.
    #[test]
    fn the_suspect_mark_does_not_change_the_numbers() {
        let mut marked: [Sample; 2] = [Sample::during(ms(21)), Sample::during(ms(60))];
        let mut unmarked: [Sample; 2] = [Sample::between(ms(21)), Sample::between(ms(60))];

        assert_eq!(
            Summary::of(&mut marked, BUDGET).map(|s: Summary| s.median),
            Summary::of(&mut unmarked, BUDGET).map(|s: Summary| s.median)
        );
    }

    proptest! {
        /// The rule that makes the three numbers a distribution rather than three timings:
        /// whatever the samples and whatever their order, `min <= median <= max`.
        #[test]
        fn the_summary_is_ordered(millis in proptest::collection::vec(0u64..500, 1..64)) {
            let mut samples: Vec<Sample> =
                millis.iter().map(|m: &u64| Sample::between(ms(*m))).collect();
            let summary: Summary =
                Summary::of(&mut samples, BUDGET).expect("a non-empty set summarises");

            prop_assert!(summary.min <= summary.median);
            prop_assert!(summary.median <= summary.max);
            prop_assert_eq!(summary.count, millis.len());
        }

        /// Every breach is counted and none is invented: the count of samples past the budget
        /// is exactly what an independent pass over the input finds.
        #[test]
        fn every_breach_is_counted(millis in proptest::collection::vec(0u64..500, 1..64)) {
            let expected: usize = millis.iter().filter(|m: &&u64| ms(**m) > BUDGET).count();
            let mut samples: Vec<Sample> =
                millis.iter().map(|m: &u64| Sample::between(ms(*m))).collect();
            let summary: Summary =
                Summary::of(&mut samples, BUDGET).expect("a non-empty set summarises");

            prop_assert_eq!(summary.over_budget, expected);
        }
    }
}
