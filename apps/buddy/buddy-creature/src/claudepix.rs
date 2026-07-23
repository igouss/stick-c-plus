//! The FIRST creature: the vendored ClaudePix character.
//!
//! Binds the seven persona states onto the 13 vendored presets exactly:
//!
//! | state       | sprites                                    |
//! |-------------|--------------------------------------------|
//! | `Sleep`     | `EXPRESSION_SLEEP`                          |
//! | `Idle`      | `IDLE_BREATHE`, `IDLE_BLINK`, `IDLE_LOOK_AROUND` |
//! | `Busy`      | `WORK_CODING`, `WORK_THINK`                 |
//! | `Attention` | `EXPRESSION_SURPRISE`                       |
//! | `Celebrate` | `DANCE_BOUNCE`, `DANCE_SWAY`                |
//! | `Dizzy`     | `DANCE_DJMIX`                              |
//! | `Heart`     | `EXPRESSION_WINK`                          |
//!
//! `Dizzy` has no natural match; `DANCE_DJMIX` is the owner's stand-in. The two DJ variants
//! (`DANCE_BOUNCE_DJ`, `DANCE_SWAY_DJ`) are deliberately spare.

use platform_display::sprite::{
    Sprite, DANCE_BOUNCE, DANCE_DJMIX, DANCE_SWAY, EXPRESSION_SLEEP, EXPRESSION_SURPRISE,
    EXPRESSION_WINK, IDLE_BLINK, IDLE_BREATHE, IDLE_LOOK_AROUND, WORK_CODING, WORK_THINK,
};

use crate::species::{Species, STATE_COUNT};

static SLEEP_SET: [&Sprite; 1] = [&EXPRESSION_SLEEP];
static IDLE_SET: [&Sprite; 3] = [&IDLE_BREATHE, &IDLE_BLINK, &IDLE_LOOK_AROUND];
static BUSY_SET: [&Sprite; 2] = [&WORK_CODING, &WORK_THINK];
static ATTENTION_SET: [&Sprite; 1] = [&EXPRESSION_SURPRISE];
static CELEBRATE_SET: [&Sprite; 2] = [&DANCE_BOUNCE, &DANCE_SWAY];
static DIZZY_SET: [&Sprite; 1] = [&DANCE_DJMIX];
static HEART_SET: [&Sprite; 1] = [&EXPRESSION_WINK];

/// Per-state sprite sets in persona-ordinal order (`Sleep = 0` .. `Heart = 6`).
static CLAUDEPIX_STATES: [&[&Sprite]; STATE_COUNT] = [
    &SLEEP_SET,
    &IDLE_SET,
    &BUSY_SET,
    &ATTENTION_SET,
    &CELEBRATE_SET,
    &DIZZY_SET,
    &HEART_SET,
];

/// The first creature, at registry index 0.
pub static CLAUDEPIX: Species = Species::new("claudepix", CLAUDEPIX_STATES);

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_core::PersonaState;

    /// The slugs of a sprite set, in order — the identity a wrong preset or wrong order breaks.
    fn slugs(set: &[&Sprite]) -> Vec<&'static str> {
        set.iter().map(|s: &&Sprite| s.slug()).collect()
    }

    /// Invariant 2: the state → preset binding is EXACTLY this table. A wrong preset, a wrong
    /// order within a state, or a wrong count fails one of these lines.
    #[test]
    fn claudepix_binds_all_seven_states_exactly() {
        assert_eq!(
            slugs(CLAUDEPIX.sprites(PersonaState::Sleep)),
            ["expression_sleep"]
        );
        assert_eq!(
            slugs(CLAUDEPIX.sprites(PersonaState::Idle)),
            ["idle_breathe", "idle_blink", "idle_look_around"]
        );
        assert_eq!(
            slugs(CLAUDEPIX.sprites(PersonaState::Busy)),
            ["work_coding", "work_think"]
        );
        assert_eq!(
            slugs(CLAUDEPIX.sprites(PersonaState::Attention)),
            ["expression_surprise"]
        );
        assert_eq!(
            slugs(CLAUDEPIX.sprites(PersonaState::Celebrate)),
            ["dance_bounce", "dance_sway"]
        );
        assert_eq!(
            slugs(CLAUDEPIX.sprites(PersonaState::Dizzy)),
            ["dance_djmix"]
        );
        assert_eq!(
            slugs(CLAUDEPIX.sprites(PersonaState::Heart)),
            ["expression_wink"]
        );
    }
}
