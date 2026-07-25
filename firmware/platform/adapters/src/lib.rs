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
//! - [`GpioButton`] — a front/side push-button (G37 / G39) as the
//!   [`ButtonLevel`](platform_input::ButtonLevel) port: a one-line active-low level read, with
//!   the pure debounce living inward.
//! - [`PekButton`] — the power button, which is not on a GPIO at all. It hangs off the AXP192's
//!   PEK input, and the PMIC debounces it and times its press in silicon, so this drains a
//!   latch as the [`LatchedGesture`](platform_input::LatchedGesture) port rather than reading a
//!   level. It yields only a click: a long press is the PMIC's own power-off, and by the time
//!   it completes there is no firmware left to hear about it.
//! - [`Axp192Backlight`] — the TFT backlight (PMIC rail LDO2) as the
//!   [`Backlight`](platform_core::Backlight) port. A different rail from the panel's LDO3, so
//!   darkening the glass leaves the ST7789 holding its framebuffer and coming back is instant.
//! - [`LedcBuzzer`] — the passive buzzer (G2) as the [`Tone`](platform_core::Tone) port, driven
//!   as an LEDC PWM square wave.
//! - [`PdmMic`] — the SPM1423 PDM microphone (G0 / G34) as the [`AudioIn`](platform_core::AudioIn)
//!   port, decimated to PCM by the I2S peripheral. Used by the chime self-test to hear the buzzer.
//! - [`Mpu6886Imu`] — the MPU6886 IMU (internal I2C bus) as the [`Imu`](platform_core::Imu)
//!   port: one burst read of the three accelerometer axes, in milli-g. What the vector
//!   *means* — the tilt angles, the resting face — is decided inward by an app's domain.
//! - [`Axp192PowerSource`] — the AXP192 PMIC (internal I2C bus) as the
//!   [`PowerSource`](platform_core::PowerSource) port: is USB (VBUS) present? Reads one status
//!   bit; the debounce and the plug/unplug chime decision live inward in `platform-core`.
//!
//! The composition root builds these from the board's peripherals and injects them; the
//! picture, the gesture policy, and the melodies all live in the pure crates inward.

mod backlight;
mod button;
mod buzzer;
mod imu;
mod panel;
mod pdm_mic;
mod power_button;
mod power_source;

pub use backlight::Axp192Backlight;
pub use button::GpioButton;
pub use buzzer::LedcBuzzer;
pub use imu::Mpu6886Imu;
pub use panel::{
    FastPanel, Fixed, Panel, PanelScreen, PanelTarget, RotationPolicy, St7789Error, Turning,
};
pub use pdm_mic::PdmMic;
pub use power_button::PekButton;
pub use power_source::Axp192PowerSource;
