//! Which chime a USB power transition makes — the domain's choice, not the buzzer's.

use crate::Note;

/// The chime a settled VBUS transition plays. [`edge`] decides *whether* and *which*; the
/// buzzer [`Tone`](crate::Tone) adapter only sounds the [`Note`]s it maps to — mirrors
/// `pomodoro_core::Jingle`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerChime {
    /// Battery -> USB: a rising contour.
    SpoolUp,
    /// USB -> battery: the descending mirror of [`SpoolUp`](PowerChime::SpoolUp).
    SpoolDown,
}

// The note tables. Each pitch sits in the ~2–9 kHz band the M5StickC Plus buzzer is measured
// loud across (see AUDIBLE_HZ in the tests below), the same band `pomodoro_core::jingle`
// already proves out — that tiny passive transducer renders these as loud broadband beeps a
// listener tells apart by contour, not tune. Spool-up climbs; spool-down is spool-up played
// backwards, so a plug-in and an unplug are opposite shapes the ear reads without looking.
const SPOOL_UP: [Note; 3] = [
    Note::new(2637, 70),  // E7
    Note::new(3136, 70),  // G7
    Note::new(4186, 110), // C8 — the top of the rising contour
];
const SPOOL_DOWN: [Note; 3] = [
    Note::new(4186, 70),  // C8
    Note::new(3136, 70),  // G7
    Note::new(2637, 110), // E7 — the bottom of the falling contour
];

impl PowerChime {
    /// Every chime, for iterating over the whole set.
    pub const ALL: [PowerChime; 2] = [PowerChime::SpoolUp, PowerChime::SpoolDown];

    /// The notes this chime plays, in order.
    pub const fn notes(self) -> &'static [Note] {
        match self {
            PowerChime::SpoolUp => &SPOOL_UP,
            PowerChime::SpoolDown => &SPOOL_DOWN,
        }
    }
}

/// The pure edge decision: a rising level (`false -> true`) plays
/// [`PowerChime::SpoolUp`], a falling level plays [`PowerChime::SpoolDown`], and an
/// unchanged level plays nothing.
///
/// Stateless and total: the caller (a debounced watch loop) supplies the last-known and
/// current *settled* levels — nothing here reads a clock, a pin, or a stream. The very first
/// sample a watch loop ever takes is never passed through `edge` at all; it seeds the
/// baseline instead, which is how a boot stays silent regardless of which side it starts on.
pub fn edge(prev: bool, now: bool) -> Option<PowerChime> {
    match (prev, now) {
        (false, true) => Some(PowerChime::SpoolUp),
        (true, false) => Some(PowerChime::SpoolDown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The band the M5StickC Plus buzzer is measured loud across; a note must sit here to
    /// sound (mirrors `pomodoro_core::jingle`'s `AUDIBLE_HZ`).
    const AUDIBLE_HZ: core::ops::RangeInclusive<u16> = 2_000..=9_000;

    // --- AC1 / AC2 / AC3 — the Gherkin scenarios, pinned directly on `edge`. ---

    /// AC1 — Plugged in -> spool up: a rising edge decides `SpoolUp`, and nothing else (the
    /// return is a single `Option`, so "nothing else" is the type itself).
    #[test]
    fn plugged_in_decides_spool_up() {
        assert_eq!(edge(false, true), Some(PowerChime::SpoolUp));
    }

    /// AC2 — Unplugged -> spool down: a falling edge decides `SpoolDown`.
    #[test]
    fn unplugged_decides_spool_down() {
        assert_eq!(edge(true, false), Some(PowerChime::SpoolDown));
    }

    /// AC3 (zero case) — an unchanged level, starting on battery, decides no chime.
    #[test]
    fn staying_on_battery_decides_no_chime() {
        assert_eq!(edge(false, false), None);
    }

    /// AC3 (zero case), the other steady state — staying on USB decides no chime either.
    #[test]
    fn staying_on_usb_decides_no_chime() {
        assert_eq!(edge(true, true), None);
    }

    // --- AC5 — every tone is audible on this buzzer. ---

    /// AC5 — every note frequency of every chime lies within 2000..=9000 Hz. Rests (0 Hz)
    /// are exempt — they are meant to be silent, exactly as `pomodoro_core::jingle` treats
    /// them.
    #[test]
    fn every_tone_is_in_the_audible_band() {
        PowerChime::ALL.iter().for_each(|chime: &PowerChime| {
            chime
                .notes()
                .iter()
                .filter(|n: &&Note| n.freq_hz > 0)
                .for_each(|n: &Note| {
                    assert!(
                        AUDIBLE_HZ.contains(&n.freq_hz),
                        "{chime:?} has a {} Hz note outside the audible band {AUDIBLE_HZ:?}",
                        n.freq_hz
                    );
                });
        });
    }

    // --- AC6 — up and down are opposite contours, and distinct. ---

    /// The non-rest frequencies of a chime's notes, in order — the shape a listener hears.
    fn contour(chime: PowerChime) -> Vec<u16> {
        chime
            .notes()
            .iter()
            .filter(|n: &&Note| n.freq_hz > 0)
            .map(|n: &Note| n.freq_hz)
            .collect()
    }

    /// AC6 — spool-up ascends: each note sits higher than the one before it.
    #[test]
    fn spool_up_ascends() {
        let freqs: Vec<u16> = contour(PowerChime::SpoolUp);
        assert!(
            freqs.windows(2).all(|w: &[u16]| w[0] < w[1]),
            "spool-up must strictly ascend: {freqs:?}"
        );
    }

    /// AC6 — spool-down descends: the mirror of spool-up's contour.
    #[test]
    fn spool_down_descends() {
        let freqs: Vec<u16> = contour(PowerChime::SpoolDown);
        assert!(
            freqs.windows(2).all(|w: &[u16]| w[0] > w[1]),
            "spool-down must strictly descend: {freqs:?}"
        );
    }

    /// AC6 — the two melodies are distinct, so the ear tells a plug-in from an unplug.
    #[test]
    fn spool_up_and_spool_down_are_distinct() {
        assert_ne!(PowerChime::SpoolUp.notes(), PowerChime::SpoolDown.notes());
    }

    // --- Property tests: the general laws. ---

    proptest! {
        /// P1 — `edge` fires a chime iff the boolean state changed, for every possible pair.
        #[test]
        fn edge_fires_iff_the_state_changed(prev in any::<bool>(), now in any::<bool>()) {
            prop_assert_eq!(edge(prev, now).is_some(), prev != now);
        }
    }

    /// P3 — the spool-down contour is exactly the reverse of the spool-up contour: the same
    /// shape, mirrored.
    #[test]
    fn spool_down_is_the_reverse_shape_of_spool_up() {
        let up: Vec<u16> = contour(PowerChime::SpoolUp);
        let reversed_up: Vec<u16> = up.into_iter().rev().collect();
        assert_eq!(contour(PowerChime::SpoolDown), reversed_up);
    }
}
