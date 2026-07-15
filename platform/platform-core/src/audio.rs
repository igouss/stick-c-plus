//! The microphone port: a source of mono PCM audio samples.

/// A microphone that yields mono PCM audio samples.
///
/// The driven port for audio capture. The firmware adapter (the M5StickC Plus PDM mic) fills a
/// caller's buffer with signed 16-bit samples; all interpretation — tone detection, energy
/// analysis — lives in the pure `platform-audio` crate, so this port stays a thin transfer of
/// samples and every acoustic rule is decided on the host. The sample rate travels with the
/// port because samples are meaningless in time without it: a tone detector needs it to turn a
/// frequency into a bin.
pub trait AudioIn {
    /// The adapter's own capture-failure type.
    type Error;

    /// The rate the samples are captured at, in hertz.
    fn sample_rate_hz(&self) -> u32;

    /// Fill `out` with the next block of mono samples, returning how many were written.
    fn read(&mut self, out: &mut [i16]) -> Result<usize, Self::Error>;
}
