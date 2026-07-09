//! Saturation — a raw ADC count sitting on a rail is not a measurement.
//!
//! An ADC that reports its floor or its full scale is telling you the input was
//! *at least* as extreme as it can express, not what the input actually was. The
//! count carries no information: every voltage beyond the rail maps to the same
//! number. Passing it downstream lets a calibration curve turn "I cannot see this"
//! into a confident, wrong percentage.
//!
//! That is not hypothetical here. The M5 Earth Unit pulls its analog output up to
//! its own 3.3 V rail through a 10 kΩ resistor, with the soil in the *lower* leg of
//! the divider. So:
//!
//! - **Electrodes open** — corroded through, or soil dry enough to exceed the
//!   ADC's usable ceiling — the node is pulled to the rail and every conversion
//!   reads full scale.
//! - **Rail down** — a probe that failed to energize (see [`crate::gated_read`])
//!   leaves the node at ground and every conversion reads zero.
//!
//! Both are faults, and under a linear calibration both look like a perfectly
//! ordinary 0 % or 100 %. A resistive probe corroded silently for two days behind
//! exactly that mask. Rejecting the rails at the ADC boundary is what turns a
//! plausible lie into an honest *unavailable*.
//!
//! Pure and hardware-free — the rule is a property of the converter's range, not of
//! any particular converter — so it is unit- and property-tested on the host.

use core::fmt;

/// Which rail a reading pinned against.
///
/// Kept as two variants rather than one opaque "invalid": the two ends have
/// different causes and a caller that wants to log or alert distinctly can, without
/// re-deriving which rail it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Saturation {
    /// The reading sat at zero — the input was at or below the converter's floor.
    /// On a divider-fed probe this is the signature of an unpowered rail.
    Floor,
    /// The reading sat at full scale — the input was at or above the converter's
    /// ceiling. On a divider-fed probe this is the signature of an open lower leg.
    Ceiling,
}

impl fmt::Display for Saturation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Saturation::Floor => f.write_str("ADC saturated at its floor (0)"),
            Saturation::Ceiling => f.write_str("ADC saturated at full scale"),
        }
    }
}

/// Accept `raw` only if it sits strictly inside `0..full_scale`.
///
/// The rails are rejected, everything between them is returned unchanged. The
/// ceiling test is `>=` rather than `==` so an out-of-spec adapter that overshoots
/// its declared `full_scale` is caught rather than waved through — this function
/// makes no assumption that its caller is honest about its own range.
///
/// A degenerate `full_scale` of 0 or 1 leaves no interior, so every reading is
/// saturated. That is the truthful answer for a converter with no usable range, and
/// it is reached without a division or a panic.
pub fn unsaturated(raw: u16, full_scale: u16) -> Result<u16, Saturation> {
    if raw == 0 {
        Err(Saturation::Floor)
    } else if raw >= full_scale {
        Err(Saturation::Ceiling)
    } else {
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The 12-bit ceiling the ESP32's ADC reports, and the one the Earth Unit
    /// adapter passes in.
    const FULL_SCALE: u16 = 4095;

    #[test]
    fn the_floor_is_rejected() {
        assert_eq!(unsaturated(0, FULL_SCALE), Err(Saturation::Floor));
    }

    #[test]
    fn the_ceiling_is_rejected() {
        assert_eq!(unsaturated(4095, FULL_SCALE), Err(Saturation::Ceiling));
    }

    #[test]
    fn one_count_inside_each_rail_is_accepted() {
        // The tightest possible interior readings: a rule written with `<`/`>`
        // instead of `<=`/`>=` rejects these and fails here.
        assert_eq!(unsaturated(1, FULL_SCALE), Ok(1));
        assert_eq!(unsaturated(4094, FULL_SCALE), Ok(4094));
    }

    #[test]
    fn a_midscale_reading_is_accepted_unchanged() {
        assert_eq!(unsaturated(2048, FULL_SCALE), Ok(2048));
    }

    #[test]
    fn a_reading_past_the_declared_ceiling_is_saturated_not_accepted() {
        // An adapter that lies about its range must not smuggle a value through.
        assert_eq!(unsaturated(5000, FULL_SCALE), Err(Saturation::Ceiling));
    }

    #[test]
    fn a_converter_with_no_interior_saturates_everywhere() {
        // full_scale 0 and 1 leave no strictly-interior count; neither panics.
        assert_eq!(unsaturated(0, 0), Err(Saturation::Floor));
        assert_eq!(unsaturated(1, 0), Err(Saturation::Ceiling));
        assert_eq!(unsaturated(0, 1), Err(Saturation::Floor));
        assert_eq!(unsaturated(1, 1), Err(Saturation::Ceiling));
    }

    proptest! {
        /// The whole rule, stated once: a count is accepted exactly when it is
        /// strictly between the rails, and an accepted count is returned untouched.
        #[test]
        fn accepted_exactly_when_strictly_interior(raw in 0u16..=u16::MAX, full_scale in 0u16..=u16::MAX) {
            let verdict: Result<u16, Saturation> = unsaturated(raw, full_scale);
            prop_assert_eq!(verdict.is_ok(), raw > 0 && raw < full_scale);
            if let Ok(value) = verdict {
                prop_assert_eq!(value, raw, "an accepted reading is passed through unchanged");
            }
        }

        /// Each rail names itself: zero is the floor, anything at or past the
        /// ceiling is the ceiling. Fixes which variant a caller can rely on.
        #[test]
        fn each_rail_reports_its_own_variant(full_scale in 1u16..=u16::MAX, overshoot in 0u16..=100) {
            prop_assert_eq!(unsaturated(0, full_scale), Err(Saturation::Floor));
            let past: u16 = full_scale.saturating_add(overshoot);
            prop_assert_eq!(unsaturated(past, full_scale), Err(Saturation::Ceiling));
        }
    }
}
