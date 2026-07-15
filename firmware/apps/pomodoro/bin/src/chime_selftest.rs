#![forbid(unsafe_code)]
//! chime-selftest — an **acoustic loopback**: is the buzzer actually audible through the mic?
//!
//! The pomodoro's jingles are meant to be heard, and "it plays" and "you can hear it" are
//! different claims — the second needs ears. This bench tool lends the board its own: it plays
//! every note of every [`Jingle`] on the buzzer (G2, LEDC) while capturing the on-board PDM
//! microphone (G0/G34, I2S), and measures how much louder the capture gets. The buzzer and the
//! mic are millimetres apart, so a working chime dwarfs the silent floor.
//!
//! ## Why level, not pitch
//!
//! The M5StickC Plus buzzer is a tiny passive transducer, not a speaker: driven at a note's
//! frequency it does **not** radiate a clean tone at that pitch — its resonance suppresses the
//! fundamental and reshapes the sound into higher harmonics (measured on-device: a 2637 Hz drive
//! comes back peaking near 6.2 kHz). So "is there energy at the commanded frequency?" is the
//! wrong question here; "did the sound get much louder when the note played?" is the right one.
//! This tool answers the second: it reads the silent floor's acoustic level ([`ac_rms`], the
//! DC-removed RMS), then plays each note and passes it if the level clears that floor by a wide
//! margin — and an absolute minimum, so a dead mic can't pass by clearing a near-zero threshold.
//!
//! A pass proves the buzzer emits audible acoustic energy on command; it does not judge whether
//! the chime sounds pleasant, and (by the physics above) not that it is on-pitch.
//!
//! ```sh
//! cd firmware && cargo run --release -p pomodoro --bin chime-selftest
//! ```

use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use log::{info, warn};
use platform_adapters::{LedcBuzzer, PdmMic};
use platform_audio::{ac_rms, present};
use platform_core::AudioIn;
use pomodoro_core::Jingle;

/// Mic sample rate. 44.1 kHz is the rate every factory mic config uses — it drives the SPM1423's
/// PDM clock (~2.8 MHz) into the mic's valid range, where 16 kHz (~1 MHz) is marginal and reads
/// silence. Measured on-device, the capture holds this rate to within 0.2 %.
const SAMPLE_RATE_HZ: u32 = 44_100;

/// Samples captured per note — 2048 at 44.1 kHz is ~46 ms, plenty to measure a steady level.
const CAPTURE_SAMPLES: usize = 2_048;

/// How long the tone sounds before the capture starts, letting it establish past its onset.
const SETTLE_MS: u32 = 20;

/// Blocks to read and discard before measuring anything: the PDM capture carries a large DC
/// transient at bring-up that decays over the first few reads. Measuring the floor before it
/// settles would inflate it and make every note look quiet by comparison.
const WARMUP_BLOCKS: usize = 8;

/// A note must be at least this many times louder than the silent floor to count as heard — the
/// relative guard, so a noisier room raises the bar with it.
const MARGIN: f32 = 8.0;

/// ...and at least this loud in absolute terms, so a silent capture (dead mic: floor ≈ 0) can't
/// pass a note by clearing a near-zero threshold. Sits well above the ~15 RMS warmed-up noise
/// floor and far below the several-thousand RMS of a sounding note.
const MIN_LEVEL: f32 = 300.0;

/// A warmed-up floor above this means the mic never settled (stuck, DC, wrong slot); the checks
/// below would still run, but their verdicts are suspect, so it is called out.
const SUSPECT_FLOOR: f32 = 200.0;

/// Sound `freq_hz` on the buzzer, capture the mic while it holds, silence it, and return the
/// capture's acoustic level.
fn probe_note(
    buzzer: &mut LedcBuzzer<'_, impl esp_idf_hal::ledc::SpeedMode>,
    mic: &mut PdmMic<'_>,
    buf: &mut [i16],
    freq_hz: u16,
) -> Result<f32, esp_idf_sys::EspError> {
    buzzer.start(freq_hz)?;
    FreeRtos::delay_ms(SETTLE_MS);
    let captured: usize = mic.read(buf)?;
    buzzer.silence()?;
    Ok(ac_rms(&buf[..captured]))
}

fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("chime-selftest: acoustic loopback — buzzer (G2) heard through the PDM mic (G0/G34)");

    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");
    let mut buzzer: LedcBuzzer<'_, _> = LedcBuzzer::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio2,
    )
    .expect("buzzer G2 (LEDC)");
    let mut mic: PdmMic = PdmMic::new(
        peripherals.i2s0,
        peripherals.pins.gpio0,  // PDM clock
        peripherals.pins.gpio34, // PDM data
        SAMPLE_RATE_HZ,
    )
    .expect("PDM mic G0/G34 (I2S)");
    // On the heap: a 2048-sample block is 4 KiB, too much for the ESP-IDF main-task stack.
    let mut buf: Vec<i16> = vec![0; CAPTURE_SAMPLES];

    // Warm the mic: the first captures carry a bring-up DC transient that decays after a few
    // reads. Discard them so the floor below is the true settled noise floor.
    for _ in 0..WARMUP_BLOCKS {
        let _ = mic.read(&mut buf);
    }

    // The silent floor: with the buzzer off, the acoustic level the notes must rise above. This
    // is the guard that the whole test *can* fail — a note is judged only against this floor.
    let floor: f32 = match mic.read(&mut buf) {
        Ok(captured) => ac_rms(&buf[..captured]),
        Err(err) => {
            warn!("floor capture failed: {err} — treating as unusable");
            f32::NAN
        }
    };
    let threshold: f32 = (floor * MARGIN).max(MIN_LEVEL);
    info!("silent floor: rms={floor:.1} → pass threshold {threshold:.1} (max of {MARGIN}× floor and {MIN_LEVEL})");
    if floor.is_nan() || floor > SUSPECT_FLOOR {
        warn!("floor {floor:.1} is not a settled quiet reading (≤ {SUSPECT_FLOOR}) — the mic may be stuck, DC-biased, or on the wrong slot; verdicts below are suspect");
    }

    // Every note of every jingle, played and heard. Rests (0 Hz) sound nothing, so skip them.
    let mut passed: u32 = 0;
    let mut total: u32 = 0;
    for jingle in Jingle::ALL {
        for (index, note) in jingle.notes().iter().enumerate() {
            if note.freq_hz == 0 {
                continue;
            }
            total += 1;
            match probe_note(&mut buzzer, &mut mic, &mut buf, note.freq_hz) {
                Ok(level) => {
                    let heard: bool = present(level, threshold);
                    passed += u32::from(heard);
                    let verdict: &str = if heard { "PASS" } else { "FAIL" };
                    info!(
                        "{jingle:?}[{index}] {} Hz: level={level:.1} (floor {floor:.1}) {verdict}",
                        note.freq_hz
                    );
                }
                Err(err) => warn!(
                    "{jingle:?}[{index}] {} Hz: probe error: {err}",
                    note.freq_hz
                ),
            }
        }
    }

    info!(
        "chime-selftest: {passed}/{total} notes heard (threshold {threshold:.1}, floor {floor:.1})"
    );
    if passed < total {
        warn!("not every chime was heard — check the buzzer, raise volume, or calibrate MARGIN/MIN_LEVEL above the floor");
    }

    // Bench tool: the result is in the log above; idle so the monitor stays attached.
    loop {
        FreeRtos::delay_ms(5_000);
    }
}
