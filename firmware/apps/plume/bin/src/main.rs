#![forbid(unsafe_code)]
//! plume — the composition root: an ambient feathered frond that breathes on the panel.
//!
//! The simplest app on the platform, and the one that shows the render loop off. No sensor and
//! no network: the plume is a clock-driven ornament, the same frond forever, only ever a little
//! further along its breath. So this root does almost nothing — it powers the LCD rails, brings
//! the panel up, and hands it to the *same* generic render loop the pomodoro timer and the
//! orientation readout use, fed a source that carries no state at all.
//!
//! - [`spawn_display`] (platform-runtime) is the shared render loop, driven by a
//!   [`PlumeView`](plume_display::PlumeView) that answers only "always animating" — so the loop
//!   repaints at its animation cadence, and `plume_display::Plume::render` draws the frond the
//!   elapsed clock implies.
//!
//! ## Pinned portrait
//!
//! The frond is a portrait picture — the board stood on its USB-C port. There is no IMU here to
//! decide which way is up, so the panel is *pinned*: [`PanelScreen::turning`] with a rotation
//! source that always answers [`ScreenRotation::Deg90`]. The panel is turned to portrait once,
//! on the first frame, and held there. An app that followed the board would hand a real
//! rotation source instead; this one commits to the one orientation the picture is drawn for.
//!
//! ## Cadence
//!
//! The render loop's animation cadence is set to [`FRAME_MS`], the same period the plume's phase
//! clock advances on, so one repaint moves the frond by exactly one frame — the speed the
//! source animation intended. Like every thread here, it is floored by one FreeRTOS tick (10 ms
//! at `CONFIG_FREERTOS_HZ = 100`): a shorter sleep on ESP-IDF busy-waits rather than yielding.
//!
//! ## The one bring-up dependency
//!
//! The panel's rails are switched by the AXP192 PMIC, so this root takes the internal I2C bus
//! just long enough to power them on. The PMIC is not retained past that — there is no
//! power-watch here to read VBUS — so once the rails are up the bus is released and the panel is
//! the app's only remaining I/O.

use board_support::{internal_i2c, Axp192};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use platform_adapters::{Panel, PanelScreen, Turning};
use platform_core::{ScreenRotation, Tick};
use platform_runtime::{spawn_display, DisplayConfig, LitFlag, Monotonic};
use plume_core::FRAME_MS;
use plume_display::{Plume, PlumeView};
use std::time::Duration;

/// The quarter turn the plume is drawn at — the board stood on its USB-C port.
const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

fn main() {
    // Patch the ESP-IDF symbols Rust's std expects, then route `log` to the ESP-IDF logger.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("plume: std/ESP-IDF up — an ambient frond on the panel");

    // A boot-time bring-up failure is unrecoverable, so panic with context: the composition
    // root owns the one place peripherals are taken and adapters are built.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // Power the LCD/TFT rails before building the panel — an unpowered panel takes a correct
    // init and still shows nothing. The PMIC is the sole user of the internal bus here, so it
    // takes the `I2cDriver` directly (no shared handle), powers the rails, and is dropped: this
    // app has no power-watch to keep reading it.
    let i2c: I2cDriver<'static> = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let mut axp: Axp192<I2cDriver<'static>> = Axp192::new(i2c);
    axp.power_on().expect("AXP192 LCD/TFT rail power-on");
    drop(axp);
    info!("AXP192: LCD/TFT rails up");

    // The panel, wrapped as a generic Screen with the plume render function. `turning`, not
    // `new`: the plume commits to portrait, so the panel is told to scan that way once, on the
    // first frame, by a rotation source that always answers the same quarter turn.
    let panel: Panel = Panel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 panel bring-up");

    // The renderer owns the sine table and the offscreen frame for the life of the app — built
    // once here, moved into the render closure, and reused every frame.
    let mut plume: Plume = Plume::new();
    let screen: PanelScreen<PlumeView, _, Turning> = PanelScreen::turning(
        panel,
        move |target: &mut _, view: PlumeView, elapsed: Tick, rotation: ScreenRotation| {
            plume.render(target, view, elapsed, rotation)
        },
    );

    // One monotonic clock: the render loop measures the frond's phase against it.
    let clock: Monotonic = Monotonic::start();

    // The frond carries no state, so the source is a constant marker — its whole job is to tell
    // the loop the plume is animating, which the loop reads off the view, not the source value.
    let source = |_now: Tick| PlumeView;
    // Which way up to draw: always portrait. A constant source pins the panel and links the
    // turning path exactly once — see the module docs.
    let rotation_source = |_now: Tick| PORTRAIT;

    // The plume is always animating, so it always paints at the animation cadence. Set that to
    // the phase clock's own frame period, so one repaint advances the breath by one frame. The
    // stack is bumped from the 8 KiB default to 16 KiB: a full-frame blit streams a 32 400-pixel
    // fill through mipidsi's batch path, deeper than the text renders the default was sized for.
    let config: DisplayConfig = DisplayConfig {
        animation_period: Duration::from_millis(FRAME_MS),
        stack_size: 16 * 1024,
        ..DisplayConfig::default()
    };
    let _display = spawn_display(
        screen,
        source,
        rotation_source,
        LitFlag::always(true),
        clock,
        config,
    )
    .expect("spawn plume-display");
    info!(
        "display thread up: ST7789 rendering the frond at {} Hz",
        1_000 / FRAME_MS
    );

    // Supervisory loop: a heartbeat only — the display thread owns the app. Nothing to report
    // but that it is alive, since the plume has no state to log.
    loop {
        FreeRtos::delay_ms(5_000);
        info!("plume: breathing");
    }
}
