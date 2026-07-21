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

/// One captioned screen: the file it lands in, and what to paint there.
struct Screen {
    file: &'static str,
    subject: Subject,
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
        Screen {
            file: "01-fresh-damp.png",
            subject: Subject::Observation(Observation::Fresh(measurement(2048, 50)), 0),
        },
        Screen {
            file: "02-fresh-dry.png",
            subject: Subject::Observation(Observation::Fresh(measurement(3900, 4)), 0),
        },
        Screen {
            file: "03-fresh-wet.png",
            subject: Subject::Observation(Observation::Fresh(measurement(1180, 100)), 0),
        },
        Screen {
            file: "04-faulted-over-range.png",
            subject: Subject::Observation(Observation::Faulted(ProbeFault::OverRange), 0),
        },
        Screen {
            file: "05-faulted-under-range.png",
            subject: Subject::Observation(Observation::Faulted(ProbeFault::UnderRange), 0),
        },
        Screen {
            file: "06-faulted-unreadable.png",
            subject: Subject::Observation(Observation::Faulted(ProbeFault::Unreadable), 0),
        },
        Screen {
            file: "07-stale.png",
            subject: Subject::Observation(Observation::Stale, 0),
        },
        Screen {
            file: "08-never-sampled.png",
            subject: Subject::Observation(Observation::NeverSampled, 0),
        },
        // The same two unhealthy states, later in their creature's loop. One frame of an
        // animated state cannot show that it animates; these are the proof, and the pair a
        // reviewer compares against 04 and 07.
        Screen {
            file: "09-faulted-over-range-mid-startle.png",
            subject: Subject::Observation(Observation::Faulted(ProbeFault::OverRange), 1_300),
        },
        Screen {
            file: "10-stale-mid-sleep.png",
            subject: Subject::Observation(Observation::Stale, 1_400),
        },
        Screen {
            file: "11-colour-check.png",
            subject: Subject::ColourBands,
        },
    ]
}

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SCREEN_SIZE);
    match screen.subject {
        Subject::Observation(observation, elapsed_ms) => {
            plant_display::render(&mut display, observation, elapsed_ms, ScreenRotation::Deg0)
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
        "\n{} screens at {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        SCREEN_SIZE.width,
        SCREEN_SIZE.height
    );
}
