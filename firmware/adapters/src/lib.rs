#![forbid(unsafe_code)]
//! # adapters
//!
//! The driven side of the firmware hexagon: concrete adapters that implement the
//! host crates' domain ports against real `esp-idf-hal` peripherals —
//!
//!   - `adc`    — `plant_core::SoilSensor` over ADC1 / GPIO33 (qhw.5) ✅
//!   - `clock`  — the shared `Clock` port on ESP-IDF time (qhw.15)
//!   - `ws2812` — `led_core::LedOutput` over the esp-idf RMT encoder (qqh.1)
//!   - `st7789` — the ST7789 display, over `mipidsi`
//!   - `wifi`   — the WiFi client adapter
//!
//! The remaining adapters arrive in their own beads; each fills in a module
//! here, pulling its peripheral and host port crate then.

/// Hardware oversampling shared by ADC-backed adapters. Pure and target-free so
/// its reduction is host-tested; see [`adc`] for why it lives at the adapter.
mod oversample;

// The ADC adapter binds real `esp-idf-hal` peripherals, so it exists only on the
// ESP-IDF target. Gating it here keeps [`oversample`] host-testable: an
// off-target `cargo test` compiles the reduction and its unit tests without
// dragging in the on-target HAL.
#[cfg(target_os = "espidf")]
pub mod adc;
