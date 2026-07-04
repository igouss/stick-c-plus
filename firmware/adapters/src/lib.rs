//! # adapters
//!
//! The driven side of the firmware hexagon: concrete adapters that implement the
//! host crates' domain ports against real `esp-idf-hal` peripherals —
//!
//!   - `adc`    — `plant_core::SoilSensor` over ADC1 (qhw.5)
//!   - `clock`  — the shared `Clock` port on ESP-IDF time (qhw.15)
//!   - `ws2812` — `led_core::LedOutput` over the esp-idf RMT encoder (qqh.1)
//!   - `st7789` — the ST7789 display, over `mipidsi`
//!   - `wifi`   — the WiFi client adapter
//!
//! Skeleton only (qhw.2 workspace carve): each adapter arrives in its own bead.
//! This crate exists now so the workspace seam and its hex-arch `driven-adapter`
//! role are in place for them to fill.
