//! Render every screen the plant monitor's glass can show, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/*.png
//! ```
//!
//! The pixels come from `plant_display::render` — the same function the ST7789
//! adapter calls on the board — drawn into a host framebuffer instead of down an SPI
//! bus. So a reviewer looks at the real layout, not at a drawing of it.
//!
//! **What these images do not show.** Everything below the `DrawTarget`: the panel's
//! colour order, its CGRAM offset, its inversion, its backlight. A host framebuffer
//! paints red as red however the glass is wired. To answer *that* question, flash
//! `just run-bin display-colour-check` and look at the board. See the crate docs.

use platform_core::ScreenRotation;
use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use plant_core::{Measurement, Moisture, Observation, ProbeFault};
use plant_display::SCREEN_SIZE;

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

/// The 240×135 panel is too small to read on a monitor; scale it up so a human can
/// actually check the alignment and the wording.
const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, what to paint there, and the way up.
struct Screen {
    file: &'static str,
    subject: Subject,
    rotation: ScreenRotation,
}

/// A landscape screen — the panel's native way up, and what most of these are.
fn flat(file: &'static str, subject: Subject) -> Screen {
    Screen {
        file,
        subject,
        rotation: ScreenRotation::Deg0,
    }
}

/// A portrait screen: the board stood on its USB-C port, drawn on the taller canvas.
fn turned(file: &'static str, subject: Subject) -> Screen {
    Screen {
        file,
        subject,
        rotation: ScreenRotation::Deg90,
    }
}

/// What to draw — an observation, or the colour self-test.
enum Subject {
    /// An observation, shown after it has been on the glass for this many ms. The
    /// elapsed value picks the creature's frame; a Fresh observation ignores it.
    Observation(Observation, u64),
    ColourBands,
}

fn measurement(raw: u16, percent: u8) -> Measurement {
    Measurement::new(raw, Moisture::new(percent).expect("percent is 0..=100"))
}

/// Every state the glass can be in. Adding a state to `Observation` without adding it
/// here means it ships un-looked-at.
fn screens() -> Vec<Screen> {
    vec![
        flat(
            "01-fresh-damp.png",
            Subject::Observation(Observation::Fresh(measurement(2048, 50)), 0),
        ),
        flat(
            "02-fresh-dry.png",
            Subject::Observation(Observation::Fresh(measurement(3900, 4)), 0),
        ),
        flat(
            "03-fresh-wet.png",
            Subject::Observation(Observation::Fresh(measurement(1180, 100)), 0),
        ),
        flat(
            "04-faulted-over-range.png",
            Subject::Observation(Observation::Faulted(ProbeFault::OverRange), 0),
        ),
        flat(
            "05-faulted-under-range.png",
            Subject::Observation(Observation::Faulted(ProbeFault::UnderRange), 0),
        ),
        flat(
            "06-faulted-unreadable.png",
            Subject::Observation(Observation::Faulted(ProbeFault::Unreadable), 0),
        ),
        flat("07-stale.png", Subject::Observation(Observation::Stale, 0)),
        flat(
            "08-never-sampled.png",
            Subject::Observation(Observation::NeverSampled, 0),
        ),
        // The same two unhealthy states, later in their creature's loop. One frame of an
        // animated state cannot show that it animates; these are the proof, and the pair a
        // reviewer compares against 04 and 07.
        flat(
            "09-faulted-over-range-mid-startle.png",
            Subject::Observation(Observation::Faulted(ProbeFault::OverRange), 1_300),
        ),
        flat(
            "10-stale-mid-sleep.png",
            Subject::Observation(Observation::Stale, 1_400),
        ),
        flat("11-colour-check.png", Subject::ColourBands),
        // Stood on the USB-C port. The narrow canvas has three columns of margin against
        // landscape's fourteen, so these cover the widest reading, the longest fault wording,
        // and the two states whose words are their whole content.
        turned(
            "12-portrait-fresh-damp.png",
            Subject::Observation(Observation::Fresh(measurement(2048, 50)), 0),
        ),
        turned(
            "13-portrait-fresh-wet.png",
            Subject::Observation(Observation::Fresh(measurement(1180, 100)), 0),
        ),
        turned(
            "14-portrait-faulted-unreadable.png",
            Subject::Observation(Observation::Faulted(ProbeFault::Unreadable), 0),
        ),
        turned(
            "15-portrait-never-sampled.png",
            Subject::Observation(Observation::NeverSampled, 0),
        ),
    ]
}

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    // Sized from the ROTATION, not from the panel: a portrait screen drawn into a landscape
    // target would be silently clipped at y=135 and the PNG would look like a layout bug.
    let mut display: SimulatorDisplay<Rgb565> =
        SimulatorDisplay::new(plant_display::canvas_size(screen.rotation));
    match screen.subject {
        Subject::Observation(observation, elapsed_ms) => {
            plant_display::render(&mut display, observation, elapsed_ms, screen.rotation)
                .expect("a framebuffer render cannot fail")
        }
        Subject::ColourBands => {
            plant_display::colour_bands(&mut display).expect("a framebuffer render cannot fail")
        }
    }
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
        "\n{} screens at {}×{} and {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        SCREEN_SIZE.width,
        SCREEN_SIZE.height,
        SCREEN_SIZE.height,
        SCREEN_SIZE.width
    );
}
