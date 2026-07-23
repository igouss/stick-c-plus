//! The charging-clock mood schedule: a pure `f(hour, dow, now_ms) -> PersonaState`.
//!
//! When the buddy is docked and idle it tells the time of day as a mood. The schedule is a
//! deterministic two-state flicker: every branch but the unconditional early-morning sleep
//! also reads `now_ms`, so a "special" state occupies one slice of `N` ms out of every
//! `N * M` ms. Pure and fully testable — drive `now_ms` to pick the slice.
//!
//! ## Defect fix (d): the late-night branch is hoisted
//!
//! Upstream ordered `h < 9` **above** `h >= 22 || h == 0`, which shadowed midnight (`h == 0`)
//! into the pre-9am flicker as dead code — Dizzy was only ever reachable at 22:00–23:59.
//! Reproducing that would mean asserting a bug in a test, so the late-night branch is hoisted
//! above `h < 9`: midnight now gets the Dizzy the author intended. The order below is the
//! fixed order.

use platform_core::Tick;

use crate::persona::PersonaState;

/// The charging-clock persona for a wall-clock hour, day-of-week, and millisecond phase.
///
/// `hour` is `0..=23`; `dow` is `0..=6` with `0 = Sunday`; `weekend = dow == 0 || dow == 6`;
/// `friday = dow == 5`. First match wins, in this (defect-fixed) order:
///
/// ```text
/// h >= 1 && h < 7   -> Sleep                                     (unconditional)
/// h >= 22 || h == 0 -> (now/7000  % 3 == 0) ? Dizzy     : Sleep   (hoisted above h < 9)
/// weekend           -> (now/8000  % 6 == 0) ? Heart     : Sleep
/// h < 9             -> (now/6000  % 4 == 0) ? Idle      : Sleep
/// h == 12           -> (now/5000  % 3 == 0) ? Heart     : Idle
/// friday && h >= 15 -> (now/4000  % 3 == 0) ? Celebrate : Idle
/// else              -> (now/10000 % 5 == 0) ? Sleep     : Idle
/// ```
pub fn charging_mood(hour: u8, dow: u8, now_ms: Tick) -> PersonaState {
    let weekend: bool = dow == 0 || dow == 6;
    let friday: bool = dow == 5;
    if (1..7).contains(&hour) {
        PersonaState::Sleep
    } else if hour >= 22 || hour == 0 {
        // Defect (d) fixed: hoisted above `hour < 9` so midnight gets the Dizzy the author
        // intended, instead of being shadowed into the pre-9am flicker as dead code.
        flicker(now_ms, 7_000, 3, PersonaState::Dizzy, PersonaState::Sleep)
    } else if weekend {
        flicker(now_ms, 8_000, 6, PersonaState::Heart, PersonaState::Sleep)
    } else if hour < 9 {
        flicker(now_ms, 6_000, 4, PersonaState::Idle, PersonaState::Sleep)
    } else if hour == 12 {
        flicker(now_ms, 5_000, 3, PersonaState::Heart, PersonaState::Idle)
    } else if friday && hour >= 15 {
        flicker(
            now_ms,
            4_000,
            3,
            PersonaState::Celebrate,
            PersonaState::Idle,
        )
    } else {
        flicker(now_ms, 10_000, 5, PersonaState::Sleep, PersonaState::Idle)
    }
}

/// The deterministic two-state flicker: the `special` state occupies the zeroth slice of every
/// `period * modulus` window, `otherwise` fills the rest.
fn flicker(
    now_ms: Tick,
    period: Tick,
    modulus: Tick,
    special: PersonaState,
    otherwise: PersonaState,
) -> PersonaState {
    if (now_ms / period).is_multiple_of(modulus) {
        special
    } else {
        otherwise
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const WEDNESDAY: u8 = 3;
    const SATURDAY: u8 = 6;
    const FRIDAY: u8 = 5;

    /// Early morning is an unconditional sleep, whatever the phase.
    #[test]
    fn early_morning_is_unconditional_sleep() {
        assert_eq!(charging_mood(3, WEDNESDAY, 0), PersonaState::Sleep);
        assert_eq!(charging_mood(3, WEDNESDAY, 999_999), PersonaState::Sleep);
    }

    /// DEFECT (d) PINNED: midnight (hour 0) reaches the late-night Dizzy flicker, not the
    /// shadowed pre-9am branch. This FAILS against the reproduced upstream order (`h < 9`
    /// above the late-night branch), where midnight could never be Dizzy.
    #[test]
    fn midnight_reaches_dizzy_after_the_hoist() {
        // 0 / 7000 == 0, % 3 == 0 → Dizzy.
        assert_eq!(charging_mood(0, WEDNESDAY, 0), PersonaState::Dizzy);
        // The other slices of the late-night window are Sleep.
        assert_eq!(charging_mood(0, WEDNESDAY, 7_000), PersonaState::Sleep);
    }

    /// Late evening flickers Dizzy against Sleep.
    #[test]
    fn late_evening_flickers_dizzy() {
        assert_eq!(charging_mood(23, WEDNESDAY, 0), PersonaState::Dizzy);
        assert_eq!(charging_mood(23, WEDNESDAY, 7_000), PersonaState::Sleep);
    }

    /// A weekend daytime flickers Heart against Sleep.
    #[test]
    fn a_weekend_daytime_flickers_heart() {
        assert_eq!(charging_mood(14, SATURDAY, 0), PersonaState::Heart);
        assert_eq!(charging_mood(14, SATURDAY, 8_000), PersonaState::Sleep);
    }

    /// Noon on a weekday flickers Heart against Idle.
    #[test]
    fn noon_flickers_heart_against_idle() {
        assert_eq!(charging_mood(12, WEDNESDAY, 0), PersonaState::Heart);
        assert_eq!(charging_mood(12, WEDNESDAY, 5_000), PersonaState::Idle);
    }

    /// Friday afternoon flickers Celebrate against Idle.
    #[test]
    fn friday_afternoon_flickers_celebrate() {
        assert_eq!(charging_mood(16, FRIDAY, 0), PersonaState::Celebrate);
        assert_eq!(charging_mood(16, FRIDAY, 4_000), PersonaState::Idle);
    }

    /// A plain weekday afternoon flickers Sleep against Idle.
    #[test]
    fn a_plain_afternoon_flickers_sleep_against_idle() {
        assert_eq!(charging_mood(14, WEDNESDAY, 0), PersonaState::Sleep);
        assert_eq!(charging_mood(14, WEDNESDAY, 10_000), PersonaState::Idle);
    }

    proptest! {
        /// The schedule is total: every (hour, dow, phase) yields a persona without panicking.
        #[test]
        fn the_schedule_is_total(hour in 0u8..24, dow in 0u8..7, now in 0u64..100_000_000) {
            let _: PersonaState = charging_mood(hour, dow, now);
        }

        /// Early morning ignores the phase entirely.
        #[test]
        fn early_morning_ignores_the_phase(hour in 1u8..7, dow in 0u8..7, now in 0u64..100_000_000) {
            prop_assert_eq!(charging_mood(hour, dow, now), PersonaState::Sleep);
        }
    }
}
