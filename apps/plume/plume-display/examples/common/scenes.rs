//! The plume's gallery — the stills of one breath, and how to raster one.
//!
//! One catalog, two consumers: the `plume-screenshots` example renders it to `target/screens/`
//! for a human to eyeball, and the `goldens` integration test renders it and compares against the
//! committed reference PNGs so an *unintended* change to the picture fails the build. Both
//! `#[path]`-include this file, so the phases and the rasterisation are defined **once** — the
//! example and the goldens can never drift.
//!
//! The plume has no state to enumerate: it is a clock-driven animation, so the catalog is a set
//! of stills sampled across the breath. That is what makes it checkable at all — the goldens pin
//! the SHAPE of the frond at fixed points of the period, so a change to the maths that moves the
//! fronds fails the build even though nothing about the app is stateful.

use std::path::Path;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use platform_core::{ScreenRotation, Tick};
use plume_core::FRAME_MS;
use plume_display::{canvas_size, Plume, PlumeView};

/// The 135×240 panel is small on a monitor; scale it up.
pub const SCALE: u32 = 3;

/// The plume is a portrait picture — the board stood on its USB-C port.
pub const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

/// How many stills to render, and how many frames of breath apart. Spread across roughly a
/// third of the period, so consecutive stills differ visibly without the gallery becoming a
/// filmstrip.
pub const STILLS: u64 = 6;
pub const FRAMES_APART: u64 = 24;

/// One still: the file it lands in, and how far into the breath it sits.
pub struct Screen {
    /// The PNG basename, e.g. `plume-01-frame-000.png` — also the golden's name. Owned, not
    /// `&'static str`, because the frame number is computed rather than written out.
    pub file: String,
    /// How far into the animation this still is taken.
    pub elapsed: Tick,
}

/// The stills of one breath, evenly spaced. Adding or moving one changes which moments are
/// looked at — and, through the goldens, which are pinned.
pub fn scenes() -> Vec<Screen> {
    (0..STILLS)
        .map(|n: u64| Screen {
            file: format!("plume-{:02}-frame-{:03}.png", n + 1, n * FRAMES_APART),
            elapsed: n * FRAMES_APART * FRAME_MS,
        })
        .collect()
}

/// Rasterise one still to a PNG at [`SCALE`], through `Plume::render` — the very function the
/// ST7789 adapter calls on the board — so the file is the real picture, not a drawing of it.
/// Shared by the example and the goldens, so both produce byte-identical PNGs.
pub fn render_png(screen: &Screen, path: &Path) {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(canvas_size(PORTRAIT));
    Plume::new()
        .render(&mut display, PlumeView, screen.elapsed, PORTRAIT)
        .expect("a framebuffer render cannot fail");
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();
    display
        .to_rgb_output_image(&settings)
        .save_png(path)
        .expect("save the screenshot");
}
