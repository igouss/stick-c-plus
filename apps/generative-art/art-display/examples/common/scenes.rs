//! The gallery's catalog — the stills every screen is pinned at, and how to raster one.
//!
//! One catalog, two consumers: the `gallery-screenshots` example renders it to `target/screens/`
//! for a human to eyeball, and the `goldens` integration test renders it and compares against the
//! committed reference PNGs so an *unintended* change to any screen fails the build. Both
//! `#[path]`-include this file, so the stills and the rasterisation are defined **once** — the
//! example and the goldens can never drift.
//!
//! The catalog samples the plume across its breath (a clock-driven animation, so a set of stills
//! is the only way to pin its shape) and takes one still of each not-yet-built sketch's
//! placeholder (static, so one frame tells the whole story). When a sketch's real rasterisation
//! lands, its still stops being a placeholder and the golden is re-blessed — the bless is the
//! human's record that the picture changed on purpose.

use std::path::Path;

use art_core::Sketch;
use art_display::{canvas_size, Frame, Gallery, GalleryView};
use embedded_graphics::pixelcolor::Rgb565;
// The stills are sampled by *phase*-frame, not by the repaint cadence: a whole `FRAME_MS` is one
// step of the source's motion, and on that boundary the continuous [`plume_core::phase`] lands
// exactly on the source's value — so each still is a canonical frame of the sweep, not an
// interpolated in-between.
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use platform_core::{ScreenRotation, Tick};
use plume_core::FRAME_MS;

/// The 135×240 panel is small on a monitor; scale it up.
pub const SCALE: u32 = 3;

/// The gallery is a portrait picture — the board stood on its USB-C port.
pub const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

/// How many stills of the plume's breath to render, and how many frames apart — enough to see the
/// frond move without the gallery becoming a filmstrip.
pub const PLUME_STILLS: u64 = 4;
pub const FRAMES_APART: u64 = 24;

/// One still: the file it lands in, the sketch it shows, and how far into that sketch's motion it
/// is taken.
pub struct Screen {
    /// The PNG basename, also the golden's name. Owned, not `&'static str`, because the plume's
    /// frame number is computed rather than written out.
    pub file: String,
    /// Which piece this still shows.
    pub sketch: Sketch,
    /// How far into the animation this still is taken.
    pub elapsed: Tick,
}

/// The stills of the gallery: the plume across its breath, then one of each unbuilt placeholder.
/// Adding or moving one changes which moments are looked at — and, through the goldens, which are
/// pinned.
pub fn scenes() -> Vec<Screen> {
    let mut screens: Vec<Screen> = (0..PLUME_STILLS)
        .map(|n: u64| Screen {
            file: format!("art-plume-{:02}-frame-{:03}.png", n + 1, n * FRAMES_APART),
            sketch: Sketch::Plume,
            elapsed: n * FRAMES_APART * FRAME_MS,
        })
        .collect();
    for sketch in [Sketch::Squares, Sketch::Fan, Sketch::Orbits, Sketch::Willow] {
        screens.push(Screen {
            file: format!("art-{}.png", slug(sketch)),
            sketch,
            elapsed: 0,
        });
    }
    screens
}

/// A filename-safe lowercase name for a sketch.
fn slug(sketch: Sketch) -> &'static str {
    match sketch {
        Sketch::Plume => "plume",
        Sketch::Squares => "squares",
        Sketch::Fan => "fan",
        Sketch::Orbits => "orbits",
        Sketch::Willow => "willow",
    }
}

/// Rasterise one still to a PNG at [`SCALE`], through `Gallery::paint_into` then a blit — the same
/// paint the ST7789 adapter drives on the board, into the same [`Frame`] the host uses — so the
/// file is the real picture, not a drawing of it. Shared by the example and the goldens, so both
/// produce byte-identical PNGs.
pub fn render_png(screen: &Screen, path: &Path) {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(canvas_size(PORTRAIT));
    let mut frame: Frame = Frame::new();
    Gallery::new().paint_into(
        &mut frame,
        GalleryView::new(screen.sketch),
        screen.elapsed,
        PORTRAIT,
    );
    frame
        .blit(&mut display)
        .expect("a framebuffer blit cannot fail");
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();
    display
        .to_rgb_output_image(&settings)
        .save_png(path)
        .expect("save the screenshot");
}
