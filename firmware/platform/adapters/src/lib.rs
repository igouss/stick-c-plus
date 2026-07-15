#![forbid(unsafe_code)]
//! # platform-adapters
//!
//! The M5StickC Plus board-generic driven adapters — the on-target implementations of the
//! `platform_core` ports, shared by every app:
//!
//! - [`Panel`] / [`PanelScreen`] — the ST7789 TFT. `Panel` is the bring-up (SPI, pins, the
//!   panel's offsets / inversion / RGB colour order); [`PanelScreen`] wraps it as a generic
//!   [`Screen`](platform_core::Screen), with the app injecting *how* to paint its state onto
//!   the panel. So one panel adapter serves the plant `Glass` and the pomodoro view alike.
//! - [`GpioButton`] — a front/side push-button (G37 / G39) as the [`Button`](platform_core::Button)
//!   port: a one-line active-low level read, with the pure debounce living inward.
//! - [`LedcBuzzer`] — the passive buzzer (G2) as the [`Tone`](platform_core::Tone) port, driven
//!   as an LEDC PWM square wave.
//!
//! The composition root builds these from the board's peripherals and injects them; the
//! picture, the gesture policy, and the melodies all live in the pure crates inward.

mod button;
mod buzzer;
mod panel;

pub use button::GpioButton;
pub use buzzer::LedcBuzzer;
pub use panel::{Panel, PanelScreen, PanelTarget, St7789Error};
