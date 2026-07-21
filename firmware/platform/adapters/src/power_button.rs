//! The AXP192 PEK as the platform [`LatchedGesture`] port: the power button's press.

use board_support::Axp192;
use embedded_hal::i2c::I2c;
use log::warn;
use platform_input::{Gesture, LatchedGesture};

/// The M5StickC Plus power button, read through the PMIC rather than a pin.
///
/// The board's third button is not on a GPIO — it is wired to the AXP192's PEK input, and the
/// PMIC debounces it and times its press duration in silicon against the thresholds set at
/// bring-up. So there is no level to poll: this adapter drains a latch
/// ([`Axp192::take_power_button_press`]) that records *that* a short press happened since the
/// last call, which is why it implements [`LatchedGesture`] and bypasses the pure recognizer
/// entirely.
///
/// It only ever yields [`Gesture::Click`]. A long press is the PMIC's own power-off at four
/// seconds; by the time it completes the rails are cut and there is no firmware left to hear
/// about it.
///
/// Generic over the I2C bus, on the same terms as
/// [`Axp192PowerSource`](crate::Axp192PowerSource): the composition root decides whether the
/// bus is owned outright or shared through an `embedded-hal-bus` device.
pub struct PekButton<I2C> {
    axp: Axp192<I2C>,
}

impl<I2C: I2c> PekButton<I2C> {
    /// Wrap a brought-up [`Axp192`] as the power button. No I/O until it is polled.
    pub const fn new(axp: Axp192<I2C>) -> Self {
        Self { axp }
    }
}

impl<I2C: I2c> LatchedGesture for PekButton<I2C> {
    /// Drain the latch, reporting a [`Click`](Gesture::Click) if a short press was waiting.
    ///
    /// A bus failure is logged and read as "no press", never propagated: the port has no error
    /// channel, and a flaky I2C read must not take the input thread down. The cost of the
    /// choice is a dropped press, which the next one recovers from — where surfacing it would
    /// mean a dead timer. Fail-visible, matching the render loop's treatment of a failed paint.
    fn take(&mut self) -> Option<Gesture> {
        match self.axp.take_power_button_press() {
            Ok(true) => Some(Gesture::Click),
            Ok(false) => None,
            Err(_bus) => {
                warn!("power-button: PEK latch read failed, treating as no press");
                None
            }
        }
    }
}
