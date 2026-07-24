//! The orientation readout's screen gallery — every pose the glass can show, and how to raster
//! one.
//!
//! One catalog, two consumers: the `orientation-screenshots` example renders it to
//! `target/screens/` for a human to eyeball, and the `goldens` integration test renders it and
//! compares against the committed reference PNGs so an *unintended* change to the picture fails
//! the build. Both `#[path]`-include this file, so the poses and the rasterisation are defined
//! **once** — the example and the goldens can never drift.
//!
//! It lives under `examples/common/` (not `src/`) on purpose: it is std, uses `Vec`, and pulls
//! the `embedded-graphics-simulator` dev-dependency, none of which belong in the `no_std`
//! library that ships to the board.

use std::path::Path;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::Size;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use orientation_core::{Orientation, Reading, SIGNAL_TIMEOUT_MS};
use orientation_display::{canvas_size, OrientationView};
use platform_core::{Acceleration, ScreenRotation, Tick, ONE_G_MG};

/// The 240×135 panel is too small to read on a monitor; scale it up.
pub const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, the reading that produces it, and how long ago
/// that reading was confirmed.
pub struct Screen {
    /// The PNG basename — also the golden's name.
    pub file: &'static str,
    /// The gravity vector this pose produces.
    pub acceleration: Acceleration,
    /// How stale the reading is. Zero for every live pose; past [`SIGNAL_TIMEOUT_MS`] for the
    /// one screen that shows a sensor which has stopped answering.
    pub age_ms: Tick,
    /// Which way up the picture is drawn — and therefore which of the two layouts, and which
    /// shape of canvas.
    pub rotation: ScreenRotation,
}

impl Screen {
    /// A live landscape screen — the reading was confirmed this instant.
    pub const fn live(file: &'static str, acceleration: Acceleration) -> Self {
        Screen {
            file,
            acceleration,
            age_ms: 0,
            rotation: ScreenRotation::Deg0,
        }
    }

    /// The same screen, drawn at `rotation`.
    pub const fn at(self, rotation: ScreenRotation) -> Self {
        Screen { rotation, ..self }
    }
}

/// Every pose the glass can show. Adding one without adding it here means it ships
/// un-looked-at.
pub fn scenes() -> Vec<Screen> {
    vec![
        Screen::live(
            "orientation-01-screen-up.png",
            Acceleration::new(0, 0, ONE_G_MG),
        ),
        Screen::live(
            "orientation-02-screen-down.png",
            Acceleration::new(0, 0, -ONE_G_MG),
        ),
        Screen::live(
            "orientation-03-upright.png",
            Acceleration::new(ONE_G_MG, 0, 0),
        ),
        Screen::live(
            "orientation-04-inverted.png",
            Acceleration::new(-ONE_G_MG, 0, 0),
        ),
        Screen::live(
            "orientation-05-left-edge.png",
            Acceleration::new(0, -ONE_G_MG, 0),
        ),
        Screen::live(
            "orientation-06-right-edge.png",
            Acceleration::new(0, ONE_G_MG, 0),
        ),
        // Between faces: the amber label, and two bars at rest mid-track.
        Screen::live("orientation-07-tilted.png", Acceleration::new(0, 707, 707)),
        // A gentle tilt that still names its face — the everyday case.
        Screen::live(
            "orientation-08-tilted-but-screen-up.png",
            Acceleration::new(-342, 0, 940),
        ),
        // Being picked up: not gravity, so no pose is named.
        Screen::live(
            "orientation-09-moving.png",
            Acceleration::new(300, -400, 1_900),
        ),
        // The sensor has stopped answering. The same reading as screen 01, drawn as a memory:
        // red NO SIGNAL over the face name, and the whole readout dimmed beneath it.
        Screen {
            file: "orientation-10-no-signal.png",
            acceleration: Acceleration::new(0, 0, ONE_G_MG),
            age_ms: SIGNAL_TIMEOUT_MS,
            rotation: ScreenRotation::Deg0,
        },
        // The other three quadrants. Screens 01–10 are all the panel's native landscape, which
        // is the quadrant a board held with its USB-C port to the right reads at; each of these
        // is drawn from the pose that actually settles to its rotation, so the gallery shows
        // the picture a reader would really be holding rather than a rotation asserted onto an
        // unrelated reading.
        //
        // A quarter turn: the board stood on its USB-C port, stick-top at the sky. The narrow
        // canvas, so the header stacks and the bars are less than half as long.
        Screen::live(
            "orientation-11-portrait-stick-top-up.png",
            Acceleration::new(ONE_G_MG, 0, 0),
        )
        .at(ScreenRotation::Deg270),
        // The other quarter turn: hung the other way up, USB-C at the sky. The same layout —
        // what differs between the two is the panel's scan order, which is below this crate,
        // so these two PNGs are expected to be identical.
        Screen::live(
            "orientation-12-portrait-usb-end-up.png",
            Acceleration::new(-ONE_G_MG, 0, 0),
        )
        .at(ScreenRotation::Deg90),
        // A half turn: still landscape, read from the other side.
        Screen::live(
            "orientation-13-landscape-inverted.png",
            Acceleration::new(0, ONE_G_MG, 0),
        )
        .at(ScreenRotation::Deg180),
        // The narrow canvas' hardest line: `NO SIGNAL` is nine characters of the thirteen a
        // portrait line holds, over a dimmed readout. If a field is going to run off an edge,
        // it is this one.
        Screen {
            file: "orientation-14-portrait-no-signal.png",
            acceleration: Acceleration::new(ONE_G_MG, 0, 0),
            age_ms: SIGNAL_TIMEOUT_MS,
            rotation: ScreenRotation::Deg270,
        },
    ]
}

/// Rasterise one screen to a PNG at [`SCALE`], through `orientation_display::render` — the very
/// function the ST7789 adapter calls on the board — so the file is the real layout, not a drawing
/// of it. Shared by the example and the goldens, so both produce byte-identical PNGs.
pub fn render_png(screen: &Screen, path: &Path) {
    let reading: Reading = Reading::aged(Orientation::of(screen.acceleration), screen.age_ms);
    let view: OrientationView = OrientationView::of(&reading);
    // Sized for the rotation, not for the panel: a landscape canvas would silently clip the
    // right-hand third of a portrait screen and save a PNG that looked fine.
    let canvas: Size = canvas_size(screen.rotation);
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(canvas);
    orientation_display::render(&mut display, view, 0, screen.rotation)
        .expect("a framebuffer render cannot fail");
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();
    display
        .to_rgb_output_image(&settings)
        .save_png(path)
        .expect("save the screenshot");
}
