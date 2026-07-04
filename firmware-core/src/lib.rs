#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # firmware-core
//!
//! Pure, framework-free primitives shared by the firmware's driven adapters — no
//! HAL, no effects — so every rule here is exercised on the host, away from the
//! on-target crates that can only be cross-compiled.
//!
//! - [`oversample`] — fold N raw ADC reads into one denoised count.
//! - [`probe_power`] — the [`ProbePower`] port and [`gated_read`], which powers a
//!   probe only across a read so a 24/7 monitor cannot electrolyze its
//!   electrodes.
//!
//! The concrete adapters (an `esp-idf-hal` ADC channel, an AXP192 power rail)
//! live in the firmware workspace and implement these seams; this crate names no
//! hardware.

pub mod oversample;
pub mod probe_power;

pub use oversample::oversampled_mean;
pub use probe_power::{gated_read, ProbePower};
