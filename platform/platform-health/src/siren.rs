//! Which sound each fault kind makes — the alarm's own vocabulary, alongside
//! `platform_core::PowerChime` and `pomodoro_core::Jingle`.

use platform_core::Note;

use crate::fault::FaultKind;

/// The chirp a fault kind sounds. One per [`FaultKind`], so the ear tells a crash from a stall
/// by contour, the same way `Jingle` already lets a listener tell phases apart.
///
/// Chirps are short (two notes, ~60-80 ms each) because they repeat on a cadence rather than
/// play once: a long melody at cadence would be the continuous tone the alarm's cadence design
/// exists to avoid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Siren {
    Crash,
    Starve,
    GaveUp,
    Stall,
}

// The note tables, in the same shape and the same 2-9 kHz band as `platform_core::chime`'s.
// Four contours the ear separates without looking, ranked the way the faults are: the crash
// plunges the whole band in one step, starvation falls a third of that, a component reporting
// its own fault is starvation's rising mirror, and a stall — the mildest, and the one most
// likely to be repeating — holds flat at the bottom. Two notes each, ~70-80 ms, because they
// repeat on a cadence: any longer and the cadence reads as a continuous tone.
const CRASH: [Note; 2] = [Note::new(8000, 70), Note::new(2637, 80)];
const STARVE: [Note; 2] = [Note::new(4186, 70), Note::new(3136, 80)];
const GAVE_UP: [Note; 2] = [Note::new(3136, 70), Note::new(4186, 80)];
const STALL: [Note; 2] = [Note::new(2637, 70), Note::new(2637, 80)];

impl Siren {
    /// Every siren, for iterating over the whole set.
    pub const ALL: [Siren; 4] = [Siren::Crash, Siren::Starve, Siren::GaveUp, Siren::Stall];

    /// The siren a given fault kind sounds — the one place `FaultKind` and `Siren` meet.
    pub const fn for_kind(kind: FaultKind) -> Siren {
        match kind {
            FaultKind::Crashed => Siren::Crash,
            FaultKind::Starved => Siren::Starve,
            FaultKind::Reported => Siren::GaveUp,
            FaultKind::Stalled => Siren::Stall,
        }
    }

    /// The notes this siren plays, in order. Every non-rest pitch sits in the ~2-9 kHz band
    /// the M5StickC Plus buzzer is measured loud across (mirrors `chime.rs`'s `AUDIBLE_HZ`).
    pub const fn notes(self) -> &'static [Note] {
        match self {
            Siren::Crash => &CRASH,
            Siren::Starve => &STARVE,
            Siren::GaveUp => &GAVE_UP,
            Siren::Stall => &STALL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The band the M5StickC Plus buzzer is measured loud across; a note must sit here to
    /// sound (mirrors `platform_core::chime`'s `AUDIBLE_HZ`).
    const AUDIBLE_HZ: core::ops::RangeInclusive<u16> = 2_000..=9_000;

    /// R19 — every non-rest note of every siren sits inside the audible band. An alarm outside
    /// that band is a silent alarm.
    #[test]
    fn every_tone_is_in_the_audible_band() {
        Siren::ALL.iter().for_each(|siren: &Siren| {
            siren
                .notes()
                .iter()
                .filter(|n: &&Note| n.freq_hz > 0)
                .for_each(|n: &Note| {
                    assert!(
                        AUDIBLE_HZ.contains(&n.freq_hz),
                        "{siren:?} has a {} Hz note outside the audible band {AUDIBLE_HZ:?}",
                        n.freq_hz
                    );
                });
        });
    }

    /// R18 — a siren is never silent: every siren sounds at least one real tone.
    #[test]
    fn every_siren_has_at_least_one_real_tone() {
        Siren::ALL.iter().for_each(|siren: &Siren| {
            assert!(
                siren.notes().iter().any(|n: &Note| n.freq_hz > 0),
                "{siren:?} is silent"
            );
        });
    }

    /// One — a single kind maps to exactly one siren.
    #[test]
    fn a_crash_sounds_the_crash_siren() {
        assert_eq!(Siren::for_kind(FaultKind::Crashed), Siren::Crash);
    }

    /// Many — the four kinds map to four distinct sirens, so the ear can tell them apart.
    #[test]
    fn every_kind_maps_to_a_distinct_siren() {
        let mapped: [Siren; 4] = [
            Siren::for_kind(FaultKind::Crashed),
            Siren::for_kind(FaultKind::Starved),
            Siren::for_kind(FaultKind::Reported),
            Siren::for_kind(FaultKind::Stalled),
        ];
        assert_eq!(mapped, Siren::ALL);
    }
}
