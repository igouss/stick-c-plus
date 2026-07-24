//! The AXP192 PMIC as the platform [`PowerSource`] port: is USB (VBUS) present, and how
//! full is the battery?

use board_support::Axp192;
use embedded_hal::i2c::I2c;
use platform_core::{charge_percent, PowerSource};

/// The M5StickC Plus AXP192 PMIC as the [`PowerSource`](platform_core::PowerSource) port.
///
/// Wraps a brought-up [`Axp192`] and answers [`on_usb`](PowerSource::on_usb) by reading the
/// PMIC's VBUS-present status bit ([`Axp192::vbus_present`]). The pure debounce and the
/// edge -> chime decision live inward in `platform-core`; this adapter only reports the raw
/// level and owns the bus error type, so the port itself names no hardware.
///
/// Generic over the I2C bus, so the composition root decides ownership: on the M5StickC Plus
/// the internal bus has no other live runtime consumer (the MPU6886/RTC are unused), so the
/// watch thread owns the `Axp192<I2cDriver>` outright — `I2cDriver` is `Send`. If the bus is
/// later shared at runtime, the root hands it an `embedded-hal-bus` `MutexDevice` instead;
/// either way the runtime sees only `impl PowerSource + Send`.
pub struct Axp192PowerSource<I2C> {
    axp: Axp192<I2C>,
}

impl<I2C: I2c> Axp192PowerSource<I2C> {
    /// Wrap a brought-up [`Axp192`] as a [`PowerSource`]. No I/O until
    /// [`on_usb`](PowerSource::on_usb) is polled.
    pub const fn new(axp: Axp192<I2C>) -> Self {
        Self { axp }
    }
}

impl<I2C: I2c> PowerSource for Axp192PowerSource<I2C> {
    type Error = I2C::Error;

    fn on_usb(&mut self) -> Result<bool, Self::Error> {
        self.axp.vbus_present()
    }

    /// The charge, from the PMIC's battery-voltage ADC through the pure curve.
    ///
    /// `Some`, because this board *does* have a gauge — the register read is the whole of the
    /// measurement and `charge_percent` is the whole of the judgement, so the two halves stay on
    /// their own sides of the port.
    fn battery_pct(&mut self) -> Result<Option<u8>, Self::Error> {
        Ok(Some(charge_percent(self.axp.battery_millivolts()?)))
    }
}
