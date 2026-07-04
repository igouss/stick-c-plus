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
//! - **Entities**: [`Moisture`], [`Calibration`] — the moisture value objects,
//!   with their invariants enforced at construction.
//! - **Entities / policy**: [`to_percent`] — the pure calibration curve mapping
//!   a raw ADC reading to a percentage.

pub mod moisture;

pub use moisture::{to_percent, Calibration, Moisture};
