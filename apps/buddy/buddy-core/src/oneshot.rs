//! The one-shot override layer: a timed persona that outranks the base while it is live.
//!
//! A [`OneShot`] holds a persona and a deadline. Three triggers arm it — a level-up, a shake,
//! and a fast approval — each with its own duration and its own preemption rule. While a
//! one-shot is live the base state is **ignored entirely**; on expiry the active state snaps
//! to the *current* base, with no saved-prior restore.
//!
//! ## Signed-wraparound expiry
//!
//! Expiry is a signed compare against the deadline — `(now.wrapping_sub(until) as i64) >= 0`
//! — not a raw `now < until`, so a `Tick` wrap does not strand a live one-shot forever. See
//! [`OneShot::is_live`].

use platform_core::Tick;

use crate::persona::PersonaState;

/// Level-up celebration duration, in milliseconds. **Preempts** a live one-shot.
pub const CELEBRATE_MS: Tick = 3_000;
/// Shake dizzy duration, in milliseconds. Does **not** preempt — only fires when idle.
pub const DIZZY_MS: Tick = 2_000;
/// Fast-approval heart duration, in milliseconds. **Preempts** a live one-shot.
pub const HEART_MS: Tick = 2_000;
/// The fast-approval window, in whole seconds: a heart fires only for an answer strictly
/// inside `{0, 1, 2, 3, 4}` — i.e. `took_s < 5`. Integer seconds, so 5 exactly does not count.
pub const APPROVAL_HEART_WINDOW_S: u32 = 5;

/// A timed persona override: which state, until when.
///
/// [`Copy`] so it threads through [`crate::step`] by value. Construct inert with
/// [`OneShot::idle`]; arm it with one of the three triggers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OneShot {
    state: PersonaState,
    until: Tick,
}

impl OneShot {
    /// An inert one-shot — already expired, overriding nothing.
    pub const fn idle() -> Self {
        OneShot {
            state: PersonaState::Idle,
            until: 0,
        }
    }

    /// Level-up: arm celebrate for [`CELEBRATE_MS`]. **Preempts** any live one-shot.
    pub fn level_up(&mut self, now: Tick) {
        self.state = PersonaState::Celebrate;
        self.until = now.wrapping_add(CELEBRATE_MS);
    }

    /// Shake: arm dizzy for [`DIZZY_MS`], but **only** when nothing is live
    /// (`now >= until`) — a shake never preempts a celebration or a heart.
    pub fn shake(&mut self, now: Tick) {
        if !self.is_live(now) {
            self.state = PersonaState::Dizzy;
            self.until = now.wrapping_add(DIZZY_MS);
        }
    }

    /// An approval was answered: arm heart for [`HEART_MS`] **only** when it was an approve
    /// (`approved`) answered fast (`took_s < `[`APPROVAL_HEART_WINDOW_S`]). A deny fires
    /// nothing. **Preempts** any live one-shot.
    pub fn approval_answered(&mut self, approved: bool, took_s: u32, now: Tick) {
        if approved && took_s < APPROVAL_HEART_WINDOW_S {
            self.state = PersonaState::Heart;
            self.until = now.wrapping_add(HEART_MS);
        }
    }

    /// Whether a one-shot is still overriding at `now`, by signed-wraparound compare:
    /// `(now.wrapping_sub(until) as i64) < 0`.
    pub fn is_live(&self, now: Tick) -> bool {
        (now.wrapping_sub(self.until) as i64) < 0
    }

    /// The active persona at `now`: the one-shot's state while live, otherwise `base`.
    ///
    /// On expiry it resolves to the *current* `base` — there is no saved-prior state to
    /// restore.
    pub fn active(&self, now: Tick, base: PersonaState) -> PersonaState {
        if self.is_live(now) {
            self.state
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A fresh one-shot overrides nothing: it resolves to the base immediately.
    #[test]
    fn an_idle_one_shot_yields_the_base() {
        let one_shot: OneShot = OneShot::idle();
        assert!(!one_shot.is_live(0));
        assert_eq!(one_shot.active(0, PersonaState::Busy), PersonaState::Busy);
    }

    /// A level-up celebrates for exactly 3000 ms, then snaps back to the current base.
    #[test]
    fn a_level_up_celebrates_then_expires() {
        let mut one_shot: OneShot = OneShot::idle();
        one_shot.level_up(1_000);
        assert_eq!(
            one_shot.active(1_000, PersonaState::Idle),
            PersonaState::Celebrate
        );
        // Still live one ms before expiry.
        assert_eq!(
            one_shot.active(3_999, PersonaState::Idle),
            PersonaState::Celebrate
        );
        // At expiry it resolves to the current base, with no saved-prior restore.
        assert_eq!(
            one_shot.active(4_000, PersonaState::Attention),
            PersonaState::Attention
        );
    }

    /// A shake does not preempt a live celebration.
    #[test]
    fn a_shake_does_not_preempt_a_celebration() {
        let mut one_shot: OneShot = OneShot::idle();
        one_shot.level_up(0);
        one_shot.shake(1_000); // celebration still live → ignored
        assert_eq!(
            one_shot.active(1_000, PersonaState::Idle),
            PersonaState::Celebrate
        );
    }

    /// A shake fires once the previous one-shot has expired.
    #[test]
    fn a_shake_fires_once_idle() {
        let mut one_shot: OneShot = OneShot::idle();
        one_shot.shake(500);
        assert_eq!(
            one_shot.active(500, PersonaState::Idle),
            PersonaState::Dizzy
        );
        assert_eq!(
            one_shot.active(2_500, PersonaState::Idle),
            PersonaState::Idle
        );
    }

    /// A fast approve fires a heart and preempts a live one-shot; a deny fires nothing.
    #[test]
    fn a_fast_approve_hearts_but_a_deny_is_silent() {
        let mut hearted: OneShot = OneShot::idle();
        hearted.shake(0); // arm dizzy first
        hearted.approval_answered(true, 4, 0); // preempts with heart
        assert_eq!(hearted.active(0, PersonaState::Idle), PersonaState::Heart);

        let mut denied: OneShot = OneShot::idle();
        denied.approval_answered(false, 1, 0);
        assert_eq!(denied.active(0, PersonaState::Idle), PersonaState::Idle);
    }

    /// The heart window is strictly `took_s < 5`: 4 fires, 5 does not.
    #[test]
    fn the_heart_window_is_strictly_under_five_seconds() {
        let mut inside: OneShot = OneShot::idle();
        inside.approval_answered(true, 4, 0);
        assert_eq!(inside.active(0, PersonaState::Idle), PersonaState::Heart);

        let mut edge: OneShot = OneShot::idle();
        edge.approval_answered(true, 5, 0);
        assert_eq!(edge.active(0, PersonaState::Idle), PersonaState::Idle);
    }

    proptest! {
        /// A level-up is live across its whole window and dead at and after expiry.
        #[test]
        fn a_level_up_is_live_exactly_across_its_window(now in 0u64..1_000_000_000, dt in 0u64..2_999) {
            let mut one_shot: OneShot = OneShot::idle();
            one_shot.level_up(now);
            prop_assert!(one_shot.is_live(now.wrapping_add(dt)));
            prop_assert!(!one_shot.is_live(now.wrapping_add(CELEBRATE_MS)));
        }

        /// A deny never arms, whatever the timing.
        #[test]
        fn a_deny_never_arms(took in 0u32..10, now in 0u64..1_000_000) {
            let mut one_shot: OneShot = OneShot::idle();
            one_shot.approval_answered(false, took, now);
            prop_assert!(!one_shot.is_live(now));
        }
    }
}
