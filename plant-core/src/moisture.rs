//! Moisture — the domain's soil-moisture value objects and calibration curve.
//!
//! The M5 Earth Unit is a *resistive* soil probe read through the ESP32 ADC
//! (12-bit, so raw readings land in `0..=4095`). Wetter soil conducts better,
//! so on most wirings a wet probe reads *higher* than a dry one — but that
//! depends on how the divider is wired, and it varies unit to unit. The
//! calibration math therefore records two raw endpoints and never assumes which
//! one is larger: `dry_raw` always maps to 0 %, `wet_raw` always maps to 100 %,
//! whichever way round the hardware happens to read.

/// The largest raw reading the ESP32's 12-bit ADC can produce.
///
/// Callers (the [`crate::ports::SoilSensor`] port, once it exists) promise their
/// readings sit in `0..=RAW_MAX`; [`to_percent`] clamps regardless, so an
/// out-of-spec adapter can never push [`Moisture`] out of range.
pub const RAW_MAX: u16 = 4095;

/// A soil-moisture reading as a percentage, `0..=100`.
///
/// A newtype over `u8` whose invariant — the value is always `0..=100` — is
/// enforced at construction: there is no way to build a `Moisture` outside that
/// range. `0` is bone dry, `100` is saturated.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Moisture(u8);

impl Moisture {
    /// The driest reading — also the safe fallback for a collapsed calibration
    /// (see [`to_percent`]): reporting "dry" errs toward raising a watering
    /// alert rather than silently masking a mis-wired probe.
    pub const DRY: Moisture = Moisture(0);

    /// A fully saturated reading.
    pub const SATURATED: Moisture = Moisture(100);

    /// Build a `Moisture` from a percentage, rejecting anything above 100.
    ///
    /// Returns `None` rather than clamping: a percent that fell outside the
    /// range is a caller bug worth surfacing, not something to paper over.
    pub const fn new(percent: u8) -> Option<Moisture> {
        if percent <= 100 {
            Some(Moisture(percent))
        } else {
            None
        }
    }

    /// The percentage, `0..=100`.
    pub const fn percent(self) -> u8 {
        self.0
    }
}

/// A calibrated soil measurement: the raw ADC mean and the [`Moisture`] it maps to.
///
/// [`to_percent`] turns a raw count into a percent and would otherwise discard the
/// raw; pairing them here keeps every reading's *provenance*. The percent is what
/// Home Assistant graphs, but the raw count is what calibration capture (the dry/wet
/// endpoints) and floating-probe detection reason about — and what the on-board
/// display shows beside the percent — so it rides alongside the derived value rather
/// than being thrown away. `raw` carries no invariant of its own beyond the port's
/// `0..=`[`RAW_MAX`] contract; the [`Moisture`] keeps its `0..=100` invariant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Measurement {
    raw: u16,
    moisture: Moisture,
}

impl Measurement {
    /// Pair a raw ADC count with the `moisture` it was already calibrated to.
    pub const fn new(raw: u16, moisture: Moisture) -> Self {
        Self { raw, moisture }
    }

    /// Calibrate `raw` against `cal` into a measurement, retaining the raw count.
    ///
    /// The one constructor the sampler uses: [`to_percent`] gives the derived
    /// percent and the raw is kept beside it. Total and direction-agnostic, like
    /// [`to_percent`] — any `raw` yields a valid measurement, never a panic.
    pub fn measure(raw: u16, cal: Calibration) -> Self {
        Self::new(raw, to_percent(raw, cal))
    }

    /// The raw ADC count this measurement was calibrated from.
    pub const fn raw(self) -> u16 {
        self.raw
    }

    /// The calibrated moisture.
    pub const fn moisture(self) -> Moisture {
        self.moisture
    }

    /// The moisture percentage, `0..=100` — shorthand for
    /// [`self.moisture().percent()`](Moisture::percent).
    pub const fn percent(self) -> u8 {
        self.moisture.percent()
    }
}

/// The two raw ADC endpoints that pin a probe's moisture curve.
///
/// `dry_raw` is the reading in dry soil (→ 0 %); `wet_raw` is the reading in
/// saturated soil (→ 100 %). Either may be the larger — the resistive probe's
/// direction is a property of the wiring, not an assumption of this type. A
/// collapsed calibration (`dry_raw == wet_raw`, e.g. a freshly-zeroed NVS
/// region) is a legal value; [`to_percent`] defines its behaviour rather than
/// dividing by zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Calibration {
    dry_raw: u16,
    wet_raw: u16,
}

impl Calibration {
    /// A calibration from its dry and wet raw endpoints.
    pub const fn new(dry_raw: u16, wet_raw: u16) -> Self {
        Self { dry_raw, wet_raw }
    }

    /// The raw reading that maps to 0 % (dry soil).
    pub const fn dry_raw(self) -> u16 {
        self.dry_raw
    }

    /// The raw reading that maps to 100 % (saturated soil).
    pub const fn wet_raw(self) -> u16 {
        self.wet_raw
    }
}

/// Map a raw ADC reading to a [`Moisture`] percentage along `cal`'s curve.
///
/// Linear interpolation with the dry endpoint at 0 % and the wet endpoint at
/// 100 %, clamped to `0..=100`. Integer-only `i32` math — no float, no `libm`,
/// no FPU dependency — and signed throughout so the sign of the span carries
/// the probe's direction: the formula is identical whether `wet_raw` reads
/// higher or lower than `dry_raw`.
///
/// A degenerate calibration (`dry_raw == wet_raw`) has no gradient to
/// interpolate along, so it maps every reading to [`Moisture::DRY`] rather than
/// dividing by zero — see that constant for why "dry" is the safe fallback.
///
/// No overflow is possible: `raw`, `dry_raw`, `wet_raw` are all `u16`, so the
/// numerator `(raw - dry) * 100` fits well inside `i32`.
pub fn to_percent(raw: u16, cal: Calibration) -> Moisture {
    let dry: i32 = cal.dry_raw() as i32;
    let wet: i32 = cal.wet_raw() as i32;
    let span: i32 = wet - dry;

    if span == 0 {
        return Moisture::DRY;
    }

    let raw: i32 = raw as i32;
    let percent: i32 = (raw - dry) * 100 / span;
    let clamped: i32 = percent.clamp(0, 100);

    // `clamped` is provably in `0..=100`, so this branch is unreachable; we
    // construct the newtype directly rather than `unwrap`-ing an `Option`.
    Moisture(clamped as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn moisture_rejects_above_one_hundred() {
        assert_eq!(Moisture::new(101), None);
        assert_eq!(Moisture::new(255), None);
    }

    #[test]
    fn moisture_accepts_the_endpoints() {
        assert_eq!(Moisture::new(0), Some(Moisture::DRY));
        assert_eq!(Moisture::new(100), Some(Moisture::SATURATED));
    }

    #[test]
    fn wet_reads_high_maps_a_midpoint() {
        // dry=1000 → 0 %, wet=3000 → 100 %, so 2000 is the halfway point.
        let cal: Calibration = Calibration::new(1000, 3000);
        assert_eq!(to_percent(2000, cal), Moisture::new(50).unwrap());
    }

    #[test]
    fn wet_reads_low_maps_the_same_midpoint() {
        // Inverted wiring: dry=3000 → 0 %, wet=1000 → 100 %. 2000 is still 50 %.
        let cal: Calibration = Calibration::new(3000, 1000);
        assert_eq!(to_percent(2000, cal), Moisture::new(50).unwrap());
    }

    #[test]
    fn degenerate_calibration_is_dry_not_a_panic() {
        let cal: Calibration = Calibration::new(2048, 2048);
        assert_eq!(to_percent(2048, cal), Moisture::DRY);
        assert_eq!(to_percent(0, cal), Moisture::DRY);
        assert_eq!(to_percent(RAW_MAX, cal), Moisture::DRY);
    }

    #[test]
    fn a_measurement_retains_its_raw_and_derives_its_percent() {
        // measure() keeps the raw count and delegates the percent to to_percent:
        // raw 30 through a linear 0->100 curve is 30 %, and raw 30 is preserved.
        let cal: Calibration = Calibration::new(0, 100);
        let m: Measurement = Measurement::measure(30, cal);
        assert_eq!(m.raw(), 30);
        assert_eq!(m.moisture(), Moisture::new(30).unwrap());
        assert_eq!(m.percent(), 30);
    }

    /// A calibration strategy that spans the full ADC range *and deliberately
    /// samples the degenerate and near-degenerate cases* — equal endpoints and
    /// endpoints one apart — which uniform sampling would almost never hit.
    fn any_calibration() -> impl Strategy<Value = Calibration> {
        let plain = (0u16..=RAW_MAX, 0u16..=RAW_MAX)
            .prop_map(|(dry, wet): (u16, u16)| Calibration::new(dry, wet));
        let equal = (0u16..=RAW_MAX).prop_map(|r: u16| Calibration::new(r, r));
        let adjacent = (0u16..RAW_MAX).prop_map(|r: u16| Calibration::new(r, r + 1));
        prop_oneof![3 => plain, 1 => equal, 1 => adjacent]
    }

    proptest! {
        /// The core invariant: whatever the reading or calibration, the result
        /// is a valid percentage. If this holds, no `to_percent` result can
        /// ever violate `Moisture`'s range.
        #[test]
        fn result_is_always_a_valid_percent(
            raw in 0u16..=RAW_MAX,
            cal in any_calibration(),
        ) {
            let m: Moisture = to_percent(raw, cal);
            prop_assert!(m.percent() <= 100);
        }

        /// The dry endpoint pins exactly to 0 % — for any calibration whose
        /// endpoints actually differ (a degenerate one is covered separately).
        #[test]
        fn dry_endpoint_maps_to_zero(dry in 0u16..=RAW_MAX, wet in 0u16..=RAW_MAX) {
            prop_assume!(dry != wet);
            let cal: Calibration = Calibration::new(dry, wet);
            prop_assert_eq!(to_percent(dry, cal), Moisture::DRY);
        }

        /// The wet endpoint pins exactly to 100 %.
        #[test]
        fn wet_endpoint_maps_to_one_hundred(dry in 0u16..=RAW_MAX, wet in 0u16..=RAW_MAX) {
            prop_assume!(dry != wet);
            let cal: Calibration = Calibration::new(dry, wet);
            prop_assert_eq!(to_percent(wet, cal), Moisture::SATURATED);
        }

        /// Readings past an endpoint clamp, never wrap: anything on the dry side
        /// of `dry_raw` is 0 %, anything past `wet_raw` is 100 %.
        #[test]
        fn readings_outside_the_endpoints_clamp(
            dry in 0u16..=RAW_MAX,
            wet in 0u16..=RAW_MAX,
            overshoot in 1u16..=RAW_MAX,
        ) {
            prop_assume!(dry != wet);
            let cal: Calibration = Calibration::new(dry, wet);
            // Step further away from wet than dry → still 0 %.
            let past_dry: u16 = if dry > wet {
                dry.saturating_add(overshoot)
            } else {
                dry.saturating_sub(overshoot)
            };
            // Step further away from dry than wet → still 100 %.
            let past_wet: u16 = if wet > dry {
                wet.saturating_add(overshoot)
            } else {
                wet.saturating_sub(overshoot)
            };
            prop_assert_eq!(to_percent(past_dry, cal), Moisture::DRY);
            prop_assert_eq!(to_percent(past_wet, cal), Moisture::SATURATED);
        }

        /// Monotonicity, direction-agnostic: as the reading moves toward the wet
        /// endpoint the percentage never decreases. Concretely, for `a <= b`,
        /// the map is non-decreasing when `wet > dry` and non-increasing when
        /// `wet < dry`.
        #[test]
        fn the_curve_is_monotone(
            dry in 0u16..=RAW_MAX,
            wet in 0u16..=RAW_MAX,
            a in 0u16..=RAW_MAX,
            b in 0u16..=RAW_MAX,
        ) {
            prop_assume!(dry != wet);
            let (lo, hi): (u16, u16) = if a <= b { (a, b) } else { (b, a) };
            let cal: Calibration = Calibration::new(dry, wet);
            let p_lo: u8 = to_percent(lo, cal).percent();
            let p_hi: u8 = to_percent(hi, cal).percent();
            if wet > dry {
                prop_assert!(p_lo <= p_hi);
            } else {
                prop_assert!(p_lo >= p_hi);
            }
        }

        /// A collapsed calibration never panics and always reports dry, for any
        /// reading. Complements the invariant test by forcing the `span == 0`
        /// branch on every case.
        #[test]
        fn degenerate_calibration_is_always_dry(raw in 0u16..=RAW_MAX, point in 0u16..=RAW_MAX) {
            let cal: Calibration = Calibration::new(point, point);
            prop_assert_eq!(to_percent(raw, cal), Moisture::DRY);
        }

        /// A measurement always keeps the exact raw it was read from, and its
        /// moisture equals the standalone calibration — pairing raw with percent
        /// perturbs neither.
        #[test]
        fn a_measurement_pairs_raw_with_the_calibrated_percent(
            raw in 0u16..=RAW_MAX,
            cal in any_calibration(),
        ) {
            let m: Measurement = Measurement::measure(raw, cal);
            prop_assert_eq!(m.raw(), raw);
            prop_assert_eq!(m.moisture(), to_percent(raw, cal));
        }
    }
}
