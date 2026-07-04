//! Ports — the interfaces the domain requires of the outside world.
//!
//! The driven side of the hexagon. The firmware supplies the adapter (an ESP32
//! ADC1 channel reading the Earth Unit on GPIO33); the domain depends only on
//! this trait, so dependencies point inward.

use crate::moisture::RAW_MAX;

/// A raw soil-moisture source: one call, one 12-bit ADC reading.
///
/// Implementations return a raw count in `0..=`[`RAW_MAX`] (the ESP32 ADC is
/// 12-bit). The [`sampler`](crate::sampler) step calibrates these raw counts;
/// the port itself carries no calibration and does no averaging, so an adapter
/// stays a thin translation from hardware to a number.
///
/// `Error` is associated so an adapter can surface its own failure type — an
/// ADC calibration or timeout error — without the domain naming any concrete
/// driver error.
pub trait SoilSensor {
    /// The adapter's own read-failure type.
    type Error;

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
