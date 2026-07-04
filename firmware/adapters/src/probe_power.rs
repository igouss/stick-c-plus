//! ESP-side [`ProbePower`] implementations for the driven adapters.
//!
//! The port and the gating orchestration ([`firmware_core::gated_read`]) are pure
//! and host-tested in `firmware-core`; this module holds the concrete power
//! mechanisms that touch hardware. Only the no-gating default lives here today —
//! the real high-side switch (an AXP192 Grove rail or a GPIO) lands with qhw.31,
//! likely in `board-support`.

use esp_idf_sys::EspError;
use firmware_core::ProbePower;

/// A probe that is always powered — the no-gating default until qhw.31 lands a
/// real switch.
///
/// `energize`/`deenergize` are no-ops, so [`crate::adc::EarthUnit`] keeps its
/// current always-on behavior while *already* routing every read through the
/// gating seam. Swapping this for a real [`ProbePower`] in the composition root
/// is then the whole of the wiring change — the adapter does not move.
///
/// `Error = EspError` (never produced here) so it unifies with the ADC read
/// error inside `gated_read`, matching the real mechanisms to come.
pub struct AlwaysOn;

impl ProbePower for AlwaysOn {
    type Error = EspError;

    fn energize(&mut self) -> Result<(), EspError> {
        Ok(())
    }

    fn deenergize(&mut self) -> Result<(), EspError> {
        Ok(())
    }
}
