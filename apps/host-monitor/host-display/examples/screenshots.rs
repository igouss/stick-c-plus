//! Render every screen the host monitor's glass can show, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/*.png
//! ```
//!
//! The pixels come from `host_display::render` — the same function the ST7789 adapter
//! calls on the board — drawn into a host framebuffer instead of down an SPI bus. So a
//! reviewer looks at the real layout, not at a drawing of it.
//!
//! **What these images do not show.** Everything below the `DrawTarget`: the panel's
//! colour order, its CGRAM offset, its inversion, its backlight. A host framebuffer
//! paints red as red however the glass is wired. See the crate docs.

use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use host_core::{History, HostFault, Percent, Sample, Status};
use host_display::{HostState, SCREEN_SIZE};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

/// The 240×135 panel is too small to read on a monitor; scale it up so a human can
/// actually check the alignment and the wording.
const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, the state to paint, and how far into the
/// creature's animation to paint it (a still creature ignores the elapsed time).
struct Screen {
    file: &'static str,
    state: HostState,
    elapsed_ms: u64,
}

/// A percentage, or panic — the screenshots are authored with in-range constants.
fn pct(value: u8) -> Percent {
    Percent::new(value).expect("screenshot percent is 0..=100")
}

/// A history of `count` samples whose CPU rides a slow triangle wave peaking near
/// `cpu_peak` and whose memory holds near `mem`, so a graph shows real motion rather
/// than a flat line. Pure integer arithmetic, deterministic (no RNG).
fn wave(count: usize, cpu_peak: u8, mem: u8) -> History {
    let mut history: History = History::new();
    for i in 0..count {
        // A triangle wave in 0..=cpu_peak, period 40 samples.
        let phase: usize = i % 40;
        let up: usize = if phase < 20 { phase } else { 40 - phase };
        let cpu: u8 = (up as u32 * cpu_peak as u32 / 20) as u8;
        let mem_wobble: u8 = mem.saturating_add((i % 7) as u8);
        history.push(Sample::new(pct(cpu.min(100)), pct(mem_wobble.min(100))));
    }
    history
}

/// A fresh status at `cpu`/`mem`, for the label.
fn fresh(cpu: u8, mem: u8) -> Status {
    Status::Fresh(Sample::new(pct(cpu), pct(mem)))
}

/// Every state the glass can be in. Adding a state without adding it here means it ships
/// un-looked-at.
fn screens() -> Vec<Screen> {
    let full: usize = History::capacity();
    vec![
        // Calm: a lightly loaded host, graph mostly low, still breathing creature.
        Screen {
            file: "01-calm.png",
            state: HostState::new(wave(full, 35, 30), fresh(22, 34)),
            elapsed_ms: 0,
        },
        // Busy: working, not stressed — a still coding creature.
        Screen {
            file: "02-busy.png",
            state: HostState::new(wave(full, 75, 55), fresh(64, 58)),
            elapsed_ms: 0,
        },
        // Pegged: the host is hammered — the label goes red and the creature dances.
        Screen {
            file: "03-pegged.png",
            state: HostState::new(wave(full, 100, 88), fresh(97, 90)),
            elapsed_ms: 0,
        },
        // The same pegged state, mid-dance — one frame cannot show that it animates.
        Screen {
            file: "04-pegged-mid-dance.png",
            state: HostState::new(wave(full, 100, 88), fresh(97, 90)),
            elapsed_ms: 700,
        },
        // Faulted: the host stopped answering — startled creature, `--` labels, and the
        // trailing history still on the glass.
        Screen {
            file: "05-faulted.png",
            state: HostState::new(wave(full, 60, 50), Status::Faulted(HostFault::Unreachable)),
            elapsed_ms: 1_200,
        },
        // Stale: the poller stopped — asleep creature, history retained.
        Screen {
            file: "06-stale.png",
            state: HostState::new(wave(full, 60, 50), Status::Stale),
            elapsed_ms: 1_400,
        },
        // Never sampled: the graph is still filling and no CPU rate exists yet.
        Screen {
            file: "07-never-sampled.png",
            state: HostState::new(History::new(), Status::NeverSampled),
            elapsed_ms: 900,
        },
        // A partially filled graph — the window growing in from the left before it scrolls.
        Screen {
            file: "08-filling.png",
            state: HostState::new(wave(full / 3, 70, 45), fresh(48, 47)),
            elapsed_ms: 0,
        },
    ]
}

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SCREEN_SIZE);
    host_display::render(&mut display, screen.state, screen.elapsed_ms)
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
        "\n{} host-monitor screens at {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        SCREEN_SIZE.width,
        SCREEN_SIZE.height
    );
}
