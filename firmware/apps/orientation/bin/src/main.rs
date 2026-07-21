#![forbid(unsafe_code)]
//! orientation — the composition root: a live readout of which way the board is pointing.
//!
//! No network and no buttons: the readout is a self-contained instrument driven by the
//! MPU6886 IMU and the screen. This root powers the LCD rails (AXP192), shares the internal
//! I2C bus between the PMIC and the IMU, and wires them to the two host-tested loops:
//!
//! - [`spawn_sampler`] (orientation-shell) polls the IMU at 100 Hz, folds each reading through
//!   the pure smoother, and publishes the [`Orientation`](orientation_core::Orientation) it
//!   implies into a shared cache.
//! - [`spawn_display`] (platform-runtime) is the *same* generic render loop the pomodoro timer
//!   and the plant monitor use, fed a source that snapshots that cache into an
//!   [`OrientationView`] and a [`PanelScreen`] that paints it with `orientation_display::render`.
//!
//! ## Tuned for responsiveness
//!
//! The render loop's default cadence repaints a *still* screen once a second, which is right
//! for a countdown and wrong for an instrument: this readout's whole purpose is to move when
//! the board moves. So it runs at [`READOUT_PERIOD`] instead, fast enough that the bars track
//! a hand movement rather than catching up with it. The loop still suppresses repaints when
//! the picture has not changed, so a board sitting still on the desk costs no SPI traffic at
//! all; the fast cadence is spent only while something is actually happening.
//!
//! Both this and the sampler's cadence are floored by one FreeRTOS tick (10 ms here): a
//! shorter sleep on ESP-IDF does not yield, it busy-waits. Asking for more than the hardware
//! can give does not buy speed, it buys a starved scheduler — see [`READOUT_PERIOD`].
//!
//! ## The picture faces the reader
//!
//! This is the first app to take the platform's rotation capability. Stand the board on its
//! USB-C port and the readout turns with it: the panel is told to scan the new way up, and
//! `orientation_display::render` picks the portrait layout the taller canvas needs.
//!
//! Both halves are opted into here and nowhere else — [`PanelScreen::turning`] for the glass,
//! and a rotation source for the verdict. An app that wants neither takes `PanelScreen::new`
//! and hands back a constant, and links none of the machinery.
//!
//! ## The shared bus
//!
//! The AXP192 and the MPU6886 sit on the same two pins and are read from two different
//! threads — the power-watch and the sampler. So the one `I2cDriver` is given a `'static`
//! home and each device holds its own `MutexDevice` handle over it, which is exactly the
//! arrangement `board_support::internal_i2c` was written to expect.

use std::sync::Mutex;

use board_support::{internal_i2c, AccelRange, Axp192, Mpu6886};
use embedded_hal_bus::i2c::MutexDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use orientation_core::{Orientation, Reading};
use orientation_display::OrientationView;
use orientation_shell::{spawn_sampler, SamplerConfig, SharedOrientation};
use platform_adapters::{Axp192PowerSource, LedcBuzzer, Mpu6886Imu, Panel, PanelScreen, Turning};
use platform_core::{ScreenRotation, Tick};
use platform_runtime::{
    spawn_buzzer, spawn_display, spawn_power_watch, DisplayConfig, LitFlag, Monotonic,
    PowerWatchConfig, SharedRotation,
};
use static_cell::StaticCell;
use std::time::Duration;

/// How often the readout repaints while something is changing — 25 Hz.
///
/// Set from a *measurement*, not from a wish. A full repaint of this screen was timed on the
/// board at about 12 ms (three bars and five text fields, once the panel's batch buffer was
/// widened to match the load). A 40 ms tick leaves roughly two thirds of the interval free,
/// so the render thread always reaches its sleep and the FreeRTOS idle task always runs.
///
/// The first attempt asked for 50 Hz against a 31 ms paint. The loop then never slept for
/// more than its 1 ms floor, the idle task on core 1 was starved, and the task watchdog fired
/// continuously — a "faster" number that produced a *worse* readout. The budget has to be one
/// the hardware can actually meet.
const READOUT_PERIOD: Duration = Duration::from_millis(40);

/// The internal I2C bus' `'static` home, shared by the PMIC and the IMU.
///
/// A `StaticCell` rather than a leaked `Box`: both give the bus a lifetime that outlives every
/// thread that borrows it, but this one is checked — a second `init` panics instead of
/// quietly handing out a second bus.
static I2C_BUS: StaticCell<Mutex<I2cDriver<'static>>> = StaticCell::new();

fn main() {
    // Patch the ESP-IDF symbols Rust's std expects, then route `log` to the ESP-IDF logger.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("orientation: std/ESP-IDF up — live IMU readout (screen + MPU6886)");

    // A boot-time bring-up failure is unrecoverable, so panic with context: the composition
    // root owns the one place peripherals are taken and adapters are built.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // The internal bus, given a 'static home so two threads can each hold a device on it.
    let i2c: I2cDriver<'static> = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let bus: &'static Mutex<I2cDriver<'static>> = I2C_BUS.init(Mutex::new(i2c));

    // Power the LCD/TFT rails before building the panel — an unpowered panel takes a correct
    // init and still shows nothing. The PMIC is retained past power-on: the power-watch thread
    // reads VBUS from it for the life of the app.
    let mut axp: Axp192<MutexDevice<'static, I2cDriver<'static>>> =
        Axp192::new(MutexDevice::new(bus));
    axp.power_on().expect("AXP192 LCD/TFT rail power-on");
    let power_source: Axp192PowerSource<_> = Axp192PowerSource::new(axp);

    // The IMU, on its own handle over the same bus. `init` checks WHO_AM_I before it writes
    // anything, so this panics on a mis-wired or unpowered sensor rather than going on to
    // report an orientation read from a floating bus.
    let mut imu: Mpu6886<MutexDevice<'static, I2cDriver<'static>>> =
        Mpu6886::new(MutexDevice::new(bus), AccelRange::G4);
    imu.init(&mut FreeRtos).expect("MPU6886 IMU bring-up");
    let imu: Mpu6886Imu<_> = Mpu6886Imu::new(imu);
    info!("MPU6886 up on the internal bus at ±4 g");

    // The panel, wrapped as a generic Screen with the orientation render function.
    let panel: Panel = Panel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 panel bring-up");
    // `turning`, not `new`: this app opts into the rotation capability, so the panel is told to
    // scan the way the board is being held before each render. An app that stays landscape takes
    // `PanelScreen::new` and links none of this — the choice is the type, not a flag.
    let screen: PanelScreen<OrientationView, _, Turning> = PanelScreen::turning(
        panel,
        |target: &mut _, view: OrientationView, elapsed: Tick, rotation: ScreenRotation| {
            orientation_display::render(target, view, elapsed, rotation)
        },
    );

    // The passive buzzer (LEDC on G2), behind its single owner thread — the power-watch's
    // plug/unplug chimes are the only caller here, but the readout gets the same one-owner
    // arrangement every app on this platform uses.
    let buzzer = LedcBuzzer::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio2,
    )
    .expect("buzzer G2 (LEDC)");
    let (_buzzer_owner, tone) = spawn_buzzer(buzzer).expect("spawn buzzer owner");

    // One monotonic clock, shared by the render loop and the power-watch.
    let clock: Monotonic = Monotonic::start();
    let shared: SharedOrientation = SharedOrientation::new();

    // Sampler: poll the IMU, smooth it, publish the pose. Held for the life of main —
    // dropping it would only detach the thread, which already runs forever.
    let _sampler = spawn_sampler(imu, shared.clone(), clock, SamplerConfig::default())
        .expect("spawn orientation-sampler");
    info!("sampler thread up: MPU6886 polled at 100 Hz, smoothed, published");

    // Display: render the readout whenever it changed, through the same generic loop the
    // pomodoro timer uses — but at the instrument cadence, not the countdown one.
    // The loop hands the source the current tick, which is exactly what the staleness verdict
    // needs — so the same clock stamps a publication and judges its age.
    let source = {
        let shared: SharedOrientation = shared.clone();
        move |now: Tick| OrientationView::of(&shared.reading(now))
    };
    let config: DisplayConfig = DisplayConfig {
        period: READOUT_PERIOD,
        animation_period: READOUT_PERIOD,
        ..DisplayConfig::default()
    };
    // Which way up to draw. This app already *moves* its IMU into the sampler, so it cannot
    // also hand one to `spawn_rotation` — a single I2C device has a single owner. It does not
    // need to: the sampler already publishes an `Orientation` carrying the very `Acceleration`
    // the settler wants, so the rotation source folds that reading in and reports the verdict
    // in one step. One device owner, one settler, no extra thread.
    //
    // It runs on the display thread's cadence — a fold every 40 ms against a 250 ms settle
    // window, so the settler sees roughly six readings inside the interval it judges over.
    //
    // This lives in the composition root rather than in the sampler because `orientation-shell`
    // is context = "orientation" and `platform-runtime` is context = "shared": the root may
    // name both, a shell crate may not, and `hex-lint` enforces that in the gate.
    let rotation_source = {
        let pose: SharedOrientation = shared.clone();
        let turned: SharedRotation = SharedRotation::new();
        move |now: Tick| {
            turned.update(pose.last_known().acceleration, now);
            turned.current()
        }
    };
    let _display = spawn_display(
        screen,
        source,
        rotation_source,
        LitFlag::always(true),
        clock,
        config,
    )
    .expect("spawn orientation-display");
    info!("display thread up: ST7789 rendering the X/Y/Z bars at 25 Hz");

    // Power-watch: poll VBUS on the retained AXP192, debounce it, and chime a settled USB plug
    // or unplug — through the same one buzzer owner, on the same clock.
    let _power_watch = spawn_power_watch(power_source, tone, clock, PowerWatchConfig::default())
        .expect("spawn orientation power-watch");
    info!("power-watch thread up: USB plug = spool-up, unplug = spool-down");

    // Supervisory loop: a heartbeat only — the sampler, display, and power-watch threads own
    // the app. It logs the live vector alongside the pose it was read as, which is what makes
    // the board's axis convention checkable against `Facing`'s documented mapping over serial
    // rather than by eye on the glass.
    loop {
        FreeRtos::delay_ms(2_000);
        // Read through the same `reading` the glass does, so the log and the screen can never
        // disagree about whether the sensor is still answering.
        let reading: Reading = shared.reading(clock.now());
        let orientation: Orientation = reading.orientation;
        info!(
            "orientation: {:<10} x={:>6} y={:>6} z={:>6} mg  pitch={:>4}° roll={:>4}°{}",
            orientation.facing.label(),
            orientation.acceleration.x_mg,
            orientation.acceleration.y_mg,
            orientation.acceleration.z_mg,
            orientation.attitude.pitch_deg,
            orientation.attitude.roll_deg,
            if reading.signal.is_live() {
                ""
            } else {
                "  [NO SIGNAL — the numbers above are the last the IMU gave]"
            },
        );
    }
}
