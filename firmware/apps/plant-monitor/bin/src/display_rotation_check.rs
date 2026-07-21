#![forbid(unsafe_code)]
//! display-rotation-check — a bench tool that answers two questions the host cannot:
//! **does the picture land in the right place at every rotation, and does it come out the
//! right way up?**
//!
//! Both faults live below [`DrawTarget`](embedded_graphics::draw_target::DrawTarget), where no
//! `cargo test` can reach. A host framebuffer places every pixel exactly where it was asked to;
//! the glass places them wherever the controller's address window points. Get the CGRAM offset
//! wrong for an orientation and the host suite stays green while the picture sits several
//! pixels off, trailed by a stripe of stale controller memory.
//!
//! The precedent for measuring rather than reasoning is this same panel: `ColorOrder` was
//! derived correctly from the factory driver's MADCTL bit and was still wrong on the glass,
//! because the pixel pipelines around that bit differ between stacks. Rotation bits live in
//! that same register. See `kb/experiments/2026-07-09-panel-colour-order/`.
//!
//! ## Using it
//!
//! ```sh
//! just run-bin display-rotation-check    # then turn the board, and watch
//! just run                               # put the monitor back
//! ```
//!
//! It steps through the four rotations on a timer — it does **not** read the IMU, deliberately.
//! The point is to test the panel in isolation: if this tool and the real app disagree later,
//! the difference is the rotation source, not the glass.
//!
//! ## Reading the glass
//!
//! At each stop the frame names its rotation. Hold the board so that rotation *should* be
//! upright — `DEG0` is the stick held horizontally with the USB-C port to the right, and each
//! step is a further quarter turn — then read two things, independently:
//!
//! - **The white band, for alignment.** It runs flush to all four edges. Even thickness all the
//!   way round means the CGRAM window is right for that orientation. Thinner on one side and
//!   thicker opposite means the window is offset by the difference; a clipped corner square
//!   means the same, larger; a stripe of noise along an edge means it is exposing controller
//!   memory this frame never wrote.
//! - **The corner colours, for which way up.** Correct is RED top-left, GREEN top-right, BLUE
//!   bottom-right, YELLOW bottom-left. Naming the colour in the top-left corner is a complete
//!   answer on its own, and does not require reading text that may be upside down.
//!
//! They fail independently and have different fixes, which is why the frame shows both. An even
//! band with the colours turned is a rotation-mapping bug (`panel_rotation` in
//! `platform-adapters`); a lopsided band is an offset bug.
//!
//! The band is thick and the corner squares are large on purpose. An earlier version drew a
//! 1-pixel outline, which was correct and unreadable on a 1.14" panel behind a bezel — an
//! instrument nobody can read cannot falsify anything, so it was rebuilt around judgements a
//! person makes reliably: comparing two thicknesses, and naming a colour.

use std::cell::RefCell;

use board_support::{internal_i2c, Axp192};
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{info, warn};
use platform_adapters::Panel;
use platform_core::ScreenRotation;

/// How long each rotation is held, in milliseconds.
///
/// Long enough to pick the board up, turn it, and look — this is read by a human with the
/// thing in their hand, not by a scope.
const DWELL_MS: u32 = 4_000;

/// The four stops, with the label each is drawn with.
const STOPS: [(ScreenRotation, &str); 4] = [
    (ScreenRotation::Deg0, "DEG0"),
    (ScreenRotation::Deg90, "DEG90"),
    (ScreenRotation::Deg180, "DEG180"),
    (ScreenRotation::Deg270, "DEG270"),
];

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("display-rotation-check: stepping through four rotations, {DWELL_MS} ms each");

    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // The panel is dark until the AXP192 lights LDO2/LDO3 — the same order the monitor uses,
    // since a self-test that skipped the PMIC would prove nothing about the production path.
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

    let mut display: Panel = Panel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 display bring-up");

    info!("hold the board so each named rotation is upright, and read TWO things:");
    info!("  1. the white band: same thickness all the way round? (lopsided = offset bug)");
    info!("  2. the corners: RED top-left, GREEN top-right, BLUE bottom-right, YELLOW");
    info!("     bottom-left? (turned = mapping bug in panel_rotation)");
    info!("naming the colour in the TOP-LEFT corner is a complete answer on its own");

    loop {
        STOPS
            .into_iter()
            .for_each(|(rotation, label): (ScreenRotation, &str)| {
                info!("--> {label}");
                display
                    .set_rotation(rotation)
                    .expect("turn the panel to the next rotation");
                display
                    .rotation_check(label)
                    .expect("paint the rotation frame");
                FreeRtos::delay_ms(DWELL_MS);
            });
    }
}
