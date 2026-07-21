//! The pure debounce: a noisy level in, a settled click or hold out.

use platform_core::Tick;

use crate::gesture::{Gesture, GestureConfig};

/// Turns a raw, bouncing button level into clean [`Gesture`]s, as a pure function of time.
///
/// Fed `(now, raw_pressed)` on every poll, it holds a little state and answers with a gesture
/// only when one truly occurred. Nothing here reads a clock or a pin — the caller supplies both
/// — so the whole timing policy is decided on the host:
///
/// - a level is *accepted* only once it has held steady for
///   [`debounce_ms`](GestureConfig::debounce_ms), so a bounce train collapses to a single
///   transition;
/// - an accepted press released before [`hold_ms`](GestureConfig::hold_ms) emits one
///   [`Click`](Gesture::Click) on release;
/// - an accepted press held to [`hold_ms`](GestureConfig::hold_ms) emits one
///   [`LongHold`](Gesture::LongHold) the moment the threshold passes, and then *nothing* on
///   release — a hold is not also a click.
///
/// It never emits [`DoubleClick`](Gesture::DoubleClick); that is the [`Cadence`](crate::Cadence)
/// stage's job.
#[derive(Clone, Copy, Debug, Default)]
pub struct Debounce {
    /// The accepted, stable level: `true` while the button is debounced-pressed.
    debounced: bool,
    /// The most recent raw level seen — the candidate for the next accepted level.
    candidate: bool,
    /// When `candidate` was first seen: the start of its stability window.
    since: Tick,
    /// When the current accepted press began; `None` while released.
    pressed_at: Option<Tick>,
    /// Whether a [`LongHold`](Gesture::LongHold) has already fired for the current press, so it
    /// fires once and suppresses the release [`Click`](Gesture::Click).
    hold_fired: bool,
}

impl Debounce {
    /// A button that starts released.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one poll `(now, raw_pressed)` into the debounce, returning the gesture it produced,
    /// if any.
    pub fn update(
        &mut self,
        now: Tick,
        raw_pressed: bool,
        config: GestureConfig,
    ) -> Option<Gesture> {
        // Track the candidate level and (re)start its stability window on any change.
        if raw_pressed != self.candidate {
            self.candidate = raw_pressed;
            self.since = now;
        }

        // Accept a new level once the candidate has held steady long enough.
        if self.candidate != self.debounced && now.saturating_sub(self.since) >= config.debounce_ms
        {
            self.debounced = self.candidate;
            return if self.debounced {
                // A press begins — no gesture yet; a click or hold is decided later.
                self.pressed_at = Some(now);
                self.hold_fired = false;
                None
            } else {
                // A release: a short press is a click; a press that already held is neither.
                let was_hold: bool = self.hold_fired;
                self.pressed_at = None;
                self.hold_fired = false;
                (!was_hold).then_some(Gesture::Click)
            };
        }

        // While an accepted press is held, fire a single LongHold as the threshold passes.
        match self.pressed_at {
            Some(pressed_at)
                if !self.hold_fired && now.saturating_sub(pressed_at) >= config.hold_ms =>
            {
                self.hold_fired = true;
                Some(Gesture::LongHold)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const C: GestureConfig = GestureConfig {
        debounce_ms: 15,
        hold_ms: 600,
        double_click_ms: 300,
    };

    /// Feed a poll sequence and collect the gestures it produced, in order.
    fn feed(db: &mut Debounce, steps: &[(Tick, bool)]) -> Vec<Gesture> {
        steps
            .iter()
            .filter_map(|&(now, raw): &(Tick, bool)| db.update(now, raw, C))
            .collect()
    }

    /// Zero: a button left alone produces nothing, however long it is polled.
    #[test]
    fn a_released_button_emits_nothing() {
        let mut db: Debounce = Debounce::new();
        let gestures: Vec<Gesture> = feed(&mut db, &[(0, false), (1_000, false), (5_000, false)]);
        assert_eq!(gestures, vec![]);
    }

    /// A press with no release emits nothing: a click is decided on release, and this press
    /// never got there.
    #[test]
    fn a_press_without_release_or_hold_emits_nothing() {
        let mut db: Debounce = Debounce::new();
        let gestures: Vec<Gesture> = feed(
            &mut db,
            &[(0, false), (10, true), (10 + C.debounce_ms, true)],
        );
        assert_eq!(gestures, vec![]);
    }

    /// One: a clean press and release, well inside the hold threshold, is exactly one click —
    /// and it lands on the release, not the press.
    #[test]
    fn a_short_press_is_one_click_on_release() {
        let mut db: Debounce = Debounce::new();
        let gestures: Vec<Gesture> = feed(
            &mut db,
            &[
                (0, false),
                (10, true),                   // raw goes high
                (10 + C.debounce_ms, true),   // press accepted here — still nothing
                (100, false),                 // raw goes low, ~75 ms after the press
                (100 + C.debounce_ms, false), // release accepted -> Click
            ],
        );
        assert_eq!(gestures, vec![Gesture::Click]);
    }

    /// One: a press held past the threshold is exactly one hold — emitted while still down —
    /// and the later release is silent, because a hold is not also a click.
    #[test]
    fn a_long_press_is_one_hold_and_no_click() {
        let mut db: Debounce = Debounce::new();
        let gestures: Vec<Gesture> = feed(
            &mut db,
            &[
                (0, false),
                (10, true),
                (10 + C.debounce_ms, true), // press accepted, held from here
                (10 + C.debounce_ms + C.hold_ms, true), // crosses the threshold -> LongHold
                (2_000, false),             // raw goes low, long after
                (2_000 + C.debounce_ms, false), // release accepted -> silent
            ],
        );
        assert_eq!(gestures, vec![Gesture::LongHold]);
    }

    /// Many: two separate press/release cycles are two clicks.
    #[test]
    fn two_short_presses_are_two_clicks() {
        let mut db: Debounce = Debounce::new();
        let gestures: Vec<Gesture> = feed(
            &mut db,
            &[
                (0, false),
                (10, true),
                (10 + C.debounce_ms, true),
                (100, false),
                (100 + C.debounce_ms, false), // Click
                (200, true),
                (200 + C.debounce_ms, true),
                (300, false),
                (300 + C.debounce_ms, false), // Click
            ],
        );
        assert_eq!(gestures, vec![Gesture::Click, Gesture::Click]);
    }

    /// A bounce train shorter than the debounce window is swallowed whole: the level never
    /// holds steady long enough to be accepted, so nothing is emitted.
    #[test]
    fn a_bounce_train_inside_the_window_emits_nothing() {
        let mut db: Debounce = Debounce::new();
        let gestures: Vec<Gesture> = feed(
            &mut db,
            &[
                (0, false),
                (2, true),
                (4, false),
                (6, true),
                (8, false),
                (10, true),
                (12, false), // every edge is < debounce_ms apart: nothing settles
            ],
        );
        assert_eq!(gestures, vec![]);
    }

    /// The thresholds are policy, not law: a config with a shorter hold turns the very same
    /// press into a hold instead of a click.
    #[test]
    fn a_shorter_hold_threshold_reclassifies_the_same_press() {
        let brisk: GestureConfig = GestureConfig { hold_ms: 100, ..C };
        let mut db: Debounce = Debounce::new();
        let steps: [(Tick, bool); 5] = [
            (0, false),
            (10, true),
            (10 + C.debounce_ms, true),
            (200, true), // 175 ms in: past the brisk hold, well inside the default one
            (300, false),
        ];
        let gestures: Vec<Gesture> = steps
            .iter()
            .filter_map(|&(now, raw): &(Tick, bool)| db.update(now, raw, brisk))
            .collect();
        assert_eq!(gestures, vec![Gesture::LongHold]);
    }

    proptest! {
        /// A press accepted and then released before the hold threshold is exactly one click,
        /// for any held duration in the click range.
        #[test]
        fn any_short_press_is_one_click(held in 15u64..600) {
            let mut db: Debounce = Debounce::new();
            let press: Tick = 10 + C.debounce_ms;
            let low: Tick = press + held;
            let gestures: Vec<Gesture> = feed(&mut db, &[
                (0, false),
                (10, true),
                (press, true),
                (low, false),
                (low + C.debounce_ms, false),
            ]);
            prop_assert_eq!(gestures, vec![Gesture::Click]);
        }

        /// A press held past the threshold is exactly one hold and never a click, for any
        /// overshoot past the threshold.
        #[test]
        fn any_long_press_is_one_hold(extra in 0u64..1_000) {
            let mut db: Debounce = Debounce::new();
            let press: Tick = 10 + C.debounce_ms;
            let cross: Tick = press + C.hold_ms + extra;
            let gestures: Vec<Gesture> = feed(&mut db, &[
                (0, false),
                (10, true),
                (press, true),
                (cross, true),
                (cross + 5, false),
                (cross + 5 + C.debounce_ms, false),
            ]);
            prop_assert_eq!(gestures, vec![Gesture::LongHold]);
        }
    }
}
