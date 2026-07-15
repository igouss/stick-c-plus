//! Which sound a phase transition makes — the domain's choice, not the buzzer's.

use platform_core::Note;

/// The sound a transition plays. The domain decides *which* jingle a transition makes; the
/// buzzer [`Tone`](platform_core::Tone) adapter only sounds the [`Note`]s it maps to, so the
/// melody is host-testable and the adapter stays a thin PWM driver.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Jingle {
    /// A focus pomodoro begins — a rising call to attention.
    FocusStart,
    /// A short break begins — a gentle falling pair.
    BreakStart,
    /// A long break begins — a longer, descending "you earned it".
    LongBreakStart,
    /// A phase's countdown reached zero — a distinct double chime.
    PhaseComplete,
    /// The whole session was restarted — a short two-note "back to the start".
    SessionRestart,
}

// The note tables. Frequencies are the intended musical pitches (Hz); a `rest` spaces two notes.
// Kept small and const so they cost only `.rodata` and nothing to build at boot.
//
// A caveat the hardware forces: the M5StickC Plus buzzer is a tiny passive transducer, not a
// speaker. Measured on-device (the chime self-test), it is plenty loud clear across ~2–9 kHz,
// but it radiates almost none of its energy at the frequency it is driven — a few percent at
// best, and only above ~8 kHz. So it renders these melodies as loud, broadband beeps that a
// listener tells apart by their rhythm and note count, not their tune. The pitches are kept in
// that loud band and shaped as a rising/falling contour anyway: honest musical intent that a
// better speaker would voice, and loud, distinct beeps on the buzzer we have (C7 = 2093 Hz …
// C8 = 4186 Hz).
const FOCUS_START: [Note; 3] = [
    Note::new(2637, 90),  // E7
    Note::new(3136, 90),  // G7
    Note::new(4186, 150), // C8 — top of the rising contour
];
const BREAK_START: [Note; 2] = [Note::new(3520, 120), Note::new(2637, 180)]; // A7 -> E7
const LONG_BREAK_START: [Note; 4] = [
    Note::new(4186, 120), // C8
    Note::new(3520, 120), // A7
    Note::new(3136, 120), // G7
    Note::new(2637, 220), // E7 — a settling descent
];
const PHASE_COMPLETE: [Note; 3] = [Note::new(4186, 130), Note::rest(70), Note::new(4186, 220)]; // C8·C8
const SESSION_RESTART: [Note; 2] = [Note::new(4186, 80), Note::new(3136, 140)]; // C8 -> G7

impl Jingle {
    /// Every jingle, for iterating over the whole set — the chime self-test plays each one's
    /// notes and checks they sound, and the tests assert set-wide properties over it.
    pub const ALL: [Jingle; 5] = [
        Jingle::FocusStart,
        Jingle::BreakStart,
        Jingle::LongBreakStart,
        Jingle::PhaseComplete,
        Jingle::SessionRestart,
    ];

    /// The notes this jingle plays, in order.
    pub const fn notes(self) -> &'static [Note] {
        match self {
            Jingle::FocusStart => &FOCUS_START,
            Jingle::BreakStart => &BREAK_START,
            Jingle::LongBreakStart => &LONG_BREAK_START,
            Jingle::PhaseComplete => &PHASE_COMPLETE,
            Jingle::SessionRestart => &SESSION_RESTART,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The band the M5StickC Plus buzzer is measured loud across; a note must sit here to sound.
    const AUDIBLE_HZ: core::ops::RangeInclusive<u16> = 2_000..=9_000;

    /// Every jingle sounds *something* — an empty melody would be a silent transition, which
    /// defeats the point of a buzzer that tells you a phase changed with your eyes closed.
    #[test]
    fn every_jingle_has_notes() {
        Jingle::ALL.iter().for_each(|jingle: &Jingle| {
            assert!(!jingle.notes().is_empty(), "{jingle:?} is silent");
            assert!(
                jingle.notes().iter().any(|n: &Note| n.freq_hz > 0),
                "{jingle:?} is all rests"
            );
        });
    }

    /// Every tone sits in the band the buzzer is measured loud across (~2–9 kHz). The check
    /// guards against a note drifting out of it — too low to move this tiny passive transducer,
    /// or past the LEDC's ~9.7 kHz 13-bit ceiling — either of which a listener would report as
    /// "the buzzer is broken". Rests (0 Hz) are exempt — they are meant to be silent.
    #[test]
    fn every_tone_is_in_the_audible_band() {
        Jingle::ALL.iter().for_each(|jingle: &Jingle| {
            jingle
                .notes()
                .iter()
                .filter(|n: &&Note| n.freq_hz > 0)
                .for_each(|n: &Note| {
                    assert!(
                        AUDIBLE_HZ.contains(&n.freq_hz),
                        "{jingle:?} has a {} Hz note outside the audible band {AUDIBLE_HZ:?}",
                        n.freq_hz
                    );
                });
        });
    }

    /// The three phase-start jingles are distinct melodies, so the ear can tell a focus start
    /// from a short break from the long one without looking at the screen.
    #[test]
    fn the_start_jingles_are_distinct() {
        let focus: &[Note] = Jingle::FocusStart.notes();
        let short: &[Note] = Jingle::BreakStart.notes();
        let long: &[Note] = Jingle::LongBreakStart.notes();
        assert_ne!(focus, short);
        assert_ne!(short, long);
        assert_ne!(focus, long);
    }
}
