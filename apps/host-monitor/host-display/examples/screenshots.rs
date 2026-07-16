//! Render every screen the host monitor's glass can show, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/*.png
//! ```
//!
//! The pixels come from `host_display::render` — the same function the ST7789 adapter calls
//! on the board — drawn into a host framebuffer instead of down an SPI bus. So a reviewer
//! looks at the real layout, not at a drawing of it.
//!
//! **What these images do not show.** Everything below the `DrawTarget`: the panel's colour
//! order, its CGRAM offset, its inversion, its backlight. A host framebuffer paints red as
//! red however the glass is wired. See the crate docs.

use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use host_core::{HostFault, Pulse, PulseBuilder, Status};
use host_display::{HostState, SCREEN_SIZE};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

/// The 240×135 panel is too small to read on a monitor; scale it up so a human can actually
/// check the alignment and the wording.
const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, and the state to paint.
struct Screen {
    file: &'static str,
    state: HostState,
}

/// A CPU/memory series that rides a slow triangle wave, so a graph shows real motion rather
/// than a flat line. `len` samples, CPU peaking near `cpu_peak`, memory holding near `mem`.
fn wave(len: usize, cpu_peak: i32, mem: i32) -> (Vec<Option<i32>>, Vec<Option<i32>>) {
    let cpu: Vec<Option<i32>> = (0..len)
        .map(|i: usize| {
            let phase: i32 = (i % 40) as i32;
            let up: i32 = if phase < 20 { phase } else { 40 - phase };
            Some((up * cpu_peak / 20).min(100))
        })
        .collect();
    let memory: Vec<Option<i32>> = (0..len)
        .map(|i: usize| Some((mem + (i % 7) as i32).min(100)))
        .collect();
    (cpu, memory)
}

/// The homelab's three hosts, each on its own wave, as one frame.
fn homelab(len: usize) -> Pulse {
    let mut b: PulseBuilder = PulseBuilder::new(30, 900);
    let (fc, fm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 40, 38);
    b.push("fedora", &fc, &fm);
    let (ac, am): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 12, 58);
    b.push("oracle-arm", &ac, &am);
    let (dc, dm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 6, 22);
    b.push("oracle-amd", &dc, &dm);
    b.build()
}

/// A pegged frame — fedora hammered, the others busy — so the red labels show.
fn pegged(len: usize) -> Pulse {
    let mut b: PulseBuilder = PulseBuilder::new(30, 900);
    let (fc, fm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 100, 90);
    b.push("fedora", &fc, &fm);
    let (ac, am): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 70, 62);
    b.push("oracle-arm", &ac, &am);
    let (dc, dm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 55, 40);
    b.push("oracle-amd", &dc, &dm);
    b.build()
}

/// A frame whose middle host is down (all-null) — the "no data" row.
fn one_host_down(len: usize) -> Pulse {
    let mut b: PulseBuilder = PulseBuilder::new(30, 900);
    let (fc, fm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 40, 38);
    b.push("fedora", &fc, &fm);
    b.push("oracle-arm", &vec![None; len], &vec![None; len]);
    let (dc, dm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 6, 22);
    b.push("oracle-amd", &dc, &dm);
    b.build()
}

/// Every state the glass can be in. Adding a state without adding it here means it ships
/// un-looked-at.
fn screens() -> Vec<Screen> {
    let len: usize = 31; // window_s / step_s + 1 = 900/30 + 1
    vec![
        // Calm: three lightly loaded hosts, fresh.
        Screen {
            file: "01-calm.png",
            state: HostState::new(Some(homelab(len)), Status::Fresh),
        },
        // Pegged: fedora hammered — its labels go red.
        Screen {
            file: "02-pegged.png",
            state: HostState::new(Some(pegged(len)), Status::Fresh),
        },
        // A single host down: its row shows "no data", the others keep drawing.
        Screen {
            file: "03-host-down.png",
            state: HostState::new(Some(one_host_down(len)), Status::Fresh),
        },
        // Faulted: the endpoint stopped answering — names tint red, a DOWN token, last frame
        // still on the glass.
        Screen {
            file: "04-faulted.png",
            state: HostState::new(Some(homelab(len)), Status::Faulted(HostFault::Unreachable)),
        },
        // Stale: the poller stopped — names dim, an OLD token, frame retained.
        Screen {
            file: "05-stale.png",
            state: HostState::new(Some(homelab(len)), Status::Stale),
        },
        // Never sampled: no frame yet — the waiting hint.
        Screen {
            file: "06-never-sampled.png",
            state: HostState::new(None, Status::NeverSampled),
        },
    ]
}

/// Paint one screen into a fresh framebuffer and save it.
fn capture(screen: &Screen, settings: &OutputSettings, out_dir: &Path) -> PathBuf {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SCREEN_SIZE);
    host_display::render(&mut display, screen.state, 0).expect("a framebuffer render cannot fail");
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
