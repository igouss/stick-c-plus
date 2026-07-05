//! Ports — the interfaces the domain requires of the outside world.
//!
//! The driven side of the hexagon. The firmware supplies the adapters — an ESP32
//! ADC1 channel reading the Earth Unit on GPIO33 ([`SoilSensor`]), an ST7789 TFT
//! rendering the value ([`MoistureDisplay`]); the domain depends only on these
//! traits, so dependencies point inward.

use crate::moisture::{Measurement, RAW_MAX};

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

/// A driven display that renders the latest soil [`Measurement`].
///
/// The driving side hands the adapter the freshest reading each render cycle; the
/// firmware supplies the adapter (the on-board ST7789 TFT), and the domain depends
/// only on this trait, so dependencies point inward. Rendering only: an adapter
/// translates a [`Measurement`] to pixels and owns no sampling or calibration.
///
/// `reading` is `None` when no fresh reading is available — nothing has been
/// measured yet, or the last one aged out ([`fresh`](crate::fresh)) — and the
/// adapter shows an unavailable placeholder rather than a frozen last value, so a
/// dead probe reads as unavailable on the glass just as it does in Home Assistant.
///
/// `Error` is associated so an adapter can surface its own failure type — an SPI
/// or panel error — without the domain naming any concrete driver error.
pub trait MoistureDisplay {
    /// The adapter's own render-failure type.
    type Error;

    /// Render `reading` — the latest measurement (raw count and percent), or the
    /// unavailable state when `None`.
    fn show(&mut self, reading: Option<Measurement>) -> Result<(), Self::Error>;
}
