//! The board's three buttons as one event source.

use platform_core::Tick;

use crate::button::{ButtonEvent, ButtonId, ButtonLevel, LatchedGesture};
use crate::gesture::{GestureConfig, Gestures};
use crate::recognizer::Recognizer;

/// How many events one [`poll`](Buttons::poll) can produce: one per button, at most.
///
/// A button reports at most one gesture per poll — the recognizer is a fold that returns an
/// `Option`, and the PMIC latch drains to one gesture — so three is not a cap that can be hit
/// and silently truncate. It is the exact width.
const MAX_EVENTS: usize = 3;

/// Which gestures each button reports, and the timing that decides them.
///
/// [`Default`] gives every button [`Gestures::Prompt`] with the default timing: the cheapest,
/// most responsive board, and nothing surprising. An app opts a button into a double-click
/// deliberately, at the point where the cost is visible:
///
/// ```
/// # use platform_input::{Gestures, InputConfig};
/// let config: InputConfig = InputConfig {
///     front: Gestures::WithDoubleClick,
///     ..InputConfig::default()
/// };
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InputConfig {
    /// What the front button (G37) reports.
    pub front: Gestures,
    /// What the side button (G39) reports.
    pub side: Gestures,
    /// The durations every gesture is decided against.
    pub timing: GestureConfig,
}

/// The board's three buttons, folded into one event stream.
///
/// Owns the three port implementations the composition root injects — the two levelled GPIO
/// buttons and the latched power button — and the pure recognizer state for each levelled one.
/// [`poll`](Buttons::poll) is the only method: hand it the current time, drain the events.
///
/// Everything about *when* is injected and everything about *what* is pure, so the whole thing
/// runs on the host against fake buttons and a hand-cranked clock.
pub struct Buttons<F, S, P> {
    front: F,
    side: S,
    power: P,
    front_recognizer: Recognizer,
    side_recognizer: Recognizer,
    config: InputConfig,
}

impl<F, S, P> Buttons<F, S, P>
where
    F: ButtonLevel,
    S: ButtonLevel,
    P: LatchedGesture,
{
    /// Bind the three buttons, with `config` deciding what each one reports.
    pub fn new(front: F, side: S, power: P, config: InputConfig) -> Self {
        Buttons {
            front,
            side,
            power,
            front_recognizer: Recognizer::new(config.front),
            side_recognizer: Recognizer::new(config.side),
            config,
        }
    }

    /// Read every button once and return the events that just became certain.
    ///
    /// Must be called on a steady cadence — the recognizers time their windows from the `now`
    /// they are handed, and the power button's latch queues presses until it is drained. A
    /// period comfortably shorter than [`debounce_ms`](GestureConfig::debounce_ms) keeps a
    /// press settling within a couple of polls; 10 ms is this board's floor and its usual
    /// choice.
    ///
    /// Returns [`Events`], which is an [`Iterator`] — usually empty, since most polls find
    /// nothing happening.
    pub fn poll(&mut self, now: Tick) -> Events {
        let timing: GestureConfig = self.config.timing;
        let front: bool = self.front.pressed();
        let side: bool = self.side.pressed();
        Events::new([
            self.front_recognizer
                .update(now, front, timing)
                .map(|gesture| ButtonEvent::new(ButtonId::Front, gesture)),
            self.side_recognizer
                .update(now, side, timing)
                .map(|gesture| ButtonEvent::new(ButtonId::Side, gesture)),
            // Already classified by the PMIC; it bypasses the recognizers entirely.
            self.power
                .take()
                .map(|gesture| ButtonEvent::new(ButtonId::Power, gesture)),
        ])
    }
}

/// The events one [`poll`](Buttons::poll) produced — an iterator over at most [`MAX_EVENTS`].
///
/// Fixed-size and [`Copy`]: no allocation, so the whole crate stays `no_std` and a poll costs
/// nothing on a quiet board. Events arrive in button order (front, side, power), which is
/// arbitrary but *stable*, so a test can assert an exact sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Events {
    slots: [Option<ButtonEvent>; MAX_EVENTS],
    next: usize,
}

impl Events {
    /// Wrap one poll's per-button results.
    const fn new(slots: [Option<ButtonEvent>; MAX_EVENTS]) -> Self {
        Events { slots, next: 0 }
    }

    /// Whether nothing happened — the common case on a quiet board.
    pub fn is_empty(&self) -> bool {
        self.slots[self.next..].iter().all(Option::is_none)
    }
}

impl Iterator for Events {
    type Item = ButtonEvent;

    fn next(&mut self) -> Option<ButtonEvent> {
        while self.next < MAX_EVENTS {
            let slot: Option<ButtonEvent> = self.slots[self.next];
            self.next += 1;
            if slot.is_some() {
                return slot;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::Gesture;
    use core::cell::Cell;

    /// A levelled button whose raw level a test drives by hand.
    struct FakeLevel<'a> {
        pressed: &'a Cell<bool>,
    }

    impl ButtonLevel for FakeLevel<'_> {
        fn pressed(&mut self) -> bool {
            self.pressed.get()
        }
    }

    /// A latch a test arms by hand; draining it clears it, like the real PMIC register.
    struct FakeLatch<'a> {
        armed: &'a Cell<Option<Gesture>>,
    }

    impl LatchedGesture for FakeLatch<'_> {
        fn take(&mut self) -> Option<Gesture> {
            self.armed.take()
        }
    }

    /// A board wired up for a test: the three levels, and the buttons that read them.
    struct Board<'a> {
        buttons: Buttons<FakeLevel<'a>, FakeLevel<'a>, FakeLatch<'a>>,
        now: Tick,
    }

    impl<'a> Board<'a> {
        fn new(
            front: &'a Cell<bool>,
            side: &'a Cell<bool>,
            power: &'a Cell<Option<Gesture>>,
            config: InputConfig,
        ) -> Self {
            Board {
                buttons: Buttons::new(
                    FakeLevel { pressed: front },
                    FakeLevel { pressed: side },
                    FakeLatch { armed: power },
                    config,
                ),
                now: 0,
            }
        }

        /// Poll `ms` times at 1 ms steps, collecting everything that came out.
        fn run(&mut self, ms: Tick) -> Vec<ButtonEvent> {
            (0..ms)
                .flat_map(|_| {
                    let events: Events = self.buttons.poll(self.now);
                    self.now += 1;
                    events
                })
                .collect()
        }
    }

    /// Zero: a board nobody touches produces nothing, however long it is polled.
    #[test]
    fn an_untouched_board_produces_nothing() {
        let front: Cell<bool> = Cell::new(false);
        let side: Cell<bool> = Cell::new(false);
        let power: Cell<Option<Gesture>> = Cell::new(None);
        let mut board: Board = Board::new(&front, &side, &power, InputConfig::default());

        assert_eq!(board.run(1_000), vec![]);
    }

    /// One: a press on the front button is reported as the front button's click, and nothing
    /// else fires alongside it.
    #[test]
    fn a_front_press_is_one_front_click() {
        let front: Cell<bool> = Cell::new(false);
        let side: Cell<bool> = Cell::new(false);
        let power: Cell<Option<Gesture>> = Cell::new(None);
        let mut board: Board = Board::new(&front, &side, &power, InputConfig::default());

        front.set(true);
        board.run(40);
        front.set(false);
        let events: Vec<ButtonEvent> = board.run(40);

        assert_eq!(
            events,
            vec![ButtonEvent::new(ButtonId::Front, Gesture::Click)]
        );
    }

    /// The power button's gesture arrives already classified, without ever being levelled — the
    /// whole reason it has its own port.
    #[test]
    fn a_latched_power_click_arrives_without_a_level() {
        let front: Cell<bool> = Cell::new(false);
        let side: Cell<bool> = Cell::new(false);
        let power: Cell<Option<Gesture>> = Cell::new(None);
        let mut board: Board = Board::new(&front, &side, &power, InputConfig::default());

        power.set(Some(Gesture::Click));
        let events: Vec<ButtonEvent> = board.run(1);

        assert_eq!(
            events,
            vec![ButtonEvent::new(ButtonId::Power, Gesture::Click)]
        );
    }

    /// Draining the latch is destructive: one armed press is reported exactly once, not on
    /// every poll thereafter.
    #[test]
    fn a_latched_press_is_reported_exactly_once() {
        let front: Cell<bool> = Cell::new(false);
        let side: Cell<bool> = Cell::new(false);
        let power: Cell<Option<Gesture>> = Cell::new(None);
        let mut board: Board = Board::new(&front, &side, &power, InputConfig::default());

        power.set(Some(Gesture::Click));
        let events: Vec<ButtonEvent> = board.run(100);

        assert_eq!(events.len(), 1);
    }

    /// Many: three buttons acting on the same poll all report, in a stable order.
    #[test]
    fn three_buttons_at_once_all_report_in_button_order() {
        let front: Cell<bool> = Cell::new(false);
        let side: Cell<bool> = Cell::new(false);
        let power: Cell<Option<Gesture>> = Cell::new(None);
        let mut board: Board = Board::new(&front, &side, &power, InputConfig::default());

        // Settle both levelled buttons pressed, then release them together and arm the latch so
        // all three land on the same poll.
        front.set(true);
        side.set(true);
        board.run(40);
        front.set(false);
        side.set(false);
        board.run(15);
        power.set(Some(Gesture::Click));
        let events: Vec<ButtonEvent> = board.run(5);

        assert_eq!(
            events,
            vec![
                ButtonEvent::new(ButtonId::Front, Gesture::Click),
                ButtonEvent::new(ButtonId::Side, Gesture::Click),
                ButtonEvent::new(ButtonId::Power, Gesture::Click),
            ]
        );
    }

    /// The per-button gesture set is honoured: the front button pairs two quick presses into a
    /// double-click while the side button, left prompt, reports two plain clicks.
    #[test]
    fn only_the_button_that_asked_for_it_double_clicks() {
        let front: Cell<bool> = Cell::new(false);
        let side: Cell<bool> = Cell::new(false);
        let power: Cell<Option<Gesture>> = Cell::new(None);
        let config: InputConfig = InputConfig {
            front: Gestures::WithDoubleClick,
            ..InputConfig::default()
        };
        let mut board: Board = Board::new(&front, &side, &power, config);

        let mut seen: Vec<ButtonEvent> = Vec::new();
        // Two quick press/release cycles on both buttons at once.
        front.set(true);
        side.set(true);
        seen.extend(board.run(40));
        front.set(false);
        side.set(false);
        seen.extend(board.run(25));
        front.set(true);
        side.set(true);
        seen.extend(board.run(40));
        front.set(false);
        side.set(false);
        seen.extend(board.run(25));

        assert_eq!(
            seen,
            vec![
                // The first release: only the prompt side button says anything.
                ButtonEvent::new(ButtonId::Side, Gesture::Click),
                // The second release lands both at once, in button order — the front button's
                // two clicks having paired into one double-click.
                ButtonEvent::new(ButtonId::Front, Gesture::DoubleClick),
                ButtonEvent::new(ButtonId::Side, Gesture::Click),
            ],
            "the side button's clicks are prompt; the front button's pair into one double-click"
        );
    }

    /// A quiet poll is empty, and an eventful one is not — the cheap check a caller leans on.
    #[test]
    fn emptiness_tracks_whether_anything_happened() {
        let front: Cell<bool> = Cell::new(false);
        let side: Cell<bool> = Cell::new(false);
        let power: Cell<Option<Gesture>> = Cell::new(None);
        let mut board: Board = Board::new(&front, &side, &power, InputConfig::default());

        assert!(board.buttons.poll(0).is_empty());
        power.set(Some(Gesture::Click));
        assert!(!board.buttons.poll(1).is_empty());
    }
}
