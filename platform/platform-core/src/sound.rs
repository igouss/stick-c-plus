//! The buzzer port: a tone sequence an app asks the board to play.

/// One note: a frequency, held for a duration.
///
/// The passive buzzer is a square-wave tone generator, so a note is fully described by its
/// pitch and length. A `freq_hz` of 0 is a rest — silence for `ms` milliseconds — which is
/// how an app spaces two notes without a second concept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Note {
    /// The tone frequency in hertz; 0 is a rest.
    pub freq_hz: u16,
    /// How long the note holds, in milliseconds.
    pub ms: u16,
}

impl Note {
    /// A note at `freq_hz` held for `ms` milliseconds.
    pub const fn new(freq_hz: u16, ms: u16) -> Self {
        Note { freq_hz, ms }
    }

    /// A rest — silence — for `ms` milliseconds.
    pub const fn rest(ms: u16) -> Self {
        Note { freq_hz: 0, ms }
    }
}

/// A buzzer that plays a sequence of [`Note`]s.
///
/// The driven port for sound. The app owns *what* to play — a domain maps an event to a
/// melody — and this adapter only sounds the notes it is handed, one after another. `Error`
/// is associated so the adapter surfaces its own PWM failure without the domain naming it.
pub trait Tone {
    /// The adapter's own play-failure type.
    type Error;

    /// Play `notes` in order, each for its own duration.
    fn play(&mut self, notes: &[Note]) -> Result<(), Self::Error>;
}
