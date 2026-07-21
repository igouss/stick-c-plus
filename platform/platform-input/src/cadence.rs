//! The double-click stage: two quick clicks in, one [`DoubleClick`](Gesture::DoubleClick) out.

use platform_core::Tick;

use crate::gesture::{Gesture, GestureConfig};

/// Reclassifies a [`Debounce`](crate::Debounce)'s [`Click`](Gesture::Click) stream into clicks
/// and [`DoubleClick`](Gesture::DoubleClick)s, as a pure function of time.
///
/// A double-click is two clicks inside [`double_click_ms`](GestureConfig::double_click_ms), so a
/// lone click cannot be reported until that window has passed with no second one — otherwise
/// every double-click would first fire a spurious single click. Fed the gesture a debounce
/// produced (often `None`) and `now` on *every* poll, it holds one pending click and answers:
///
/// - a first [`Click`](Gesture::Click) starts the window and emits nothing yet;
/// - a second [`Click`](Gesture::Click) inside the window emits one
///   [`DoubleClick`](Gesture::DoubleClick);
/// - a poll on which the window elapses with no second click emits the pending
///   [`Click`](Gesture::Click);
/// - a [`LongHold`](Gesture::LongHold) passes straight through and cancels any pending click.
///
/// It must be updated on every poll, not only when the debounce produced a gesture — that is how
/// a lone click's window is timed out. Because of that, a pending click is never older than the
/// window when a second click arrives, so `pending.is_some()` alone decides a double-click.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cadence {
    /// When a lone click is waiting for a possible second one; `None` while nothing is pending.
    pending: Option<Tick>,
}

impl Cadence {
    /// A cadence with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one poll `(now, gesture)` — the gesture a debounce produced this poll, if any — into
    /// the cadence, returning the gesture to act on now.
    pub fn update(
        &mut self,
        now: Tick,
        gesture: Option<Gesture>,
        config: GestureConfig,
    ) -> Option<Gesture> {
        match gesture {
            // A hold is unambiguous and immediate; it also abandons any waiting click.
            Some(Gesture::LongHold) => {
                self.pending = None;
                Some(Gesture::LongHold)
            }
            // A click either closes a waiting one into a double-click, or opens the window.
            Some(Gesture::Click) => match self.pending.take() {
                Some(_) => Some(Gesture::DoubleClick),
                None => {
                    self.pending = Some(now);
                    None
                }
            },
            // A double-click can only reach here if some other producer made one; pass it on.
            Some(Gesture::DoubleClick) => Some(Gesture::DoubleClick),
            // No new gesture: release a waiting click once its window has passed.
            None => match self.pending {
                Some(at) if now.saturating_sub(at) >= config.double_click_ms => {
                    self.pending = None;
                    Some(Gesture::Click)
                }
                _ => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: GestureConfig = GestureConfig {
        debounce_ms: 15,
        hold_ms: 600,
        double_click_ms: 300,
    };

    /// Feed a cadence a sequence of `(now, gesture)` polls and collect what it emitted, in order.
    fn feed(cadence: &mut Cadence, polls: &[(Tick, Option<Gesture>)]) -> Vec<Gesture> {
        polls
            .iter()
            .filter_map(|&(now, gesture): &(Tick, Option<Gesture>)| cadence.update(now, gesture, C))
            .collect()
    }

    /// Zero: a cadence fed only empty polls emits nothing, however long it is ticked.
    #[test]
    fn an_idle_cadence_emits_nothing() {
        let mut cadence: Cadence = Cadence::new();
        let gestures: Vec<Gesture> = feed(&mut cadence, &[(0, None), (1_000, None), (5_000, None)]);
        assert_eq!(gestures, vec![]);
    }

    /// One: a lone click is held back through its window and then emitted once as a plain click.
    #[test]
    fn a_lone_click_is_released_after_the_window() {
        let mut cadence: Cadence = Cadence::new();
        let gestures: Vec<Gesture> = feed(
            &mut cadence,
            &[
                (100, Some(Gesture::Click)),         // window opens — nothing yet
                (100 + C.double_click_ms - 1, None), // still inside the window — nothing
                (100 + C.double_click_ms, None),     // window elapses -> Click
            ],
        );
        assert_eq!(gestures, vec![Gesture::Click]);
    }

    /// One: two clicks inside the window are exactly one double-click, and no stray single click.
    #[test]
    fn two_quick_clicks_are_one_double_click() {
        let mut cadence: Cadence = Cadence::new();
        let gestures: Vec<Gesture> = feed(
            &mut cadence,
            &[
                (100, Some(Gesture::Click)),                          // first click
                (100 + C.double_click_ms - 50, Some(Gesture::Click)), // second, inside -> DoubleClick
                (100 + C.double_click_ms + 500, None),                // long after — silent
            ],
        );
        assert_eq!(gestures, vec![Gesture::DoubleClick]);
    }

    /// Many: two clicks too far apart are two separate single clicks, not a double-click.
    #[test]
    fn two_slow_clicks_are_two_single_clicks() {
        let mut cadence: Cadence = Cadence::new();
        let gestures: Vec<Gesture> = feed(
            &mut cadence,
            &[
                (100, Some(Gesture::Click)),     // first window opens
                (100 + C.double_click_ms, None), // it elapses -> Click
                (900, Some(Gesture::Click)),     // second window opens
                (900 + C.double_click_ms, None), // it elapses -> Click
            ],
        );
        assert_eq!(gestures, vec![Gesture::Click, Gesture::Click]);
    }

    /// A hold passes straight through, immediately, and cancels a waiting click so it never fires.
    #[test]
    fn a_hold_passes_through_and_cancels_a_pending_click() {
        let mut cadence: Cadence = Cadence::new();
        let gestures: Vec<Gesture> = feed(
            &mut cadence,
            &[
                (100, Some(Gesture::Click)),         // a click starts waiting
                (150, Some(Gesture::LongHold)), // a hold arrives -> LongHold now, click dropped
                (100 + C.double_click_ms + 1, None), // the old window passes — nothing left
            ],
        );
        assert_eq!(gestures, vec![Gesture::LongHold]);
    }
}
