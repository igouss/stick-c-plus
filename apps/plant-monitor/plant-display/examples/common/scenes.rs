//! The plant monitor's screen gallery — every state the glass can show, and how to raster one.
//!
//! One catalog, two consumers: the `plant-screenshots` example renders it to `target/screens/`
//! for a human to eyeball, and the `goldens` integration test renders it and compares against the
//! committed reference PNGs so an *unintended* change to the picture fails the build. Both
//! `#[path]`-include this file, so the states and the rasterisation are defined **once** — the
//! example and the goldens can never drift.
//!
//! It lives under `examples/common/` (not `src/`) on purpose: it is std, uses `Vec`, and pulls
//! the `embedded-graphics-simulator` dev-dependency, none of which belong in the `no_std`
//! library that ships to the board. Cargo does not auto-compile files in an example
//! subdirectory without a `main.rs`, so this is a shared module, not an example of its own.

use platform_core::ScreenRotation;
use std::path::Path;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use plant_core::{Measurement, Moisture, Observation, ProbeFault};

/// The 240×135 panel is too small to read on a monitor; scale it up so a human can
/// actually check the alignment and the wording.
pub const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, what to paint there, and the way up.
pub struct Screen {
    /// The PNG basename, e.g. `01-fresh-damp.png` — also the golden's name.
    pub file: &'static str,
    /// What to paint.
    pub subject: Subject,
    /// The way up the board is held.
    pub rotation: ScreenRotation,
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
pub enum Subject {
    /// An observation, shown after it has been on the glass for this many ms. The
    /// elapsed value picks the creature's frame; a Fresh observation ignores it.
    Observation(Observation, u64),
    /// The RGB colour-check bands.
    ColourBands,
}

fn measurement(raw: u16, percent: u8) -> Measurement {
    Measurement::new(raw, Moisture::new(percent).expect("percent is 0..=100"))
}

/// Every state the glass can be in. Adding a state to `Observation` without adding it
/// here means it ships un-looked-at.
pub fn scenes() -> Vec<Screen> {
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

/// Rasterise one screen to a PNG at [`SCALE`], through `plant_display::render` — the very
/// function the ST7789 adapter calls on the board — so the file is the real layout, not a
/// drawing of it. Shared by the example and the goldens, so both produce byte-identical PNGs.
pub fn render_png(screen: &Screen, path: &Path) {
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
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();
    display
        .to_rgb_output_image(&settings)
        .save_png(path)
        .expect("save the screenshot");
}
