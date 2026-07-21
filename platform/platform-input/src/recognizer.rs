//! One levelled button's whole gesture pipeline, assembled from its declared [`Gestures`].

use platform_core::Tick;

use crate::cadence::Cadence;
use crate::debounce::Debounce;
use crate::gesture::{Gesture, GestureConfig, Gestures};

/// The pipeline a single levelled button's raw level runs through.
///
/// Always a [`Debounce`]; plus a [`Cadence`] exactly when the button declared
/// [`Gestures::WithDoubleClick`]. That `Option` is the whole of the trade: a button that never
/// asked for a double-click has no cadence to hold its clicks back, so its clicks stay prompt,
/// and it is structurally incapable of emitting a [`DoubleClick`](Gesture::DoubleClick) rather
/// than merely unlikely to.
#[derive(Clone, Copy, Debug)]
pub struct Recognizer {
    debounce: Debounce,
    cadence: Option<Cadence>,
}

impl Recognizer {
    /// A recognizer for a button that reports `gestures`, starting released and idle.
    pub fn new(gestures: Gestures) -> Self {
        Recognizer {
            debounce: Debounce::new(),
            cadence: match gestures {
                Gestures::Prompt => None,
                Gestures::WithDoubleClick => Some(Cadence::new()),
            },
        }
    }

    /// Fold one poll `(now, raw_pressed)` through the whole pipeline, returning the gesture to
    /// act on now, if any.
    ///
    /// The cadence — when there is one — is stepped on *every* poll, including the ones the
    /// debounce was silent for. That is not an optimisation to skip: it is how a lone click's
    /// window is timed out and the click finally released.
    pub fn update(
        &mut self,
        now: Tick,
        raw_pressed: bool,
        config: GestureConfig,
    ) -> Option<Gesture> {
        let settled: Option<Gesture> = self.debounce.update(now, raw_pressed, config);
        match &mut self.cadence {
            Some(cadence) => cadence.update(now, settled, config),
            None => settled,
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

    /// Hold `raw` steady for `ms` polls at 1 ms each, collecting whatever came out.
    fn hold(rec: &mut Recognizer, now: &mut Tick, raw: bool, ms: Tick) -> Vec<Gesture> {
        (0..ms)
            .filter_map(|_| {
                let out: Option<Gesture> = rec.update(*now, raw, C);
                *now += 1;
                out
            })
            .collect()
    }

    /// A prompt button reports its click the moment the release settles — no window to wait out.
    #[test]
    fn a_prompt_button_clicks_without_waiting() {
        let mut rec: Recognizer = Recognizer::new(Gestures::Prompt);
        let mut now: Tick = 0;

        let pressing: Vec<Gesture> = hold(&mut rec, &mut now, true, 40);
        let releasing: Vec<Gesture> = hold(&mut rec, &mut now, false, 20);

        assert_eq!(pressing, vec![]);
        assert_eq!(
            releasing,
            vec![Gesture::Click],
            "a prompt button must not hold its click back"
        );
    }

    /// A prompt button cannot produce a double-click, however fast the two clicks are: with no
    /// cadence there is nothing to pair them, so two quick presses are simply two clicks.
    #[test]
    fn a_prompt_button_never_double_clicks() {
        let mut rec: Recognizer = Recognizer::new(Gestures::Prompt);
        let mut now: Tick = 0;

        let mut seen: Vec<Gesture> = Vec::new();
        seen.extend(hold(&mut rec, &mut now, true, 40));
        seen.extend(hold(&mut rec, &mut now, false, 25));
        seen.extend(hold(&mut rec, &mut now, true, 40));
        seen.extend(hold(&mut rec, &mut now, false, 25));

        assert_eq!(seen, vec![Gesture::Click, Gesture::Click]);
    }

    /// A double-click button holds a lone click back until its window has passed — the cost the
    /// [`Gestures::WithDoubleClick`] declaration buys.
    #[test]
    fn a_double_click_button_delays_a_lone_click() {
        let mut rec: Recognizer = Recognizer::new(Gestures::WithDoubleClick);
        let mut now: Tick = 0;

        let pressing: Vec<Gesture> = hold(&mut rec, &mut now, true, 40);
        let just_after: Vec<Gesture> = hold(&mut rec, &mut now, false, 20);
        let past_window: Vec<Gesture> = hold(&mut rec, &mut now, false, C.double_click_ms);

        assert_eq!(pressing, vec![]);
        assert_eq!(
            just_after,
            vec![],
            "the click is still waiting for a partner"
        );
        assert_eq!(past_window, vec![Gesture::Click]);
    }

    /// Two quick presses on a double-click button are one double-click, and no stray single.
    #[test]
    fn a_double_click_button_pairs_two_quick_presses() {
        let mut rec: Recognizer = Recognizer::new(Gestures::WithDoubleClick);
        let mut now: Tick = 0;

        let mut seen: Vec<Gesture> = Vec::new();
        seen.extend(hold(&mut rec, &mut now, true, 40));
        seen.extend(hold(&mut rec, &mut now, false, 25));
        seen.extend(hold(&mut rec, &mut now, true, 40));
        seen.extend(hold(&mut rec, &mut now, false, 25));

        assert_eq!(seen, vec![Gesture::DoubleClick]);
    }

    /// A hold is prompt on either kind of button: it is unambiguous the instant the threshold
    /// passes, so no window ever delays it.
    #[test]
    fn a_hold_is_prompt_on_a_double_click_button() {
        let mut rec: Recognizer = Recognizer::new(Gestures::WithDoubleClick);
        let mut now: Tick = 0;

        let held: Vec<Gesture> = hold(&mut rec, &mut now, true, C.hold_ms + 40);

        assert_eq!(held, vec![Gesture::LongHold]);
    }
}
