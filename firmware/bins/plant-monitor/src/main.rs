//! plant-monitor — the composition root (bin #1) on std/ESP-IDF.
//!
//! qhw.2 workspace carve: the qhw.1 boot skeleton, moved verbatim into
//! `bins/plant-monitor` and now linking the host `plant-core` domain across the
//! workspace boundary — the cross-workspace path-dependency proof this bead
//! exists to establish. The real composition (WiFi → native-API server host →
//! ADC sampler → Sensor entity) lands in qhw.5 / .7 / .8 / .9 / .27; until then
//! this stays a boot-and-heartbeat probe.
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

    info!("plant-monitor: std/ESP-IDF boot skeleton up (qhw.2 workspace carve)");
    // Proof the host domain cross-compiled into this Xtensa binary: reference a
    // real plant-core constant. A broken cross-workspace path dep fails this
    // link rather than passing silently.
    info!(
        "plant-core domain linked (ADC full-scale = {})",
        plant_core::moisture::RAW_MAX
    );

    let mut uptime_s: u64 = 0;
    loop {
        FreeRtos::delay_ms(1000);
        uptime_s += 1;
        info!("alive: {uptime_s}s");
    }
}
