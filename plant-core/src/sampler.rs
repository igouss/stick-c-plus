//! The sampling use case (Control), as a pure Functional Core.
//!
//! One [`step`] folds a batch of raw readings into a moisture value and decides
//! whether it is worth reporting. It is a *pure function* of its inputs — no
//! sensor handle, no clock, no caching, no interior mutability — so the whole
//! sampling policy is exercisable on the host with plain values. The firmware
//! shell (a later bead) is the only thing that touches a [`SoilSensor`]: it
//! gathers a slice of readings, hands them here, and pushes any [`report`] to
//! the native-API entity.
//!
//! [`report`]: Sample::report

use crate::moisture::{Calibration, Measurement, Moisture};

/// The outcome of one sampling step.
///
/// `state` is the [`Measurement`] to carry into the next step — the calibrated
/// percent *and* the raw count it came from; `report` is the percent to push
/// downstream — `Some` only when the percent actually changed, so an unchanged
/// plant produces no traffic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// The measurement state after this step: `None` only before the first
    /// non-empty reading has ever been taken. Carries the raw ADC count beside
    /// the calibrated percent (see [`Measurement`]).
    pub state: Option<Measurement>,
    /// The percent to report, present only when it differs from the previous
    /// state's percent — a batch of zero readings, or an unchanged percent,
    /// reports nothing. Keyed on the percent, not the raw count, so a raw that
    /// drifts within one percent bucket never generates downstream traffic.
    pub report: Option<Moisture>,
}

/// Fold `raws` into a [`Measurement`] against `cal`, relative to `prev`.
///
/// Averages the raw readings (integer mean, no float), calibrates the mean
/// through [`Measurement::measure`] — keeping the raw beside the derived percent —
/// and reports the percent only if it differs from `prev`'s. An empty `raws` is
/// not an error — there is simply nothing new to measure, so the previous `state`
/// (raw and all) is carried forward and nothing is reported.
///
/// Report-on-change is keyed on the *percent*, not the raw mean: two raws that
/// calibrate to the same percent produce no report, so a steady plant generates no
/// downstream traffic even as its raw count jitters within a percent bucket.
///
/// Pure and deterministic: the same `(prev, raws, cal)` always yields the same
/// [`Sample`], with no side effects.
pub fn step(prev: Option<Measurement>, raws: &[u16], cal: Calibration) -> Sample {
    if raws.is_empty() {
        return Sample {
            state: prev,
            report: None,
        };
    }

    // Integer mean. `raws.len() >= 1` here, so the division is safe; the sum of
    // 12-bit counts cannot overflow u32 for any realistic batch.
    let sum: u32 = raws.iter().map(|&raw: &u16| raw as u32).sum();
    let mean: u16 = (sum / raws.len() as u32) as u16;

    let measured: Measurement = Measurement::measure(mean, cal);
    let report: Option<Moisture> = if prev.map(Measurement::moisture) == Some(measured.moisture()) {
        None
    } else {
        Some(measured.moisture())
    };

    Sample {
        state: Some(measured),
        report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moisture::RAW_MAX;
    use crate::observation::ProbeFault;
    use crate::ports::{SoilFault, SoilSensor};

    /// A calibration where a raw count maps to the same number as a percent
    /// (`raw N → N %` for `N <= 100`), so tests can read the arithmetic
    /// directly: raw 30 → 30 %.
    const LINEAR: Calibration = Calibration::new(0, 100);

    /// A measurement read from `raw` through [`LINEAR`]: `measured(30)` is raw 30
    /// calibrated to 30 %, with the raw preserved.
    fn measured(raw: u16) -> Measurement {
        Measurement::measure(raw, LINEAR)
    }

    /// A `SoilSensor` fake that yields a fixed script of readings, then errors.
    /// Reading through the real port (rather than passing a literal slice)
    /// proves the shell pattern — gather from the port, fold with [`step`] —
    /// composes, while `step` itself stays sensor-free.
    struct ScriptedSensor {
        script: Vec<u16>,
        next: usize,
    }

    impl ScriptedSensor {
        fn new(script: &[u16]) -> Self {
            Self {
                script: script.to_vec(),
                next: 0,
            }
        }
    }

    /// The fake's read failure: the script ran out. Classifies as `Unreadable`,
    /// the same way a real ADC error would — the port requires every failure to be
    /// nameable as a domain fault.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Exhausted;

    impl SoilFault for Exhausted {
        fn fault(&self) -> ProbeFault {
            ProbeFault::Unreadable
        }
    }

    impl SoilSensor for ScriptedSensor {
        type Error = Exhausted;

        fn read_raw(&mut self) -> Result<u16, Self::Error> {
            let value: u16 = *self.script.get(self.next).ok_or(Exhausted)?;
            self.next += 1;
            Ok(value)
        }
    }

    /// Drain the fake sensor's whole script into a batch, the way the firmware
    /// shell will gather a burst of readings before folding them.
    fn gather(sensor: &mut ScriptedSensor) -> Vec<u16> {
        let mut batch: Vec<u16> = Vec::new();
        while let Ok(raw) = sensor.read_raw() {
            batch.push(raw);
        }
        batch
    }

    #[test]
    fn zero_readings_report_nothing_and_keep_state() {
        let prev: Option<Measurement> = Some(measured(42));
        let out: Sample = step(prev, &[], LINEAR);
        assert_eq!(
            out.state, prev,
            "the whole measurement, raw and all, carries forward"
        );
        assert_eq!(out.report, None);
    }

    #[test]
    fn one_reading_from_a_fresh_state_is_reported() {
        let mut sensor: ScriptedSensor = ScriptedSensor::new(&[70]);
        let batch: Vec<u16> = gather(&mut sensor);
        let out: Sample = step(None, &batch, LINEAR);
        assert_eq!(out.state, Some(measured(70)));
        assert_eq!(out.report, Moisture::new(70));
    }

    #[test]
    fn many_readings_are_averaged_not_picked() {
        // mean(10,20,60) = 30, which equals none of first(10), last(60),
        // min(10) or max(60): a non-averaging implementation reports the wrong
        // percent and fails this test. The state carries that mean as its raw.
        let mut sensor: ScriptedSensor = ScriptedSensor::new(&[10, 20, 60]);
        let batch: Vec<u16> = gather(&mut sensor);
        let out: Sample = step(None, &batch, LINEAR);
        assert_eq!(out.report, Moisture::new(30));
        assert_eq!(out.state, Some(measured(30)));
        assert_eq!(out.state.unwrap().raw(), 30, "the raw mean is retained");
    }

    #[test]
    fn an_unchanged_value_is_not_reported() {
        let prev: Option<Measurement> = Some(measured(30));
        let out: Sample = step(prev, &[30], LINEAR);
        assert_eq!(out.state, Some(measured(30)));
        assert_eq!(out.report, None, "same value must not generate traffic");
    }

    #[test]
    fn a_steady_percent_with_a_different_raw_reports_nothing() {
        // Report-on-change keys on the *percent*, not the raw mean. Under this
        // curve percent = raw / 10, so raw 300 and 305 both calibrate to 30 %: a
        // step between them carries the new raw in `state` yet reports nothing.
        let cal: Calibration = Calibration::new(0, 1000);
        let prev: Option<Measurement> = Some(Measurement::measure(300, cal));
        let out: Sample = step(prev, &[305], cal);
        assert_eq!(out.state, Some(Measurement::measure(305, cal)));
        assert_eq!(out.state.unwrap().raw(), 305, "state carries the new raw");
        assert_eq!(out.state.unwrap().percent(), 30);
        assert_eq!(
            out.report, None,
            "an unchanged percent generates no traffic"
        );
    }

    #[test]
    fn a_changed_value_is_reported() {
        let prev: Option<Measurement> = Some(measured(30));
        let out: Sample = step(prev, &[55], LINEAR);
        assert_eq!(out.state, Some(measured(55)));
        assert_eq!(out.report, Moisture::new(55));
    }

    #[test]
    fn readings_are_clamped_through_calibration() {
        // A reading past the wet endpoint still lands at 100 %, never wraps.
        let out: Sample = step(None, &[RAW_MAX], Calibration::new(0, 100));
        assert_eq!(out.report, Some(Moisture::SATURATED));
    }
}
