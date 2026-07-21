//! The backlight port: whether the glass is lit.

/// The display's backlight, switchable independently of the panel.
///
/// On this board the two are separate PMIC rails — LDO2 lights the backlight, LDO3 powers the
/// ST7789 itself — and that separation is the whole point of this port. Cutting *only* the
/// backlight goes dark while the panel keeps its state and its framebuffer, so coming back is
/// instant and needs no re-initialisation. Cutting both would save marginally more and cost a
/// full panel bring-up on every wake.
///
/// The port says nothing about a PMIC, a rail or a register: an app decides *when* the glass
/// should be dark, and the adapter knows how to make it so.
pub trait Backlight {
    /// What can go wrong reaching the backlight — an I2C failure, on this board.
    type Error;

    /// Light the glass, or darken it.
    ///
    /// Idempotent: setting the state it already holds is a no-op to the caller, so a shell may
    /// call this without tracking what it last asked for.
    fn set(&mut self, lit: bool) -> Result<(), Self::Error>;
}
