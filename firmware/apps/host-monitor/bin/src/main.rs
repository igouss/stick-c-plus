#![forbid(unsafe_code)]
//! host-monitor — the composition root on std/ESP-IDF.
//!
//! Brings the board onto the network via [`net::wifi`], then wires the driven
//! [`host_adapters::HttpPulseSource`] into the host [`host_shell`] poller thread: every poll
//! period the thread fetches the bearer-gated hostpulse endpoint (`GET /pulse`), which
//! returns a ready-to-plot per-host CPU/memory series for every homelab host in one call, and
//! publishes the whole [`Pulse`](host_core::Pulse) frame into a [`SharedMetrics`] cache. The
//! onboard TFT then draws one row per host — a name, two live percentages, and two sparklines
//! (CPU cyan, memory yellow) — the same `host_display::render` proven on the host, wired here
//! to the real ST7789 panel through [`platform_runtime::spawn_display`].
//!
//! Unlike the plant monitor this is a pure *client*: it consumes the endpoint's frame rather
//! than exposing its own metrics, so there is no mDNS advert and no native-API server — just
//! WiFi, the HTTP poller, and the display. The endpoint has already done the PromQL `rate()`,
//! so there is no on-device parsing or rate arithmetic; a fetch simply replaces the frame. The
//! supervisory loop keeps the WiFi link up and logs a heartbeat.
//!
//! The endpoint and its bearer token are baked in at build time from the git-ignored
//! `firmware/secrets.toml` `[host_monitor]` table (see `build.rs`); the WiFi credentials come
//! the same way through the shared `net` crate. The token is never logged.

use board_support::{internal_i2c, Axp192};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use host_adapters::HttpPulseSource;
use host_core::{Status, Tick};
use host_display::Glass;
use host_shell::{spawn_poller, PollerConfig, SharedMetrics};
use log::{error, info, warn};
use net::wifi::WifiStation;
use platform_adapters::{Axp192PowerSource, LedcBuzzer, Panel, PanelScreen};
use platform_runtime::{
    spawn_buzzer, spawn_display, spawn_power_watch, DisplayConfig, Monotonic, PowerWatchConfig,
};

/// The hostpulse endpoint (`host:port`), baked in at build time from `firmware/secrets.toml`'s
/// `[host_monitor]` table.
///
/// A compile-time constant, not tracked source: `build.rs` fails the build if the secrets file
/// or its `[host_monitor]` table is missing, so an image is never produced that fetches
/// nowhere (the second `env!` argument is the message shown if that guarantee ever breaks).
const HOST_ENDPOINT: &str = env!(
    "HOST_MONITOR_ENDPOINT",
    "HOST_MONITOR_ENDPOINT not set — host-monitor/build.rs must emit it"
);

/// The bearer token the endpoint requires, baked in the same way. A secret: it is sent only
/// in the `Authorization` header and is never logged.
const HOST_TOKEN: &str = env!(
    "HOST_MONITOR_TOKEN",
    "HOST_MONITOR_TOKEN not set — host-monitor/build.rs must emit it"
);

fn main() {
    // Patch a few ESP-IDF symbols Rust's std expects, then route `log` records to the
    // ESP-IDF logger so `info!`/`warn!` reach the serial monitor.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("host-monitor: std/ESP-IDF up — fetching {HOST_ENDPOINT}/pulse for all hosts");

    // A boot-time bring-up failure is unrecoverable, so panic with context rather than
    // limp on: the composition root owns the one place peripherals are taken.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // WiFi first: the whole point is scraping a host over the network, so join before the
    // poller starts. The station owns the modem for the life of `main` (hence 'static). A
    // boot-time join failure is fatal — almost always a bad secrets.toml — but a later AP
    // reboot is ridden out by `ensure_connected` in the loop.
    let sysloop: EspSystemEventLoop = EspSystemEventLoop::take().expect("system event loop");
    let nvs: EspDefaultNvsPartition =
        EspDefaultNvsPartition::take().expect("default NVS partition");
    let mut wifi: WifiStation<'static> =
        WifiStation::connect(peripherals.modem, sysloop, nvs).expect("wifi station bring-up");

    // Power the LCD/TFT rails before building the display — an unpowered panel takes a
    // correct ST7789 init and still shows nothing (qhw.20). The AXP192 latches its LDO
    // enables, but this root now *retains* the PMIC past power-on rather than dropping it:
    // the power-watch thread reads VBUS from this same device for the life of the app. The
    // internal bus has no other live runtime consumer (the MPU6886/RTC are unused), so the
    // watcher owns the `Axp192<I2cDriver>` outright — `I2cDriver` is `Send`, unlike the
    // `RefCellDevice` a shared bus would need. Fatal on failure: a dark screen is a broken
    // monitor.
    let i2c = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let mut axp: Axp192<_> = Axp192::new(i2c);
    axp.power_on().expect("AXP192 LCD/TFT rail power-on");
    match axp.display_rails_enabled() {
        Ok(true) => info!("axp192: LCD/TFT rails enabled (reg 0x12 read back)"),
        Ok(false) => warn!("axp192: rails did not read back as enabled"),
        Err(err) => warn!("axp192: rail read-back failed: {err}"),
    }
    let power_source: Axp192PowerSource<_> = Axp192PowerSource::new(axp);

    // One monotonic clock, shared by the poller (writer) and the display loop (reader), so
    // a reading's age is measured on a single time base.
    let clock: Monotonic = Monotonic::start();
    let shared: SharedMetrics = SharedMetrics::new();
    let config: PollerConfig = PollerConfig::new();
    let max_age: Tick = config.max_age();
    // The cadence adapts to each frame's step_s; before the first frame it polls at this
    // ceiling. Logged only to show the bound the operator can expect.
    let max_period: core::time::Duration = config.max_period;

    // The driven adapter: fetch the bearer-gated /pulse endpoint over HTTP and hand the body
    // to the pure host-wire codec. Handed to the poller thread, which owns the timing and
    // cache. The token goes only into the adapter's Authorization header — never a log line.
    let source: HttpPulseSource = HttpPulseSource::new(HOST_ENDPOINT, HOST_TOKEN);
    info!("host-monitor: GET {}", source.url());
    let _poller =
        spawn_poller(source, shared.clone(), clock, config).expect("spawn host-poller thread");
    info!("poller thread up: cadence adapts to step_s (≤ {max_period:?}), unavailable after {max_age} ms stale");

    // The onboard TFT: the ST7789 adapter renders one row per host (name + two live
    // percentages + two sparklines) from the SAME shared cache, on the SAME clock and
    // staleness bound, so the glass tints and flips to *unavailable* the instant fetches age
    // out — while keeping the last good frame. The render loop + freshness are host-tested in
    // host-shell/host-display; this root only binds the real panel. The rails were powered
    // above, so the panel is live.
    let panel: Panel = Panel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 panel bring-up");
    // Wrap the panel as a generic Screen with the host render function: unwrap the Glass and
    // hand the HostState to host_display::render. The panel adapter knows nothing about the
    // host monitor; the picture is injected here.
    let screen: PanelScreen<Glass, _> = PanelScreen::new(
        panel,
        |target: &mut _, Glass(state): Glass, elapsed: Tick| {
            host_display::render(target, state, elapsed)
        },
    );
    // host_display draws three host rows — three names, six percentages, six sparklines —
    // heavier than the plant/pomodoro glass, and the HTTP poller's preemption pushes
    // context-switch frames onto this stack mid-SPI. The shared 8 KiB default overflowed on
    // the metal (a context switch deep in the mipidsi SPI path corrupted the SPI bus-lock
    // semaphore → a spi_bus_lock_acquire_start assert, and a reboot loop), so host-monitor
    // gives the display thread a larger stack. Validate the high-water mark if the layout grows.
    let display_config: DisplayConfig = DisplayConfig {
        stack_size: 16 * 1024,
        ..DisplayConfig::default()
    };
    let display_period: core::time::Duration = display_config.period;
    // The source the generic render loop pulls each tick: the freshest snapshot (last good
    // frame + current status) from the SAME cache and staleness bound the heartbeat reads,
    // wrapped in the Glass view the panel Screen paints. `now` is the render loop's own
    // clock — the same Monotonic the poller stamps against — so the glass tints and flips to
    // *unavailable* the instant a fetch ages out, while keeping the last good frame.
    let display_source = {
        let shared: SharedMetrics = shared.clone();
        move |now: Tick| Glass(shared.snapshot(now, max_age))
    };
    let _display = spawn_display(screen, display_source, clock, display_config)
        .expect("spawn host-monitor display");
    info!("display thread up: ST7789 rendering three host rows every {display_period:?}");

    // The on-board passive buzzer (LEDC on G2), behind one owner thread so the power-watch chime
    // and any future sound share the single hardware buzzer without interleaving. host-monitor
    // sounds only the shared USB power chime today.
    let buzzer = LedcBuzzer::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio2,
    )
    .expect("buzzer G2 (LEDC)");
    let (_buzzer_owner, tone) = spawn_buzzer(buzzer).expect("spawn buzzer owner");

    // Power-watch: poll VBUS on the retained AXP192, debounce it, and sound the spool-up /
    // spool-down chime a settled USB plug or unplug decides — the shared platform capability,
    // on the same clock. Silent at boot: the first sample only seeds the baseline. Held for the
    // life of main.
    let _power_watch = spawn_power_watch(power_source, tone, clock, PowerWatchConfig::default())
        .expect("spawn host power-watch");
    info!("power-watch thread up: USB plug = spool-up, unplug = spool-down");

    // Supervisory loop: keep the WiFi link up (a no-op while connected, a re-join once the
    // router returns) and log a heartbeat so the serial console shows liveness. The display
    // thread is the real consumer of the shared cache.
    //
    // The heartbeat prints the whole status, not just "available / not". An unreachable
    // endpoint and a dead poller thread are different lines, because they are different
    // problems: one wants the control node checked, the other wants a stack trace. The
    // per-host detail is on the glass; the console only needs the endpoint's health.
    loop {
        FreeRtos::delay_ms(1000);
        if let Err(err) = wifi.ensure_connected() {
            error!("wifi reconnect failed: {err}");
        }
        let snapshot = shared.snapshot(clock.now(), max_age);
        match snapshot.status {
            Status::Fresh => {
                let hosts: usize = snapshot.frame.map(|frame| frame.len()).unwrap_or(0);
                info!("pulse: fresh frame — {hosts} host(s)");
            }
            Status::Faulted(fault) => {
                warn!("poller alive, endpoint not answering — keeping last frame: {fault}")
            }
            Status::Stale => {
                error!("no fresh fetch within {max_age} ms — the poller thread may be dead or hung")
            }
            Status::NeverSampled => info!("waiting for the first /pulse fetch"),
        }
    }
}
