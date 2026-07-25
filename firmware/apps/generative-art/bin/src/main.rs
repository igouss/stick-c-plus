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
//!   the start of its own motion. The loop paints through [`GalleryScreen`], which computes the
//!   frame and blits it below mipidsi (see below).
//!
//! ## The fast blit, and pinned portrait
//!
//! Every frame repaints all 32 400 pixels, so the ceiling is the blit. mipidsi's per-pixel
//! `fill_contiguous` gather cost ~59 ms of a 78 ms frame; below it, a whole-frame DMA burst is
//! ~⟨TBD⟩ ms. So the panel is a [`FastPanel`]: mipidsi runs the delicate bring-up and arms the
//! full-screen window, then the bus is reclaimed and each frame is swapped to the wire's byte
//! order and streamed in one DMA transaction. That commits the panel to one orientation — there is
//! no per-frame turn below mipidsi — which suits a portrait gallery (the board stood on its USB-C
//! port, no IMU to ask). It is pinned to [`ScreenRotation::Deg90`] once, at bring-up.
//!
//! ## Cadence and the display stack
//!
//! The gallery is a continuous animation, so it repaints as fast as its work allows rather than at
//! a fixed rate. The cadence target [`REPAINT_MS`] is set at the render loop's one mandatory yield
//! per frame (one FreeRTOS tick, 10 ms at `CONFIG_FREERTOS_HZ = 100`), so the target never itself
//! throttles: the real frame time is a frame's compute-and-blit plus that yield, and the loop
//! reports the fps it actually holds. The frond's *speed* is unaffected — [`plume_core::phase`] is
//! a function of wall-clock time, so a faster repaint only shows more distinct pictures of the same
//! sweep. The display thread runs at **16 KiB**, not the 8 KiB default, because it computes a full
//! 32 400-pixel frame and swaps it into the DMA buffer each tick, deeper than the text renders the
//! default was sized for — the plume proved that on the metal.
//!
//! ## How the frond is evaluated
//!
//! The plume streams its ~5000-point cloud through a [`FrondCompute`](art_display::FrondCompute)
//! port injected here — the cloud is never buffered, it flows point-by-point straight into the
//! frame. By default the port is the one-core evaluator. With the `dual-core` feature it is a
//! two-core one (`src/dual_core.rs`): a worker pinned to Core1 fills the far half of the cloud while
//! the render thread streams the near half, roughly halving the evaluation's wall-clock — the two
//! cores share the field and table read-only and touch disjoint halves, no lock and no `unsafe`,
//! and the cloud is proven bit-identical to the one-core path. That feature is off by default
//! because its worker buffer does not yet fit the heap alongside the two full-screen frame buffers;
//! see the FPS-optimization plan under `docs/plans/`.
//!
//! ## The one bring-up dependency
//!
//! The panel's rails are switched by the AXP192 PMIC, so this root takes the internal I2C bus just
//! long enough to power them on, then releases it: there is no power-watch here to keep reading
//! VBUS, so once the rails are up the panel and the button are the app's only I/O.

#[cfg(feature = "dual-core")]
mod dual_core;

use art_display::{Gallery, GalleryView, Rgb565, REPAINT_MS, SCREEN_SIZE};
use art_shell::{spawn_input, SharedSelector, INPUT_CONFIG};
use board_support::{internal_i2c, Axp192};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use platform_adapters::{FastPanel, GpioButton, St7789Error};
use platform_core::{Screen, ScreenRotation, Tick};
use platform_runtime::{spawn_display, DisplayConfig, LitFlag, Monotonic};
use std::time::Duration;

/// The quarter turn the gallery is drawn at — the board stood on its USB-C port.
const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

/// The gallery's [`Screen`]: compute the selected sketch into the offscreen frame, then blit that
/// frame to the panel as one DMA burst.
///
/// This is the seam the fast blit needs. The generic `PanelScreen` hands a mipidsi draw target to
/// a render closure — the per-pixel `fill_contiguous` path, whose gather is the frame ceiling.
/// Below mipidsi there is no draw target, so the render loop drives this instead:
/// [`Gallery::paint`] computes the frame and returns it as a pixel slice, and [`FastPanel::blit`]
/// swaps it to the wire's byte order and streams it in one DMA transaction. `gallery` and `panel`
/// are separate fields, so painting (which borrows the gallery) and blitting (which borrows the
/// panel) do not contend.
struct GalleryScreen {
    gallery: Gallery,
    panel: FastPanel,
}

impl Screen<GalleryView> for GalleryScreen {
    type Error = St7789Error;

    fn show(
        &mut self,
        view: GalleryView,
        elapsed: Tick,
        rotation: ScreenRotation,
    ) -> Result<(), St7789Error> {
        let pixels: &[Rgb565] = self.gallery.paint(view, elapsed, rotation);
        self.panel.blit(pixels)
    }
}

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
    info!(
        "heap after rails up: {} B free (default), largest DMA block {} B",
        dma_mem::free_default(),
        dma_mem::largest_free_dma()
    );

    // Every frame here is a whole-screen blit, so the panel is driven **below mipidsi** over SPI
    // DMA (see `FastPanel`): the finished frame is swapped to the wire's byte order and streamed in
    // one DMA transaction, skipping mipidsi's per-pixel gather — the frame ceiling on a full-screen
    // sketch. The DMA engine can only read DMA-capable RAM, so the whole-frame buffer (`w·h·2`
    // bytes) comes from `dma_mem`, the one place that asks ESP-IDF for such memory; a null means it
    // did not fit. A non-DMA-capable buffer would not run slow but double-fault before frame one.
    const FRAME_BYTES: usize = (SCREEN_SIZE.width * SCREEN_SIZE.height * 2) as usize;
    let frame_buffer: &'static mut [u8] =
        dma_mem::dma_buffer(FRAME_BYTES).expect("DMA-capable panel frame buffer");

    // The panel, pinned to portrait once at bring-up: mipidsi runs the delicate init and arms the
    // full-screen window, then the bus is reclaimed for the direct DMA blit path. No per-frame
    // turn — the gallery is a fixed-portrait picture.
    let panel: FastPanel = FastPanel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
        PORTRAIT,
        frame_buffer,
    )
    .expect("ST7789 panel bring-up");

    // The renderer, built on a frond-compute port injected here at the composition root. The plume
    // streams its ~5000-point cloud through that port straight into the frame — never buffered — so
    // the gallery domain stays single-threaded and framework-free. With the `dual-core` feature the
    // port is the two-core evaluator (a Core1 worker fills the far half while the render thread
    // streams the near half, roughly halving the frame's dominant cost, bit-identically); by default
    // it is the one-core [`SerialFrond`]. The renderer owns its capital and the offscreen frame for
    // the life of the app; paired with the panel in the gallery's Screen, they are moved into the
    // display thread together.
    #[cfg(feature = "dual-core")]
    let gallery: Gallery = Gallery::with_frond(|| Box::new(dual_core::DualCoreFrond::new()));
    #[cfg(not(feature = "dual-core"))]
    let gallery: Gallery = Gallery::with_frond(|| Box::new(art_display::SerialFrond::new()));
    let screen: GalleryScreen = GalleryScreen { gallery, panel };

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
    // Which way up to draw: always portrait — the sketches size their canvas from it. It matches
    // the rotation the panel was pinned to at bring-up; a constant source keeps them in step.
    let rotation_source = |_now: Tick| PORTRAIT;

    // The gallery is always animating, so it repaints as fast as its work allows. `REPAINT_MS` is
    // set at the loop's mandatory per-frame yield, so it is never itself the throttle: the real
    // cadence is a frame's compute-and-blit plus that one-tick yield. The stack is bumped from the
    // 8 KiB default to 16 KiB: the render thread streams a full 32 400-pixel frame and swaps it to
    // the DMA buffer each tick, deeper than the text renders the default was sized for — the plume
    // proved 8 KiB overflows on the metal.
    let config: DisplayConfig = DisplayConfig {
        animation_period: Duration::from_millis(REPAINT_MS),
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
        "display thread up: ST7789 rendering the gallery work-bound (repaint ceiling {REPAINT_MS} ms) \
         — the loop reports its achieved fps"
    );

    // Supervisory loop: a heartbeat only — the input and display threads own the app. It reports
    // the piece currently on the glass, so the serial log follows the button.
    loop {
        FreeRtos::delay_ms(5_000);
        info!("generative-art: showing {:?}", selector.current());
    }
}
