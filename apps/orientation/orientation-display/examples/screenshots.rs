//! Render every pose the orientation readout's glass can show, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/orientation-*.png
//! ```
//!
//! The pixels come from `orientation_display::render` — the same function the ST7789 adapter
//! calls on the board — drawn into a host framebuffer instead of down an SPI bus. So a
//! reviewer looks at the real layout, not at a drawing of it.
//!
//! Each screen is built from a raw acceleration through the *real* `Orientation` transform and
//! the *real* staleness rule, so a PNG that looks wrong is evidence about the domain, not just
//! about the layout.

use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use orientation_core::{Orientation, Reading, SIGNAL_TIMEOUT_MS};
use orientation_display::{OrientationView, SCREEN_SIZE};
use platform_core::{Acceleration, Tick, ONE_G_MG};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

/// The 240×135 panel is too small to read on a monitor; scale it up.
const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, the reading that produces it, and how long ago
/// that reading was confirmed.
struct Screen {
    file: &'static str,
    acceleration: Acceleration,
    /// How stale the reading is. Zero for every live pose; past [`SIGNAL_TIMEOUT_MS`] for the
    /// one screen that shows a sensor which has stopped answering.
    age_ms: Tick,
}

impl Screen {
    /// A live screen — the reading was confirmed this instant.
    const fn live(file: &'static str, acceleration: Acceleration) -> Self {
        Screen {
            file,
            acceleration,
            age_ms: 0,
        }
    }
}

/// Every pose the glass can show. Adding one without adding it here means it ships
/// un-looked-at.
fn screens() -> Vec<Screen> {
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
        },
    ]
}

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    let reading: Reading = Reading::aged(Orientation::of(screen.acceleration), screen.age_ms);
    let view: OrientationView = OrientationView::of(&reading);
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SCREEN_SIZE);
    orientation_display::render(&mut display, view, 0).expect("a framebuffer render cannot fail");
    let path: PathBuf = out_dir.join(screen.file);
    display
        .to_rgb_output_image(settings)
        .save_png(&path)
        .expect("save the screenshot");
    path
}

fn main() {
    let out_dir: &Path = Path::new(OUT_DIR);
    fs::create_dir_all(out_dir).expect("create the screenshot directory");
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();

    let written: Vec<PathBuf> = screens()
        .iter()
        .map(|screen: &Screen| capture(screen, &settings, out_dir))
        .collect();

    written
        .iter()
        .for_each(|path: &PathBuf| println!("{}", path.display()));
    println!(
        "\n{} orientation screens at {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        SCREEN_SIZE.width,
        SCREEN_SIZE.height
    );
}
