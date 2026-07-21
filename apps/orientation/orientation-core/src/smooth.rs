//! The pure smoother — how a jittery stream of samples becomes a readout you can read.
//!
//! An accelerometer sampled as fast as it can be read is *noisy*: a board sitting perfectly
//! still reports a few milli-g of jitter on every axis, and a display driven straight from
//! the raw stream flickers its last digit forever and repaints on every single frame. So the
//! readings are folded through an exponential moving average first.
//!
//! The tension is real, and the resolution is deliberate: heavy smoothing makes a beautifully
//! steady number that *lags behind the user's hand*, which is exactly the failure a
//! responsive orientation readout must not have. So the default weight is light — the
//! readout tracks a turn essentially as fast as the sensor can report it, and pays for that
//! with a milli-g or two of visible jitter rather than with lag.

use platform_core::Acceleration;

/// The default smoothing weight: each new sample moves the average half the way.
///
/// Chosen for *responsiveness first*, and set against a measurement. The sampler can poll no
/// faster than one FreeRTOS tick (10 ms), so the settle time has to be bought with a lighter
/// weight rather than with a faster cadence: at a half-share per sample a full 90° turn is on
/// the glass in about seven samples, or 70 ms.
///
/// It is still ample smoothing for the noise actually present. The board at rest was measured
/// jittering by only a few milli-g per axis — around a thousandth of the bars' full scale, or
/// well under a pixel — so there is nothing here that wants a heavier average, and paying for
/// one in lag would be a poor trade.
pub const RESPONSIVE_WEIGHT: i32 = 2;

/// An exponential moving average over the three axes.
///
/// Holds no clock and no history beyond the running average, so it is `Copy` and costs three
/// words: the sampler thread keeps one and folds every reading through it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Smoother {
    /// How many samples the average is spread over; `1` passes samples through untouched.
    weight: i32,
    /// The running average, or `None` until the first sample seeds it.
    average: Option<Acceleration>,
}

impl Smoother {
    /// A smoother of the given `weight`, holding no samples yet.
    ///
    /// A `weight` below `1` would invert or divide by zero, so it is clamped to `1` — which
    /// is the identity smoother, the honest reading of "average over one sample".
    pub const fn new(weight: i32) -> Self {
        Smoother {
            weight: if weight < 1 { 1 } else { weight },
            average: None,
        }
    }

    /// The running average so far, or `None` before the first sample.
    pub const fn average(&self) -> Option<Acceleration> {
        self.average
    }

    /// Fold `sample` into the average and return the new value.
    ///
    /// The **first** sample is adopted whole rather than averaged toward from zero. Starting
    /// from a zero vector would make the readout sweep up from "free fall" to the true
    /// attitude over the first few frames — a visible lie at boot, and one that would show
    /// the wrong face while it settled.
    pub fn update(&mut self, sample: Acceleration) -> Acceleration {
        let next: Acceleration = match self.average {
            None => sample,
            Some(average) => Acceleration::new(
                self.step(average.x_mg, sample.x_mg),
                self.step(average.y_mg, sample.y_mg),
                self.step(average.z_mg, sample.z_mg),
            ),
        };
        self.average = Some(next);
        next
    }

    /// Move one axis' average toward `sample` by its share of the difference.
    ///
    /// Integer division truncates, so a difference smaller than the weight would divide to
    /// zero and the average would stall a milli-g or two short of a steady reading — forever
    /// reporting a value it can see is wrong. The final clause spends the last step at one
    /// milli-g per sample instead, so the average always converges *exactly*. Widened to
    /// `i64` for the subtraction: two opposite-signed extremes would overflow an `i32`.
    fn step(&self, average: i32, sample: i32) -> i32 {
        let difference: i64 = i64::from(sample) - i64::from(average);
        let share: i64 = difference / i64::from(self.weight);
        let movement: i64 = if share == 0 {
            difference.signum()
        } else {
            share
        };
        (i64::from(average) + movement).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }
}

impl Default for Smoother {
    /// The [`RESPONSIVE_WEIGHT`] smoother — what the orientation app wants.
    fn default() -> Self {
        Smoother::new(RESPONSIVE_WEIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::ONE_G_MG;
    use proptest::prelude::*;

    /// Feed `samples` through a fresh smoother of `weight` and return its final average.
    fn folded(weight: i32, samples: &[Acceleration]) -> Acceleration {
        let mut smoother: Smoother = Smoother::new(weight);
        samples
            .iter()
            .fold(Acceleration::default(), |_, sample: &Acceleration| {
                smoother.update(*sample)
            })
    }

    /// Zero: a smoother that has seen nothing holds nothing — distinct from holding a zero
    /// reading, which is what makes the first-sample rule expressible at all.
    #[test]
    fn a_fresh_smoother_holds_no_average() {
        assert_eq!(Smoother::default().average(), None);
    }

    /// One: the first sample is adopted whole, so the readout is correct from the very first
    /// frame rather than sweeping up to the truth from free fall.
    #[test]
    fn the_first_sample_is_adopted_whole() {
        let flat: Acceleration = Acceleration::new(0, 0, ONE_G_MG);
        assert_eq!(folded(RESPONSIVE_WEIGHT, &[flat]), flat);
    }

    /// Many: a constant input converges to exactly that input and stays there — no residual
    /// offset left behind by integer division.
    #[test]
    fn a_constant_input_converges_exactly() {
        let steady: Acceleration = Acceleration::new(-137, 42, 991);
        let samples: [Acceleration; 40] = [steady; 40];
        assert_eq!(folded(RESPONSIVE_WEIGHT, &samples), steady);
    }

    /// The convergence guarantee at its hardest: a one-milli-g difference is smaller than the
    /// weight, so plain integer division would truncate it to zero and stall here forever.
    #[test]
    fn a_difference_smaller_than_the_weight_still_converges() {
        let mut smoother: Smoother = Smoother::new(8);
        smoother.update(Acceleration::new(0, 0, 0));
        assert_eq!(
            smoother.update(Acceleration::new(1, 0, 0)),
            Acceleration::new(1, 0, 0),
            "the average stalled short of the sample"
        );
    }

    /// A weight of one is the identity: every sample passes straight through, unsmoothed.
    #[test]
    fn a_unit_weight_passes_every_sample_through() {
        let first: Acceleration = Acceleration::new(0, 0, ONE_G_MG);
        let second: Acceleration = Acceleration::new(ONE_G_MG, 0, 0);
        assert_eq!(folded(1, &[first, second]), second);
    }

    /// A nonsensical weight is clamped rather than dividing by zero or inverting the step.
    #[test]
    fn a_non_positive_weight_is_clamped_to_the_identity() {
        let sample: Acceleration = Acceleration::new(0, 0, ONE_G_MG);
        assert_eq!(folded(0, &[Acceleration::default(), sample]), sample);
        assert_eq!(folded(-5, &[Acceleration::default(), sample]), sample);
    }

    /// Smoothing actually smooths: a single wild spike in an otherwise steady stream moves
    /// the average far less than the spike itself. This is the whole point of the crate —
    /// without it the test suite would happily pass an identity function.
    #[test]
    fn a_single_spike_moves_the_average_far_less_than_the_spike() {
        let steady: Acceleration = Acceleration::new(0, 0, ONE_G_MG);
        let spike: Acceleration = Acceleration::new(0, 0, 4 * ONE_G_MG);
        let after: Acceleration = folded(4, &[steady, steady, steady, spike]);
        assert!(
            after.z_mg < steady.z_mg + (spike.z_mg - steady.z_mg) / 2,
            "the spike dominated the average — nothing was smoothed"
        );
        assert!(after.z_mg > steady.z_mg, "the spike was ignored entirely");
    }

    /// How many samples the default smoother needs to track a full 90° turn to within one
    /// percent of a gravity. Each sample closes half the remaining gap, so `1000 mg` falls
    /// under `10 mg` on the seventh.
    const SETTLE_SAMPLES: usize = 7;

    /// Responsiveness, stated as a measured bound rather than a hope: the default smoother
    /// tracks a sharp 90° turn to within a percent of a gravity in [`SETTLE_SAMPLES`]. At the
    /// sampler's 10 ms cadence that is about **70 ms** — under the ~100 ms at which a hand
    /// movement and its readout stop feeling simultaneous, and far quicker than the board can
    /// actually be turned. This bound is the reason the weight is 2 and not something
    /// steadier: it is the budget the whole readout's responsiveness is spent from.
    #[test]
    fn the_default_smoother_tracks_a_full_turn_inside_its_responsiveness_budget() {
        let mut smoother: Smoother = Smoother::default();
        smoother.update(Acceleration::new(0, 0, ONE_G_MG));

        let turned: Acceleration = Acceleration::new(ONE_G_MG, 0, 0);
        let settled: Acceleration =
            (0..SETTLE_SAMPLES).fold(Acceleration::default(), |_, _| smoother.update(turned));

        assert!(
            (settled.x_mg - turned.x_mg).abs() < 10,
            "the readout lags the board by more than a percent of a gravity after {SETTLE_SAMPLES} samples"
        );
        assert!((settled.z_mg - turned.z_mg).abs() < 10);
    }

    /// Most of the movement lands on the *first* sample, which is what makes the readout feel
    /// immediate rather than merely arriving quickly: half of any turn is on the glass one
    /// sample — 10 ms — after the board moved.
    #[test]
    fn the_first_sample_after_a_turn_already_shows_half_of_it() {
        let mut smoother: Smoother = Smoother::default();
        smoother.update(Acceleration::new(0, 0, ONE_G_MG));
        let moved: Acceleration = smoother.update(Acceleration::new(ONE_G_MG, 0, 0));
        assert!(moved.x_mg >= ONE_G_MG / 2);
    }

    proptest! {
        /// The average never leaves the interval between where it was and the sample it is
        /// moving toward — it cannot overshoot, oscillate, or run away.
        #[test]
        fn the_average_never_overshoots_the_sample(
            start in -16_000i32..=16_000,
            sample in -16_000i32..=16_000,
            weight in 1i32..=16,
        ) {
            let mut smoother: Smoother = Smoother::new(weight);
            smoother.update(Acceleration::new(start, 0, 0));
            let moved: i32 = smoother.update(Acceleration::new(sample, 0, 0)).x_mg;
            prop_assert!(moved >= start.min(sample));
            prop_assert!(moved <= start.max(sample));
        }

        /// Repeating any sample long enough always reaches it exactly, at every weight — the
        /// convergence guarantee, over the whole input range rather than one example.
        #[test]
        fn repeating_a_sample_always_reaches_it_exactly(
            start in -16_000i32..=16_000,
            sample in -16_000i32..=16_000,
            weight in 1i32..=16,
        ) {
            let mut smoother: Smoother = Smoother::new(weight);
            smoother.update(Acceleration::new(start, 0, 0));
            let target: Acceleration = Acceleration::new(sample, 0, 0);
            // Ample: each step closes a `1/weight` share, and the last milli-g one per
            // sample, so the widest range at the heaviest weight is well inside this.
            let settled: i32 = (0..2_000).fold(0, |_, _| smoother.update(target).x_mg);
            prop_assert_eq!(settled, sample);
        }
    }
}
