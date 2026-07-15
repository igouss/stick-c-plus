//! Ports — the interfaces the domain requires of the outside world.
//!
//! The driven side of the hexagon. The firmware supplies the adapters — an ESP32
//! ADC1 channel reading the Earth Unit on GPIO33 ([`SoilSensor`]), an ST7789 TFT
//! rendering the value ([`MoistureDisplay`]); the domain depends only on these
//! traits, so dependencies point inward.

use crate::moisture::RAW_MAX;
use crate::observation::ProbeFault;

/// Classifies an adapter's own read failure into a domain [`ProbeFault`].
///
/// The domain must be able to *publish* why a reading was refused — a fresh fault
/// is what distinguishes a lying probe from a dead sampler — but it must not learn
/// what an `EspError` is. So the port asks the adapter, which is the only party
/// that knows what its own failures mean, to say which domain fault it represents.
///
/// The adapter keeps its rich error for the log line; the domain gets the verdict.
pub trait SoilFault {
    /// Which domain fault this failure represents.
    fn fault(&self) -> ProbeFault;
}

/// A raw soil-moisture source: one call, one 12-bit ADC reading.
///
/// Implementations return a raw count in `0..=`[`RAW_MAX`] (the ESP32 ADC is
/// 12-bit). The [`sampler`](crate::sampler) step calibrates these raw counts;
/// the port itself carries no calibration and does no averaging, so an adapter
/// stays a thin translation from hardware to a number.
///
/// `Error` is associated so an adapter can surface its own failure type — an ADC
/// calibration or timeout error — without the domain naming any concrete driver
/// error. It is bound by [`SoilFault`] because the domain requires every failure to
/// be classifiable: a reading that cannot be trusted must still be *reportable*,
/// or it degrades into silence and a consumer cannot tell it from a dead device.
///
/// Note that a reading the adapter judges meaningless — an ADC pinned to a rail —
/// is an `Err`, not an `Ok` of an extreme value. The adapter, which knows its
/// converter's range, is the only layer that can make that call.
pub trait SoilSensor {
    /// The adapter's own read-failure type, classifiable into a [`ProbeFault`].
    type Error: SoilFault;

    /// Take one raw reading, in `0..=`[`RAW_MAX`].
    ///
    /// The range is the adapter's contract; the sampler clamps through
    /// [`to_percent`](crate::moisture::to_percent) regardless, so an
    /// out-of-spec reading can still never push moisture out of `0..=100`.
    fn read_raw(&mut self) -> Result<u16, Self::Error>;
}

/// The inclusive upper bound an honest [`SoilSensor`] adapter reads up to.
///
/// Re-exported from [`crate::moisture`] so an adapter can reference the ADC
/// ceiling from the port module it implements against.
pub const MAX_READING: u16 = RAW_MAX;

// The display port is no longer here. Rendering an Observation is one instance of the
// board-generic `platform_core::Screen<S>` port (implemented for `plant_display::Glass`,
// which wraps an Observation) — so the plant monitor and the pomodoro timer share one
// display port and one render loop. The freshness policy that produces the Observation
// still lives here, in [`observe`](crate::observe); only the pixels moved out.
