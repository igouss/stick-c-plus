//! Hardware oversampling — fold N raw ADC reads into one denoised sample.
//!
//! The ESP32 ADC's per-conversion noise is large enough that a single read
//! jitters by tens of counts. Averaging a rapid burst suppresses it — an
//! *electrical* concern the driven adapter owns, distinct from a domain's
//! temporal sampling policy (e.g. `plant_core::sampler::step`, which folds
//! readings taken seconds apart). Pure and hardware-free, so the reduction is
//! unit- and property-tested on the host.

use core::num::NonZeroU16;

/// Average `samples` raw reads from `read_one` into a single count.
///
/// `samples` is [`NonZeroU16`] so an empty average — which has no defined value
/// and would divide by zero — is unrepresentable at the type level rather than a
/// runtime guard. Reads short-circuit on the first error: a flaky ADC surfaces
/// its error immediately instead of averaging past a failure.
///
/// No overflow is possible: each read is a 12-bit count (`<= 4095`) and at most
/// `u16::MAX` of them are summed, topping out near 268 million — well inside
/// `u32`.
pub fn oversampled_mean<E>(
    samples: NonZeroU16,
    mut read_one: impl FnMut() -> Result<u16, E>,
) -> Result<u16, E> {
    let count: u32 = samples.get() as u32;
    let sum: u32 = (0..samples.get()).try_fold(0u32, |acc: u32, _| {
        read_one().map(|raw: u16| acc + raw as u32)
    })?;
    Ok((sum / count) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Sample counts in these tests are always literal and nonzero; the "empty
    /// average" case is the type itself — [`NonZeroU16`] makes zero
    /// unrepresentable, so there is no runtime zero branch to exercise.
    fn nz(value: u16) -> NonZeroU16 {
        NonZeroU16::new(value).expect("test sample count is nonzero")
    }

    #[test]
    fn one_sample_is_itself() {
        let mut calls: u16 = 0;
        let mean: Result<u16, ()> = oversampled_mean(nz(1), || {
            calls += 1;
            Ok(1234)
        });
        assert_eq!(mean, Ok(1234));
        assert_eq!(calls, 1, "one sample means exactly one read");
    }

    #[test]
    fn many_samples_are_averaged() {
        // mean(10, 20, 60) = 30 — distinct from first, last, min, and max, so a
        // pick-one implementation would fail here rather than average.
        let mut script: core::array::IntoIter<u16, 3> = [10u16, 20, 60].into_iter();
        let mean: Result<u16, ()> = oversampled_mean(nz(3), || Ok(script.next().unwrap()));
        assert_eq!(mean, Ok(30));
    }

    #[test]
    fn the_mean_floors_rather_than_rounds() {
        // 45 / 4 = 11.25 → 11: integer division truncates, it never rounds up.
        let mut script: core::array::IntoIter<u16, 4> = [10u16, 10, 10, 15].into_iter();
        let mean: Result<u16, ()> = oversampled_mean(nz(4), || Ok(script.next().unwrap()));
        assert_eq!(mean, Ok(11));
    }

    #[test]
    fn a_read_error_short_circuits() {
        // The second read fails; the fold must stop there — a third read would
        // panic on the exhausted counter, proving we never advanced past the
        // error.
        let mut calls: u16 = 0;
        let mean: Result<u16, &str> = oversampled_mean(nz(3), || {
            calls += 1;
            if calls == 2 {
                Err("adc timeout")
            } else {
                Ok(100)
            }
        });
        assert_eq!(mean, Err("adc timeout"));
        assert_eq!(calls, 2, "must stop on the failing read, not read past it");
    }

    proptest! {
        /// The mean of a batch of 12-bit reads always lands within the batch's
        /// own range. This proves the sum never overflows or wraps (a wrapped
        /// sum would push the mean outside [min, max]) for any batch size.
        #[test]
        fn mean_stays_within_the_sample_range(
            reads in prop::collection::vec(0u16..=4095, 1..=500),
        ) {
            let low: u16 = *reads.iter().min().unwrap();
            let high: u16 = *reads.iter().max().unwrap();
            let mut script: core::slice::Iter<u16> = reads.iter();
            let mean: u16 = oversampled_mean(nz(reads.len() as u16), || {
                Ok::<u16, ()>(*script.next().unwrap())
            })
            .unwrap();
            prop_assert!(low <= mean && mean <= high);
        }
    }
}
