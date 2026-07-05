#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # plant-core
//!
//! Framework-free soil-moisture core for the M5StickC Plus + M5 Earth Unit
//! plant monitor.
//!
//! Pure `no_std`, zero dependencies, no allocation, no floating point: just the
//! moisture value types and the [ports](ports) the firmware wires to real
//! hardware. Everything here is deterministic and host-testable — the Xtensa
//! side only supplies an ADC reading.
//!
//! ## Hexagon
//! - **Entities**: [`Moisture`], [`Calibration`], [`Measurement`] — the moisture
//!   value objects, with their invariants enforced at construction. A
//!   [`Measurement`] pairs a calibrated [`Moisture`] with the raw ADC count it came
//!   from, so a reading's provenance survives to the display and future calibration.
//! - **Entities / policy**: [`to_percent`] — the pure calibration curve mapping
//!   a raw ADC reading to a percentage.
//! - **Control**: [`step`] — the sampling use case (average → calibrate →
//!   report-on-change), a pure function of its inputs.
//! - **Control / policy**: [`fresh`] — the staleness rule that turns a cached
//!   [`Reading`] unavailable once it ages out, so a dead sensor never keeps
//!   reporting its last healthy value.
//! - **Ports**: [`SoilSensor`] — the driven interface the firmware's ADC
//!   adapter implements; [`MoistureDisplay`] — the driven interface the ST7789
//!   TFT adapter implements to render the value.

pub mod freshness;
pub mod moisture;
pub mod ports;
pub mod sampler;

pub use freshness::{fresh, Reading, Tick};
pub use moisture::{to_percent, Calibration, Measurement, Moisture};
pub use ports::{MoistureDisplay, SoilSensor};
pub use sampler::{step, Sample};
