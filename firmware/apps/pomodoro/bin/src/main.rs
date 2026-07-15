#![forbid(unsafe_code)]
//! pomodoro — the composition root: a standalone, offline pomodoro timer on std/ESP-IDF.
//!
//! No network: the timer is a self-contained device driven by the screen, the two buttons, and
//! the buzzer. This root powers the LCD rails (AXP192), builds the board-generic adapters (the
//! ST7789 [`Panel`], the G37/G39 [`GpioButton`]s, the G2 [`LedcBuzzer`]), and wires them to the
//! two host-tested loops:
//!
//! - [`spawn_input`] (pomodoro-shell) polls the buttons, folds each gesture through the pure
//!   `pomodoro_core::step`, fires a tick, and sounds the jingles — front tap starts / pauses,
//!   front hold resets, side tap skips.
//! - [`spawn_display`] (platform-runtime) is the *same* generic render loop the plant monitor
//!   uses, fed a source that snapshots the shared [`Timer`](pomodoro_core::Timer) into a
//!   [`PomodoroView`] each tick, and a [`PanelScreen`] that paints it with
//!   `pomodoro_display::render`.
//!
//! Both loops share one [`Monotonic`] clock, so the countdown the input loop advances and the
//! `mm:ss` the display shows agree. Durations are the classic 25 / 5 / 15 ([`CLASSIC`]); change
//! that one constant (or build a `Durations`) to retune — or to shrink them for a bench test.

use std::cell::RefCell;

use board_support::{internal_i2c, Axp192};
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use platform_adapters::{GpioButton, LedcBuzzer, Panel, PanelScreen};
use platform_core::Tick;
use platform_runtime::{spawn_display, DisplayConfig, Monotonic};
use pomodoro_core::CLASSIC;
use pomodoro_display::PomodoroView;
use pomodoro_shell::{spawn_input, SharedTimer};

fn main() {
    // Patch the ESP-IDF symbols Rust's std expects, then route `log` to the ESP-IDF logger.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("pomodoro: std/ESP-IDF up — offline timer (screen + buttons + buzzer)");

    // A boot-time bring-up failure is unrecoverable, so panic with context: the composition
    // root owns the one place peripherals are taken and adapters are built.
    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // Power the LCD/TFT rails before building the panel — an unpowered panel takes a correct
    // init and still shows nothing. The AXP192 latches its LDO enables, so this is scoped: once
    // the rails are up the PMIC and its bus can be dropped.
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
    }

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
    let screen: PanelScreen<PomodoroView, _> = PanelScreen::new(
        panel,
        |target: &mut _, view: PomodoroView, elapsed: Tick| {
            pomodoro_display::render(target, view, elapsed)
        },
    );

    // The two buttons (active-low, no internal pull) and the passive buzzer (LEDC on G2).
    let front: GpioButton = GpioButton::new(peripherals.pins.gpio37).expect("front button G37");
    let side: GpioButton = GpioButton::new(peripherals.pins.gpio39).expect("side button G39");
    let buzzer = LedcBuzzer::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio2,
    )
    .expect("buzzer G2 (LEDC)");

    // One monotonic clock, shared by the input thread (writer) and the render loop (reader), so
    // the countdown and the displayed mm:ss are measured on one time base.
    let clock: Monotonic = Monotonic::start();
    let shared: SharedTimer = SharedTimer::new();

    // Input: poll the buttons, step the FSM, sound the jingles. Held for the life of main —
    // dropping it would only detach the thread, which already runs forever.
    let _input = spawn_input(front, side, buzzer, shared.clone(), clock, CLASSIC)
        .expect("spawn pomodoro-input");
    info!("input thread up: front tap = start/pause, front hold = reset, side tap = skip");

    // Display: render the timer view every tick, through the same generic loop the plant
    // monitor uses. The source snapshots the shared timer and turns it into a PomodoroView at
    // `now` — the render loop's own clock, the same Monotonic the input loop steps against.
    let source = {
        let shared: SharedTimer = shared.clone();
        move |now: Tick| PomodoroView::of(&shared.snapshot(), now, CLASSIC)
    };
    let _display = spawn_display(screen, source, clock, DisplayConfig::default())
        .expect("spawn pomodoro-display");
    info!("display thread up: ST7789 rendering mm:ss + the Claude creature");

    // Supervisory loop: a heartbeat only — the input and display threads own the app.
    loop {
        FreeRtos::delay_ms(5_000);
        let timer = shared.snapshot();
        info!("pomodoro: phase {:?}, {:?}", timer.phase(), timer.status());
    }
}
