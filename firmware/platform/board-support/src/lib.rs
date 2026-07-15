#![forbid(unsafe_code)]
//! # board-support
//!
//! M5StickC Plus board-support package (BSP): AXP192 PMIC power-on, the internal
//! I2C bus, and peripheral bring-up shared by all three projects (plant-monitor,
//! led-driver, rover).
//!
//! - [`internal_i2c`] — bring up the internal I2C bus (SDA G21, SCL G22, 400 kHz),
//!   the bus the AXP192 PMIC, the MPU6886 IMU and the BM8563 RTC share.
//! - [`axp192`] / [`Axp192`] — the thin AXP192 driver that powers the LCD/TFT rails
//!   (qhw.20). Generic over `embedded-hal` I2c, so the composition root can share
//!   the one bus (via `embedded-hal-bus`) across every device that sits on it.
//!
//! The rails must be powered *before* the display: an unpowered LDO2/LDO3 leaves
//! the TFT dark regardless of a correct ST7789 init. The composition root brings
//! the bus up, powers the AXP192, then builds the display.

pub mod axp192;

pub use axp192::Axp192;

use esp_idf_hal::gpio::{Gpio21, Gpio22};
use esp_idf_hal::i2c::{I2cConfig, I2cDriver, I2C0};
use esp_idf_hal::units::FromValueType;
use esp_idf_sys::EspError;

/// The internal-bus I2C clock — 400 kHz, matching the M5 factory driver
/// (`Wire1.setClock(400000)`).
const INTERNAL_I2C_HZ: u32 = 400_000;

/// Bring up the M5StickC Plus internal I2C bus on I2C0 (SDA G21, SCL G22).
///
/// The internal bus is distinct from the Grove port: the AXP192 PMIC (0x34), the
/// MPU6886 IMU (0x68) and the BM8563 RTC (0x51) all live on it. The pins are fixed
/// by the board wiring, so they are taken by concrete type (like the ADC adapter's
/// GPIO33) rather than generically. Returns the driver at 400 kHz; the composition
/// root wraps it (`embedded-hal-bus`) so each device can hold its own bus handle
/// over the one controller.
pub fn internal_i2c<'d>(
    i2c: I2C0<'d>,
    sda: Gpio21<'d>,
    scl: Gpio22<'d>,
) -> Result<I2cDriver<'d>, EspError> {
    let config: I2cConfig = I2cConfig::new().baudrate(INTERNAL_I2C_HZ.Hz());
    I2cDriver::new(i2c, sda, scl, &config)
}
