//! `st7789` — the M5StickC Plus onboard TFT as a [`MoistureDisplay`].
//!
//! The driven adapter for [`plant_core::MoistureDisplay`]: it drives the 1.14″
//! ST7789V2 panel over SPI with `mipidsi` and renders the current soil
//! [`Measurement`] — the raw ADC count and the moisture percent — with
//! `embedded-graphics`. All freshness and cadence live inward (the pure
//! [`fresh`](plant_core::fresh) policy and the `spawn_display` loop in
//! `plant-shell`); this adapter is the thin hardware translation that turns a
//! measurement into pixels, and nothing more.
//!
//! ## Panel quirks (M5StickC Plus, verified against the factory firmware)
//!
//! The panel is a *partial* window on a 240×320 controller framebuffer, so it needs
//! the CGRAM offset (col 52 / row 40 in the native portrait orientation), and both
//! **INVON** (colour inversion) *and* **BGR** colour order — miss either and the
//! image is shifted or the colours are wrong. The backlight and panel rails are
//! powered by the AXP192 (`board-support`), not a GPIO, so the composition root must
//! power those rails *before* this adapter is built or the screen stays black.

use core::fmt::Write as _;

use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::{MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};
use embedded_hal::spi::MODE_0;
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{AnyIOPin, Gpio13, Gpio15, Gpio18, Gpio23, Gpio5, Output, PinDriver};
use esp_idf_hal::spi::config::{Config as SpiConfig, DriverConfig as SpiDriverConfig};
use esp_idf_hal::spi::{SpiDeviceDriver, SpiDriver, SPI2};
use esp_idf_hal::units::FromValueType;
use mipidsi::models::ST7789;
use mipidsi::options::{ColorInversion, ColorOrder, Orientation, Rotation};
use mipidsi::{interface::SpiInterface, Builder, Display};
use plant_core::{Measurement, MoistureDisplay};
use static_cell::StaticCell;

/// Native (unrotated) panel width — the short axis, in portrait.
const PANEL_W: u16 = 135;
/// Native (unrotated) panel height — the long axis, in portrait.
const PANEL_H: u16 = 240;
/// CGRAM column offset of the visible window in the native portrait orientation.
const OFFSET_X: u16 = 52;
/// CGRAM row offset of the visible window in the native portrait orientation.
const OFFSET_Y: u16 = 40;
/// The display SPI clock — the factory `SPI_FREQUENCY`.
const SPI_HZ: u32 = 27_000_000;
/// The pixel-batch buffer `mipidsi`'s SPI interface gathers writes into. Larger is
/// faster; a few hundred bytes is ample for text and a full-screen clear.
const BUFFER_LEN: usize = 512;

/// Baseline-Top x inset of both text lines.
const TEXT_X: i32 = 8;
/// Baseline-Top y of the raw line, and of the percent line (landscape, 240×135).
const RAW_Y: i32 = 34;
const PCT_Y: i32 = 80;
/// Fixed character width both lines are padded to, so a shorter new value's trailing
/// spaces (painted with the opaque background) erase the leftover glyphs of a longer
/// old one — no full-screen clear, and therefore no per-update flash.
const LINE_WIDTH: usize = 10;
/// Stack capacity of a rendered line — [`LINE_WIDTH`] plus headroom, so a line is
/// built with no heap allocation on the render path.
const LINE_CAP: usize = 16;

/// A stack-allocated line buffer: the render path formats into this instead of a
/// heap [`String`], so a redraw allocates nothing on the ESP32's scarce SRAM.
type Line = heapless::String<LINE_CAP>;

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

/// The M5StickC Plus onboard ST7789 TFT.
pub struct St7789Display {
    panel: Panel,
}

impl St7789Display {
    /// Bring up the panel: build the SPI device, drive DC/RST, and run the ST7789
    /// init with the M5StickC Plus offsets, colour inversion and BGR order.
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
            .color_order(ColorOrder::Bgr)
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

    /// Draw one baseline-top text line at `y`, padded to [`LINE_WIDTH`] with an
    /// opaque black background so it overwrites its whole row in place.
    ///
    /// `content` is formatted into a stack [`Line`] — no heap allocation on the
    /// render path. Trailing spaces pad it to [`LINE_WIDTH`] so a shorter value fully
    /// erases a longer one it replaces.
    fn line(
        &mut self,
        y: i32,
        color: Rgb565,
        content: core::fmt::Arguments,
    ) -> Result<(), St7789Error> {
        let mut buf: Line = Line::new();
        // Content is a handful of ASCII chars into a 16-byte buffer, so this cannot
        // overflow in practice; surface it as a fault rather than truncate silently.
        buf.write_fmt(content)
            .map_err(|_| St7789Error("ST7789 line: format buffer overflow".to_owned()))?;
        while buf.len() < LINE_WIDTH && buf.push(' ').is_ok() {}

        let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
            .font(&FONT_10X20)
            .text_color(color)
            .background_color(Rgb565::BLACK)
            .build();
        Text::with_baseline(buf.as_str(), Point::new(TEXT_X, y), style, Baseline::Top)
            .draw(&mut self.panel)
            .map_err(|e| fault("draw", e))?;
        Ok(())
    }
}

/// Render the latest measurement, or the unavailable state.
///
/// A fresh reading paints two lines — the raw ADC count and the percent; a `None`
/// (nothing measured yet, or a reading that aged out) paints an unavailable
/// placeholder in red, so a dead probe reads as unavailable on the glass rather than
/// as a frozen last value. Rendering only: the freshness decision was made upstream.
impl MoistureDisplay for St7789Display {
    type Error = St7789Error;

    fn show(&mut self, reading: Option<Measurement>) -> Result<(), St7789Error> {
        // No full-screen clear: each line paints its own row over an opaque
        // background (see `line`), so only the two text rows change and there is no
        // flash. The initial black background was laid down once in `new`. Numbers
        // are right-aligned in a fixed field so the digits hold a steady column as
        // the value's width changes.
        match reading {
            Some(m) => {
                self.line(RAW_Y, Rgb565::WHITE, format_args!("RAW  {:>4}", m.raw()))?;
                self.line(
                    PCT_Y,
                    Rgb565::WHITE,
                    format_args!("SOIL {:>3}%", m.percent()),
                )?;
            }
            None => {
                self.line(RAW_Y, Rgb565::RED, format_args!("SOIL --"))?;
                self.line(PCT_Y, Rgb565::RED, format_args!("no probe"))?;
            }
        }
        Ok(())
    }
}
