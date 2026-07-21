#![forbid(unsafe_code)]
//! paint-profile — a bench tool that answers **where the paint budget goes, and whether the
//! board's orientation changes it**.
//!
//! ## The observation that prompted it
//!
//! With the timer running, serial reported occasional over-budget paints:
//!
//! ```text
//! W (21521) platform_runtime::display: platform-display: paint took 60.334ms, over the 50ms tick budget
//! ```
//!
//! Ten of them in sixty seconds, and the interesting part was not the count but the *spread*:
//! every sample landed between 60.31 ms and 60.94 ms. A 0.6 ms spread on a 60 ms measurement is
//! about one percent, which is far too tight for scheduler jitter or for another thread
//! stealing the core. Work that varies costs varying amounts; **work that is deterministic
//! costs the same every time**. So the paints that overran were not unlucky, they were doing
//! different work — and the thing that had just changed about this app was that its panel now
//! turns.
//!
//! ## Why the production log could not answer it
//!
//! The render loop times the whole paint and reports one number. That is the right instrument
//! for *noticing* the problem and useless for locating it: it cannot say which part of the
//! picture was slow, and it cannot say which rotation was showing, because by the time the
//! warning is printed the board has often been put down again.
//!
//! Worse, it reports by logging, and a `warn!` at 115200 baud is milliseconds of blocking UART
//! inside the region being measured. This tool never logs inside a timed region: it fills a
//! fixed array of samples and prints the summary afterwards.
//!
//! ## The method
//!
//! For each of the four rotations, paint [`SAMPLES`] frames and record how long each took, then
//! report the distribution. Two different pictures are profiled at every stop, because they
//! separate two different explanations:
//!
//! - **The timer's own screen** — two short text fields and an 80×80 creature. The creature is
//!   one large `fill_contiguous`, which is the operation the 2026-07-09 sprite experiment showed
//!   to be worth 85 ms when it degrades into 400 address windows and well under budget when it
//!   stays as one.
//! - **The rotation frame** — four thin edge strips and four small corner squares, and no large
//!   fill at all.
//!
//! If only the first is slow when turned, the cost is in the large contiguous fill: streaming
//! row-major in a rotated frame is being broken into many address windows. If both are slow, the
//! cost is in the turned panel generally — every window setup, not the big one. If neither is,
//! the rotation is innocent and the next suspect is the sprite's own animation.
//!
//! Every frame is forced to differ from the one before it — the clock advances a second and the
//! creature's animation clock moves — so no paint can be skipped as unchanged, and `SAMPLES`
//! paints really are `SAMPLES` paints.
//!
//! ## Using it
//!
//! ```sh
//! just run-bin-pomodoro paint-profile    # leave it on the desk; it needs no hands
//! just run-pomodoro                      # put the timer back
//! ```
//!
//! It sets each rotation itself and never reads the IMU, so the result is a property of the
//! panel and the picture rather than of how the board happened to be held. Read the `median`
//! column against the 50 ms animation budget; `max` catches a cost that only some frames pay.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use board_support::{internal_i2c, Axp192};
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{info, warn};
use platform_adapters::{Panel, PanelScreen, Turning};
use platform_core::{Screen, ScreenRotation, Tick};
use pomodoro_core::{Phase, Status};
use pomodoro_display::PomodoroView;

/// Paints timed at each rotation, for each picture.
///
/// Enough that a median means something and a rare expensive frame still shows up in `max`,
/// and small enough that the whole sweep is over in a few seconds and the samples fit in a
/// fixed array — no allocation on the measured path.
const SAMPLES: usize = 60;

/// The budget the timer's animated screen is actually held to — `ANIMATION_PERIOD` in
/// `platform-runtime`. Reported alongside the numbers so the summary is self-contained.
const BUDGET: Duration = Duration::from_millis(50);

/// The four stops, named as they are logged.
const STOPS: [(ScreenRotation, &str); 4] = [
    (ScreenRotation::Deg0, "DEG0   (landscape)"),
    (ScreenRotation::Deg90, "DEG90  (portrait) "),
    (ScreenRotation::Deg180, "DEG180 (landscape)"),
    (ScreenRotation::Deg270, "DEG270 (portrait) "),
];

/// A view whose clock and creature frame both differ from the previous sample's, so the picture
/// genuinely changes on every paint.
fn sample_view(index: usize) -> PomodoroView {
    PomodoroView {
        phase: Phase::Focus,
        status: Status::Running,
        // Counts down a second at a time: the `MM:SS` field differs from the last frame's.
        remaining_secs: (25 * 60) - index as u32,
    }
}

/// The creature's animation clock for sample `index` — stepped well past one frame's hold, so
/// consecutive samples draw different sprite frames rather than the same one twice.
fn sample_elapsed(index: usize) -> Tick {
    index as Tick * 200
}

/// Min, median and max of a set of samples — the three numbers worth reading.
///
/// A median rather than a mean: one 200 ms outlier from an unlucky preemption would drag a mean
/// somewhere misleading, and the question here is what a *typical* frame costs.
struct Summary {
    min: Duration,
    median: Duration,
    max: Duration,
}

impl Summary {
    fn of(samples: &mut [Duration]) -> Summary {
        samples.sort_unstable();
        Summary {
            min: samples[0],
            median: samples[samples.len() / 2],
            max: samples[samples.len() - 1],
        }
    }

    /// The summary as one aligned line, with a verdict against the budget.
    fn line(&self, label: &str) -> String {
        format!(
            "{label}  min {:>7.2?}  median {:>7.2?}  max {:>7.2?}   {}",
            self.min,
            self.median,
            self.max,
            if self.median > BUDGET {
                "OVER BUDGET"
            } else if self.max > BUDGET {
                "over on some frames"
            } else {
                "fits"
            }
        )
    }
}

/// Time [`SAMPLES`] paints of the timer's own screen at `rotation`.
///
/// The screen is handed the rotation on every call, exactly as the render loop hands it one, so
/// this measures the production path — `PanelScreen::show` under [`Turning`], including the
/// MADCTL write it makes when the rotation actually changes.
fn profile_timer_screen<S>(screen: &mut S, rotation: ScreenRotation) -> Summary
where
    S: Screen<PomodoroView>,
    S::Error: core::fmt::Debug,
{
    // One warm-up paint, untimed: it carries the MADCTL write and the clear that a *change* of
    // rotation costs, which is a real cost but a once-per-turn one, and averaging it into every
    // frame would answer a question nobody asked.
    screen
        .show(sample_view(0), 0, rotation)
        .expect("warm-up paint");

    let mut samples: [Duration; SAMPLES] = [Duration::ZERO; SAMPLES];
    (0..SAMPLES).for_each(|index: usize| {
        let view: PomodoroView = sample_view(index + 1);
        let elapsed: Tick = sample_elapsed(index + 1);
        let started: Instant = Instant::now();
        screen
            .show(view, elapsed, rotation)
            .expect("paint the timer screen");
        samples[index] = started.elapsed();
    });
    Summary::of(&mut samples)
}

/// Time [`SAMPLES`] paints of the rotation frame at `rotation` — the picture with no large fill.
fn profile_rotation_frame(panel: &mut Panel, rotation: ScreenRotation) -> Summary {
    panel.set_rotation(rotation).expect("turn the panel");
    panel.rotation_check("WARM").expect("warm-up paint");

    let mut samples: [Duration; SAMPLES] = [Duration::ZERO; SAMPLES];
    (0..SAMPLES).for_each(|index: usize| {
        // A different label each time, so the text field really is redrawn.
        let label: String = format!("N{index:03}");
        let started: Instant = Instant::now();
        panel
            .rotation_check(&label)
            .expect("paint the rotation frame");
        samples[index] = started.elapsed();
    });
    Summary::of(&mut samples)
}

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("paint-profile: {SAMPLES} timed paints per picture, at each of four rotations");
    info!("budget for the timer's animated screen is {BUDGET:?}");

    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // The panel is dark until the AXP192 lights LDO2/LDO3 — the same order the timer uses, since
    // a measurement taken off the production bring-up path would not be evidence about it.
    {
        let i2c = internal_i2c(
            peripherals.i2c0,
            peripherals.pins.gpio21,
            peripherals.pins.gpio22,
        )
        .expect("internal I2C bring-up");
        let i2c_bus: RefCell<_> = RefCell::new(i2c);
        let mut axp: Axp192<_> = Axp192::new(RefCellDevice::new(&i2c_bus));
        axp.power_on().expect("AXP192 LCD/TFT rail power-on");
        match axp.display_rails_enabled() {
            Ok(true) => info!("axp192: LCD/TFT rails enabled"),
            Ok(false) => warn!("axp192: rails did not read back as enabled"),
            Err(err) => warn!("axp192: rail read-back failed: {err}"),
        }
    }

    let panel: Panel = Panel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 panel bring-up");

    // The no-large-fill picture goes first, while the panel is still owned directly — that
    // ordering is only so this tool never needs to take the panel back out of the screen.
    let mut panel: Panel = panel;
    info!("--- the rotation frame: four edge strips and four corner squares (no large fill) ---");
    let frame_lines: Vec<String> = STOPS
        .into_iter()
        .map(|(rotation, label): (ScreenRotation, &str)| {
            profile_rotation_frame(&mut panel, rotation).line(label)
        })
        .collect();
    frame_lines.iter().for_each(|line: &String| info!("{line}"));

    // `turning`, as the timer itself now takes it — the point is to profile that path.
    let mut screen: PanelScreen<PomodoroView, _, Turning> = PanelScreen::turning(
        panel,
        |target: &mut _, view: PomodoroView, elapsed: Tick, rotation: ScreenRotation| {
            pomodoro_display::render(target, view, elapsed, rotation)
        },
    );

    info!("--- the timer's screen: two text fields and an 80x80 creature (one large fill) ---");
    let timer_lines: Vec<String> = STOPS
        .into_iter()
        .map(|(rotation, label): (ScreenRotation, &str)| {
            profile_timer_screen(&mut screen, rotation).line(label)
        })
        .collect();
    timer_lines.iter().for_each(|line: &String| info!("{line}"));

    info!("--- how to read it ---");
    info!("only the timer screen slow when turned -> the large contiguous fill is being broken");
    info!("   into many address windows in the rotated frame (see the 2026-07-09 sprite study)");
    info!("both slow when turned -> the turned panel costs more per window, not per pixel");
    info!("neither slow -> rotation is innocent; suspect the sprite's animation itself");

    loop {
        FreeRtos::delay_ms(5_000);
    }
}
