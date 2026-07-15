//! The button port, and the pure debounce that turns a noisy level into events.

use crate::tick::Tick;

/// How long a raw level must hold steady before the debounce accepts it, in milliseconds.
///
/// A mechanical button bounces for a few milliseconds on each edge; 15 ms is comfortably
/// past the bounce yet far below human reaction time, so a real press is never missed and a
/// bounce train never counts twice.
pub const DEBOUNCE_MS: Tick = 15;

/// How long an accepted press must be held to count as a [`Hold`](ButtonEvent::Hold) rather
/// than a [`Tap`](ButtonEvent::Tap), in milliseconds.
pub const HOLD_MS: Tick = 600;

/// A momentary push-button, read as a raw pressed/released level.
///
/// The driven port for an input pin: the firmware adapter reads the GPIO (active-low on the
/// M5StickC Plus front/side buttons) and reports `true` while pressed. All timing and
/// bounce rejection live in the pure [`Debounce`], so the adapter stays a one-line level
/// read and every button rule is host-tested.
pub trait Button {
    /// The current raw level: `true` while the button is physically pressed.
    fn poll(&mut self) -> bool;
}

/// What a debounced button did — the only two gestures this board needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonEvent {
    /// A short press: pressed and released inside [`HOLD_MS`]. Emitted on release.
    Tap,
    /// A long press: held for at least [`HOLD_MS`]. Emitted once, the moment the threshold
    /// passes — while the button is still down — so a hold feels immediate.
    Hold,
}

/// Turns a raw, bouncing button level into clean [`ButtonEvent`]s, as a pure function of
/// time.
///
/// Fed `(now, raw_pressed)` on every poll, it holds a little state and answers with an
/// event only when one truly occurred. Nothing here reads a clock or a pin — the shell
/// supplies both arguments — so the whole gesture policy is decided on the host:
///
/// - a level is *accepted* only once it has held steady for [`DEBOUNCE_MS`], so a bounce
///   train collapses to a single transition;
/// - an accepted press that is released before [`HOLD_MS`] emits one [`Tap`](ButtonEvent::Tap)
///   on release;
/// - an accepted press held to [`HOLD_MS`] emits one [`Hold`](ButtonEvent::Hold) the moment
///   the threshold passes, and then *nothing* on release — a hold is not also a tap.
#[derive(Clone, Copy, Debug)]
pub struct Debounce {
    /// The accepted, stable level: `true` while the button is debounced-pressed.
    debounced: bool,
    /// The most recent raw level seen — the candidate for the next accepted level.
    candidate: bool,
    /// When `candidate` was first seen: the start of its stability window.
    since: Tick,
    /// When the current accepted press began; `None` while released.
    pressed_at: Option<Tick>,
    /// Whether a [`Hold`](ButtonEvent::Hold) has already fired for the current press, so it
    /// fires once and suppresses the release [`Tap`](ButtonEvent::Tap).
    hold_fired: bool,
}

impl Default for Debounce {
    /// A button that starts released.
    fn default() -> Self {
        Debounce {
            debounced: false,
            candidate: false,
            since: 0,
            pressed_at: None,
            hold_fired: false,
        }
    }
}

impl Debounce {
    /// A button that starts released.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one poll `(now, raw_pressed)` into the debounce, returning the event it
    /// produced, if any.
    pub fn update(&mut self, now: Tick, raw_pressed: bool) -> Option<ButtonEvent> {
        // Track the candidate level and (re)start its stability window on any change.
        if raw_pressed != self.candidate {
            self.candidate = raw_pressed;
            self.since = now;
        }

        // Accept a new level once the candidate has held steady long enough.
        if self.candidate != self.debounced && now.saturating_sub(self.since) >= DEBOUNCE_MS {
            self.debounced = self.candidate;
            return if self.debounced {
                // A press begins — no event yet; a tap or hold is decided later.
                self.pressed_at = Some(now);
                self.hold_fired = false;
                None
            } else {
                // A release: a short press is a tap; a press that already held is neither.
                let was_hold: bool = self.hold_fired;
                self.pressed_at = None;
                self.hold_fired = false;
                (!was_hold).then_some(ButtonEvent::Tap)
            };
        }

        // While an accepted press is held, fire a single Hold as the threshold passes.
        match self.pressed_at {
            Some(pressed_at) if !self.hold_fired && now.saturating_sub(pressed_at) >= HOLD_MS => {
                self.hold_fired = true;
                Some(ButtonEvent::Hold)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Feed a poll sequence and collect the events it produced, in order.
    fn feed(db: &mut Debounce, steps: &[(Tick, bool)]) -> Vec<ButtonEvent> {
        steps
            .iter()
            .filter_map(|&(now, raw): &(Tick, bool)| db.update(now, raw))
            .collect()
    }

    /// Zero: a button left alone produces nothing, however long it is polled.
    #[test]
    fn a_released_button_emits_nothing() {
        let mut db: Debounce = Debounce::new();
        let events: Vec<ButtonEvent> = feed(&mut db, &[(0, false), (1_000, false), (5_000, false)]);
        assert_eq!(events, vec![]);
    }

    /// A press with no release emits nothing: a tap is decided on release, and this press
    /// never got there.
    #[test]
    fn a_press_without_release_or_hold_emits_nothing() {
        let mut db: Debounce = Debounce::new();
        let events: Vec<ButtonEvent> =
            feed(&mut db, &[(0, false), (10, true), (10 + DEBOUNCE_MS, true)]);
        assert_eq!(events, vec![]);
    }

    /// One: a clean press and release, well inside the hold threshold, is exactly one tap —
    /// and it lands on the release, not the press.
    #[test]
    fn a_short_press_is_one_tap_on_release() {
        let mut db: Debounce = Debounce::new();
        let events: Vec<ButtonEvent> = feed(
            &mut db,
            &[
                (0, false),
                (10, true),                 // raw goes high
                (10 + DEBOUNCE_MS, true),   // press accepted here — still nothing
                (100, false),               // raw goes low, ~75 ms after the press
                (100 + DEBOUNCE_MS, false), // release accepted -> Tap
            ],
        );
        assert_eq!(events, vec![ButtonEvent::Tap]);
    }

    /// One: a press held past the threshold is exactly one hold — emitted while still down —
    /// and the later release is silent, because a hold is not also a tap.
    #[test]
    fn a_long_press_is_one_hold_and_no_tap() {
        let mut db: Debounce = Debounce::new();
        let events: Vec<ButtonEvent> = feed(
            &mut db,
            &[
                (0, false),
                (10, true),
                (10 + DEBOUNCE_MS, true), // press accepted, held from here
                (10 + DEBOUNCE_MS + HOLD_MS, true), // crosses the hold threshold -> Hold
                (2_000, false),           // raw goes low, long after
                (2_000 + DEBOUNCE_MS, false), // release accepted -> silent
            ],
        );
        assert_eq!(events, vec![ButtonEvent::Hold]);
    }

    /// Many: two separate press/release cycles are two taps.
    #[test]
    fn two_short_presses_are_two_taps() {
        let mut db: Debounce = Debounce::new();
        let events: Vec<ButtonEvent> = feed(
            &mut db,
            &[
                (0, false),
                (10, true),
                (10 + DEBOUNCE_MS, true),
                (100, false),
                (100 + DEBOUNCE_MS, false), // Tap
                (200, true),
                (200 + DEBOUNCE_MS, true),
                (300, false),
                (300 + DEBOUNCE_MS, false), // Tap
            ],
        );
        assert_eq!(events, vec![ButtonEvent::Tap, ButtonEvent::Tap]);
    }

    /// A bounce train shorter than the debounce window is swallowed whole: the level never
    /// holds steady long enough to be accepted, so nothing is emitted.
    #[test]
    fn a_bounce_train_inside_the_window_emits_nothing() {
        let mut db: Debounce = Debounce::new();
        let events: Vec<ButtonEvent> = feed(
            &mut db,
            &[
                (0, false),
                (2, true),
                (4, false),
                (6, true),
                (8, false),
                (10, true),
                (12, false), // every edge is < DEBOUNCE_MS apart: nothing settles
            ],
        );
        assert_eq!(events, vec![]);
    }

    proptest! {
        /// A press accepted and then released before the hold threshold is exactly one tap,
        /// for any held duration in the tap range.
        #[test]
        fn any_short_press_is_one_tap(held in DEBOUNCE_MS..HOLD_MS) {
            let mut db: Debounce = Debounce::new();
            let press: Tick = 10 + DEBOUNCE_MS;
            let low: Tick = press + held;
            let events: Vec<ButtonEvent> = feed(&mut db, &[
                (0, false),
                (10, true),
                (press, true),
                (low, false),
                (low + DEBOUNCE_MS, false),
            ]);
            prop_assert_eq!(events, vec![ButtonEvent::Tap]);
        }

        /// A press held past the threshold is exactly one hold and never a tap, for any
        /// overshoot past the threshold.
        #[test]
        fn any_long_press_is_one_hold(extra in 0u64..1_000) {
            let mut db: Debounce = Debounce::new();
            let press: Tick = 10 + DEBOUNCE_MS;
            let cross: Tick = press + HOLD_MS + extra;
            let events: Vec<ButtonEvent> = feed(&mut db, &[
                (0, false),
                (10, true),
                (press, true),
                (cross, true),
                (cross + 5, false),
                (cross + 5 + DEBOUNCE_MS, false),
            ]);
            prop_assert_eq!(events, vec![ButtonEvent::Hold]);
        }
    }
}
