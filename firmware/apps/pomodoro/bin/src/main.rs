#![forbid(unsafe_code)]
//! pomodoro — the composition root: a standalone, offline pomodoro timer on std/ESP-IDF.
//!
//! No network: the timer is a self-contained device driven by the screen, the two buttons, and
//! the buzzer. This root powers the LCD rails (AXP192), builds the board-generic adapters (the
//! ST7789 [`Panel`], the G37/G39 [`GpioButton`]s, the MPU6886 IMU, the G2 [`LedcBuzzer`]), and
//! wires them to the host-tested loops:
//!
//! - [`spawn_input`] (pomodoro-shell) polls the buttons, folds each gesture through the pure
//!   `pomodoro_core::step`, fires a tick, and sounds the jingles — front tap starts / pauses,
//!   front double-tap restarts the whole session, front hold resets the phase, side tap skips.
//! - [`spawn_display`] (platform-runtime) is the *same* generic render loop the plant monitor
//!   uses, fed a source that snapshots the shared [`Timer`](pomodoro_core::Timer) into a
//!   [`PomodoroView`] each tick, and a [`PanelScreen`] that paints it with
//!   `pomodoro_display::render`.
//!
//! ## The picture faces the reader
//!
//! Stand the board on its USB-C port and the timer turns with it: the panel is told to scan
//! the new way up, and `pomodoro_display::render` picks the portrait layout the taller canvas
//! needs. Unlike the orientation readout, this app has no IMU thread of its own, so it takes
//! the easy half of the platform's capability — [`spawn_rotation`] owns the sensor outright
//! and feeds a [`SharedRotation`], and the display loop reads the settled answer.
//!
//! Both loops share one [`Monotonic`] clock, so the countdown the input loop advances and the
//! `mm:ss` the display shows agree. Durations are the classic 25 / 5 / 15 ([`CLASSIC`]); change
//! that one constant (or build a `Durations`) to retune — or to shrink them for a bench test.

use std::sync::Mutex;

use board_support::{internal_i2c, AccelRange, Axp192, Mpu6886};
use embedded_hal_bus::i2c::MutexDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use platform_adapters::{
    Axp192Backlight, Axp192PowerSource, GpioButton, LedcBuzzer, Mpu6886Imu, Panel, PanelScreen,
    PekButton, Turning,
};
use platform_core::{ScreenRotation, Tick};
use platform_input::Buttons;
use platform_runtime::{
    spawn_buzzer, spawn_display, spawn_power_watch, spawn_rotation, BacklightSwitch, DisplayConfig,
    LitFlag, Monotonic, PowerWatchConfig, RotationConfig, SharedRotation,
};
use pomodoro_core::CLASSIC;
use pomodoro_display::PomodoroView;
use pomodoro_shell::{spawn_input, SharedTimer, INPUT_CONFIG};
use static_cell::StaticCell;

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
    info!("pomodoro: std/ESP-IDF up — offline timer (screen + buttons + buzzer)");

    // A boot-time bring-up failure is unrecoverable, so panic with context: the composition
    // root owns the one place peripherals are taken and adapters are built.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // The internal bus, given a 'static home so two threads can each hold a device on it.
    // It gained a second consumer when this app took the rotation capability: the PMIC is read
    // by the power-watch and the IMU by the rotation thread, so the single `I2cDriver` can no
    // longer be owned outright by either. `MutexDevice` is what makes that safe across threads
    // — a `RefCellDevice` could not cross into one.
    let i2c: I2cDriver<'static> = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let bus: &'static Mutex<I2cDriver<'static>> = I2C_BUS.init(Mutex::new(i2c));

    // Power the LCD/TFT rails before building the panel — an unpowered panel takes a correct
    // init and still shows nothing. The AXP192 latches its LDO enables, but this root *retains*
    // the PMIC past power-on: the power-watch thread reads VBUS from this same device for the
    // life of the app.
    let mut axp: Axp192<MutexDevice<'static, I2cDriver<'static>>> =
        Axp192::new(MutexDevice::new(bus));
    axp.power_on().expect("AXP192 LCD/TFT rail power-on");
    let power_source: Axp192PowerSource<_> = Axp192PowerSource::new(axp);

    // The IMU, on its own handle over the same bus — the rotation source and nothing else.
    // `init` checks WHO_AM_I before it writes anything, so this panics on a mis-wired or
    // unpowered sensor rather than going on to report a rotation read from a floating bus.
    let mut imu: Mpu6886<MutexDevice<'static, I2cDriver<'static>>> =
        Mpu6886::new(MutexDevice::new(bus), AccelRange::G4);
    imu.init(&mut FreeRtos).expect("MPU6886 IMU bring-up");
    let imu: Mpu6886Imu<_> = Mpu6886Imu::new(imu);

    // The panel, wrapped as a generic Screen with the pomodoro render function.
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
    let screen: PanelScreen<PomodoroView, _, Turning> = PanelScreen::turning(
        panel,
        |target: &mut _, view: PomodoroView, elapsed: Tick, rotation: ScreenRotation| {
            pomodoro_display::render(target, view, elapsed, rotation)
        },
    );

    // The two levelled buttons (active-low, no internal pull) and the passive buzzer (LEDC on G2).
    let front: GpioButton = GpioButton::new(peripherals.pins.gpio37).expect("front button G37");
    let side: GpioButton = GpioButton::new(peripherals.pins.gpio39).expect("side button G39");
    // The third button is not on a pin at all: it hangs off the PMIC's PEK input, which does its
    // own debouncing and press timing, so it arrives already classified through its own handle on
    // the shared bus.
    let power_button: PekButton<MutexDevice<'static, I2cDriver<'static>>> =
        PekButton::new(Axp192::new(MutexDevice::new(bus)));
    let buttons: Buttons<_, _, _> = Buttons::new(front, side, power_button, INPUT_CONFIG);

    // The backlight (PMIC rail LDO2), wrapped so the display thread can see it. `power_on` has
    // already lit the glass, hence `true` — the wrapper publishes that rather than writing it.
    // The switch goes to the input thread (the only writer); the flag to the render loop, which
    // skips the paint entirely while the glass is dark.
    let backlight: BacklightSwitch<_> = BacklightSwitch::new(Axp192Backlight::new(
        Axp192::new(MutexDevice::new(bus)),
        true,
    ));
    let lit: LitFlag = backlight.flag();
    let buzzer = LedcBuzzer::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio2,
    )
    .expect("buzzer G2 (LEDC)");

    // One buzzer, one owner: the LEDC buzzer moves into a single owner thread, and every caller
    // — the input thread's jingles, the power-watch thread's chimes — plays through a Clone +
    // Send handle, so a chime and a jingle can never interleave or truncate one another. Held
    // for the life of main; the owner runs until every handle is dropped.
    let (_buzzer_owner, tone) = spawn_buzzer(buzzer).expect("spawn buzzer owner");

    // One monotonic clock, shared by the input thread (writer) and the render loop (reader), so
    // the countdown and the displayed mm:ss are measured on one time base.
    let clock: Monotonic = Monotonic::start();
    let shared: SharedTimer = SharedTimer::new();

    // Input: poll the buttons, step the FSM, sound the jingles on a clone of the one buzzer
    // handle. Held for the life of main — dropping it would only detach the thread, which
    // already runs forever.
    let _input = spawn_input(
        buttons,
        backlight,
        tone.clone(),
        shared.clone(),
        clock,
        CLASSIC,
    )
    .expect("spawn pomodoro-input");
    info!(
        "input thread up: front click = start/pause, front double-click = restart session, \
         front hold = reset phase, side click = skip, power click = backlight"
    );

    // Display: render the timer view every tick, through the same generic loop the plant
    // monitor uses. The source snapshots the shared timer and turns it into a PomodoroView at
    // `now` — the render loop's own clock, the same Monotonic the input loop steps against.
    let source = {
        let shared: SharedTimer = shared.clone();
        move |now: Tick| PomodoroView::of(&shared.snapshot(), now, CLASSIC)
    };
    // Which way up to draw. This app has no sampler of its own, so the sensor moves into the
    // rotation thread outright — the simple half of the capability. That thread polls at 50 ms
    // against a 250 ms settle window; the display loop only reads the answer it has settled on.
    let rotation: SharedRotation = SharedRotation::new();
    let _rotation = spawn_rotation(imu, rotation.clone(), clock, RotationConfig::default())
        .expect("spawn pomodoro-rotation");
    info!("rotation thread up: MPU6886 polled at 50 Hz, settled, published");

    let _display = spawn_display(
        screen,
        source,
        rotation.source(),
        lit,
        clock,
        DisplayConfig::default(),
    )
    .expect("spawn pomodoro-display");
    info!("display thread up: ST7789 rendering mm:ss + the Claude creature");

    // Power-watch: poll VBUS on the retained AXP192, debounce it, and sound the spool-up /
    // spool-down chime a settled USB plug or unplug decides — through the same one buzzer
    // owner, on the same clock. Silent at boot: the first sample only seeds the baseline. Held
    // for the life of main.
    let _power_watch = spawn_power_watch(power_source, tone, clock, PowerWatchConfig::default())
        .expect("spawn pomodoro power-watch");
    info!("power-watch thread up: USB plug = spool-up, unplug = spool-down");

    // Supervisory loop: a heartbeat only — the input, display, and power-watch threads own the
    // app.
    loop {
        FreeRtos::delay_ms(5_000);
        let timer = shared.snapshot();
        info!("pomodoro: phase {:?}, {:?}", timer.phase(), timer.status());
    }
}
