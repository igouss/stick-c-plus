//! Ports — the interfaces the domain requires of the outside world.
//!
//! The driven side of the hexagon. The firmware supplies the adapters — an ESP32
//! ADC1 channel reading the Earth Unit on GPIO33 ([`SoilSensor`]), an ST7789 TFT
//! rendering the value ([`MoistureDisplay`]); the domain depends only on these
//! traits, so dependencies point inward.

use crate::freshness::Tick;
use crate::moisture::RAW_MAX;
use crate::observation::{Observation, ProbeFault};

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

/// A driven display that renders the latest [`Observation`] of the soil.
///
/// The driving side hands the adapter the freshest observation each render cycle;
/// the firmware supplies the adapter (the on-board ST7789 TFT), and the domain
/// depends only on this trait, so dependencies point inward. Rendering only: an
/// adapter translates an observation to pixels and owns no sampling or calibration.
///
/// It takes an [`Observation`] rather than an `Option<Measurement>` so the glass
/// can say *why* there is no number — a corroded probe, a rail that never rose, a
/// sampler that stopped — instead of showing one indiscriminate placeholder. The
/// freshness decision was already made upstream by [`observe`](crate::observe); the
/// adapter only chooses words and pixels for the verdict it is handed.
///
/// `Error` is associated so an adapter can surface its own failure type — an SPI
/// or panel error — without the domain naming any concrete driver error.
pub trait MoistureDisplay {
    /// The adapter's own render-failure type.
    type Error;

    /// Render `observation` — the latest measurement, the fault that replaced it,
    /// or the reason there is neither.
    ///
    /// `elapsed` is how long this observation has been the current one, in [`Tick`]
    /// milliseconds. The domain has no opinion on what an adapter does with it; a glass
    /// that only prints a number ignores it, and one that wants to draw attention to a
    /// fault has the clock it needs to do so without inventing its own. It is *not* the
    /// reading's age — [`observe`](crate::observe) has already ruled on freshness, and a
    /// verdict of `Stale` grows older the longer the sampler stays dead.
    fn show(&mut self, observation: Observation, elapsed: Tick) -> Result<(), Self::Error>;
}
