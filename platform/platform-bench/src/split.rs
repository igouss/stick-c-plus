//! The discriminator: the same measurement read twice, once for each side of a suspect.

use core::time::Duration;

use crate::sample::Sample;
use crate::summary::Summary;

/// What a split measurement found — stated as **evidence**, not as a conclusion.
///
/// The distinction is deliberate. An instrument that reports "the buzzer blocks the paint" has
/// decided something; one that reports "breaches occurred only while the buzzer sounded" has
/// measured something, and leaves the deciding to a human who also knows what else was running.
/// The 2026-07-21 rotation study went the second way and came back contradicting the hypothesis
/// that prompted it, which is the tool working.
///
/// The comparison is on **whether** each half broke the budget, not on how badly. That is
/// deliberate too: the shape being hunted is a rare breach against a healthy median — 0.8% of
/// paints, in the case this crate was written for — and a comparison of medians would call that
/// run clean.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evidence {
    /// Breaches only among the samples taken while the suspect was active.
    ///
    /// The suspect is implicated: it is the one thing that differed between the two halves of
    /// a single run, on one board, under one build.
    OnlyDuring,
    /// Breaches in both halves.
    ///
    /// Something blocks that is not the suspect — or something blocks *as well as* it. Either
    /// way this run does not implicate the suspect, because the breaches do not need it.
    Both,
    /// Breaches only among the samples taken while the suspect was idle.
    ///
    /// Incoherent against any hypothesis that the suspect costs time, and reported rather than
    /// folded into [`Both`](Evidence::Both) because a result that makes no sense is a signal
    /// about the *instrument* and must not be smoothed away.
    OnlyBetween,
    /// No breaches at all: the run did not reproduce the problem.
    ///
    /// Nothing has been shown about the suspect either way. This is the reading that keeps the
    /// instrument honest — without it, a bench that failed to provoke the fault would report a
    /// clean bill of health for whatever it happened to be pointed at.
    Neither,
    /// One of the halves was empty, so there is nothing to compare.
    ///
    /// Usually means the suspect never fired, or never stopped, during the run.
    NoComparison,
}

/// One set of samples, read as the two halves the suspect's activity divides it into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Split {
    /// The samples taken while the suspect was active.
    pub during: Option<Summary>,
    /// The samples taken while it was idle.
    pub between: Option<Summary>,
}

impl Split {
    /// Split `samples` on their mark and summarise each half against `budget`.
    ///
    /// Reorders `samples` in place — the caller's array is scratch space, so this allocates
    /// nothing.
    pub fn of(samples: &mut [Sample], budget: Duration) -> Split {
        // Marked samples to the front, so each half is one contiguous slice and neither needs a
        // buffer of its own.
        samples.sort_unstable_by_key(|sample: &Sample| !sample.during_suspect);
        let marked: usize = samples
            .iter()
            .filter(|sample: &&Sample| sample.during_suspect)
            .count();
        let (during, between): (&mut [Sample], &mut [Sample]) = samples.split_at_mut(marked);
        Split {
            during: Summary::of(during, budget),
            between: Summary::of(between, budget),
        }
    }

    /// What the two halves show — see [`Evidence`] for what each reading licenses.
    pub fn evidence(&self) -> Evidence {
        match (self.during, self.between) {
            (Some(during), Some(between)) => {
                Split::compare(during.over_budget > 0, between.over_budget > 0)
            }
            _ => Evidence::NoComparison,
        }
    }

    /// The four-way reading of "did this half breach?" against "did that one?".
    fn compare(during_breached: bool, between_breached: bool) -> Evidence {
        match (during_breached, between_breached) {
            (true, false) => Evidence::OnlyDuring,
            (true, true) => Evidence::Both,
            (false, true) => Evidence::OnlyBetween,
            (false, false) => Evidence::Neither,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: Duration = Duration::from_millis(50);

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// Zero — nothing measured at all: no halves, so nothing to compare.
    #[test]
    fn no_samples_give_no_comparison() {
        let split: Split = Split::of(&mut [], BUDGET);

        assert_eq!(split.during, None);
        assert_eq!(split.between, None);
        assert_eq!(split.evidence(), Evidence::NoComparison);
    }

    /// One-sided — the suspect never fired, so however clean the samples look, they say nothing
    /// about it. The reading that stops a bench from clearing a suspect it never exercised.
    #[test]
    fn samples_from_only_one_half_give_no_comparison() {
        let mut samples: [Sample; 2] = [Sample::between(ms(21)), Sample::between(ms(21))];
        let split: Split = Split::of(&mut samples, BUDGET);

        assert!(split.between.is_some(), "the idle half was measured");
        assert_eq!(split.during, None, "the suspect never fired");
        assert_eq!(split.evidence(), Evidence::NoComparison);
    }

    /// THE result this crate exists to report: a healthy idle half, and breaches confined to the
    /// samples taken while the suspect was active.
    #[test]
    fn breaches_confined_to_the_suspect_implicate_it() {
        let mut samples: [Sample; 4] = [
            Sample::during(ms(60)),
            Sample::between(ms(21)),
            Sample::during(ms(60)),
            Sample::between(ms(21)),
        ];
        let split: Split = Split::of(&mut samples, BUDGET);

        assert_eq!(split.evidence(), Evidence::OnlyDuring);
        assert_eq!(split.during.expect("the active half").over_budget, 2);
        assert_eq!(split.between.expect("the idle half").over_budget, 0);
    }

    /// The suspect is a bystander: the breaches do not need it, so this run does not implicate
    /// it — even though half of them happened while it was active.
    #[test]
    fn breaches_in_both_halves_do_not_implicate_the_suspect() {
        let mut samples: [Sample; 4] = [
            Sample::during(ms(60)),
            Sample::between(ms(60)),
            Sample::during(ms(21)),
            Sample::between(ms(21)),
        ];

        assert_eq!(Split::of(&mut samples, BUDGET).evidence(), Evidence::Both);
    }

    /// The null result, and the one an instrument must be able to return about its own run: both
    /// halves fit, so the fault was not reproduced and the suspect is neither implicated nor
    /// cleared.
    #[test]
    fn a_run_that_reproduces_nothing_says_so() {
        let mut samples: [Sample; 4] = [
            Sample::during(ms(21)),
            Sample::between(ms(21)),
            Sample::during(ms(22)),
            Sample::between(ms(22)),
        ];

        assert_eq!(
            Split::of(&mut samples, BUDGET).evidence(),
            Evidence::Neither
        );
    }

    /// The incoherent result, kept separate: breaches only while the suspect was *idle*. It
    /// argues against the hypothesis and for a problem with the run, and folding it into
    /// [`Evidence::Both`] would hide exactly that.
    #[test]
    fn breaches_only_while_the_suspect_was_idle_are_reported_as_such() {
        let mut samples: [Sample; 4] = [
            Sample::during(ms(21)),
            Sample::between(ms(60)),
            Sample::during(ms(21)),
            Sample::between(ms(60)),
        ];

        assert_eq!(
            Split::of(&mut samples, BUDGET).evidence(),
            Evidence::OnlyBetween
        );
    }

    /// A rare breach against a healthy median still implicates: the production shape is 0.8% of
    /// samples, so a discriminator that compared medians would call this run clean and clear the
    /// suspect. It compares breaches instead, and this test is what pins that choice.
    #[test]
    fn a_rare_breach_still_implicates_the_suspect() {
        let mut samples: [Sample; 6] = [
            Sample::during(ms(21)),
            Sample::during(ms(21)),
            Sample::during(ms(60)),
            Sample::between(ms(21)),
            Sample::between(ms(21)),
            Sample::between(ms(21)),
        ];
        let split: Split = Split::of(&mut samples, BUDGET);

        assert!(
            !split
                .during
                .expect("the active half")
                .median_over_budget(BUDGET),
            "the median fits — this is the shape a median comparison would miss"
        );
        assert_eq!(split.evidence(), Evidence::OnlyDuring);
    }

    /// The split keeps every sample: the two halves together are the whole set, whichever order
    /// the marks arrived in.
    #[test]
    fn the_two_halves_account_for_every_sample() {
        let mut samples: [Sample; 5] = [
            Sample::between(ms(21)),
            Sample::during(ms(60)),
            Sample::between(ms(22)),
            Sample::during(ms(61)),
            Sample::between(ms(23)),
        ];
        let split: Split = Split::of(&mut samples, BUDGET);

        assert_eq!(split.during.expect("the active half").count, 2);
        assert_eq!(split.between.expect("the idle half").count, 3);
    }
}
