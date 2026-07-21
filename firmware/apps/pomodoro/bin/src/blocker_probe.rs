#![forbid(unsafe_code)]
//! blocker-probe — a bench tool that answers **which of the timer's own threads blocks a
//! paint**, by taking them away one at a time.
//!
//! ## What is already known, and what is not
//!
//! `paint-profile` measured the timer's screen at **21.3 ms** against its 50 ms budget, identical
//! at all four rotations across 240 samples. Production reports **60.4 ms** in about 0.8% of
//! frames. So the paint is not slow — it is *blocked*, by roughly 39 ms, and the spread across
//! production samples was only 0.63 ms, which is far too tight for jitter. Something
//! deterministic takes the core.
//!
//! `paint-profile` cannot say what, and not by accident: it runs with **none** of the app's other
//! threads, which is exactly why it reads clean, and is also precisely the suspect list. This
//! tool is the other half — it brings those threads back.
//!
//! ## The method: subtract, do not add
//!
//! The bench starts as a **replica of production** — the deliberate jingle, the input poll, the
//! power watch, the rotation sampler — and then stops one thread per stage, in order of
//! suspicion. Each stage is one thread different from the one before it, so a distribution that
//! recovers between two stages names the thread that was removed between them.
//!
//! Subtractive rather than additive because the threads own their peripherals outright: the IMU
//! moves into the rotation thread, the PMIC into the power watch. They can be stopped and cannot
//! be restarted, so the sweep must run downhill.
//!
//! ```text
//!   1  production    jingle + input + power-watch + rotation
//!   2  - input               jingle + power-watch + rotation
//!   3  - power-watch                  jingle + rotation
//!   4  - rotation                              jingle          <- the suspect, alone
//!   5  - jingle                                (nothing)       <- must read 21.3 ms
//!   6  the display alone, but TURNED every 10 paints           <- what a hand does
//! ```
//!
//! The jingle is kept until last on purpose: it is the leading suspect, so every stage but the
//! final one can also be read the *fine* way — see below — and stage 4 tests it against nothing
//! else at all.
//!
//! Stage 6 is a different kind of stage and was added after the first run, which found nothing.
//! Stages 1-5 replicate what the timer *runs*; stage 6 replicates what a person *does*. A board
//! is picked up to have its button pressed, and a board in a hand gets turned — and
//! `Panel::set_rotation` on a real change writes MADCTL **and clears the whole screen**, inside
//! the same `show` the render loop is timing. Neither earlier measurement could see that:
//! `paint-profile` deliberately excluded the turning paint as an untimed warm-up, and stages 1-5
//! ran with the board flat on a desk where the rotation never changes.
//!
//! ## The fine discriminator, inside a single stage
//!
//! Every sample is marked with whether a jingle was sounding while it was taken, so one stage
//! answers three questions at once without waiting for the next
//! ([`Split`](platform_bench::Split)):
//!
//! - breaches **only** while a jingle sounded → the buzzer path blocks;
//! - breaches in **both** halves → something else blocks, and the jingle is a bystander;
//! - breaches in **neither** → this stage did not reproduce the problem, and nothing has been
//!   shown about the jingle either way.
//!
//! ## The two places this tool can refute itself
//!
//! An instrument that cannot fail is an instrument that agrees with whoever built it. Two stages
//! are calibration rather than measurement, and the summary says so:
//!
//! - **stage 1 must break the budget.** If the production replica paints clean, the bench has
//!   not reproduced the fault and every later stage is uninformative — no thread can be cleared
//!   by a run that never showed the problem.
//! - **stage 5 must land near 21.3 ms.** That is `paint-profile`'s answer for the same picture on
//!   the same panel. If this bench disagrees with it alone on the glass, the two tools are
//!   measuring different things and this one is wrong.
//!
//! ## Not perturbing what it measures
//!
//! Nothing is logged inside a timed region — a `warn!` at 115200 baud is milliseconds of blocking
//! UART, which is enough to *become* the thing being measured. Samples go into a vector reserved
//! to its full length before the stage starts, so a `push` on the measured path is a pointer
//! bump and never an allocation, and every summary is printed after the stage has finished.
//!
//! ## Using it
//!
//! ```sh
//! timeout 120 just run-bin-pomodoro blocker-probe > probe.log 2>&1
//! just run-pomodoro                      # put the timer back
//! ```
//!
//! Leave the board on the desk and do not press anything: a button press would sound a jingle
//! this tool did not schedule and would mark the wrong samples. Stage 6 turns the panel by
//! command rather than by sensor, so the board does not need to be moved for it either.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use board_support::{internal_i2c, AccelRange, Axp192, Mpu6886};
use embedded_hal_bus::i2c::MutexDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{info, warn};
use platform_adapters::{
    Axp192Backlight, Axp192PowerSource, GpioButton, LedcBuzzer, Mpu6886Imu, Panel, PanelScreen,
    PekButton, Turning,
};
use platform_bench::{Evidence, Sample, Split, Summary};
use platform_core::{Screen, ScreenRotation, Tick, Tone};
use platform_input::Buttons;
use platform_runtime::{
    spawn_buzzer, spawn_power_watch, spawn_rotation, BacklightSwitch, BuzzerHandle, Monotonic,
    PowerWatchConfig, PowerWatchTask, RotationConfig, RotationTask, SharedRotation,
    ANIMATION_PERIOD, MIN_YIELD,
};
use pomodoro_core::{Jingle, Phase, Status, CLASSIC};
use pomodoro_display::PomodoroView;
use pomodoro_shell::{spawn_input, InputTask, SharedTimer, INPUT_CONFIG};

/// Paints timed per stage.
///
/// The production fault appears in roughly 0.8% of frames, so a stage has to be long enough that
/// a handful of breaches is expected rather than lucky: 200 paints at the 50 ms animation cadence
/// is about ten seconds, and six stages fit inside a minute of serial.
const SAMPLES: usize = 200;

/// The budget the timer's animated screen is held to — `ANIMATION_PERIOD`, quoted from
/// `platform-runtime` rather than restated, so this bench and the production alarm can never
/// drift apart.
const BUDGET: Duration = ANIMATION_PERIOD;

/// What `paint-profile` measured for this same picture with nothing else running, and therefore
/// what the final stage has to reproduce for this bench to be believable.
const PAINT_PROFILE_MEDIAN: Duration = Duration::from_micros(21_300);

/// How far the final stage's median may sit from [`PAINT_PROFILE_MEDIAN`] before the two tools
/// are measuring different things. Generous — the point is to catch a bench that is wrong by a
/// factor, not to re-measure the panel to the microsecond.
const CALIBRATION_TOLERANCE: Duration = Duration::from_millis(5);

/// The quiet between deliberate jingles.
///
/// Long enough that most samples in a stage are taken with the buzzer silent — the comparison
/// needs a healthy idle half as much as it needs the active one — and short enough that a
/// 200-paint stage still covers several jingles. `Jingle::FocusStart` itself sounds for 330 ms.
const JINGLE_GAP: Duration = Duration::from_millis(1_000);

/// The jingle thread's stack. It blocks on the buzzer owner and formats nothing; 4 KiB would do,
/// but the other hardware-adjacent threads here are sized to 8 KiB and consistency is worth more
/// than 4 KiB of SRAM on a bench tool.
const JINGLE_STACK_SIZE: usize = 8 * 1024;

/// The sweep thread's stack, in bytes.
///
/// The production display thread is sized to 8 KiB, and that is enough for a render alone. This
/// thread renders *and* formats the per-stage reports, and `core::fmt`'s call tree is deep: at
/// 8 KiB it overflowed and the board rebooted in a loop. 16 KiB is the size the plant monitor's
/// display thread was already raised to for the same reason.
const SWEEP_STACK_SIZE: usize = 16 * 1024;

/// A deliberate jingle on a cadence this tool controls, and a flag saying when one is sounding.
///
/// The production overruns clustered in the seconds after the start button was pressed, which is
/// what put the buzzer path under suspicion. Waiting for that to happen by hand is not an
/// experiment; sounding the same jingle through the same one buzzer owner, on a schedule, is.
///
/// It plays through [`BuzzerHandle`] exactly as the input thread does — submitting the melody and
/// **blocking** until the owner has finished it — because that blocking is part of the suspected
/// mechanism and a fire-and-forget version would not reproduce it.
struct Jingler {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
    sounding: Arc<AtomicBool>,
}

impl Jingler {
    /// Start sounding `Jingle::FocusStart` every [`JINGLE_GAP`], flagging while it sounds.
    fn spawn(mut tone: BuzzerHandle) -> std::io::Result<Jingler> {
        let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let sounding: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
        let sounding_in_thread: Arc<AtomicBool> = Arc::clone(&sounding);
        let handle: JoinHandle<()> = thread::Builder::new()
            .name("probe-jingler".to_string())
            .stack_size(JINGLE_STACK_SIZE)
            .spawn(move || {
                while !stop_in_thread.load(Ordering::Relaxed) {
                    thread::sleep(JINGLE_GAP);
                    // The flag is raised before the melody is submitted and lowered after it has
                    // finished, so it brackets the whole blocking play — including the time the
                    // request spends queued at the owner, which costs the caller just as much.
                    sounding_in_thread.store(true, Ordering::Relaxed);
                    let played: Result<(), _> = tone.play(Jingle::FocusStart.notes());
                    sounding_in_thread.store(false, Ordering::Relaxed);
                    if let Err(err) = played {
                        warn!("blocker-probe: jingle failed: {err}");
                    }
                }
            })?;
        Ok(Jingler {
            handle,
            stop,
            sounding,
        })
    }

    /// A reader of the sounding flag, for marking samples.
    fn sounding(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.sounding)
    }

    /// Ask the jingle thread to finish, and wait until it has — so the next stage genuinely runs
    /// without it rather than overlapping its last melody.
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

/// A view whose clock differs from the previous sample's, so the picture really changes and no
/// paint is suppressed as unchanged — the same forcing `paint-profile` uses.
fn sample_view(index: usize) -> PomodoroView {
    PomodoroView {
        phase: Phase::Focus,
        status: Status::Running,
        remaining_secs: (25 * 60) - index as u32,
    }
}

/// The creature's animation clock for sample `index`, stepped past one frame's hold so
/// consecutive samples draw different sprite frames.
fn sample_elapsed(index: usize) -> Tick {
    index as Tick * 200
}

/// Time [`SAMPLES`] paints at the production cadence, marking each with whether a jingle sounded.
///
/// This mirrors `platform_runtime::render_loop`: paint, then sleep the remainder of the budget
/// with the [`MIN_YIELD`] floor, so the display gives up the core between frames exactly as it
/// does in the timer. A bench that painted flat out would starve the very threads it is trying to
/// catch.
///
/// A sample is marked if a jingle was sounding at *either* end of it. A paint is tens of
/// milliseconds long and a jingle is hundreds, so an overlap that began or ended mid-paint is
/// still an overlap, and taking only the leading edge would leave the last paint of every jingle
/// misfiled into the idle half.
fn measure<S: Screen<PomodoroView>>(
    screen: &mut S,
    rotation: &SharedRotation,
    sounding: &AtomicBool,
    samples: &mut Vec<Sample>,
) {
    samples.clear();
    (0..SAMPLES).for_each(|index: usize| {
        let at: ScreenRotation = rotation.current();
        let before: bool = sounding.load(Ordering::Relaxed);
        let started: Instant = Instant::now();
        let painted: Result<(), _> = screen.show(sample_view(index), sample_elapsed(index), at);
        let took: Duration = started.elapsed();
        let after: bool = sounding.load(Ordering::Relaxed);

        // The one branch on the measured path, and it costs nothing worth measuring; the error
        // is remembered rather than logged, because logging here is the observer effect this
        // whole tool exists to avoid.
        samples.push(Sample {
            took,
            during_suspect: before || after,
        });
        if painted.is_err() {
            // A failed paint is not a timing; drop it rather than let a fast error path look
            // like a fast paint.
            samples.pop();
        }
        thread::sleep(BUDGET.saturating_sub(took).max(MIN_YIELD));
    });
}

/// How often the turning stage actually turns the panel.
///
/// Every tenth paint, so a stage holds twenty turns against a hundred and eighty ordinary paints
/// — enough of each half for the split to mean something.
const TURN_EVERY: usize = 10;

/// The four rotations, cycled by the turning stage.
const STOPS: [ScreenRotation; 4] = [
    ScreenRotation::Deg0,
    ScreenRotation::Deg90,
    ScreenRotation::Deg180,
    ScreenRotation::Deg270,
];

/// Time [`SAMPLES`] paints alone on the glass, **turning the panel** every [`TURN_EVERY`] of
/// them, and marking the paints that carried a turn.
///
/// The stage the first run of this tool asked for. Everything else here reproduces production by
/// replicating what the timer *runs*; this reproduces what a person *does* — a board is picked
/// up to have its button pressed, and it is turned while it is in a hand.
///
/// It matters because `Panel::set_rotation` early-returns when the rotation is unchanged, but on
/// a real change it writes MADCTL **and clears the whole screen**, and that clear lands inside
/// the same `show` the render loop is timing. Both earlier measurements missed this by
/// construction rather than by accident: `paint-profile` deliberately excluded the turning paint
/// as an untimed warm-up, on the grounds that a once-per-turn cost should not be averaged into
/// every frame — which is right for the question it was asking and hides this one — and the
/// first four stages above ran with the board flat on a desk, where the rotation never changes.
///
/// So "rotation is innocent" has been established for a *rotated* paint and never tested for a
/// *turning* one. This stage tests the turn itself.
fn measure_turning<S: Screen<PomodoroView>>(screen: &mut S, samples: &mut Vec<Sample>) {
    samples.clear();
    (0..SAMPLES).for_each(|index: usize| {
        // The rotation advances only on a turning sample; every other paint is handed the one
        // already showing, so `set_rotation` early-returns exactly as it does in production.
        let turning: bool = index.is_multiple_of(TURN_EVERY);
        let at: ScreenRotation = STOPS[(index / TURN_EVERY) % STOPS.len()];

        let started: Instant = Instant::now();
        let painted: Result<(), _> = screen.show(sample_view(index), sample_elapsed(index), at);
        let took: Duration = started.elapsed();

        if painted.is_ok() {
            samples.push(Sample {
                took,
                during_suspect: turning,
            });
        }
        thread::sleep(BUDGET.saturating_sub(took).max(MIN_YIELD));
    });
}

/// One summary as an aligned line.
fn line(label: &str, summary: Summary) -> String {
    format!(
        "{label:<22} n {:>3}  min {:>7.2?}  median {:>7.2?}  max {:>7.2?}  over budget {:>3}",
        summary.count, summary.min, summary.median, summary.max, summary.over_budget
    )
}

/// Report a stage: the whole distribution, then the jingle split that stage supports.
///
/// Called only after the stage's last paint, never between samples.
fn report(stage: &str, suspect: Suspect, samples: &mut [Sample]) {
    info!("--- {stage} ---");
    match Summary::of(samples, BUDGET) {
        Some(summary) => info!("{}", line("all samples", summary)),
        None => warn!("{stage}: captured nothing — every paint failed"),
    }

    let split: Split = Split::of(samples, BUDGET);
    if let Some(during) = split.during {
        info!("{}", line(suspect.active, during));
    }
    if let Some(between) = split.between {
        info!("{}", line(suspect.idle, between));
    }
    info!(
        "  evidence about {}: {}",
        suspect.name,
        reading(split.evidence(), suspect)
    );
}

/// What a stage's samples are marked on, in words — so one reporting path serves every stage
/// rather than each stage growing its own.
#[derive(Clone, Copy)]
struct Suspect {
    /// The suspect's name, for the verdict line.
    name: &'static str,
    /// The label for the half taken while it was active.
    active: &'static str,
    /// The label for the half taken while it was idle.
    idle: &'static str,
}

/// The deliberate jingle: the leading suspect the epic named.
const JINGLE: Suspect = Suspect {
    name: "the jingle",
    active: "  while a jingle rang",
    idle: "  between jingles",
};

/// The turn itself — MADCTL plus a full-screen clear, inside the timed paint.
const TURN: Suspect = Suspect {
    name: "the turn",
    active: "  paints carrying a turn",
    idle: "  paints at a settled rotation",
};

/// What an [`Evidence`] licenses, spelled out — the summary has to be readable off the serial
/// log by someone who has not read this file.
fn reading(evidence: Evidence, suspect: Suspect) -> String {
    let name: &str = suspect.name;
    match evidence {
        Evidence::OnlyDuring => format!("IMPLICATED — breaches only while {name} was active"),
        Evidence::Both => format!("bystander — breaches happen without {name} too"),
        Evidence::OnlyBetween => format!("INCOHERENT — breaches only while {name} was idle"),
        Evidence::Neither => "nothing shown — this stage reproduced no breach at all".to_string(),
        Evidence::NoComparison => format!("no comparison — {name} did not run in this stage"),
    }
}

/// Whether two medians agree to within [`CALIBRATION_TOLERANCE`], in either direction.
fn agrees(measured: Duration, expected: Duration) -> bool {
    measured
        .saturating_sub(expected)
        .max(expected.saturating_sub(measured))
        <= CALIBRATION_TOLERANCE
}

/// The two calibration checks, reported last so they are the final word on serial.
///
/// Stated as pass/fail against numbers fixed before the run, rather than left for a reader to
/// eyeball, because the failure mode being guarded against is a bench that quietly measured the
/// wrong thing and was believed.
fn calibrate(production: Option<Summary>, alone: Option<Summary>) {
    info!("--- calibration: is this bench believable? ---");
    match production {
        Some(summary) if summary.over_budget > 0 => info!(
            "PASS  stage 1 reproduced the fault: {} of {} paints broke the budget",
            summary.over_budget, summary.count
        ),
        Some(summary) => warn!(
            "FAIL  stage 1 painted clean ({} paints, max {:.2?}) — the fault was NOT reproduced, \
             so no thread is cleared by this run",
            summary.count, summary.max
        ),
        None => warn!("FAIL  stage 1 captured nothing"),
    }
    match alone {
        Some(summary) if agrees(summary.median, PAINT_PROFILE_MEDIAN) => {
            info!(
                "PASS  stage 5 agrees with paint-profile: median {:.2?} against {:.2?}",
                summary.median, PAINT_PROFILE_MEDIAN
            )
        }
        Some(summary) => warn!(
            "FAIL  stage 5 median {:.2?} disagrees with paint-profile's {:.2?} — this bench and \
             that one are measuring different things",
            summary.median, PAINT_PROFILE_MEDIAN
        ),
        None => warn!("FAIL  stage 5 captured nothing"),
    }
}

/// The internal I2C bus' `'static` home, shared by the PMIC and the IMU — as in the timer itself.
static I2C_BUS: static_cell::StaticCell<Mutex<I2cDriver<'static>>> = static_cell::StaticCell::new();

/// Every thread the sweep stops, in the order it stops them.
struct Threads {
    input: InputTask,
    power_watch: PowerWatchTask,
    rotation: RotationTask,
    jingler: Jingler,
}

/// The whole subtractive sweep, on the thread that owns the panel.
///
/// Stops one thread per stage and reports each stage's distribution once the stage has finished,
/// never between its samples.
fn sweep<S: Screen<PomodoroView>>(mut screen: S, rotation: SharedRotation, threads: Threads) {
    let sounding: Arc<AtomicBool> = threads.jingler.sounding();
    // Reserved to its full length here, so no `push` on a measured path ever allocates.
    let mut samples: Vec<Sample> = Vec::with_capacity(SAMPLES);

    // Stage 1 — the production replica. Its result is kept for the calibration verdict.
    measure(&mut screen, &rotation, &sounding, &mut samples);
    let production: Option<Summary> = Summary::of(&mut samples, BUDGET);
    report(
        "stage 1: production (jingle + input + power-watch + rotation)",
        JINGLE,
        &mut samples,
    );

    // Stages 2-4 — one thread removed each, in ascending order of suspicion, so the jingle is the
    // last thing standing before the calibration stage.
    threads.input.stop();
    measure(&mut screen, &rotation, &sounding, &mut samples);
    report("stage 2: minus input", JINGLE, &mut samples);

    threads.power_watch.stop();
    measure(&mut screen, &rotation, &sounding, &mut samples);
    report("stage 3: minus power-watch", JINGLE, &mut samples);

    threads.rotation.stop();
    measure(&mut screen, &rotation, &sounding, &mut samples);
    report(
        "stage 4: minus rotation (the jingle, alone)",
        JINGLE,
        &mut samples,
    );

    // Stage 5 — nothing else running at all. This is `paint-profile`'s configuration, and it has
    // to reproduce `paint-profile`'s answer.
    threads.jingler.stop();
    measure(&mut screen, &rotation, &sounding, &mut samples);
    let alone: Option<Summary> = Summary::of(&mut samples, BUDGET);
    report(
        "stage 5: minus the jingle (the display, alone)",
        JINGLE,
        &mut samples,
    );

    // Stage 6 — the display still alone, but now the panel is TURNED every few paints. Nothing
    // the timer runs was ever the difference between this bench and production; what a person
    // does might be, and a turn is the one thing a hand does that a desk does not.
    measure_turning(&mut screen, &mut samples);
    report(
        "stage 6: the display alone, turned every 10 paints",
        TURN,
        &mut samples,
    );

    calibrate(production, alone);

    info!("--- how to read the sweep ---");
    info!("a stage whose breaches vanish names the thread removed just before it");
    info!("breaches surviving to stage 5 are the display's own cost, not a blocker");
    info!("breaches in stage 1 but nowhere else mean the blocker needs several threads at once");
    info!("stage 6 breaching while 1-5 do not means the blocker is the TURN, not any thread —");
    info!("   a cost no replica of what the timer RUNS can reach, only one of what a hand DOES");
}

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("blocker-probe: {SAMPLES} timed paints per stage, against a {BUDGET:?} budget");
    info!("the board must be left alone — a button press sounds an unscheduled jingle");

    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // Bring-up, in the timer's own order: an instrument that took a shortcut here would not be
    // evidence about the timer.
    let i2c: I2cDriver<'static> = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let bus: &'static Mutex<I2cDriver<'static>> = I2C_BUS.init(Mutex::new(i2c));

    let mut axp: Axp192<MutexDevice<'static, I2cDriver<'static>>> =
        Axp192::new(MutexDevice::new(bus));
    axp.power_on().expect("AXP192 LCD/TFT rail power-on");
    let power_source: Axp192PowerSource<_> = Axp192PowerSource::new(axp);

    let mut imu: Mpu6886<MutexDevice<'static, I2cDriver<'static>>> =
        Mpu6886::new(MutexDevice::new(bus), AccelRange::G4);
    imu.init(&mut FreeRtos).expect("MPU6886 IMU bring-up");
    let imu: Mpu6886Imu<_> = Mpu6886Imu::new(imu);

    let panel: Panel = Panel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 panel bring-up");
    let screen: PanelScreen<PomodoroView, _, Turning> = PanelScreen::turning(
        panel,
        |target: &mut _, view: PomodoroView, elapsed: Tick, at: ScreenRotation| {
            pomodoro_display::render(target, view, elapsed, at)
        },
    );

    let front: GpioButton = GpioButton::new(peripherals.pins.gpio37).expect("front button G37");
    let side: GpioButton = GpioButton::new(peripherals.pins.gpio39).expect("side button G39");
    let power_button: PekButton<MutexDevice<'static, I2cDriver<'static>>> =
        PekButton::new(Axp192::new(MutexDevice::new(bus)));
    let buttons: Buttons<_, _, _> = Buttons::new(front, side, power_button, INPUT_CONFIG);
    // Wired exactly as production wires it, so the input thread under measurement is the real
    // one. The probe drives its own render loop (it is timing the paint), so no flag is read
    // here — and it never clicks the power button, so the glass stays lit for the whole sweep.
    let backlight: BacklightSwitch<_> = BacklightSwitch::new(Axp192Backlight::new(
        Axp192::new(MutexDevice::new(bus)),
        true,
    ));
    let buzzer = LedcBuzzer::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio2,
    )
    .expect("buzzer G2 (LEDC)");
    let (_buzzer_owner, tone) = spawn_buzzer(buzzer).expect("spawn buzzer owner");

    let clock: Monotonic = Monotonic::start();
    let shared: SharedTimer = SharedTimer::new();
    let rotation: SharedRotation = SharedRotation::new();

    // Every thread the timer runs, plus the deliberate jingle standing in for the button press
    // that the production overruns clustered behind.
    let threads: Threads = Threads {
        input: spawn_input(
            buttons,
            backlight,
            tone.clone(),
            shared.clone(),
            clock,
            CLASSIC,
        )
        .expect("spawn pomodoro-input"),
        power_watch: spawn_power_watch(
            power_source,
            tone.clone(),
            clock,
            PowerWatchConfig::default(),
        )
        .expect("spawn power-watch"),
        rotation: spawn_rotation(imu, rotation.clone(), clock, RotationConfig::default())
            .expect("spawn rotation"),
        jingler: Jingler::spawn(tone).expect("spawn the deliberate jingle"),
    };

    // The sweep runs on a thread of its own rather than on `main`, for two reasons that happen
    // to be the same reason. Fidelity: in the timer, the paint runs on the thread
    // `spawn_display` makes, so a bench that painted on the main task would be measuring a
    // different task than production uses. And headroom: `main` gets
    // CONFIG_ESP_MAIN_TASK_STACK_SIZE, which is 8 KiB here, and a render plus the report
    // formatting overflowed it — the board rebooted in a loop until this moved off it.
    let sweeper: JoinHandle<()> = thread::Builder::new()
        .name("probe-sweep".to_string())
        .stack_size(SWEEP_STACK_SIZE)
        .spawn(move || sweep(screen, rotation, threads))
        .expect("spawn the sweep");
    let _ = sweeper.join();

    loop {
        FreeRtos::delay_ms(5_000);
    }
}
