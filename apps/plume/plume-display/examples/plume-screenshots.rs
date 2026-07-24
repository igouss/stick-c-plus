//! Render several phases of the plume's breath, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/plume-*.png
//! ```
//!
//! The pixels come from `plume_display::Plume::render` — the same code the ST7789 adapter calls
//! on the board — drawn into a host framebuffer instead of down an SPI bus. So a reviewer looks
//! at the real frond at the real 135×240, one still per phase, rather than at a drawing of it.
//! A gallery of stills cannot show motion, but placed side by side the sweep of the barbs
//! between phases is plain.

use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use platform_core::{ScreenRotation, Tick};
use plume_core::FRAME_MS;
use plume_display::{canvas_size, Plume, PlumeView};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

/// The 135×240 panel is small on a monitor; scale it up.
const SCALE: u32 = 3;

/// The plume is a portrait picture — the board stood on its USB-C port.
const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

/// How many stills to render, and how many frames of breath apart. Spread across roughly a
/// third of the period, so consecutive stills differ visibly without the gallery becoming a
/// filmstrip.
const STILLS: u64 = 6;
const FRAMES_APART: u64 = 24;

/// Paint the frond at `elapsed` into a fresh portrait framebuffer and save it as `file`.
fn capture(file: &str, elapsed: Tick, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(canvas_size(PORTRAIT));
    Plume::new()
        .render(&mut display, PlumeView, elapsed, PORTRAIT)
        .expect("a framebuffer render cannot fail");
    let path: PathBuf = out_dir.join(file);
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

    let written: Vec<PathBuf> = (0..STILLS)
        .map(|n: u64| {
            let elapsed: Tick = n * FRAMES_APART * FRAME_MS;
            capture(
                &format!("plume-{:02}-frame-{:03}.png", n + 1, n * FRAMES_APART),
                elapsed,
                &settings,
                out_dir,
            )
        })
        .collect();

    written
        .iter()
        .for_each(|path: &PathBuf| println!("{}", path.display()));
    let canvas = canvas_size(PORTRAIT);
    println!(
        "\n{} plume phases at {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        canvas.width,
        canvas.height
    );
}
