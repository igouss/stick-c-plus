#![forbid(unsafe_code)]
//! plant-monitor — the composition root (bin #1) on std/ESP-IDF.
//!
//! Wires the driven [`adapters::adc::EarthUnit`] to the host
//! [`plant_core::SoilSensor`] port and logs one raw soil-moisture count per
//! second. That serial stream is the qhw.5 acceptance harness: with the probe in
//! dry air, dry soil, then wet soil, the counts must move monotonically (one
//! direction, a clear gap between states) and sit still between reads at a
//! constant moisture — the oversampled adapter suppressing ADC jitter.
//!
//! The reading is deliberately *raw*: calibrating it to a percentage needs the
//! dry/wet endpoints captured in qhw.29, and reporting it to Home Assistant is
//! the sampler thread (qhw.21) feeding the native-API Sensor entity (qhw.9). Until
//! then this stays a probe — but a probe against real ADC1 hardware, not a stub.

use adapters::adc::EarthUnit;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{error, info};
use plant_core::ports::MAX_READING;
use plant_core::SoilSensor;

fn main() {
    // Patch a few ESP-IDF symbols Rust's std expects, then route `log` records
    // to the ESP-IDF logger so `info!` reaches the serial monitor.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("plant-monitor: std/ESP-IDF up — ADC SoilSensor probe (qhw.5)");

    // A boot-time bring-up failure is unrecoverable, so panic with context
    // rather than limp on: the composition root owns the one place peripherals
    // are taken and the adapter is built.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");
    let mut earth: EarthUnit = EarthUnit::new(peripherals.adc1, peripherals.pins.gpio33)
        .expect("Earth Unit ADC1/GPIO33 bring-up");

    info!(
        "earth unit bound: ADC1 ch5 / GPIO33, 12 dB, {}x oversampled, raw 0..={MAX_READING}",
        EarthUnit::SAMPLES
    );

    let mut uptime_s: u64 = 0;
    loop {
        FreeRtos::delay_ms(1000);
        uptime_s += 1;
        match earth.read_raw() {
            Ok(raw) => info!("[{uptime_s:>4}s] earth raw = {raw}"),
            Err(err) => error!("[{uptime_s:>4}s] earth read failed: {err}"),
        }
    }
}
