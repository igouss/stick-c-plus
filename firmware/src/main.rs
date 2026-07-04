//! M5StickC Plus firmware — std/ESP-IDF boot skeleton (qhw.1).
//!
//! Scope is deliberately minimal: bring up the std/ESP-IDF runtime, log over
//! serial, and stay alive so the board can be proven to boot without looping.
//! Everything else — the WS2812/Clock adapters (re-homed under qqh.1 / qhw.15),
//! the workspace carve (qhw.2), WiFi/native-API/plant-monitor — lands later.
//!
//! The heartbeat below is the boot-loop probe: a device that reset would restart
//! the `alive: 1s` count, so a monotonically climbing counter on the serial
//! monitor is the acceptance signal (runs 60s+ past `main` without a reset).

use esp_idf_hal::delay::FreeRtos;
use esp_idf_svc::log::EspLogger;
use log::info;

fn main() {
    // Patch a few ESP-IDF symbols Rust's std expects, then route `log` records
    // to the ESP-IDF logger so `info!` reaches the serial monitor.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("stick-firmware: std/ESP-IDF boot skeleton up (qhw.1)");

    let mut uptime_s: u64 = 0;
    loop {
        FreeRtos::delay_ms(1000);
        uptime_s += 1;
        info!("alive: {uptime_s}s");
    }
}
