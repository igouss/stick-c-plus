#![forbid(unsafe_code)]
//! display-colour-check — a bench tool that answers one question: **does this panel
//! render red as red?**
//!
//! Reported 2026-07-09: the new `FAULT` line, the first non-greyscale pixel this
//! firmware has ever drawn, comes out bluish instead of red. Neither white nor black
//! can reveal a red/blue swap — both channels are symmetric in `0xFFFF` and `0x0000`
//! — so a wrong [`ColorOrder`](mipidsi::options::ColorOrder) could sit in
//! `Panel::new` since the display landed and never once be observed.
//!
//! Source archaeology is inconclusive and must not be trusted here. The pinned
//! factory library resolves `TFT_MAD_COLOR_ORDER` to `TFT_MAD_BGR` (its
//! `TFT_RGB_ORDER` is undefined and `CGRAM_OFFSET` is defined), and `mipidsi`'s
//! `ColorOrder::Bgr` sets the very same MADCTL bit 3 — so on paper our init already
//! matches the factory's, yet the glass disagrees. When the paper and the panel
//! disagree, the panel is right.
//!
//! ## Reading the result
//!
//! Three full-width bands are painted through the **production** init path — the same
//! `Panel::new` the monitor calls, not a replica — labelled in white with the
//! colour each is meant to be:
//!
//! - **RED / GREEN / BLUE, top to bottom** — colour order is correct, and the bluish
//!   `FAULT` line has some other cause.
//! - **the top band blue and the bottom band red**, green unchanged — red and blue are
//!   swapped. Green is invariant under that swap, which is exactly what makes the
//!   diagnosis unambiguous. Fix: `ColorOrder::Rgb` in `Panel::new`.
//! - **green renders magenta** — the inversion is wrong, not the order. This should be
//!   impossible while the white labels stay white, which is why they are white.
//!
//! ```sh
//! just run-bin display-colour-check      # look at the glass
//! just run                               # put the monitor back
//! ```

use std::cell::RefCell;

use board_support::{internal_i2c, Axp192};
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{info, warn};
use platform_adapters::Panel;

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("display-colour-check: painting RED / GREEN / BLUE bands, top to bottom");

    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // The panel is dark until the AXP192 lights LDO2/LDO3 — same order the monitor
    // uses, since a self-test that skipped the PMIC would prove nothing.
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

    display.colour_check().expect("paint the colour bands");

    info!("painted. Top band should be RED, middle GREEN, bottom BLUE.");
    info!("If the top reads blue and the bottom red (green unchanged), red and blue");
    info!("are swapped: ColorOrder::Bgr in Panel::new is wrong for this panel.");

    loop {
        FreeRtos::delay_ms(10_000);
    }
}
