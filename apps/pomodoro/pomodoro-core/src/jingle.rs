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
}

// The note tables. Frequencies are rough musical pitches (Hz); a `rest` spaces two notes.
// Kept small and const so they cost only `.rodata` and nothing to build at boot.
const FOCUS_START: [Note; 3] = [
    Note::new(880, 90),
    Note::new(1175, 90),
    Note::new(1568, 150),
];
const BREAK_START: [Note; 2] = [Note::new(784, 120), Note::new(523, 180)];
const LONG_BREAK_START: [Note; 4] = [
    Note::new(784, 120),
    Note::new(659, 120),
    Note::new(523, 120),
    Note::new(392, 220),
];
const PHASE_COMPLETE: [Note; 3] = [Note::new(1047, 130), Note::rest(70), Note::new(1047, 220)];

impl Jingle {
    /// The notes this jingle plays, in order.
    pub const fn notes(self) -> &'static [Note] {
        match self {
            Jingle::FocusStart => &FOCUS_START,
            Jingle::BreakStart => &BREAK_START,
            Jingle::LongBreakStart => &LONG_BREAK_START,
            Jingle::PhaseComplete => &PHASE_COMPLETE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every jingle sounds *something* — an empty melody would be a silent transition, which
    /// defeats the point of a buzzer that tells you a phase changed with your eyes closed.
    #[test]
    fn every_jingle_has_notes() {
        let all: [Jingle; 4] = [
            Jingle::FocusStart,
            Jingle::BreakStart,
            Jingle::LongBreakStart,
            Jingle::PhaseComplete,
        ];
        all.iter().for_each(|jingle: &Jingle| {
            assert!(!jingle.notes().is_empty(), "{jingle:?} is silent");
            assert!(
                jingle.notes().iter().any(|n: &Note| n.freq_hz > 0),
                "{jingle:?} is all rests"
            );
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
