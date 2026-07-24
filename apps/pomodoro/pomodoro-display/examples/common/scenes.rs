//! The pomodoro timer's screen gallery — every state the glass can show, and how to raster one.
//!
//! One catalog, two consumers: the `pomodoro-screenshots` example renders it to `target/screens/`
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
use pomodoro_core::{Phase, Status};
use pomodoro_display::PomodoroView;

/// The 240×135 panel is too small to read on a monitor; scale it up so a human — and a golden
/// diff — sees the alignment and the wording, not a postage stamp. Shared so the example and the
/// goldens raster at the identical size (byte-for-byte comparable).
pub const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, the view to paint, the creature's clock, and the
/// way up it is drawn.
pub struct Screen {
    /// The PNG basename, e.g. `pomodoro-01-ready.png` — also the golden's name.
    pub file: &'static str,
    /// The view to render.
    pub view: PomodoroView,
    /// How far into the creature's animation loop this frame sits.
    pub elapsed_ms: u64,
    /// The way up the board is held.
    pub rotation: ScreenRotation,
}

/// A landscape screen — the panel's native way up, and what most of these are.
fn flat(file: &'static str, view: PomodoroView, elapsed_ms: u64) -> Screen {
    Screen {
        file,
        view,
        elapsed_ms,
        rotation: ScreenRotation::Deg0,
    }
}

/// A portrait screen: the board stood on its USB-C port, drawn on the taller canvas.
fn turned(file: &'static str, view: PomodoroView, elapsed_ms: u64) -> Screen {
    Screen {
        file,
        view,
        elapsed_ms,
        rotation: ScreenRotation::Deg90,
    }
}

fn view(phase: Phase, status: Status, remaining_secs: u32) -> PomodoroView {
    PomodoroView {
        phase,
        status,
        remaining_secs,
    }
}

/// Every state the glass can be in, in both shapes. Adding a state without adding it here
/// means it ships un-looked-at.
///
/// The portrait set is deliberately shorter than the landscape one: the animation and the
/// pause behaviour are properties of the *view*, which the two shapes share, so re-shooting
/// every frame of the creature's loop at a quarter turn would add files without adding a
/// question. What portrait has to answer is whether each element fits and reads on thirteen
/// columns — so it covers the widest label, the largest clock, and the finished state.
pub fn scenes() -> Vec<Screen> {
    vec![
        flat(
            "pomodoro-01-ready.png",
            view(Phase::Focus, Status::Idle, 25 * 60),
            0,
        ),
        flat(
            "pomodoro-02-focus.png",
            view(Phase::Focus, Status::Running, 24 * 60 + 37),
            400,
        ),
        // The same focus, later in the coding creature's loop — proof it animates.
        flat(
            "pomodoro-03-focus-mid-frame.png",
            view(Phase::Focus, Status::Running, 24 * 60 + 37),
            1_200,
        ),
        flat(
            "pomodoro-04-paused.png",
            view(Phase::Focus, Status::Paused, 12 * 60 + 30),
            0,
        ),
        flat(
            "pomodoro-05-short-break.png",
            view(Phase::ShortBreak, Status::Running, 5 * 60),
            400,
        ),
        flat(
            "pomodoro-06-long-break.png",
            view(Phase::LongBreak, Status::Running, 15 * 60),
            400,
        ),
        flat(
            "pomodoro-07-done.png",
            view(Phase::Focus, Status::Finished, 0),
            0,
        ),
        flat(
            "pomodoro-08-done-mid-wink.png",
            view(Phase::Focus, Status::Finished, 0),
            500,
        ),
        // Stood on the USB-C port. `LONG BREAK` is the widest label and `25:00` the largest
        // clock, so between them these three put every field at its full width on the narrow
        // canvas.
        turned(
            "pomodoro-09-portrait-focus.png",
            view(Phase::Focus, Status::Running, 24 * 60 + 37),
            400,
        ),
        turned(
            "pomodoro-10-portrait-long-break.png",
            view(Phase::LongBreak, Status::Running, 15 * 60),
            400,
        ),
        turned(
            "pomodoro-11-portrait-ready.png",
            view(Phase::Focus, Status::Idle, 25 * 60),
            0,
        ),
        turned(
            "pomodoro-12-portrait-done.png",
            view(Phase::Focus, Status::Finished, 0),
            0,
        ),
    ]
}

/// Rasterise one screen to a PNG at [`SCALE`], through `pomodoro_display::render` — the very
/// function the ST7789 adapter calls on the board — so the file is the real layout, not a
/// drawing of it. Shared by the example and the goldens, so both produce byte-identical PNGs.
pub fn render_png(screen: &Screen, path: &Path) {
    // Sized from the ROTATION, not from the panel: a portrait screen drawn into a landscape
    // target would be silently clipped at y=135 and the PNG would look like a layout bug.
    let mut display: SimulatorDisplay<Rgb565> =
        SimulatorDisplay::new(pomodoro_display::canvas_size(screen.rotation));
    pomodoro_display::render(
        &mut display,
        screen.view,
        screen.elapsed_ms,
        screen.rotation,
    )
    .expect("a framebuffer render cannot fail");
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();
    display
        .to_rgb_output_image(&settings)
        .save_png(path)
        .expect("save the screenshot");
}
