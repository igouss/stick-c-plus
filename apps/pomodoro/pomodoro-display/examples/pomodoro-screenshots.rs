//! Render every screen the pomodoro timer's glass can show, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/pomodoro-*.png
//! ```
//!
//! The pixels come from `pomodoro_display::render` — the same function the ST7789 adapter
//! calls on the board — drawn into a host framebuffer instead of down an SPI bus. So a
//! reviewer looks at the real layout, not at a drawing of it.

use platform_core::ScreenRotation;
use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use pomodoro_core::{Phase, Status};
use pomodoro_display::{PomodoroView, SCREEN_SIZE};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

/// The 240×135 panel is too small to read on a monitor; scale it up.
const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, the view to paint, the creature's clock, and
/// the way up it is drawn.
struct Screen {
    file: &'static str,
    view: PomodoroView,
    elapsed_ms: u64,
    rotation: ScreenRotation,
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
fn screens() -> Vec<Screen> {
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

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
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
        "\n{} pomodoro screens at {}×{} and {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        SCREEN_SIZE.width,
        SCREEN_SIZE.height,
        SCREEN_SIZE.height,
        SCREEN_SIZE.width
    );
}
