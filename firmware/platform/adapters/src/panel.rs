//! `panel` — the M5StickC Plus onboard ST7789 TFT, as a generic [`Screen`].
//!
//! [`Panel`] is the bring-up: it drives the 1.14″ ST7789V2 over SPI with `mipidsi`, applying
//! the panel's offsets, inversion and RGB colour order — and nothing else. [`PanelScreen`]
//! wraps a `Panel` as a [`platform_core::Screen`] of any app state `S`, with the app injecting
//! a render function that paints an `S` onto the panel's [`DrawTarget`]. So one panel adapter
//! serves the plant monitor's `Glass` and the pomodoro timer's view through the same port,
//! and neither this crate nor the panel knows which app it is drawing.
//!
//! What is left here — and all that should be — is the hardware: the SPI bus, the pins, and
//! the panel's own quirks. The picture lives in each app's display crate, drawn into any
//! `DrawTarget`, so `just screens` renders it on the host from the same code.
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
use platform_core::{Screen, ScreenRotation, Tick};
use platform_display::{RenderError, SCREEN_SIZE};
use static_cell::StaticCell;

use core::marker::PhantomData;

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
/// The pixel-batch buffer `mipidsi`'s SPI interface gathers writes into.
///
/// Every batch costs a bus transaction, so this sets how many of them a frame pays for:
/// `pixels * 2 / BUFFER_LEN`, rounded up per drawn primitive. 512 bytes (256 pixels) was
/// ample for the two text rows and a sprite the timer and the plant monitor paint, but the
/// orientation readout paints roughly 13 700 pixels a frame — three bars, five text fields —
/// and at 256 pixels a batch that measured **31 ms**, which overran its tick budget and left
/// the render thread no room to yield.
///
/// 4 KiB (2048 pixels) cuts that to a handful of transactions per frame. It costs 3.5 KiB
/// more of the ESP32's 520 KiB SRAM, statically, once — a cheap trade for a frame time that
/// leaves the scheduler headroom, and every app on the platform gets it.
const BUFFER_LEN: usize = 4096;

/// The pixel-batch buffer mipidsi's SPI interface borrows for the program's life.
/// A `StaticCell` gives it a truly `static` home — no allocator, no leaked `Box` —
/// initialised exactly once when the single display is built.
static SPI_BUFFER: StaticCell<[u8; BUFFER_LEN]> = StaticCell::new();

type BusDevice = SpiDeviceDriver<'static, SpiDriver<'static>>;
type Dc = PinDriver<'static, Output>;
type Rst = PinDriver<'static, Output>;
type Interface = SpiInterface<'static, BusDevice, Dc>;

/// The M5StickC Plus panel as an embedded-graphics [`DrawTarget`] — what an app's render
/// function draws into. Named `pub` so [`PanelScreen`] can bound that function over it
/// (`FnMut(&mut PanelTarget, S, Tick) -> ...`) without any caller spelling the full
/// mipidsi/esp-idf-hal type out.
pub type PanelTarget = Display<Interface, ST7789, Rst>;

/// A failure driving the ST7789, carrying its underlying cause for the log.
///
/// The panel's SPI and pin errors are `esp-idf-hal`'s own opaque newtypes, and
/// `mipidsi` composes them into an init/interface error that is only `Debug`; rather
/// than leak those types across the [`Screen`] port, this captures the
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

/// Flatten a render function's failure into this adapter's error.
///
/// A render function is generic in the target's error, so a panel bus failure arrives
/// wrapped; a line that would not fit its buffer is the renderer's own complaint. The
/// port carries neither type outward — only [`St7789Error`].
fn render_fault<E: core::fmt::Debug>(op: &str, err: RenderError<E>) -> St7789Error {
    match err {
        RenderError::Draw(err) => fault(op, err),
        RenderError::LineOverflow => St7789Error(format!("ST7789 {op}: line buffer overflow")),
    }
}

/// The panel rotation that shows the app's native landscape.
///
/// The panel is a 135×240 portrait part mounted sideways, so the picture every app has drawn
/// since the display landed is the controller turned a quarter turn. This is that quarter turn,
/// named once: it is the *zero* of the app-facing [`ScreenRotation`] scale, not of mipidsi's.
const NATIVE_LANDSCAPE: Rotation = Rotation::Deg90;

/// The mipidsi rotation that draws the picture at `rotation`.
///
/// The two scales share a step but not an origin. [`ScreenRotation::Deg0`] means "the panel's
/// native landscape", which is [`NATIVE_LANDSCAPE`] on the controller — so the app scale is the
/// controller scale shifted by a quarter turn, and this function is that shift.
///
/// **The direction of the shift is a hypothesis until the glass rules on it.** Both scales call
/// their steps "clockwise", but they are describing different things — mipidsi rotates the
/// *memory scan*, `ScreenRotation` rotates the *image* — and those run opposite ways whenever
/// the panel is mounted turned, which this one is. The precedent for not reasoning this out is
/// expensive and local: `ColorOrder` was derived correctly from the factory driver and was
/// still wrong on the glass (see the module docs).
///
/// So: this is the +90° reading. If `display-rotation-check` shows a clean, correctly placed
/// border whose `UP` points a quarter turn the wrong way, and the error is consistently the
/// *same* quarter turn at all four stops, the shift is inverted — swap the `Deg90` and `Deg270`
/// arms below and nothing else changes. If instead the border itself is broken, this mapping is
/// not the suspect; the offsets are.
const fn panel_rotation(rotation: ScreenRotation) -> Rotation {
    match rotation {
        ScreenRotation::Deg0 => NATIVE_LANDSCAPE,
        ScreenRotation::Deg90 => Rotation::Deg180,
        ScreenRotation::Deg180 => Rotation::Deg270,
        ScreenRotation::Deg270 => Rotation::Deg0,
    }
}

/// The M5StickC Plus onboard ST7789 TFT, brought up and ready to draw into.
pub struct Panel {
    target: PanelTarget,
    /// The rotation the controller is currently scanning at, in app terms.
    ///
    /// Held so [`Panel::set_rotation`] can skip a redundant `set_orientation`: it is a MADCTL
    /// write plus a full clear, and the render loop offers a rotation on *every* tick. Without
    /// this the panel would be reconfigured 25 times a second to the value it already had.
    showing: ScreenRotation,
}

impl Panel {
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
        // initialised once here (a second Panel::new would panic).
        let buffer: &'static mut [u8] = SPI_BUFFER.init([0u8; BUFFER_LEN]);
        let interface: Interface = SpiInterface::new(bus, dc, buffer);

        let mut delay: Ets = Ets;
        let mut target: PanelTarget = Builder::new(ST7789, interface)
            .display_size(PANEL_W, PANEL_H)
            .display_offset(OFFSET_X, OFFSET_Y)
            // The offsets above are the *native portrait* window, and they must stay that way:
            // mipidsi derives every other orientation's window from this pair together with
            // `display_size` and the model's framebuffer size, in `set_address_window`. Bake a
            // rotated offset in here and the derivation compounds it.
            .orientation(Orientation::new().rotate(NATIVE_LANDSCAPE))
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
        target.clear(Rgb565::BLACK).map_err(|e| fault("clear", e))?;

        Ok(Self {
            target,
            showing: ScreenRotation::Deg0,
        })
    }

    /// Turn the panel to `rotation`, if it is not already there.
    ///
    /// This is **hardware** rotation: it rewrites MADCTL so the controller scans its memory
    /// differently, and mipidsi recomputes the CGRAM window to match. The alternative — a
    /// rotating [`DrawTarget`] wrapper — was rejected because mapping rows onto columns
    /// destroys `fill_contiguous`, and at roughly 13 700 pixels a frame that is the difference
    /// between fitting the tick budget and missing it (see
    /// `kb/findings/mipidsi-rectangle-fill-costs-an-address-window.md`).
    ///
    /// A turn **clears the glass**. It has to: the canvas changes shape between landscape and
    /// portrait, so the pixels of the old picture are not merely stale but are in coordinates
    /// that no longer exist. Leaving them would show a fragment of the previous layout down the
    /// edge that the new one does not reach. The caller repaints immediately afterwards — the
    /// render loop's `Shown` carries the rotation, so a turn always forces a repaint — which
    /// keeps the black frame to a single tick.
    ///
    /// A no-op when the rotation has not changed, which is the overwhelmingly common case.
    pub fn set_rotation(&mut self, rotation: ScreenRotation) -> Result<(), St7789Error> {
        if rotation == self.showing {
            return Ok(());
        }
        self.target
            .set_orientation(Orientation::new().rotate(panel_rotation(rotation)))
            .map_err(|e| fault("set orientation", e))?;
        self.target
            .clear(Rgb565::BLACK)
            .map_err(|e| fault("clear after turn", e))?;
        self.showing = rotation;
        Ok(())
    }

    /// Paint the rotation self-test frame, labelled with the rotation it is drawn at.
    ///
    /// The sibling of [`Self::colour_check`], and evidence for the same reason: the picture is
    /// [`platform_display::rotation_frame`], but what makes it *proof* is that it is drawn
    /// through the production panel — this init, these offsets, this MADCTL — rather than into
    /// a host framebuffer that would place every pixel perfectly whatever the controller does.
    ///
    /// See `rotation_frame`'s own docs for how to read the glass; the short version is that the
    /// border answers "is the window right?" and `UP` answers "is it the right way up?", and
    /// they fail independently.
    pub fn rotation_check(&mut self, label: &str) -> Result<(), St7789Error> {
        platform_display::rotation_frame(&mut self.target, label)
            .map_err(|err| render_fault("rotation frame", err))
    }

    /// Bring-up self-test: paint the three primary bands on the real glass.
    ///
    /// The picture is [`platform_display::colour_bands`]; what this method contributes is
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
        platform_display::colour_bands(&mut self.target)
            .map_err(|err| render_fault("colour bands", err))
    }
}

/// Whether a screen's panel follows the rotation it is handed.
///
/// The rotation capability is **opt-in per binary**, and this is where opting in happens: not a
/// runtime flag but a type, chosen by the composition root. An app that will never turn
/// instantiates [`Fixed`], whose [`apply`](RotationPolicy::apply) is an empty function over a
/// zero-sized type — so the turn, the MADCTL write and the clear behind it are not merely
/// skipped at runtime, they are never generated.
///
/// That is the difference between "costs nothing to run" and "costs nothing at all", and it is
/// the requirement: an app that does not rotate must not pay for rotation in flash. Measured on
/// this board, an unconditional `set_rotation` call on the render path cost `host-monitor`
/// 744 bytes of text for a branch it could never take.
pub trait RotationPolicy {
    /// Bring `panel` to `rotation`, or do nothing at all.
    fn apply(panel: &mut Panel, rotation: ScreenRotation) -> Result<(), St7789Error>;
}

/// The panel never turns: it stays in the native landscape it was built in.
///
/// The default, and what every app did before rotation existed. Costs nothing.
pub struct Fixed;

impl RotationPolicy for Fixed {
    /// Deliberately empty. The rotation is still *offered* to the render function — an app can
    /// lay itself out differently without the panel turning — but the panel is left alone.
    fn apply(_panel: &mut Panel, _rotation: ScreenRotation) -> Result<(), St7789Error> {
        Ok(())
    }
}

/// The panel follows the rotation it is handed.
pub struct Turning;

impl RotationPolicy for Turning {
    fn apply(panel: &mut Panel, rotation: ScreenRotation) -> Result<(), St7789Error> {
        panel.set_rotation(rotation)
    }
}

/// The panel as a generic [`Screen`] of app state `S`.
///
/// It owns a [`Panel`] and a `render` function — `plant_display::render` for the plant
/// monitor, `pomodoro_display::render` for the timer — that the app supplies. On each
/// [`show`](Screen::show) it hands the panel's [`DrawTarget`] to that function, so *this* crate
/// stays app-agnostic: it knows how to bring the panel up and how to log a failure, and
/// nothing about what is drawn. The composition root builds the panel, injects the render
/// function, and hands the result to `platform_runtime::spawn_display`.
/// `P` is the [`RotationPolicy`] — [`Fixed`] unless the composition root asks for [`Turning`].
pub struct PanelScreen<S, F, P = Fixed>
where
    F: FnMut(&mut PanelTarget, S, Tick, ScreenRotation) -> Result<(), RenderError<PanelDrawError>>,
{
    panel: Panel,
    render: F,
    _state: PhantomData<S>,
    _rotation: PhantomData<P>,
}

/// The panel [`DrawTarget`]'s own error — an SPI/interface failure — that a render function
/// returns wrapped in a [`RenderError`].
type PanelDrawError = <PanelTarget as DrawTarget>::Error;

impl<S, F> PanelScreen<S, F, Fixed>
where
    F: FnMut(&mut PanelTarget, S, Tick, ScreenRotation) -> Result<(), RenderError<PanelDrawError>>,
{
    /// Wrap `panel` with the app's `render` function, leaving the panel in its native
    /// landscape however the board is held.
    ///
    /// The render function is still handed the rotation — an app may lay itself out for it —
    /// but the glass does not turn, and this binary links none of the machinery that would
    /// turn it. For an app that follows the board, use [`turning`](Self::turning).
    pub fn new(panel: Panel, render: F) -> Self {
        Self {
            panel,
            render,
            _state: PhantomData,
            _rotation: PhantomData,
        }
    }
}

impl<S, F> PanelScreen<S, F, Turning>
where
    F: FnMut(&mut PanelTarget, S, Tick, ScreenRotation) -> Result<(), RenderError<PanelDrawError>>,
{
    /// Wrap `panel` with the app's `render` function, turning the glass to whatever rotation
    /// it is handed.
    ///
    /// **This is the opt-in.** Choosing this constructor is what links the rotation path into a
    /// binary; choosing [`new`](Self::new) is what keeps it out. Pair it with a real rotation
    /// source at `spawn_display`, or the panel will faithfully hold the one constant rotation
    /// it is given forever.
    pub fn turning(panel: Panel, render: F) -> Self {
        Self {
            panel,
            render,
            _state: PhantomData,
            _rotation: PhantomData,
        }
    }
}

impl<S, F, P> Screen<S> for PanelScreen<S, F, P>
where
    F: FnMut(&mut PanelTarget, S, Tick, ScreenRotation) -> Result<(), RenderError<PanelDrawError>>,
    P: RotationPolicy,
{
    type Error = St7789Error;

    /// Hand the panel's draw target to the app's render function. What appears — the wording,
    /// the colours, the creature and its frame, the in-place erase — is the app display
    /// crate's decision, made once and reviewable on the host; this adapter's contribution is
    /// the panel it draws on.
    fn show(
        &mut self,
        state: S,
        elapsed: Tick,
        rotation: ScreenRotation,
    ) -> Result<(), St7789Error> {
        // Turn the panel *before* rendering, and in the same call. The order is the whole of
        // the correctness argument: turning changes the shape of the draw target, so a render
        // that ran first would lay the picture out for the canvas the panel is about to stop
        // being. Doing both here also means no app can do one without the other — the render
        // function is handed a target already scanning the right way, and simply draws.
        //
        // Under `Fixed` this call is an empty function on a zero-sized type and disappears
        // entirely; under `Turning` it is the MADCTL write. That is the opt-in, resolved at
        // compile time rather than paid for on every tick.
        P::apply(&mut self.panel, rotation)?;
        (self.render)(&mut self.panel.target, state, elapsed, rotation)
            .map_err(|err| render_fault("render", err))
    }
}
