//! `adc` — the M5 Earth Unit soil probe as a [`SoilSensor`], over ADC1.
//!
//! The driven adapter for [`plant_core::SoilSensor`]: it reads the Earth Unit's
//! resistive analog output on GPIO33 (ADC1 channel 5) and hands the domain a raw
//! 12-bit count. All moisture math — the dry/wet calibration curve, temporal
//! averaging, report-on-change — lives inward in `plant-core`; the pure
//! oversampling and power-gating live in `firmware-core`; this adapter is the
//! thin hardware translation that binds them.

use core::num::NonZeroU16;

use esp_idf_hal::adc::attenuation::DB_12;
use esp_idf_hal::adc::oneshot::config::{AdcChannelConfig, Calibration, Resolution};
use esp_idf_hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_hal::adc::{ADC1, ADCCH5, ADCU1};
use esp_idf_hal::gpio::Gpio33;
use esp_idf_sys::EspError;
use firmware_core::{gated_read, oversampled_mean, ProbePower};
use plant_core::SoilSensor;

/// Raw ADC conversions folded into one [`read_raw`](SoilSensor::read_raw).
///
/// 64 — a power of two, so the burst is cheap and the mean divides evenly — is
/// enough multisampling to pull the ESP32 ADC's per-read jitter down to a few
/// counts while completing in well under a millisecond.
pub const SAMPLES: NonZeroU16 = match NonZeroU16::new(64) {
    Some(samples) => samples,
    None => unreachable!(),
};

/// The M5 Earth Unit resistive soil probe, read on ADC1 channel 5 / GPIO33.
///
/// ADC1 is mandatory: ADC2 shares hardware with the WiFi radio and refuses reads
/// while WiFi is up, so a plant monitor — always online — could never sample a
/// probe wired to ADC2. That requirement is enforced by the type, not a comment:
/// the channel is `ADCCH5<ADCU1>`, so constructing this over `peripherals.adc2`
/// does not compile.
///
/// The probe's power is gated through `P`: every read is wrapped in
/// [`firmware_core::gated_read`], so a [`ProbePower`] that actually switches the
/// rail (qhw.31) energizes the electrodes only across the burst — no adapter
/// change needed, just a different `P`.
pub struct EarthUnit<'d, P> {
    channel: AdcChannelDriver<'d, ADCCH5<ADCU1>, AdcDriver<'d, ADCU1>>,
    power: P,
}

impl<'d, P: ProbePower<Error = EspError>> EarthUnit<'d, P> {
    /// Bring up ADC1, bind GPIO33 at 12 dB attenuation, and take ownership of the
    /// probe's power mechanism.
    ///
    /// 12 dB (the widest attenuation) maps the input to roughly 0–3.1 V, which
    /// covers the Earth Unit's full analog swing; a narrower range would clip a
    /// dry (high-voltage) reading.
    ///
    /// Calibration is deliberately [`None`](Calibration::None): the port's
    /// contract is *raw counts*, and the domain pins the dry/wet endpoints itself
    /// ([`plant_core::moisture::Calibration`]). esp-idf-hal 0.46 *does* expose
    /// ESP32 line-fitting calibration (`Calibration::Line`, backed by the eFuse
    /// Vref) — this bead's open item — but it only converts a raw count to
    /// millivolts, which the raw-count contract neither needs nor wants.
    /// (Curve-fitting calibration is not offered on the plain ESP32; it is a
    /// C3/C6/S3 feature.)
    pub fn new(adc1: ADC1<'d>, pin: Gpio33<'d>, power: P) -> Result<Self, EspError> {
        let driver: AdcDriver<'d, ADCU1> = AdcDriver::new(adc1)?;
        let config: AdcChannelConfig = AdcChannelConfig {
            attenuation: DB_12,
            resolution: Resolution::new(),
            calibration: Calibration::None,
        };
        let channel: AdcChannelDriver<'d, ADCCH5<ADCU1>, AdcDriver<'d, ADCU1>> =
            AdcChannelDriver::new(driver, pin, &config)?;
        Ok(Self { channel, power })
    }
}

/// One [`read_raw`](SoilSensor::read_raw) energizes the probe, takes the mean of
/// [`SAMPLES`] rapid conversions, and de-energizes — a single denoised count in
/// `0..=`[`MAX_READING`](plant_core::ports::MAX_READING).
///
/// The gating ([`gated_read`]) powers the electrodes only across the burst so a
/// 24/7 monitor doesn't electrolyze them; the oversampling ([`oversampled_mean`])
/// suppresses per-conversion electrical noise. Both are pure and host-tested in
/// `firmware-core`; the temporal averaging in [`plant_core::sampler::step`] —
/// smoothing readings taken seconds apart — stays a separate, inward concern.
impl<P: ProbePower<Error = EspError>> SoilSensor for EarthUnit<'_, P> {
    type Error = EspError;

    fn read_raw(&mut self) -> Result<u16, Self::Error> {
        let Self { channel, power } = self;
        gated_read(power, || oversampled_mean(SAMPLES, || channel.read_raw()))
    }
}
