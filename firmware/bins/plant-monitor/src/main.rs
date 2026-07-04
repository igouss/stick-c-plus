#![forbid(unsafe_code)]
//! plant-monitor — the composition root (bin #1) on std/ESP-IDF.
//!
//! Wires the driven [`adapters::adc::EarthUnit`] into the host [`plant_shell`]
//! sampler thread: the thread reads the ADC every sample period, folds each
//! reading through the pure [`plant_core::sampler::step`], and publishes the
//! latest [`plant_core::Moisture`] into a [`SharedMoisture`] cache. This bin then
//! plays a consumer — logging the cached value once a second — until the display
//! (qhw.6) and the native-API server (qhw.9/.27) read that same slot.
//!
//! The plumbing (a live cache tracking the probe as it wets and dries, going
//! *unavailable* when the sensor stops reporting) is what qhw.21 proves. The
//! moisture *percent* is not yet trustworthy: it rides a provisional calibration
//! ([`PROVISIONAL_CAL`]) until qhw.29 captures the probe's real dry/wet endpoints.

use adapters::adc::{EarthUnit, SAMPLES};
use adapters::probe_power::AlwaysOn;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{info, warn};
use plant_core::moisture::Calibration;
use plant_core::ports::MAX_READING;
use plant_core::Tick;
use plant_shell::{spawn_sampler, Monotonic, SamplerConfig, SharedMoisture};

/// Provisional soil calibration — placeholder dry/wet endpoints, *not* measured.
///
/// The real endpoints are captured and persisted to NVS in qhw.29; until then the
/// sampler needs *some* curve to turn raw counts into a percent. These are
/// deliberate guesses (a resistive probe often reads higher in dry soil, lower
/// when wet), so the reported percent may be off or even inverted — but
/// [`plant_core::moisture::to_percent`] is direction-agnostic and monotone, so the
/// cache still *tracks* the probe wetting and drying, which is all qhw.21 asserts.
const PROVISIONAL_CAL: Calibration =
    Calibration::new(/* dry_raw */ 2600, /* wet_raw */ 1200);

fn main() {
    // Patch a few ESP-IDF symbols Rust's std expects, then route `log` records to
    // the ESP-IDF logger so `info!`/`warn!` reach the serial monitor.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("plant-monitor: std/ESP-IDF up — sampler thread -> shared Moisture (qhw.21)");

    // A boot-time bring-up failure is unrecoverable, so panic with context rather
    // than limp on: the composition root owns the one place peripherals are taken
    // and the adapter is built.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");
    // AlwaysOn: the probe stays powered for now. qhw.31 swaps this for a real
    // ProbePower (AXP192 rail / GPIO switch) — the only wiring change — and the
    // adapter energizes the electrodes only across each read.
    //
    // 'static: the sampler thread owns the sensor, so its peripherals must outlive
    // the thread. `Peripherals::take()` yields the board's singletons, so the ADC1
    // and GPIO33 handles built from them are 'static.
    let earth: EarthUnit<'static, AlwaysOn> =
        EarthUnit::new(peripherals.adc1, peripherals.pins.gpio33, AlwaysOn)
            .expect("Earth Unit ADC1/GPIO33 bring-up");

    info!(
        "earth unit bound: ADC1 ch5 / GPIO33, 12 dB, {SAMPLES}x oversampled, raw 0..={MAX_READING}"
    );

    // One monotonic clock, shared by the sampler (writer) and this loop (reader),
    // so a reading's age is measured on a single time base.
    let clock: Monotonic = Monotonic::start();
    let shared: SharedMoisture = SharedMoisture::new();
    let config: SamplerConfig = SamplerConfig::new(PROVISIONAL_CAL);
    let max_age: Tick = config.max_age();
    let period: core::time::Duration = config.period;

    // Hand the sensor to the sampler thread; it owns the timing and the cache.
    // Held in `_sampler` for the life of `main` — dropping it would only detach
    // the thread, which already samples forever.
    let _sampler =
        spawn_sampler(earth, shared.clone(), clock, config).expect("spawn plant-sampler thread");
    info!("sampler thread up: {period:?} period, unavailable after {max_age} ms stale");

    // Stand-in consumer: report the cached moisture, or that it has gone stale.
    loop {
        FreeRtos::delay_ms(1000);
        match shared.latest(clock.now(), max_age) {
            Some(moisture) => info!("moisture = {}%", moisture.percent()),
            None => warn!("moisture unavailable — no fresh sample within {max_age} ms"),
        }
    }
}
