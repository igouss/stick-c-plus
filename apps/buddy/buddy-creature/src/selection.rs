//! The pure selection over time — two layers, kept distinct.
//!
//! [`current`] answers "what should be on the glass now?" for a `(species, state, elapsed_ms)`,
//! with `elapsed_ms` the injected time and never a clock read. It composes two rules:
//!
//! 1. a cross-animation **variety** rule picks WHICH sprite in the state's set is active as
//!    elapsed advances — a multi-sprite state plays `set[0]` for its full
//!    [`loop_ms`](platform_display::sprite::Sprite::loop_ms), then `set[1]`, cycling, so every
//!    member is reachable and a state shows real variety;
//! 2. frame timing is **delegated** to the chosen sprite's own
//!    [`frame_at`](platform_display::sprite::Sprite::frame_at) at the sprite-local elapsed —
//!    this crate does not reimplement the frame clock.

use buddy_core::PersonaState;
use platform_display::sprite::{Frame, Sprite};

use crate::species::Species;

/// What [`current`] chose to render: the active sprite and the frame within it.
///
/// A render loop compares [`Selected::frame_index`] against the last index it painted — two
/// equal indices mean nothing to redraw.
#[derive(Clone, Copy, Debug)]
pub struct Selected {
    /// The sprite the variety rule made active for this `(state, elapsed_ms)`.
    pub sprite: &'static Sprite,
    /// The frame index within `sprite` at the sprite-local elapsed, from the sprite's own clock.
    pub frame_index: usize,
}

impl Selected {
    /// The chosen frame — `sprite.frames()[frame_index]`, with the sprite's `'static` lifetime.
    pub fn frame(&self) -> &'static Frame {
        &self.sprite.frames()[self.frame_index]
    }
}

/// The pure selection: pick the active sprite in `state`'s set by the variety rule, then read
/// its frame at the sprite-local elapsed. `elapsed_ms` is injected; no clock is read.
///
/// The variety rule plays `set[0]` for its full [`loop_ms`](Sprite::loop_ms), then `set[1]`,
/// and so on, wrapping after the last — one full "variety cycle" is the sum of every member's
/// loop. `elapsed_ms` is reduced modulo that cycle and the windows are walked to find the active
/// member; the remainder after the walk is the sprite-**local** elapsed (window-relative, so
/// each member plays from its own frame 0), handed to the chosen sprite's own
/// [`frame_index_at`](Sprite::frame_index_at). Both the cycle and each member's loop are
/// positive — a set is non-empty and every vendored sprite loops for a positive time — so
/// neither modulo divides by zero.
pub fn current(species: &Species, state: PersonaState, elapsed_ms: u64) -> Selected {
    let set: &'static [&'static Sprite] = species.sprites(state);
    let cycle_ms: u64 = set.iter().map(|s: &&Sprite| u64::from(s.loop_ms())).sum();
    let mut local: u64 = elapsed_ms % cycle_ms;
    let mut index: usize = 0;
    while local >= u64::from(set[index].loop_ms()) {
        local -= u64::from(set[index].loop_ms());
        index += 1;
    }
    let sprite: &'static Sprite = set[index];
    Selected {
        sprite,
        frame_index: sprite.frame_index_at(local),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claudepix::CLAUDEPIX;
    use crate::species::STATE_COUNT;
    use platform_display::sprite::{IDLE_BLINK, IDLE_BREATHE, WORK_CODING, WORK_THINK};
    use proptest::prelude::*;

    const STATES: [PersonaState; STATE_COUNT] = [
        PersonaState::Sleep,
        PersonaState::Idle,
        PersonaState::Busy,
        PersonaState::Attention,
        PersonaState::Celebrate,
        PersonaState::Dizzy,
        PersonaState::Heart,
    ];

    /// Invariant 5 (determinism): equal `(species, state, elapsed_ms)` yield the identical
    /// chosen sprite and frame — the time is injected, never read.
    #[test]
    fn the_selection_is_deterministic_in_elapsed() {
        let a: Selected = current(&CLAUDEPIX, PersonaState::Idle, 5_000);
        let b: Selected = current(&CLAUDEPIX, PersonaState::Idle, 5_000);
        assert!(core::ptr::eq(a.sprite, b.sprite));
        assert_eq!(a.frame_index, b.frame_index);
    }

    /// Invariant 5 (variety actually varies): as elapsed advances through the idle set's
    /// cumulative loop windows, all three members appear — none is stranded. The window starts
    /// are the sprites' own `loop_ms`, which is the variety rule itself, not a reimplementation.
    #[test]
    fn every_idle_sprite_is_reachable_as_elapsed_advances() {
        let at_breathe: u64 = 0;
        let at_blink: u64 = u64::from(IDLE_BREATHE.loop_ms());
        let at_look_around: u64 = at_blink + u64::from(IDLE_BLINK.loop_ms());
        assert_eq!(
            current(&CLAUDEPIX, PersonaState::Idle, at_breathe)
                .sprite
                .slug(),
            "idle_breathe"
        );
        assert_eq!(
            current(&CLAUDEPIX, PersonaState::Idle, at_blink)
                .sprite
                .slug(),
            "idle_blink"
        );
        assert_eq!(
            current(&CLAUDEPIX, PersonaState::Idle, at_look_around)
                .sprite
                .slug(),
            "idle_look_around"
        );
    }

    /// Invariant 6 (frame timing is DELEGATED): for a single-sprite state the sprite-local
    /// elapsed equals the injected elapsed, so the reported frame is the sprite's own
    /// `frame_index_at` — `current` does not reimplement the frame clock.
    #[test]
    fn the_reported_frame_is_the_sprites_own_frame_at() {
        let elapsed: u64 = 1_234;
        let selected: Selected = current(&CLAUDEPIX, PersonaState::Sleep, elapsed);
        assert_eq!(
            selected.frame_index,
            selected.sprite.frame_index_at(elapsed)
        );
    }

    /// Invariant 6 (frame timing is DELEGATED — the MULTI-sprite path): once the variety rule
    /// has advanced past `set[0]` into `set[1]`'s window, the elapsed handed to the chosen
    /// sprite's own `frame_index_at` is WINDOW-RELATIVE — it is `elapsed - set[0].loop_ms()`,
    /// so the second sprite plays from its own frame 0, not from the raw injected elapsed.
    ///
    /// This is the case the single-sprite `Sleep` test cannot reach, where the sprite-local
    /// elapsed diverges from the injected one. The constants are chosen so the two readings
    /// genuinely disagree (window-relative lands on a different frame than raw modulo would):
    /// the inequality assert pins that, so if `current` ever fed the raw elapsed to
    /// `frame_index_at` the equality below would go red.
    #[test]
    fn a_multi_sprite_states_frame_is_read_at_the_window_relative_elapsed() {
        let into_second: u64 = 700;
        let elapsed: u64 = u64::from(WORK_CODING.loop_ms()) + into_second;
        let selected: Selected = current(&CLAUDEPIX, PersonaState::Busy, elapsed);
        assert!(core::ptr::eq(selected.sprite, &WORK_THINK));
        assert_eq!(selected.frame_index, WORK_THINK.frame_index_at(into_second));
        assert_ne!(
            WORK_THINK.frame_index_at(into_second),
            WORK_THINK.frame_index_at(elapsed),
            "constants must exercise window-relative vs raw disagreement"
        );
    }

    proptest! {
        /// Invariant 5 (TOTALITY over elapsed): for any `elapsed_ms` and any state, the chosen
        /// sprite is a member of that state's set — the variety rule never invents a sprite.
        #[test]
        fn the_chosen_sprite_is_always_a_member_of_the_state_set(
            elapsed in any::<u64>(),
            state_idx in 0usize..STATE_COUNT,
        ) {
            let state: PersonaState = STATES[state_idx];
            let selected: Selected = current(&CLAUDEPIX, state, elapsed);
            let set: &[&Sprite] = CLAUDEPIX.sprites(state);
            let member: bool = set.iter().any(|s: &&Sprite| core::ptr::eq(*s, selected.sprite));
            prop_assert!(member);
        }
    }
}
