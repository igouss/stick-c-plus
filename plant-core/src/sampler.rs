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

use crate::moisture::{to_percent, Calibration, Moisture};

/// The outcome of one sampling step.
///
/// `state` is the moisture to carry into the next step; `report` is the value
/// to push downstream — `Some` only when the reading actually changed, so an
/// unchanged plant produces no traffic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    /// The moisture state after this step: `None` only before the first
    /// non-empty reading has ever been taken.
    pub state: Option<Moisture>,
    /// The value to report, present only when it differs from the previous
    /// state — a batch of zero readings, or an unchanged value, reports nothing.
    pub report: Option<Moisture>,
}

/// Fold `raws` into a moisture value against `cal`, relative to `prev`.
///
/// Averages the raw readings (integer mean, no float), calibrates the mean
/// through [`to_percent`], and reports the result only if it differs from
/// `prev`. An empty `raws` is not an error — there is simply nothing new to
/// measure, so the previous `state` is carried forward and nothing is reported.
///
/// Pure and deterministic: the same `(prev, raws, cal)` always yields the same
/// [`Sample`], with no side effects.
pub fn step(prev: Option<Moisture>, raws: &[u16], cal: Calibration) -> Sample {
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

    let measured: Moisture = to_percent(mean, cal);
    let report: Option<Moisture> = if prev == Some(measured) {
        None
    } else {
        Some(measured)
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
    use crate::ports::SoilSensor;

    /// A calibration where a raw count maps to the same number as a percent
    /// (`raw N → N %` for `N <= 100`), so tests can read the arithmetic
    /// directly: raw 30 → 30 %.
    const LINEAR: Calibration = Calibration::new(0, 100);

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

    impl SoilSensor for ScriptedSensor {
        type Error = ();

        fn read_raw(&mut self) -> Result<u16, Self::Error> {
            let value: u16 = *self.script.get(self.next).ok_or(())?;
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
        let prev: Option<Moisture> = Moisture::new(42);
        let out: Sample = step(prev, &[], LINEAR);
        assert_eq!(out.state, prev);
        assert_eq!(out.report, None);
    }

    #[test]
    fn one_reading_from_a_fresh_state_is_reported() {
        let mut sensor: ScriptedSensor = ScriptedSensor::new(&[70]);
        let batch: Vec<u16> = gather(&mut sensor);
        let out: Sample = step(None, &batch, LINEAR);
        assert_eq!(out.state, Moisture::new(70));
        assert_eq!(out.report, Moisture::new(70));
    }

    #[test]
    fn many_readings_are_averaged_not_picked() {
        // mean(10,20,60) = 30, which equals none of first(10), last(60),
        // min(10) or max(60): a non-averaging implementation reports the wrong
        // percent and fails this test.
        let mut sensor: ScriptedSensor = ScriptedSensor::new(&[10, 20, 60]);
        let batch: Vec<u16> = gather(&mut sensor);
        let out: Sample = step(None, &batch, LINEAR);
        assert_eq!(out.report, Moisture::new(30));
        assert_eq!(out.state, Moisture::new(30));
    }

    #[test]
    fn an_unchanged_value_is_not_reported() {
        let prev: Option<Moisture> = Moisture::new(30);
        let out: Sample = step(prev, &[30], LINEAR);
        assert_eq!(out.state, Moisture::new(30));
        assert_eq!(out.report, None, "same value must not generate traffic");
    }

    #[test]
    fn a_changed_value_is_reported() {
        let prev: Option<Moisture> = Moisture::new(30);
        let out: Sample = step(prev, &[55], LINEAR);
        assert_eq!(out.state, Moisture::new(55));
        assert_eq!(out.report, Moisture::new(55));
    }

    #[test]
    fn readings_are_clamped_through_calibration() {
        // A reading past the wet endpoint still lands at 100 %, never wraps.
        let out: Sample = step(None, &[RAW_MAX], Calibration::new(0, 100));
        assert_eq!(out.report, Some(Moisture::SATURATED));
    }
}
