#![forbid(unsafe_code)]
//! plant-monitor — the composition root (bin #1) on std/ESP-IDF.
//!
//! Brings the board onto the network via [`firmware_infra::wifi`] (qhw.7) and
//! advertises it for Home Assistant discovery via [`firmware_infra::mdns`]
//! (qhw.8), then wires the driven [`adapters::adc::EarthUnit`] into the host
//! [`plant_shell`]
//! sampler thread: the thread reads the ADC every sample period, folds each
//! reading through the pure [`plant_core::sampler::step`], and publishes the
//! latest [`plant_core::Moisture`] into a [`SharedMoisture`] cache. It then stands
//! up the reusable native-API server host ([`esphome_server`], qhw.27) on :6053,
//! serving a single Soil Moisture [`SensorDevice`] whose value is pulled from that
//! same cache (qhw.9) — so Home Assistant adopts the device and graphs the probe as
//! it wets and dries. The supervisory loop keeps the WiFi link up and logs a
//! heartbeat; the server thread is the real consumer of the cache.
//!
//! The `plant`↔`api` bounded contexts stay isolated in the host workspace and are
//! host-tested apart (the sampler + freshness in `plant-shell`, the `SensorDevice`
//! against the real HA client in the `esphome-server` oracle); this composition
//! root is the one place they are wired together — the source closure that reads
//! [`SharedMoisture::observe`] and hands the value to the api core.
//!
//! The plumbing (a live cache tracking the probe as it wets and dries, going
//! *unavailable* when the sensor stops reporting) is what qhw.21 proves. The
//! moisture *percent* is not yet trustworthy: it rides a provisional calibration
//! ([`PROVISIONAL_CAL`]) until qhw.29 captures the probe's real dry/wet endpoints.

use std::cell::RefCell;
use std::sync::Arc;
use std::thread;

use adapters::adc::{EarthUnit, SAMPLES};
use adapters::probe_power::AlwaysOn;
use adapters::st7789::St7789Display;
use board_support::{internal_i2c, Axp192};
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esphome_api::proto::{DeviceInfoResponse, SensorStateClass};
use esphome_api::{SensorConfig, SensorDevice};
use esphome_server::{Server, ServerConfig};
use firmware_infra::mdns::{self, EsphomeService};
use firmware_infra::wifi::{self, WifiStation};
use log::{error, info, warn};
use plant_core::moisture::Calibration;
use plant_core::ports::MAX_READING;
use plant_core::{Observation, Tick};
use plant_shell::{
    spawn_display, spawn_sampler, DisplayConfig, Monotonic, SamplerConfig, SharedMoisture,
};

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

/// A stable public host resolved once at boot to prove the DNS path works, not
/// just that we hold a lease (qhw.7). Resolution failure is logged, not fatal:
/// the monitor still samples soil without a working resolver.
const DNS_CHECK_HOST: &str = "google.com";

/// mDNS hostname (sans `.local`) — the device answers `plant-monitor.local` (qhw.8).
const HOSTNAME: &str = "plant-monitor";
/// The device's display name in Home Assistant and mDNS browsers (qhw.8).
const FRIENDLY_NAME: &str = "Plant Monitor";
/// ESPHome `platform` TXT — the chip family.
const PLATFORM: &str = "ESP32";
/// ESPHome `board` TXT — informational device identity (M5StickC Plus).
const BOARD: &str = "m5stick-c";
/// Native-API device node name — HA's `device.name`, a stable slug (distinct from
/// [`FRIENDLY_NAME`], the display name). Matches the identity the aioesphomeapi
/// oracle asserts, so the on-device oracle flow (qhw.9) reuses it unchanged.
const DEVICE_NAME: &str = "plantmon";
/// Native-API device model — the hardware, shown in HA's device page.
const MODEL: &str = "M5StickC Plus";

/// The native-API accept-loop thread's stack, in bytes.
///
/// The loop only accepts a connection and spawns a sized per-connection thread
/// (each [`esphome_server::PLAINTEXT_STACK_SIZE`]), so its own frame is shallow;
/// 8 KiB holds it with headroom on the ESP32's scarce SRAM. Sized explicitly, not
/// inherited, and — like the sampler's — validated against the true high-water mark
/// on the metal (qhw.9 on-board acceptance) before it is trusted.
const SERVER_ACCEPT_STACK: usize = 8 * 1024;

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

    // WiFi first: reporting soil moisture to Home Assistant is the whole point, so
    // join the network before the sampler starts. The station owns the modem for
    // the life of `main` (hence 'static, like the sensor below); the native-API
    // server (qhw.27) will read its netif. A boot-time join failure is fatal — it
    // is almost always a bad secrets.toml, worth surfacing loudly — but a later
    // AP reboot is ridden out by `ensure_connected` in the loop (qhw.7).
    let sysloop: EspSystemEventLoop = EspSystemEventLoop::take().expect("system event loop");
    let nvs: EspDefaultNvsPartition =
        EspDefaultNvsPartition::take().expect("default NVS partition");
    let mut wifi: WifiStation<'static> =
        WifiStation::connect(peripherals.modem, sysloop, nvs).expect("wifi station bring-up");

    // Prove the resolver path, not just that we hold a lease (qhw.7). Non-fatal:
    // soil sampling does not need DNS.
    match wifi::resolve(DNS_CHECK_HOST, 443) {
        Ok(addrs) => info!("dns ok: {DNS_CHECK_HOST} -> {addrs:?}"),
        Err(err) => warn!("dns resolve of {DNS_CHECK_HOST} failed: {err}"),
    }

    // Advertise as an ESPHome device so Home Assistant auto-discovers us (qhw.8).
    // The MAC comes from the live station; _mdns owns the advert for the life of
    // main — dropping it retracts the record, which is how power-off makes the
    // device vanish within its TTL. Fatal on failure: without discovery HA cannot
    // find us, defeating the point of being online. The native-API server that
    // answers on :6053 lands in qhw.27/.9.
    let mac_bytes: [u8; 6] = wifi.mac().expect("read station MAC");
    let mac: String = mdns::esphome_mac(&mac_bytes);
    let service: EsphomeService = EsphomeService {
        hostname: HOSTNAME,
        instance_name: FRIENDLY_NAME,
        friendly_name: FRIENDLY_NAME,
        version: env!("CARGO_PKG_VERSION"),
        mac: &mac,
        platform: PLATFORM,
        board: BOARD,
    };
    let _mdns = mdns::advertise(&service).expect("mDNS advertise");
    info!(
        "mdns: advertising _esphomelib._tcp:{} as {FRIENDLY_NAME:?} (mac {mac})",
        mdns::ESPHOME_API_PORT
    );

    // Power the LCD/TFT rails before building the display — an unpowered panel takes
    // a correct ST7789 init and still shows nothing (qhw.20). The AXP192 latches its
    // LDO enables, so this bring-up is scoped: once the rails are up the PMIC and its
    // bus can be dropped (the rails stay on). The internal I2C is wrapped in a
    // RefCellDevice so the MPU6886/RTC on the same bus can join later without
    // rewiring; qhw.31 (probe-power gating) reopens this to hold the AXP192 for
    // runtime LDO toggling. Fatal on failure: a dark screen is a broken monitor.
    {
        let i2c = internal_i2c(
            peripherals.i2c0,
            peripherals.pins.gpio21,
            peripherals.pins.gpio22,
        )
        .expect("internal I2C bring-up");
        let i2c_bus: RefCell<_> = RefCell::new(i2c);
        let mut axp: Axp192<_> = Axp192::new(RefCellDevice::new(&i2c_bus));
        axp.power_on().expect("AXP192 LCD/TFT rail power-on");
        match axp.display_rails_enabled() {
            Ok(true) => info!("axp192: LCD/TFT rails enabled (reg 0x12 read back)"),
            Ok(false) => warn!("axp192: rails did not read back as enabled"),
            Err(err) => warn!("axp192: rail read-back failed: {err}"),
        }
    }

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

    // The onboard TFT mirrors what HA sees: the ST7789 adapter renders the raw ADC
    // count and the moisture percent pulled from the SAME shared cache, on the SAME
    // clock and staleness bound, so the glass shows *unavailable* the instant a
    // reading ages out — never a frozen value. The render loop + freshness are
    // host-tested in plant-shell (qhw.6); this root only binds the real panel. The
    // rails were powered above, so the panel is live. Bring-up failure is fatal (a
    // mis-wired panel is worth surfacing loudly); a later render error only skips a
    // frame — the loop logs it and repaints next tick.
    let display: St7789Display = St7789Display::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 display bring-up");
    let display_config: DisplayConfig = DisplayConfig::new(max_age);
    let display_period: core::time::Duration = display_config.period;
    let _display =
        spawn_display(display, shared.clone(), clock, display_config).expect("spawn plant-display");
    info!("display thread up: ST7789 rendering raw + percent every {display_period:?}");

    // The native-API device HA adopts: one Soil Moisture sensor whose live value is
    // PULLED from the shared cache on every server poll. The freshness check lives
    // in the pure `SharedMoisture::observe` (host-tested in plant-shell): a stale or
    // absent reading returns `None`, which `SensorDevice` serves as `missing_state`
    // — HA shows *unavailable*, never a frozen last value. This closure is the one
    // point the plant and api bounded contexts meet; everything either side of it
    // is host-tested in its own context.
    let device_info: DeviceInfoResponse = DeviceInfoResponse {
        name: DEVICE_NAME.to_string(),
        friendly_name: FRIENDLY_NAME.to_string(),
        mac_address: mac.clone(),
        model: MODEL.to_string(),
        esphome_version: env!("CARGO_PKG_VERSION").to_string(),
        ..Default::default()
    };
    let sensor: SensorConfig = SensorConfig {
        object_id: "soil_moisture".to_string(),
        name: "Soil Moisture".to_string(),
        unit_of_measurement: "%".to_string(),
        accuracy_decimals: 0,
        device_class: "moisture".to_string(),
        state_class: SensorStateClass::StateClassMeasurement,
    };
    let source = {
        let shared: SharedMoisture = shared.clone();
        move || {
            shared
                .observe(clock.now(), max_age)
                .measurement()
                .map(|m: plant_core::Measurement| f32::from(m.percent()))
        }
    };
    let device: Arc<SensorDevice<_>> = Arc::new(SensorDevice::new(device_info, sensor, source));

    // Bind :6053 and run the accept loop on its own thread — `serve` blocks for the
    // life of the program (there is no shutdown path here; the monitor serves until
    // power-off). The per-connection threads are sized inside the host
    // (PLAINTEXT_STACK_SIZE); this thread only accepts and dispatches.
    let server: Server = Server::bind(ServerConfig::default()).expect("bind native-API :6053");
    let _server_thread = thread::Builder::new()
        .name("esphome-api".to_string())
        .stack_size(SERVER_ACCEPT_STACK)
        .spawn(move || {
            if let Err(err) = server.serve(device) {
                error!("native-API server exited: {err}");
            }
        })
        .expect("spawn native-API server thread");
    info!(
        "native-API server up on :{}",
        esphome_server::NATIVE_API_PORT
    );

    // Supervisory loop: keep the WiFi link up (a no-op while connected, a re-join
    // once the router returns — qhw.7) and log a heartbeat so the serial console
    // shows liveness. The server thread is the real consumer of the shared cache.
    //
    // The heartbeat prints the whole Observation, not just "available / not". A
    // faulted probe and a dead sampler thread are different lines, because they are
    // different problems: one wants a new probe, the other wants a stack trace.
    loop {
        FreeRtos::delay_ms(1000);
        if let Err(err) = wifi.ensure_connected() {
            error!("wifi reconnect failed: {err}");
        }
        match shared.observe(clock.now(), max_age) {
            Observation::Fresh(m) => {
                info!("serving moisture = {}% (raw {})", m.percent(), m.raw())
            }
            Observation::Faulted(fault) => {
                warn!("sampler alive, probe faulted — serving unavailable: {fault}")
            }
            Observation::Stale => error!(
                "no fresh sample within {max_age} ms — the sampler thread may be dead or hung"
            ),
            Observation::NeverSampled => info!("waiting for the first sample"),
        }
    }
}
