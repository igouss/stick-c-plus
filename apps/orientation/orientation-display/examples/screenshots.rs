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
//! Each screen is built from a raw acceleration through the *real* `Orientation` transform,
//! so a PNG that looks wrong is evidence about the domain, not just about the layout.

use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use orientation_core::Orientation;
use orientation_display::{OrientationView, SCREEN_SIZE};
use platform_core::{Acceleration, ONE_G_MG};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

/// The 240×135 panel is too small to read on a monitor; scale it up.
const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, and the reading that produces it.
struct Screen {
    file: &'static str,
    acceleration: Acceleration,
}

/// Every pose the glass can show. Adding one without adding it here means it ships
/// un-looked-at.
fn screens() -> Vec<Screen> {
    vec![
        Screen {
            file: "orientation-01-screen-up.png",
            acceleration: Acceleration::new(0, 0, ONE_G_MG),
        },
        Screen {
            file: "orientation-02-screen-down.png",
            acceleration: Acceleration::new(0, 0, -ONE_G_MG),
        },
        Screen {
            file: "orientation-03-upright.png",
            acceleration: Acceleration::new(-ONE_G_MG, 0, 0),
        },
        Screen {
            file: "orientation-04-inverted.png",
            acceleration: Acceleration::new(ONE_G_MG, 0, 0),
        },
        Screen {
            file: "orientation-05-left-edge.png",
            acceleration: Acceleration::new(0, ONE_G_MG, 0),
        },
        Screen {
            file: "orientation-06-right-edge.png",
            acceleration: Acceleration::new(0, -ONE_G_MG, 0),
        },
        // Between faces: the amber label, and two bars at rest mid-track.
        Screen {
            file: "orientation-07-tilted.png",
            acceleration: Acceleration::new(0, 707, 707),
        },
        // A gentle tilt that still names its face — the everyday case.
        Screen {
            file: "orientation-08-tilted-but-screen-up.png",
            acceleration: Acceleration::new(-342, 0, 940),
        },
        // Being picked up: not gravity, so no pose is named.
        Screen {
            file: "orientation-09-moving.png",
            acceleration: Acceleration::new(300, -400, 1_900),
        },
    ]
}

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    let view: OrientationView = OrientationView::of(&Orientation::of(screen.acceleration));
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
