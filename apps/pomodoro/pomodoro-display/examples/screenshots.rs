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

/// One captioned screen: the file it lands in, the view to paint, and the creature's clock.
struct Screen {
    file: &'static str,
    view: PomodoroView,
    elapsed_ms: u64,
}

fn view(phase: Phase, status: Status, remaining_secs: u32) -> PomodoroView {
    PomodoroView {
        phase,
        status,
        remaining_secs,
    }
}

/// Every state the glass can be in. Adding a state without adding it here means it ships
/// un-looked-at.
fn screens() -> Vec<Screen> {
    vec![
        Screen {
            file: "pomodoro-01-ready.png",
            view: view(Phase::Focus, Status::Idle, 25 * 60),
            elapsed_ms: 0,
        },
        Screen {
            file: "pomodoro-02-focus.png",
            view: view(Phase::Focus, Status::Running, 24 * 60 + 37),
            elapsed_ms: 400,
        },
        // The same focus, later in the coding creature's loop — proof it animates.
        Screen {
            file: "pomodoro-03-focus-mid-frame.png",
            view: view(Phase::Focus, Status::Running, 24 * 60 + 37),
            elapsed_ms: 1_200,
        },
        Screen {
            file: "pomodoro-04-paused.png",
            view: view(Phase::Focus, Status::Paused, 12 * 60 + 30),
            elapsed_ms: 0,
        },
        Screen {
            file: "pomodoro-05-short-break.png",
            view: view(Phase::ShortBreak, Status::Running, 5 * 60),
            elapsed_ms: 400,
        },
        Screen {
            file: "pomodoro-06-long-break.png",
            view: view(Phase::LongBreak, Status::Running, 15 * 60),
            elapsed_ms: 400,
        },
        Screen {
            file: "pomodoro-07-done.png",
            view: view(Phase::Focus, Status::Finished, 0),
            elapsed_ms: 0,
        },
        Screen {
            file: "pomodoro-08-done-mid-wink.png",
            view: view(Phase::Focus, Status::Finished, 0),
            elapsed_ms: 500,
        },
    ]
}

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SCREEN_SIZE);
    pomodoro_display::render(
        &mut display,
        screen.view,
        screen.elapsed_ms,
        ScreenRotation::Deg0,
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
        "\n{} pomodoro screens at {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        SCREEN_SIZE.width,
        SCREEN_SIZE.height
    );
}
