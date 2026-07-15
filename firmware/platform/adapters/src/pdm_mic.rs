//! The SPM1423 PDM microphone (G0 clock / G34 data) as the platform [`AudioIn`] port, via I2S.

use esp_idf_hal::delay::BLOCK;
use esp_idf_hal::gpio::{InputPin, OutputPin};
use esp_idf_hal::i2s::config::{
    Config, DataBitWidth, PdmRxClkConfig, PdmRxConfig, PdmRxGpioConfig, PdmRxSlotConfig,
    PdmSlotMask, SlotMode,
};
use esp_idf_hal::i2s::{I2s, I2sDriver, I2sRx};
use esp_idf_sys::EspError;
use platform_core::AudioIn;

/// The on-board PDM MEMS microphone, read as mono 16-bit PCM over I2S.
///
/// The M5StickC Plus carries an SPM1423 PDM microphone: its clock is driven out on G0 and its
/// one-bit density-modulated data comes back on G34, which the ESP32's I2S peripheral decimates
/// to PCM in hardware. This adapter brings that up in PDM-RX mode and hands the [`AudioIn`] port
/// a block of `i16` samples per [`read`](AudioIn::read); all interpretation (tone detection)
/// lives in the pure `platform-audio` crate, so this stays a thin sample pump.
///
/// The mic and the buzzer (G2, LEDC) are independent peripherals — I2S versus LEDC — so the
/// chime self-test can hold a tone on one while it captures on the other.
pub struct PdmMic<'d> {
    driver: I2sDriver<'d, I2sRx>,
    sample_rate_hz: u32,
}

impl<'d> PdmMic<'d> {
    /// Bring the mic up on `i2s` with its clock on `clk` (G0) and data on `din` (G34), sampling
    /// at `sample_rate_hz`. The receive channel is enabled, so [`read`](AudioIn::read) may be
    /// called at once.
    pub fn new<I2S: I2s + 'd>(
        i2s: I2S,
        clk: impl OutputPin + 'd,
        din: impl InputPin + 'd,
        sample_rate_hz: u32,
    ) -> Result<Self, EspError> {
        // The SPM1423 drives the *right* PDM slot (the factory configures the mic
        // `ONLY_RIGHT`/`ALL_RIGHT` everywhere), but esp-idf-hal's mono helper defaults to the
        // left slot — which reads pure silence. Override the mask to the right slot, or the
        // capture is all zeros and every tone check falsely fails.
        let slot: PdmRxSlotConfig = PdmRxSlotConfig::from_bits_per_sample_and_slot_mode(
            DataBitWidth::Bits16,
            SlotMode::Mono,
        )
        .slot_mode_mask(SlotMode::Mono, PdmSlotMask::Right);
        let config: PdmRxConfig = PdmRxConfig::new(
            Config::default(),
            PdmRxClkConfig::from_sample_rate_hz(sample_rate_hz),
            slot,
            PdmRxGpioConfig::new(false),
        );
        let mut driver: I2sDriver<'d, I2sRx> = I2sDriver::new_pdm_rx(i2s, &config, clk, din)?;
        driver.rx_enable()?;
        Ok(Self {
            driver,
            sample_rate_hz,
        })
    }
}

impl AudioIn for PdmMic<'_> {
    type Error = EspError;

    fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    /// Fill `out` with mono PCM samples, returning how many were written.
    ///
    /// The I2S driver hands back little-endian bytes, so this reads into a bounded byte scratch
    /// and reassembles 16-bit samples with [`i16::from_le_bytes`] — no `unsafe` reinterpret. It
    /// loops until `out` is full because one DMA read may return fewer samples than asked.
    fn read(&mut self, out: &mut [i16]) -> Result<usize, EspError> {
        // 128 samples per DMA read: a small, fixed stack buffer, refilled until `out` is full.
        const CHUNK: usize = 128;
        let mut scratch: [u8; CHUNK * 2] = [0; CHUNK * 2];
        let mut written: usize = 0;

        while written < out.len() {
            let want_samples: usize = (out.len() - written).min(CHUNK);
            let bytes: usize = self.driver.read(&mut scratch[..want_samples * 2], BLOCK)?;
            if bytes == 0 {
                break;
            }
            scratch[..bytes]
                .chunks_exact(2)
                .zip(out[written..].iter_mut())
                .for_each(|(pair, slot): (&[u8], &mut i16)| {
                    *slot = i16::from_le_bytes([pair[0], pair[1]]);
                });
            written += bytes / 2;
        }
        Ok(written)
    }
}
