//! The host monitor's screen gallery — every state the glass can show, and how to raster one.
//!
//! One catalog, two consumers: the `screenshots` example renders it to `target/screens/` for
//! a human to eyeball, and the `goldens` integration test renders it and compares against the
//! committed reference PNGs so an *unintended* change to the picture fails the build. Both
//! `#[path]`-include this file, so the states and the rasterisation are defined **once** —
//! the example and the goldens can never drift.
//!
//! It lives under `examples/common/` (not `src/`) on purpose: it is std, uses `Vec`, and pulls
//! the `embedded-graphics-simulator` dev-dependency, none of which belong in the `no_std`
//! library that ships to the board. Cargo does not auto-compile files in an example
//! subdirectory without a `main.rs`, so this is a shared module, not an example of its own.

use platform_core::ScreenRotation;
use std::path::Path;

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use host_core::{HostFault, Pulse, PulseBuilder, Status};
use host_display::{HostState, SCREEN_SIZE};

/// The 240×135 panel is too small to read on a monitor; scale it up so a human — and a golden
/// diff — sees the alignment and the wording, not a postage stamp. Shared so the example and
/// the goldens raster at the identical size (byte-for-byte comparable).
pub const SCALE: u32 = 4;

/// The number of samples in a full window: `window_s / step_s + 1 = 900/30 + 1`.
pub const WINDOW_SAMPLES: usize = 31;

/// One captioned screen: the file it lands in, and the state to paint.
pub struct Screen {
    /// The PNG basename, e.g. `01-calm.png` — also the golden's name.
    pub file: &'static str,
    /// The state to render.
    pub state: HostState,
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

/// A frame with a run of gaps punched into fedora's CPU series — an up host whose scrape went
/// missing for a stretch. The gap must read as baseline ticks (no data), *not* as a floor of
/// `0%` bars: this is the scene that locks that distinction into a golden.
fn gappy(len: usize) -> Pulse {
    let mut b: PulseBuilder = PulseBuilder::new(30, 900);
    let (mut fc, fm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 60, 44);
    // Punch out the middle third of the CPU samples — a missing-scrape window.
    for sample in fc.iter_mut().skip(len / 3).take(len / 3) {
        *sample = None;
    }
    b.push("fedora", &fc, &fm);
    let (ac, am): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 12, 58);
    b.push("oracle-arm", &ac, &am);
    let (dc, dm): (Vec<Option<i32>>, Vec<Option<i32>>) = wave(len, 6, 22);
    b.push("oracle-amd", &dc, &dm);
    b.build()
}

/// Every state the glass can be in. Adding a state without adding it here means it ships
/// un-looked-at — and un-golden'd.
pub fn scenes() -> Vec<Screen> {
    let len: usize = WINDOW_SAMPLES;
    vec![
        // Calm: three lightly loaded hosts, fresh. The corner shows the 15m window span.
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
        // Gaps: an up host whose CPU scrape went missing for a stretch — baseline ticks, not
        // a floor of 0% bars.
        Screen {
            file: "07-gaps.png",
            state: HostState::new(Some(gappy(len)), Status::Fresh),
        },
    ]
}

/// Rasterise `state` to a PNG at [`SCALE`], through `host_display::render` — the very function
/// the ST7789 adapter calls on the board — so the file is the real layout, not a drawing of
/// it. Shared by the example and the goldens, so both produce byte-identical PNGs.
pub fn render_png(state: HostState, path: &Path) {
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(SCREEN_SIZE);
    host_display::render(&mut display, state, 0, ScreenRotation::Deg0)
        .expect("a framebuffer render cannot fail");
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();
    display
        .to_rgb_output_image(&settings)
        .save_png(path)
        .expect("save the PNG");
}
