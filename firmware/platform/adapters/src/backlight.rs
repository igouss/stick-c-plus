//! The AXP192 LDO2 rail as the platform [`Backlight`] port: whether the glass is lit.

use board_support::Axp192;
use embedded_hal::i2c::I2c;
use platform_core::Backlight;

/// The M5StickC Plus TFT backlight, as the [`Backlight`] port.
///
/// The backlight is a PMIC rail (LDO2), not a GPIO or a PWM channel, so switching it is one
/// I2C read-modify-write ([`Axp192::set_backlight`]). It is deliberately a *different* rail
/// from the panel's LDO3, which is what makes the toggle cheap in both directions: the ST7789
/// stays powered and keeps its framebuffer, so the glass darkens and returns with no
/// re-initialisation.
///
/// Generic over the I2C bus, on the same terms as
/// [`Axp192PowerSource`](crate::Axp192PowerSource).
pub struct Axp192Backlight<I2C> {
    axp: Axp192<I2C>,
    lit: bool,
}

impl<I2C: I2c> Axp192Backlight<I2C> {
    /// Wrap a brought-up [`Axp192`] as the backlight, declaring the state it is already in.
    ///
    /// `lit` is the truth at construction, not a command — after `power_on` the rail is up and
    /// the glass is lit, so a composition root passes `true` and no redundant write is issued.
    /// Tracked here rather than read back per query because this adapter is the rail's only
    /// writer, and a status read would put an I2C round trip on every `is_lit`.
    pub const fn new(axp: Axp192<I2C>, lit: bool) -> Self {
        Self { axp, lit }
    }
}

impl<I2C: I2c> Backlight for Axp192Backlight<I2C> {
    type Error = I2C::Error;

    fn is_lit(&self) -> bool {
        self.lit
    }

    /// Set the rail, then remember it — and only on success, so a failed I2C write leaves this
    /// reporting the state the glass is genuinely still in.
    fn set(&mut self, lit: bool) -> Result<(), Self::Error> {
        self.axp.set_backlight(lit)?;
        self.lit = lit;
        Ok(())
    }
}
