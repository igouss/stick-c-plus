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
//! - **Entities**: [`Observation`] and [`ProbeFault`] — what a consumer learns when
//!   it asks about the soil. Carries two orthogonal facts a bare `Option` cannot:
//!   whether the *sampler* is alive, and whether the *probe* is honest.
//! - **Control / policy**: [`observe`] — the staleness rule that turns a cached
//!   [`Reading`] into an [`Observation`], so a dead sensor never keeps reporting
//!   its last healthy value and a lying probe is never mistaken for a dead one.
//! - **Ports**: [`SoilSensor`] — the driven interface the firmware's ADC
//!   adapter implements, with [`SoilFault`] classifying its failures into domain
//!   faults. (The display port is the board-generic `platform_core::Screen`, rendering a
//!   `plant_display::Glass` wrapper over the [`Observation`] — so the pixels are no longer
//!   a plant-core concern.)

pub mod freshness;
pub mod moisture;
pub mod observation;
pub mod ports;
pub mod sampler;

pub use freshness::{observe, Outcome, Reading, Tick};
pub use moisture::{to_percent, Calibration, Measurement, Moisture};
pub use observation::{Observation, ProbeFault};
pub use ports::{SoilFault, SoilSensor};
pub use sampler::{step, Sample};
