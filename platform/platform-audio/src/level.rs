//! The acoustic level of a PCM block, and the sound-present verdict.

/// The AC (DC-removed) root-mean-square amplitude of a PCM block — its acoustic level.
///
/// A microphone capture is the sound as an AC swing riding on a DC bias (the PDM/ADC zero
/// offset, which drifts and is largest right after bring-up). Loudness is the AC part alone, so
/// this subtracts the block's own mean before taking the RMS: silence reads near zero whatever
/// the bias, and a sinusoid of amplitude `A` reads `A/√2`. Empty input reads zero.
///
/// This is the measurement the chime self-test leans on. The M5StickC Plus buzzer is a tiny
/// resonant transducer: it does *not* radiate a clean tone at the pitch it is driven, so asking
/// "is there energy at the commanded frequency?" is the wrong question on this hardware. "Did the
/// acoustic level rise well above silence when the note played?" is the right one, and it is what
/// [`present`] turns into a pass/fail.
pub fn ac_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let n: f64 = samples.len() as f64;
    let mean: f64 = samples.iter().map(|&s: &i16| f64::from(s)).sum::<f64>() / n;
    let sum_sq: f64 = samples
        .iter()
        .map(|&s: &i16| {
            let deviation: f64 = f64::from(s) - mean;
            deviation * deviation
        })
        .sum::<f64>();
    libm::sqrt(sum_sq / n) as f32
}

/// Whether a measured level clears `threshold` — the verdict a self-test turns into pass/fail.
/// Split from the measurement (physics) so the threshold (policy, board-empirical) is named
/// separately: a note is "heard" when its [`ac_rms`] rises above the silent floor's.
pub fn present(level: f32, threshold: f32) -> bool {
    level >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::PI;
    use proptest::prelude::*;

    const SAMPLE_RATE: f64 = 44_100.0;

    /// Synthesize `n` samples of a sine at `freq_hz`, amplitude `amp`, on a DC `bias`.
    fn sine(freq_hz: f64, amp: f64, bias: f64, n: usize) -> Vec<i16> {
        (0..n)
            .map(|i: usize| {
                let t: f64 = f64::from(i as u32) / SAMPLE_RATE;
                (bias + amp * (2.0 * PI * freq_hz * t).sin()) as i16
            })
            .collect()
    }

    /// Zero: empty input has no level.
    #[test]
    fn empty_input_reads_zero() {
        assert_eq!(ac_rms(&[]), 0.0);
    }

    /// Zero: a constant block (pure DC, any bias) is silence — the AC level is zero.
    #[test]
    fn constant_block_reads_zero() {
        assert_eq!(ac_rms(&[7_000; 1_024]), 0.0);
    }

    /// One: a sinusoid of amplitude `A` reads `A/√2`, and the DC bias it rides on is removed.
    #[test]
    fn a_sinusoid_reads_amplitude_over_root_two() {
        let amp: f64 = 10_000.0;
        let level: f32 = ac_rms(&sine(4_000.0, amp, 6_000.0, 4_096));
        let expected: f32 = (amp / core::f64::consts::SQRT_2) as f32;
        assert!(
            (level - expected).abs() < 100.0,
            "expected ~{expected}, got {level}"
        );
    }

    // Many: the level tracks amplitude and ignores the DC bias — louder always reads higher, and
    // shifting the whole block by a constant leaves the level unchanged.
    proptest! {
        #[test]
        fn level_scales_with_amplitude_and_ignores_bias(
            quiet in 200.0f64..2_000.0,
            louder in 4_000.0f64..12_000.0,
            bias in -8_000.0f64..8_000.0,
        ) {
            let quiet_level: f32 = ac_rms(&sine(3_000.0, quiet, 0.0, 2_048));
            let loud_level: f32 = ac_rms(&sine(3_000.0, louder, 0.0, 2_048));
            prop_assert!(loud_level > quiet_level, "louder {loud_level} !> quieter {quiet_level}");

            // Same tone, ridden on an arbitrary DC bias: the level is unchanged (± rounding).
            let unbiased: f32 = ac_rms(&sine(3_000.0, louder, 0.0, 2_048));
            let biased: f32 = ac_rms(&sine(3_000.0, louder, bias, 2_048));
            prop_assert!((unbiased - biased).abs() < 50.0, "bias moved level: {unbiased} vs {biased}");
        }
    }

    /// The verdict is a plain threshold: at or above passes, below fails.
    #[test]
    fn present_is_a_threshold() {
        assert!(present(300.0, 150.0));
        assert!(present(150.0, 150.0), "at the threshold passes");
        assert!(!present(149.0, 150.0));
    }
}
