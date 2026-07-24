#![forbid(unsafe_code)]
//! generative-art — the composition root: a button-cycled gallery of generative sketches.
//!
//! One panel, one button. The gallery shows a single sketch at a time; a front-button click
//! advances to the next, wrapping after the last back to the first. No sensor and no network —
//! this root powers the LCD rails, brings the panel and the button up, and hands them to the two
//! host-tested loops the platform already provides:
//!
//! - [`spawn_input`] (art-shell) polls the front button, folds its gesture through the
//!   single-button recogniser, and advances the shared [`SharedSelector`] on a click — the only
//!   control the gallery has.
//! - [`spawn_display`] (platform-runtime) is the *same* generic render loop the pomodoro timer and
//!   the plume used, fed a source that snapshots the shared selector into a
//!   [`GalleryView`](art_display::GalleryView) each frame. When the selected sketch changes, the
//!   view's anchor changes and the loop resets the animation clock — so the new piece begins from
//!   the start of its own motion.
//!
//! ## Pinned portrait
//!
//! The sketches are portrait pictures — the board stood on its USB-C port. There is no IMU here to
//! decide which way is up, so the panel is *pinned*: [`PanelScreen::turning`] with a rotation
//! source that always answers [`ScreenRotation::Deg90`]. The panel is turned to portrait once, on
//! the first frame, and held there.
//!
//! ## Cadence and the display stack
//!
//! The render loop's animation cadence is [`FRAME_MS`] (a 30 fps target), floored — like every
//! thread here — by one FreeRTOS tick (10 ms at `CONFIG_FREERTOS_HZ = 100`): a shorter sleep
//! busy-waits rather than yielding. The display thread runs at **16 KiB**, not the 8 KiB default,
//! because a full-frame colour blit streams a 32 400-pixel fill deeper than the text renders the
//! default was sized for — the plume proved that on the metal.
//!
//! ## The one bring-up dependency
//!
//! The panel's rails are switched by the AXP192 PMIC, so this root takes the internal I2C bus just
//! long enough to power them on, then releases it: there is no power-watch here to keep reading
//! VBUS, so once the rails are up the panel and the button are the app's only I/O.

use art_display::{Gallery, GalleryView, FRAME_MS};
use art_shell::{spawn_input, SharedSelector, INPUT_CONFIG};
use board_support::{internal_i2c, Axp192};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use platform_adapters::{GpioButton, Panel, PanelScreen, Turning};
use platform_core::{ScreenRotation, Tick};
use platform_runtime::{spawn_display, DisplayConfig, LitFlag, Monotonic};
use std::time::Duration;

/// The quarter turn the gallery is drawn at — the board stood on its USB-C port.
const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

fn main() {
    // Patch the ESP-IDF symbols Rust's std expects, then route `log` to the ESP-IDF logger.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("generative-art: std/ESP-IDF up — a button-cycled gallery on the panel");

    // A boot-time bring-up failure is unrecoverable, so panic with context: the composition root
    // owns the one place peripherals are taken and adapters are built.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // Power the LCD/TFT rails before building the panel — an unpowered panel takes a correct init
    // and still shows nothing. The PMIC is the sole user of the internal bus here, so it takes the
    // `I2cDriver` directly, powers the rails, and is dropped: this app has no power-watch to keep
    // reading it.
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

    // The panel, wrapped as a generic Screen with the gallery render function. `turning`, not
    // `new`: the gallery commits to portrait, so the panel is told to scan that way once, on the
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
    let mut gallery: Gallery = Gallery::new();
    let screen: PanelScreen<GalleryView, _, Turning> = PanelScreen::turning(
        panel,
        move |target: &mut _, view: GalleryView, elapsed: Tick, rotation: ScreenRotation| {
            gallery.render(target, view, elapsed, rotation)
        },
    );

    // The one control: the front push-button on G37 (active-low, no internal pull). The gallery
    // has no other input, so the side and power buttons are left unwired.
    let front: GpioButton = GpioButton::new(peripherals.pins.gpio37).expect("front button G37");

    // One monotonic clock, shared by the input thread (writer) and the render loop (reader), so
    // the gesture pipeline and the animation clock are measured on one time base.
    let clock: Monotonic = Monotonic::start();
    // The selector: the input thread advances it on a click, the render loop reads it each frame.
    let selector: SharedSelector = SharedSelector::new();

    // Input: poll the front button, advance the gallery on a click. Held for the life of main —
    // dropping it would only detach the thread, which already runs forever.
    let _input =
        spawn_input(front, selector.clone(), clock, INPUT_CONFIG).expect("spawn art-input");
    info!("input thread up: front click = next sketch (wrapping)");

    // Display: render the selected sketch every frame, through the same generic loop the plume
    // used. The source snapshots the shared selector into a GalleryView; a switch changes the
    // view's anchor, which resets the animation clock for the new piece.
    let source = {
        let selector: SharedSelector = selector.clone();
        move |_now: Tick| GalleryView::new(selector.current())
    };
    // Which way up to draw: always portrait. A constant source pins the panel and links the
    // turning path exactly once — see the module docs.
    let rotation_source = |_now: Tick| PORTRAIT;

    // The gallery is always animating, so it always paints at the animation cadence. The stack is
    // bumped from the 8 KiB default to 16 KiB: a full-frame colour blit streams a 32 400-pixel
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
    .expect("spawn generative-art-display");
    info!(
        "display thread up: ST7789 rendering the gallery at {} Hz",
        1_000 / FRAME_MS
    );

    // Supervisory loop: a heartbeat only — the input and display threads own the app. It reports
    // the piece currently on the glass, so the serial log follows the button.
    loop {
        FreeRtos::delay_ms(5_000);
        info!("generative-art: showing {:?}", selector.current());
    }
}
