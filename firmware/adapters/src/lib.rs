#![forbid(unsafe_code)]
//! # adapters
//!
//! The driven side of the firmware hexagon: concrete adapters that implement the
//! host crates' domain ports against real `esp-idf-hal` peripherals —
//!
//!   - `adc`         — `plant_core::SoilSensor` over ADC1 / GPIO33, power-gated (qhw.5) ✅
//!   - `probe_power` — ESP-side [`firmware_core::ProbePower`] implementations
//!   - `clock`       — the shared `Clock` port on ESP-IDF time (qhw.15)
//!   - `ws2812`      — `led_core::LedOutput` over the esp-idf RMT encoder (qqh.1)
//!
//! The board-generic adapters (the ST7789 panel, the buttons, the buzzer) are shared by
//! every app and live in `platform-adapters`; this crate holds only the plant-specific ones.
//! WiFi is *not* here either: station bring-up has no inward domain port, so it lives in
//! `firmware-infra` as infrastructure (qhw.7), not among these driven adapters.
//!
//! The pure, host-testable pieces (the oversampling reduction, the ProbePower
//! port + gating orchestration) live in `firmware-core`; this crate is the thin
//! on-target shell that binds them to real hardware. The remaining adapters
//! arrive in their own beads.

pub mod adc;
pub mod probe_power;
