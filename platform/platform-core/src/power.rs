//! The power-source port: whether the board is drawing from USB or running on battery, and how
//! full its cell is.

/// The cell voltage, in millivolts, at which this board's battery is called empty.
///
/// Not the chemistry's floor — a single-cell Li-ion is cut off nearer 3.0 V — but the point at
/// which the board is minutes from losing its rails, which is what a charge reading is *for*.
pub const BATTERY_EMPTY_MV: u16 = 3_300;

/// The cell voltage, in millivolts, at which it is called full: the AXP192's own charge target,
/// written by its bring-up (`0x33 = 0xC0`, 4.2 V).
pub const BATTERY_FULL_MV: u16 = 4_200;

/// Charge in percent, from a cell voltage in millivolts.
///
/// A straight line between [`BATTERY_EMPTY_MV`] and [`BATTERY_FULL_MV`], clamped at both ends.
/// A Li-ion discharge curve is not a line, so this is an *estimate* — but it is monotonic in the
/// voltage, which is the only property anything here depends on: "is the battery low" must not
/// flap while the cell is steadily draining.
///
/// A reading taken while charging is high by the charge current's IR drop, which is why a screen
/// policy that acts on a low battery reads it off the charger.
pub const fn charge_percent(millivolts: u16) -> u8 {
    if millivolts <= BATTERY_EMPTY_MV {
        return 0;
    }
    if millivolts >= BATTERY_FULL_MV {
        return 100;
    }
    let above: u32 = (millivolts - BATTERY_EMPTY_MV) as u32;
    let span: u32 = (BATTERY_FULL_MV - BATTERY_EMPTY_MV) as u32;
    (above * 100 / span) as u8
}

/// Whether the board is currently drawing power from USB, and how full its battery is.
///
/// The driven port for the PMIC's power questions — a thin read of one status bit and, where the
/// board has a gauge, of an ADC. Sibling to [`Button`](crate::Button) and [`Tone`](crate::Tone).
/// The adapter owns its own failure type; this port names no hardware, no register, no bus.
pub trait PowerSource {
    /// The adapter's own read-failure type.
    type Error;

    /// `true` while USB (VBUS) is present; `false` while running on battery.
    fn on_usb(&mut self) -> Result<bool, Self::Error>;

    /// The battery charge in percent, or `None` on a board with no gauge.
    ///
    /// Defaulted to `None` because *not measuring* is the honest answer for an adapter that
    /// cannot, and a fabricated percentage is worse than a blank: it is a reading the owner
    /// would act on. An adapter that can measure overrides this; every other one says nothing,
    /// and the glass says so too.
    fn battery_pct(&mut self) -> Result<Option<u8>, Self::Error> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ends, and outside them: a flat battery is 0 and a full one is 100, and neither
    /// wraps when the ADC reports past the range this board charges to.
    #[test]
    fn the_ends_are_flat_and_clamped() {
        assert_eq!(charge_percent(0), 0);
        assert_eq!(charge_percent(BATTERY_EMPTY_MV), 0);
        assert_eq!(charge_percent(BATTERY_FULL_MV), 100);
        assert_eq!(charge_percent(u16::MAX), 100);
    }

    /// The middle is the middle — the one thing the estimate actually claims.
    #[test]
    fn the_midpoint_reads_about_half() {
        let middle: u16 = (BATTERY_EMPTY_MV + BATTERY_FULL_MV) / 2;
        assert_eq!(charge_percent(middle), 50);
    }

    /// Monotonic: a draining cell never reads fuller than it did. This is what keeps a "the
    /// battery is low" decision from flapping as the voltage sags and recovers by a millivolt.
    #[test]
    fn the_reading_never_rises_as_the_voltage_falls() {
        let rising: usize = (3_000..4_400u16)
            .step_by(7)
            .zip((3_007..4_407u16).step_by(7))
            .filter(|(low, high): &(u16, u16)| charge_percent(*low) > charge_percent(*high))
            .count();
        assert_eq!(rising, 0);
    }

    /// A board with no gauge says nothing rather than guessing — the default, stated as a test
    /// so a later edit that invents a number has to argue with it.
    #[test]
    fn a_source_with_no_gauge_reports_no_charge() {
        struct Ungauged;
        impl PowerSource for Ungauged {
            type Error = ();
            fn on_usb(&mut self) -> Result<bool, ()> {
                Ok(true)
            }
        }
        assert_eq!(Ungauged.battery_pct(), Ok(None));
    }
}
