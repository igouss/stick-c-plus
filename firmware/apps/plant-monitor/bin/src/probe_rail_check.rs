#![forbid(unsafe_code)]
//! probe-rail-check — a bench tool that answers one question: **does the AXP192's
//! EXTEN bit actually gate the Grove 5 V rail that powers the Earth Unit?**
//!
//! That question blocks qhw.31 (probe power-gating). If EXTEN cuts the rail, the
//! gating is pure firmware and drops straight into the existing `ProbePower` seam.
//! If it does not, the Grove 5 V line needs an external high-side switch, and
//! qhw.31 becomes a hardware change.
//!
//! ## Why the ADC can answer it without a multimeter
//!
//! The Earth Unit regulates Grove 5 V down to 3.3 V locally (HT7533) and pulls its
//! analog output up to that rail through a 10 kΩ resistor (R1); the soil sits in
//! the *lower* leg of the divider, between the output node and ground. So the
//! reading on GPIO33 tracks the probe's own supply:
//!
//! - **Rail up, electrodes open** (a corroded or dry probe) — the node is pulled to
//!   3.3 V through R1, above the ADC's 12 dB ceiling, so it reads a saturated 4095.
//! - **Rail down** — R1 has nothing to pull up to, and the node collapses.
//!
//! A probe whose electrodes have corroded open is therefore the *ideal* fixture for
//! this measurement: it pins the node hard at the rail, so `raw` is a clean
//! read-out of "is the probe powered", with the soil contributing nothing. Run it
//! with the dead probe still in the soil and the Grove cable undisturbed — changing
//! the rail and the probe's environment at once would confound the result.
//!
//! ## Reading the output
//!
//! A clean `4095 → collapse → 4095` across every cycle means EXTEN gates the rail.
//! The rise sweep also measures the **settle delay** `ProbePower::energize` owes its
//! caller: the first offset at which `raw` has stopped climbing. Because the soil
//! leg is open, that settle is an upper bound — a conducting probe's node is a
//! stiffer divider and settles sooner.
//!
//! A *negative* result is weaker than a positive one. If `raw` never falls, either
//! EXTEN does not gate this rail, or Grove 5 V is fed from VBUS while USB is
//! plugged in. Re-run on battery before concluding anything, and reach for the
//! meter if it still will not budge.
//!
//! ```sh
//! cd firmware && cargo run --release -p plant-monitor --bin probe-rail-check
//! ```

use adapters::adc::SAMPLES;
use board_support::{internal_i2c, Axp192};
use esp_idf_hal::adc::attenuation::DB_12;
use esp_idf_hal::adc::oneshot::config::{AdcChannelConfig, Calibration, Resolution};
use esp_idf_hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_hal::adc::{ADCCH5, ADCU1};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use firmware_core::oversampled_mean;
use log::{info, warn};

/// How many energize/de-energize cycles to run. Three, so a one-off transient
/// cannot masquerade as a repeatable effect.
const CYCLES: u32 = 3;

/// Cumulative offsets (ms) after `EXTEN <- 0` at which the node is sampled.
///
/// Runs out to two seconds: the probe's 10 µF reservoir caps bleed down through
/// R1 and the LM393's quiescent draw, which is far slower than the rise.
const DECAY_OFFSETS_MS: &[u32] = &[0, 10, 50, 100, 250, 500, 1000, 2000];

/// Cumulative offsets (ms) after `EXTEN <- 1` at which the node is sampled.
///
/// The settle delay `energize` must honour is the first offset at which the
/// reading has plateaued. Runs out to four seconds because the 5 V boost
/// soft-starts: measured on an open-electrode probe — where the node carries no
/// current through R1 and therefore reports the probe's 3.3 V rail directly — the
/// rail was still climbing at 500 ms.
const RISE_OFFSETS_MS: &[u32] = &[
    0, 1, 2, 5, 10, 20, 50, 100, 200, 500, 750, 1000, 1500, 2000, 3000, 4000,
];

/// The ADC channel the Earth Unit's analog output lands on: ADC1 ch5 / GPIO33.
type ProbeChannel<'d> = AdcChannelDriver<'d, ADCCH5<ADCU1>, AdcDriver<'d, ADCU1>>;

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("probe-rail-check: does AXP192 EXTEN gate the Grove 5 V rail? (qhw.31)");

    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // The PMIC owns the internal bus outright here — this tool drives nothing else
    // on it, so there is no bus to share and no RefCell/Mutex to justify.
    let i2c: I2cDriver<'static> = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let mut axp: Axp192<I2cDriver<'static>> = Axp192::new(i2c);
    axp.power_on().expect("AXP192 power-on");

    let config: AdcChannelConfig = AdcChannelConfig {
        attenuation: DB_12,
        resolution: Resolution::new(),
        calibration: Calibration::None,
    };
    let driver: AdcDriver<'static, ADCU1> =
        AdcDriver::new(peripherals.adc1).expect("ADC1 bring-up");
    let mut channel: ProbeChannel<'static> =
        AdcChannelDriver::new(driver, peripherals.pins.gpio33, &config).expect("GPIO33 bind");

    let baseline: u16 = sample(&mut channel);
    let exten: bool = axp.exten_enabled().expect("read EXTEN");
    let reg: u8 = axp.power_output().expect("read reg 0x12");
    info!("baseline: raw={baseline} exten={exten} reg_0x12=0x{reg:02X}");
    if baseline != 4095 {
        warn!(
            "baseline raw={baseline}, expected a saturated 4095 — this tool assumes an \
             open-electrode probe pinning the node at its rail; a conducting probe still \
             works, but read the collapse as a *shift*, not a fall to the floor"
        );
    }

    let mut cycle: u32 = 0;
    while cycle < CYCLES {
        cycle += 1;
        info!("── cycle {cycle}/{CYCLES} ─────────────────────────────");

        axp.set_exten(false).expect("EXTEN <- 0");
        report(&mut axp, "off");
        sweep(&mut channel, "off", DECAY_OFFSETS_MS);

        axp.set_exten(true).expect("EXTEN <- 1");
        report(&mut axp, "on");
        sweep(&mut channel, "on ", RISE_OFFSETS_MS);
    }

    // Leave the board as we found it: rail up, so a subsequent plant-monitor flash
    // (or a bare reboot into the old image) starts from the documented state.
    axp.set_exten(true).expect("restore EXTEN");
    info!("done — EXTEN restored. Rail gates iff raw collapsed with exten=false above.");

    loop {
        FreeRtos::delay_ms(10_000);
    }
}

/// One denoised reading, folded exactly the way `adapters::adc::EarthUnit` folds
/// its own — same burst width, same reduction — so these numbers are comparable
/// with the `raw` the plant-monitor logs and renders.
fn sample(channel: &mut ProbeChannel<'_>) -> u16 {
    oversampled_mean(SAMPLES, || channel.read_raw()).expect("ADC read")
}

/// Log the register the toggle just wrote, read back over the wire rather than
/// assumed — a blind write that the PMIC NAKed would otherwise read as success.
fn report(axp: &mut Axp192<I2cDriver<'static>>, want: &str) {
    match (axp.exten_enabled(), axp.power_output()) {
        (Ok(exten), Ok(reg)) => {
            info!("  exten<-{want}: read back exten={exten} reg_0x12=0x{reg:02X}")
        }
        _ => warn!("  exten<-{want}: read-back failed"),
    }
}

/// Sample the node at each cumulative offset, sleeping only the gap between them.
fn sweep(channel: &mut ProbeChannel<'_>, rail: &str, offsets_ms: &[u32]) {
    let mut elapsed_ms: u32 = 0;
    for &offset_ms in offsets_ms {
        FreeRtos::delay_ms(offset_ms - elapsed_ms);
        elapsed_ms = offset_ms;
        let raw: u16 = sample(channel);
        info!("  rail={rail} t=+{offset_ms:>4}ms raw={raw}");
    }
}
