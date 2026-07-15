//! `st7789` — the M5StickC Plus onboard TFT as a [`MoistureDisplay`].
//!
//! The driven adapter for [`plant_core::MoistureDisplay`]: it drives the 1.14″
//! ST7789V2 panel over SPI with `mipidsi`, and hands the panel to `plant-display`,
//! which paints the current soil [`Observation`] onto it. All freshness and cadence
//! live inward (the pure [`observe`](plant_core::observe) policy and the
//! `spawn_display` loop in `plant-shell`); the *picture* lives in `plant-display`.
//! What is left here — and all that should be — is the hardware: the SPI bus, the
//! pins, and the panel's own quirks.
//!
//! That split is what makes the screen reviewable. `plant_display::render` draws into
//! any `DrawTarget`, so the very same code that paints this panel paints a host
//! framebuffer: `just screens` writes a PNG of every state the glass can show. The
//! images are made by the production renderer, not a replica of it.
//!
//! ## Panel quirks (M5StickC Plus)
//!
//! The panel is a *partial* window on a 240×320 controller framebuffer, so it needs
//! the CGRAM offset (col 52 / row 40 in the native portrait orientation) and **INVON**
//! (colour inversion) — miss either and the image is shifted or inverted. The
//! backlight and panel rails are powered by the AXP192 (`board-support`), not a GPIO,
//! so the composition root must power those rails *before* this adapter is built or
//! the screen stays black.
//!
//! ### Colour order is `Rgb`, and the factory driver will lie to you about it
//!
//! This adapter must pass [`ColorOrder::Rgb`]. That contradicts the pinned factory
//! library, whose `TFT_MAD_COLOR_ORDER` resolves to `TFT_MAD_BGR` (its `TFT_RGB_ORDER`
//! is undefined while `CGRAM_OFFSET` is defined) — and `mipidsi`'s `ColorOrder::Bgr`
//! sets that very same MADCTL bit 3. On paper the two inits agree. On the glass they
//! do not: with `Bgr`, `Rgb565::RED` renders blue.
//!
//! The bit is not portable because the pixel pipeline around it is not. TFT_eSPI and
//! `mipidsi` do not hand the controller identical bytes, so a MADCTL value lifted from
//! one stack means nothing in the other. Measured, not reasoned — see
//! `kb/experiments/2026-07-09-panel-colour-order/` and [`Self::colour_check`], which
//! makes the claim falsifiable in one flash.
//!
//! This hid for the display's entire life because white and black are symmetric in
//! red and blue: `0xFFFF` and `0x0000` cannot show a swap. It surfaced the first time
//! a genuinely coloured pixel shipped — the red `FAULT` line — and it would have been
//! caught on day one by drawing three bands instead of two text rows.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_hal::spi::MODE_0;
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{AnyIOPin, Gpio13, Gpio15, Gpio18, Gpio23, Gpio5, Output, PinDriver};
use esp_idf_hal::spi::config::{Config as SpiConfig, DriverConfig as SpiDriverConfig};
use esp_idf_hal::spi::{SpiDeviceDriver, SpiDriver, SPI2};
use esp_idf_hal::units::FromValueType;
use mipidsi::models::ST7789;
use mipidsi::options::{ColorInversion, ColorOrder, Orientation, Rotation};
use mipidsi::{interface::SpiInterface, Builder, Display};
use plant_core::{MoistureDisplay, Observation, Tick};
use plant_display::{RenderError, SCREEN_SIZE};
use static_cell::StaticCell;

/// Native (unrotated) panel width — the short axis, in portrait.
///
/// Derived from `plant-display`'s landscape canvas by swapping the axes back, since
/// this adapter rotates the panel 90°. One source of truth: a change to the canvas
/// cannot leave the panel configured for the old geometry.
const PANEL_W: u16 = SCREEN_SIZE.height as u16;
/// Native (unrotated) panel height — the long axis, in portrait.
const PANEL_H: u16 = SCREEN_SIZE.width as u16;
/// CGRAM column offset of the visible window in the native portrait orientation.
const OFFSET_X: u16 = 52;
/// CGRAM row offset of the visible window in the native portrait orientation.
const OFFSET_Y: u16 = 40;
/// The display SPI clock — the factory `SPI_FREQUENCY`.
const SPI_HZ: u32 = 27_000_000;
/// The pixel-batch buffer `mipidsi`'s SPI interface gathers writes into. Larger is
/// faster; a few hundred bytes is ample for text and a full-screen clear.
const BUFFER_LEN: usize = 512;

/// The pixel-batch buffer mipidsi's SPI interface borrows for the program's life.
/// A `StaticCell` gives it a truly `static` home — no allocator, no leaked `Box` —
/// initialised exactly once when the single display is built.
static SPI_BUFFER: StaticCell<[u8; BUFFER_LEN]> = StaticCell::new();

// The one concrete panel type this adapter drives. Named here so the on-target
// composition root never has to spell the full mipidsi/esp-idf-hal type out; it
// builds a `St7789Display` by inference and hands it to `spawn_display`.
type BusDevice = SpiDeviceDriver<'static, SpiDriver<'static>>;
type Dc = PinDriver<'static, Output>;
type Rst = PinDriver<'static, Output>;
type Interface = SpiInterface<'static, BusDevice, Dc>;
type Panel = Display<Interface, ST7789, Rst>;

/// A failure driving the ST7789, carrying its underlying cause for the log.
///
/// The panel's SPI and pin errors are `esp-idf-hal`'s own opaque newtypes, and
/// `mipidsi` composes them into an init/interface error that is only `Debug`; rather
/// than leak those types across the [`MoistureDisplay`] port, this captures the
/// formatted cause behind a `Display` face the render loop can log.
#[derive(Debug)]
pub struct St7789Error(String);

impl core::fmt::Display for St7789Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for St7789Error {}

/// Wrap any `Debug` failure with the operation that produced it.
fn fault<E: core::fmt::Debug>(op: &str, err: E) -> St7789Error {
    St7789Error(format!("ST7789 {op}: {err:?}"))
}

/// Flatten a `plant-display` render failure into this adapter's error.
///
/// The renderer is generic in the target's error, so a panel bus failure arrives
/// wrapped; a line that would not fit its buffer is the renderer's own complaint. The
/// port carries neither type outward — only [`St7789Error`].
fn render_fault<E: core::fmt::Debug>(op: &str, err: RenderError<E>) -> St7789Error {
    match err {
        RenderError::Draw(err) => fault(op, err),
        RenderError::LineOverflow => St7789Error(format!("ST7789 {op}: line buffer overflow")),
    }
}

/// The M5StickC Plus onboard ST7789 TFT.
pub struct St7789Display {
    panel: Panel,
}

impl St7789Display {
    /// Bring up the panel: build the SPI device, drive DC/RST, and run the ST7789
    /// init with the M5StickC Plus offsets, colour inversion and **RGB** colour order
    /// (see the module docs — `Bgr` renders red as blue here, whatever the factory
    /// library's MADCTL bit says).
    ///
    /// The pins are fixed by the board (MOSI 15, SCLK 13, DC 23, RST 18, CS 5), so
    /// they are taken by concrete type. Requires the AXP192 LCD/TFT rails already
    /// powered — an unpowered panel takes a correct init and still shows nothing.
    ///
    /// Constructed **once**: it claims the single `static` [`SPI_BUFFER`], so a
    /// second call panics. The board has one display, so the composition root builds
    /// one adapter.
    pub fn new(
        spi: SPI2<'static>,
        sclk: Gpio13<'static>,
        mosi: Gpio15<'static>,
        cs: Gpio5<'static>,
        dc: Gpio23<'static>,
        rst: Gpio18<'static>,
    ) -> Result<Self, St7789Error> {
        let bus: BusDevice = SpiDeviceDriver::new_single(
            spi,
            sclk,
            mosi,
            None::<AnyIOPin>, // write-only panel: no MISO.
            Some(cs),
            &SpiDriverConfig::new(),
            &SpiConfig::new().baudrate(SPI_HZ.Hz()).data_mode(MODE_0),
        )
        .map_err(|e| fault("spi", e))?;
        let dc: Dc = PinDriver::output(dc).map_err(|e| fault("dc pin", e))?;
        let rst: Rst = PinDriver::output(rst).map_err(|e| fault("rst pin", e))?;

        // The SPI interface borrows a pixel-batch buffer for its whole life. A
        // StaticCell hands it a 'static buffer with no allocator and no leaked Box —
        // initialised once here (a second St7789Display::new would panic).
        let buffer: &'static mut [u8] = SPI_BUFFER.init([0u8; BUFFER_LEN]);
        let interface: Interface = SpiInterface::new(bus, dc, buffer);

        let mut delay: Ets = Ets;
        let mut panel: Panel = Builder::new(ST7789, interface)
            .display_size(PANEL_W, PANEL_H)
            .display_offset(OFFSET_X, OFFSET_Y)
            .orientation(Orientation::new().rotate(Rotation::Deg90))
            .invert_colors(ColorInversion::Inverted)
            // Rgb, NOT Bgr — measured on the glass, not inferred from the factory
            // driver. See the module docs: `Bgr` renders red as blue on this panel.
            .color_order(ColorOrder::Rgb)
            .reset_pin(rst)
            .init(&mut delay)
            .map_err(|e| fault("init", e))?;

        // Clear once at bring-up. From here on, each text line is drawn with an
        // opaque background (see `line`), overwriting its own row in place — so the
        // per-update full-screen clear that caused a visible flash is gone, and only
        // a changed value repaints (the render loop suppresses steady ticks).
        panel.clear(Rgb565::BLACK).map_err(|e| fault("clear", e))?;

        Ok(Self { panel })
    }

    /// Bring-up self-test: paint the three primary bands on the real glass.
    ///
    /// The picture is [`plant_display::colour_bands`]; what this method contributes is
    /// the thing that makes it evidence — the *production* init path in [`Self::new`],
    /// with this panel's real [`ColorOrder`], inversion and offsets beneath it. The
    /// same bands drawn into a host framebuffer prove nothing about colour order,
    /// because a framebuffer paints red as red however the panel is wired.
    ///
    /// Read the glass:
    ///
    /// - **R / G / B in order** — the colour order is right.
    /// - **bands read B / G / R** — red and blue are swapped: the [`ColorOrder`] in
    ///   [`Self::new`] is wrong for this panel. Green is invariant under that swap,
    ///   which is what makes the diagnosis unambiguous.
    /// - **green looks magenta** (and red cyan, blue yellow) — the inversion is
    ///   wrong, not the order. This should be impossible while white renders white,
    ///   which is why the labels are drawn in white.
    pub fn colour_check(&mut self) -> Result<(), St7789Error> {
        plant_display::colour_bands(&mut self.panel)
            .map_err(|err| render_fault("colour bands", err))
    }
}

impl MoistureDisplay for St7789Display {
    type Error = St7789Error;

    /// Hand the panel to the renderer. What appears — the wording, the colours, the two
    /// rows, the creature and its frame, the in-place erase — is `plant-display`'s
    /// decision, made once and reviewable on the host. This adapter's contribution is the
    /// panel it draws on.
    fn show(&mut self, observation: Observation, elapsed: Tick) -> Result<(), St7789Error> {
        plant_display::render(&mut self.panel, observation, elapsed)
            .map_err(|err| render_fault("render", err))
    }
}
