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
//!   front double-tap restarts the whole session, front hold resets the phase, side tap skips.
//! - [`spawn_display`] (platform-runtime) is the *same* generic render loop the plant monitor
//!   uses, fed a source that snapshots the shared [`Timer`](pomodoro_core::Timer) into a
//!   [`PomodoroView`] each tick, and a [`PanelScreen`] that paints it with
//!   `pomodoro_display::render`.
//!
//! Both loops share one [`Monotonic`] clock, so the countdown the input loop advances and the
//! `mm:ss` the display shows agree. Durations are the classic 25 / 5 / 15 ([`CLASSIC`]); change
//! that one constant (or build a `Durations`) to retune — or to shrink them for a bench test.

use board_support::{internal_i2c, Axp192};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::info;
use platform_adapters::{Axp192PowerSource, GpioButton, LedcBuzzer, Panel, PanelScreen};
use platform_core::Tick;
use platform_runtime::{
    spawn_buzzer, spawn_display, spawn_power_watch, DisplayConfig, Monotonic, PowerWatchConfig,
};
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
    // init and still shows nothing. The AXP192 latches its LDO enables, but this root now
    // *retains* the PMIC past power-on rather than dropping it: the power-watch thread reads
    // VBUS from this same device for the life of the app. The internal bus has no other live
    // runtime consumer, so the watcher owns the `Axp192<I2cDriver>` outright (`I2cDriver` is
    // `Send`) — no `RefCellDevice`, which could not cross into the thread.
    let i2c = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let mut axp: Axp192<_> = Axp192::new(i2c);
    axp.power_on().expect("AXP192 LCD/TFT rail power-on");
    let power_source: Axp192PowerSource<_> = Axp192PowerSource::new(axp);

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
    let _input = spawn_input(front, side, tone.clone(), shared.clone(), clock, CLASSIC)
        .expect("spawn pomodoro-input");
    info!(
        "input thread up: front tap = start/pause, front double-tap = restart session, \
         front hold = reset phase, side tap = skip"
    );

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
