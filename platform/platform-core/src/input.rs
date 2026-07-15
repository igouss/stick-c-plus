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

/// How long a [`Tap`](ButtonEvent::Tap) waits for a second one before it counts as a lone tap,
/// in milliseconds — the [`TapCadence`] window.
///
/// A double-tap can only be told from a single tap by waiting: a lone tap is confirmed once
/// this window passes with no second tap, so a button routed through [`TapCadence`] reports a
/// [`Tap`](ButtonEvent::Tap) this much later than a bare [`Debounce`] would. 300 ms is a
/// comfortable double-click gap yet short enough that start/pause still feels prompt.
pub const DOUBLE_TAP_MS: Tick = 300;

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

/// What a debounced button did.
///
/// [`Debounce`] emits only [`Tap`](ButtonEvent::Tap) and [`Hold`](ButtonEvent::Hold);
/// [`DoubleTap`](ButtonEvent::DoubleTap) is produced only by [`TapCadence`], which reclassifies
/// two quick taps. An app maps whichever of these it wants onto its own commands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonEvent {
    /// A short press: pressed and released inside [`HOLD_MS`]. Emitted on release — or, through
    /// [`TapCadence`], once its [`DOUBLE_TAP_MS`] window has passed with no second tap.
    Tap,
    /// A long press: held for at least [`HOLD_MS`]. Emitted once, the moment the threshold
    /// passes — while the button is still down — so a hold feels immediate.
    Hold,
    /// Two taps inside [`DOUBLE_TAP_MS`]. Only [`TapCadence`] emits this.
    DoubleTap,
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

/// Reclassifies a [`Debounce`]'s [`Tap`](ButtonEvent::Tap) stream into taps and
/// [`DoubleTap`](ButtonEvent::DoubleTap)s, as a pure function of time.
///
/// A double-tap is two taps inside [`DOUBLE_TAP_MS`], so a lone tap cannot be reported until
/// that window has passed with no second tap — otherwise every double-tap would first fire a
/// spurious single tap. Fed the event a [`Debounce`] produced (often `None`) and `now` on
/// *every* poll, it holds one pending tap and answers:
///
/// - a first [`Tap`](ButtonEvent::Tap) starts the window and emits nothing yet;
/// - a second [`Tap`](ButtonEvent::Tap) inside the window emits one
///   [`DoubleTap`](ButtonEvent::DoubleTap);
/// - a poll on which the window elapses with no second tap emits the pending
///   [`Tap`](ButtonEvent::Tap);
/// - a [`Hold`](ButtonEvent::Hold) passes straight through and cancels any pending tap.
///
/// It must be updated on every poll, not only when the debounce produced an event — that is how
/// a lone tap's window is timed out. Because of that, a pending tap is never older than the
/// window when a second tap arrives, so `pending.is_some()` alone decides a double-tap.
#[derive(Clone, Copy, Debug, Default)]
pub struct TapCadence {
    /// When a lone tap is waiting for a possible second one; `None` while nothing is pending.
    pending: Option<Tick>,
}

impl TapCadence {
    /// A cadence with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one poll `(now, event)` — the event a [`Debounce`] produced this poll, if any —
    /// into the cadence, returning the gesture to act on now.
    pub fn update(&mut self, now: Tick, event: Option<ButtonEvent>) -> Option<ButtonEvent> {
        match event {
            // A hold is unambiguous and immediate; it also abandons any waiting tap.
            Some(ButtonEvent::Hold) => {
                self.pending = None;
                Some(ButtonEvent::Hold)
            }
            // A tap either closes a waiting one into a double-tap, or opens the window itself.
            Some(ButtonEvent::Tap) => match self.pending.take() {
                Some(_) => Some(ButtonEvent::DoubleTap),
                None => {
                    self.pending = Some(now);
                    None
                }
            },
            // A double-tap can only reach here if some other producer made one; pass it on.
            Some(ButtonEvent::DoubleTap) => Some(ButtonEvent::DoubleTap),
            // No new gesture: release a waiting tap once its window has passed.
            None => match self.pending {
                Some(at) if now.saturating_sub(at) >= DOUBLE_TAP_MS => {
                    self.pending = None;
                    Some(ButtonEvent::Tap)
                }
                _ => None,
            },
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

    /// Feed a cadence a sequence of `(now, event)` polls and collect what it emitted, in order.
    fn feed_cadence(
        cadence: &mut TapCadence,
        polls: &[(Tick, Option<ButtonEvent>)],
    ) -> Vec<ButtonEvent> {
        polls
            .iter()
            .filter_map(|&(now, event): &(Tick, Option<ButtonEvent>)| cadence.update(now, event))
            .collect()
    }

    /// Zero: a cadence fed only empty polls emits nothing, however long it is ticked.
    #[test]
    fn an_idle_cadence_emits_nothing() {
        let mut cadence: TapCadence = TapCadence::new();
        let events: Vec<ButtonEvent> =
            feed_cadence(&mut cadence, &[(0, None), (1_000, None), (5_000, None)]);
        assert_eq!(events, vec![]);
    }

    /// One: a lone tap is held back through its window and then emitted once as a plain tap.
    #[test]
    fn a_lone_tap_is_released_after_the_window() {
        let mut cadence: TapCadence = TapCadence::new();
        let events: Vec<ButtonEvent> = feed_cadence(
            &mut cadence,
            &[
                (100, Some(ButtonEvent::Tap)),   // window opens — nothing yet
                (100 + DOUBLE_TAP_MS - 1, None), // still inside the window — nothing
                (100 + DOUBLE_TAP_MS, None),     // window elapses -> Tap
            ],
        );
        assert_eq!(events, vec![ButtonEvent::Tap]);
    }

    /// One: two taps inside the window are exactly one double-tap, and no stray single tap.
    #[test]
    fn two_quick_taps_are_one_double_tap() {
        let mut cadence: TapCadence = TapCadence::new();
        let events: Vec<ButtonEvent> = feed_cadence(
            &mut cadence,
            &[
                (100, Some(ButtonEvent::Tap)),                      // first tap
                (100 + DOUBLE_TAP_MS - 50, Some(ButtonEvent::Tap)), // second, inside -> DoubleTap
                (100 + DOUBLE_TAP_MS + 500, None),                  // long after — silent
            ],
        );
        assert_eq!(events, vec![ButtonEvent::DoubleTap]);
    }

    /// Many: two taps too far apart are two separate single taps, not a double-tap.
    #[test]
    fn two_slow_taps_are_two_single_taps() {
        let mut cadence: TapCadence = TapCadence::new();
        let events: Vec<ButtonEvent> = feed_cadence(
            &mut cadence,
            &[
                (100, Some(ButtonEvent::Tap)), // first window opens
                (100 + DOUBLE_TAP_MS, None),   // it elapses -> Tap
                (900, Some(ButtonEvent::Tap)), // second window opens
                (900 + DOUBLE_TAP_MS, None),   // it elapses -> Tap
            ],
        );
        assert_eq!(events, vec![ButtonEvent::Tap, ButtonEvent::Tap]);
    }

    /// A hold passes straight through, immediately, and cancels a waiting tap so it never fires.
    #[test]
    fn a_hold_passes_through_and_cancels_a_pending_tap() {
        let mut cadence: TapCadence = TapCadence::new();
        let events: Vec<ButtonEvent> = feed_cadence(
            &mut cadence,
            &[
                (100, Some(ButtonEvent::Tap)),   // a tap starts waiting
                (150, Some(ButtonEvent::Hold)),  // a hold arrives -> Hold now, tap dropped
                (100 + DOUBLE_TAP_MS + 1, None), // the old window passes — nothing left
            ],
        );
        assert_eq!(events, vec![ButtonEvent::Hold]);
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
