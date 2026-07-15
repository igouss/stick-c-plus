//! The passive buzzer (G2) as the platform [`Tone`] port, via LEDC PWM.

use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::ledc::config::TimerConfig;
use esp_idf_hal::ledc::{
    LedcChannel, LedcDriver, LedcTimer, LedcTimerDriver, Resolution, SpeedMode,
};
use esp_idf_hal::units::FromValueType;
use esp_idf_sys::EspError;
use platform_core::{Note, Tone};

/// The timer's placeholder frequency at bring-up; the duty starts at 0, so nothing sounds
/// until the first note sets both.
const IDLE_HZ: u32 = 1_000;

/// PWM resolution — 13 bits, matching the M5 factory `Speaker` driver. A square wave only
/// needs a 50 % duty, so the resolution is not load-bearing; it is pinned for parity.
const RESOLUTION: Resolution = Resolution::Bits13;

/// The passive buzzer on G2, driven as an LEDC PWM square wave.
///
/// A passive buzzer is a tone generator: a note is a square wave at the note's frequency, so
/// this sets the LEDC timer's frequency per note and the channel's duty to 50 % (0 for a
/// rest). The app owns the melody (`pomodoro_core::Jingle`); this adapter only sounds the
/// [`Note`]s it is handed, one after another, blocking for each note's duration — so a
/// composition root plays a jingle on a short dedicated thread, off the render loop.
pub struct LedcBuzzer<'d, S: SpeedMode> {
    timer: LedcTimerDriver<'d, S>,
    channel: LedcDriver<'d>,
}

impl<'d, S: SpeedMode> LedcBuzzer<'d, S> {
    /// Bring up the buzzer on `timer`/`channel`/`pin` (G2), starting silent.
    pub fn new<T, C>(
        timer: T,
        channel: C,
        pin: impl esp_idf_hal::gpio::OutputPin + 'd,
    ) -> Result<Self, EspError>
    where
        T: LedcTimer<SpeedMode = S> + 'd,
        C: LedcChannel<SpeedMode = S> + 'd,
    {
        let timer: LedcTimerDriver<'d, S> = LedcTimerDriver::new(
            timer,
            &TimerConfig::default()
                .frequency(IDLE_HZ.Hz())
                .resolution(RESOLUTION),
        )?;
        let mut channel: LedcDriver<'d> = LedcDriver::new(channel, &timer, pin)?;
        channel.set_duty(0)?;
        Ok(Self { timer, channel })
    }

    /// Sound one note: set the frequency and a 50 % duty for a tone, or duty 0 for a rest, then
    /// hold for the note's duration.
    fn sound(&mut self, note: Note) -> Result<(), EspError> {
        if note.freq_hz > 0 {
            self.timer.set_frequency(u32::from(note.freq_hz).Hz())?;
            self.channel.set_duty(self.channel.get_max_duty() / 2)?;
        } else {
            self.channel.set_duty(0)?;
        }
        FreeRtos::delay_ms(u32::from(note.ms));
        Ok(())
    }
}

impl<S: SpeedMode> Tone for LedcBuzzer<'_, S> {
    type Error = EspError;

    /// Play `notes` in order, then fall silent.
    fn play(&mut self, notes: &[Note]) -> Result<(), EspError> {
        notes.iter().try_for_each(|note: &Note| self.sound(*note))?;
        self.channel.set_duty(0)
    }
}
