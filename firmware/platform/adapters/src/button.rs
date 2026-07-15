//! The front/side push-buttons (G37 / G39) as the platform [`Button`] port.

use esp_idf_hal::gpio::{Input, InputPin, PinDriver, Pull};
use esp_idf_sys::EspError;
use platform_core::Button;

/// A momentary push-button on an input GPIO, read active-low.
///
/// The M5StickC Plus front (G37) and side (G39) buttons are input-only pins (GPIO34–39 have no
/// internal pull resistors) with external pull-ups on the board, so a press reads LOW. All
/// timing and bounce rejection live in the pure `platform_core::Debounce`; this adapter is a
/// one-line level read, so nothing about a gesture is decided on the metal.
pub struct GpioButton<'d> {
    pin: PinDriver<'d, Input>,
}

impl<'d> GpioButton<'d> {
    /// Bind `pin` (G37 or G39) as an input. The pull is left floating: these pins have no
    /// internal pull to set, and the board provides the external pull-up.
    pub fn new<T: InputPin + 'd>(pin: T) -> Result<Self, EspError> {
        Ok(Self {
            pin: PinDriver::input(pin, Pull::Floating)?,
        })
    }
}

impl Button for GpioButton<'_> {
    /// `true` while the button is pressed — the active-low pin reads LOW.
    fn poll(&mut self) -> bool {
        self.pin.is_low()
    }
}
